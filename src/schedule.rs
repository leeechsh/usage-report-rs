//! Persist only the fixed usage-report timer override; no shell or caller-chosen path.
use crate::model::*;
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

const TIMER: &str = "usage-report-scan.timer";
const OVERRIDE: &str = "50-usage-report.conf";

fn duration_seconds(input: &str) -> Result<f64> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut total = 0.0;
    let mut any = false;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        ensure!(i > start, "时间格式应为 30m、3h、1d 或 1h30m");
        let value: f64 = input[start..i].parse().context("无效的时间数值")?;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let scale = match &input[start..i] {
            "" | "s" | "sec" | "second" | "seconds" => 1.0,
            "m" | "min" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hour" | "hours" => 3600.0,
            "d" | "day" | "days" => 86400.0,
            "w" | "week" | "weeks" => 604800.0,
            "ms" => 0.001,
            "us" => 0.000001,
            _ => bail!("不支持的时间单位"),
        };
        total += value * scale;
        any = true;
    }
    ensure!(
        any && total.is_finite() && total >= 0.0 && total <= u64::MAX as f64,
        "时间值无效或过大"
    );
    Ok(total)
}

pub fn parse_duration(input: &str) -> Result<u64> {
    let value = duration_seconds(input)?;
    ensure!(value.fract() == 0.0, "请使用整数秒");
    Ok(value as u64)
}

pub fn format_duration(seconds: u64) -> String {
    if seconds > 0 && seconds.is_multiple_of(86400) {
        format!("{}d", seconds / 86400)
    } else if seconds > 0 && seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds > 0 && seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

pub fn next_run_text(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp else {
        return "未排期".into();
    };
    let raw = timestamp as libc::time_t;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe { libc::localtime_r(&raw, local.as_mut_ptr()) }.is_null() {
        return timestamp.to_string();
    }
    let mut buffer = [0_u8; 80];
    let written = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            c"%Y-%m-%d %H:%M:%S %Z".as_ptr(),
            local.as_ptr(),
        )
    };
    String::from_utf8_lossy(&buffer[..written]).into_owned()
}

impl ScanSettings {
    pub fn patched(&self, patch: &ScanSettingsPatch) -> Result<Self> {
        let mut result = self.clone();
        if let Some(v) = patch.interval_secs {
            result.interval_secs = v;
        }
        if let Some(v) = patch.boot_delay_secs {
            result.boot_delay_secs = v;
        }
        if let Some(v) = patch.jitter_secs {
            result.jitter_secs = v;
        }
        if let Some(v) = patch.enabled {
            result.enabled = v;
        }
        ensure!(
            (60..=30 * 86400).contains(&result.interval_secs),
            "检索间隔须在 1 分钟到 30 天之间"
        );
        ensure!(
            result.boot_delay_secs <= 86400,
            "开机延迟须在 0 秒到 1 天之间"
        );
        ensure!(
            result.jitter_secs <= 3600 && result.jitter_secs <= result.interval_secs,
            "随机延迟不能超过 1 小时或检索间隔"
        );
        Ok(result)
    }
}

trait TimerControl: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<String>;
}

struct Systemctl;
impl TimerControl for Systemctl {
    fn run(&self, args: &[&str]) -> Result<String> {
        let out = std::process::Command::new("/usr/bin/timeout")
            .args(["8", "/usr/bin/systemctl"])
            .args(args)
            .env("LC_ALL", "C")
            .output()?;
        ensure!(
            out.status.success(),
            "systemctl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(String::from_utf8(out.stdout)?.trim().into())
    }
}

fn effective_values(text: &str) -> Result<(u64, u64, u64)> {
    let mut section = "";
    let mut interval = Vec::new();
    let mut boot = Vec::new();
    let mut other_triggers = Vec::new();
    let mut jitter = 0;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section != "[Timer]" || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if matches!(
            key,
            "OnUnitInactiveSec"
                | "OnBootSec"
                | "OnCalendar"
                | "OnUnitActiveSec"
                | "OnStartupSec"
                | "OnActiveSec"
        ) && value.is_empty()
        {
            // systemd resets the entire timer expression list, across all trigger types.
            interval.clear();
            boot.clear();
            other_triggers.clear();
            continue;
        }
        match key {
            "OnUnitInactiveSec" | "OnBootSec" => {
                let values = if key == "OnBootSec" {
                    &mut boot
                } else {
                    &mut interval
                };
                values.push(parse_duration(value)?);
            }
            "RandomizedDelaySec" => {
                jitter = if value.is_empty() {
                    0
                } else {
                    parse_duration(value)?
                };
            }
            "OnCalendar" | "OnUnitActiveSec" | "OnStartupSec" | "OnActiveSec" => {
                other_triggers.push(key)
            }
            _ => {}
        }
    }
    ensure!(
        other_triggers.is_empty(),
        "定时器存在其他触发规则，请先由系统管理员整理配置"
    );
    ensure!(
        interval.len() == 1 && boot.len() <= 1,
        "定时器存在多重触发规则或缺少检索间隔，请检查 systemctl cat {TIMER}"
    );
    Ok((interval[0], boot.first().copied().unwrap_or(0), jitter))
}

pub struct ScheduleManager {
    directory: PathBuf,
    control: Box<dyn TimerControl>,
    lock: Mutex<()>,
}

impl Default for ScheduleManager {
    fn default() -> Self {
        Self {
            directory: "/etc/systemd/system/usage-report-scan.timer.d".into(),
            control: Box::new(Systemctl),
            lock: Mutex::new(()),
        }
    }
}

impl ScheduleManager {
    fn read_unlocked(&self, can_edit: bool) -> Result<ScanSettings> {
        let (interval_secs, boot_delay_secs, jitter_secs) =
            effective_values(&self.control.run(&["cat", TIMER])?)?;
        let state = self
            .control
            .run(&["show", TIMER, "--property=UnitFileState", "--value"])?;
        let active = self
            .control
            .run(&["show", TIMER, "--property=ActiveState", "--value"])?
            == "active";
        let next = self.control.run(&[
            "show",
            TIMER,
            "--property=NextElapseUSecMonotonic",
            "--value",
        ])?;
        let next_run = if active && !next.is_empty() && next != "infinity" && next != "0" {
            let mut clock = std::mem::MaybeUninit::<libc::timespec>::uninit();
            let code = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, clock.as_mut_ptr()) };
            if code == 0 {
                duration_seconds(&next).ok().filter(|v| *v > 0.0).map(|v| {
                    now()
                        + (v - unsafe { clock.assume_init() }.tv_sec as f64)
                            .max(0.0)
                            .round() as i64
                })
            } else {
                None
            }
        } else {
            None
        };
        Ok(ScanSettings {
            interval_secs,
            boot_delay_secs,
            jitter_secs,
            enabled: matches!(state.as_str(), "enabled" | "enabled-runtime"),
            active,
            next_run,
            can_edit,
            message: "按上次扫描完成时间计时；调度精度另有约 1 分钟浮动。".into(),
        })
    }

    pub fn read(&self, can_edit: bool) -> Result<ScanSettings> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("配置锁不可用"))?;
        self.read_unlocked(can_edit)
    }

    pub fn update(&self, patch: &ScanSettingsPatch) -> Result<ScanSettings> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("配置锁不可用"))?;
        let old = self.read_unlocked(true)?;
        let new = old.patched(patch)?;
        if old.interval_secs == new.interval_secs
            && old.boot_delay_secs == new.boot_delay_secs
            && old.jitter_secs == new.jitter_secs
            && old.enabled == new.enabled
            && old.active == new.enabled
        {
            let mut result = old;
            result.message = "配置未变化，无需重载定时器。".into();
            return Ok(result);
        }
        let path = self.directory.join(OVERRIDE);
        let backup = match fs::read(&path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let text = format!(
            "# Managed by usage-report; administrator settings\n[Timer]\nOnUnitInactiveSec=\nOnBootSec=\nOnUnitInactiveSec={}s\nOnBootSec={}s\nRandomizedDelaySec={}s\n",
            new.interval_secs, new.boot_delay_secs, new.jitter_secs
        );
        let apply = (|| -> Result<ScanSettings> {
            self.write(&path, text.as_bytes())?;
            self.control.run(&["daemon-reload"])?;
            if new.enabled {
                self.control.run(&["enable", TIMER])?;
                self.control.run(&["restart", TIMER])?;
            } else {
                self.control.run(&["disable", "--now", TIMER])?;
            }
            let mut result = self.read_unlocked(true)?;
            ensure!(
                result.interval_secs == new.interval_secs
                    && result.boot_delay_secs == new.boot_delay_secs
                    && result.jitter_secs == new.jitter_secs
                    && result.enabled == new.enabled
                    && result.active == new.enabled,
                "定时器生效值不符，请检查其他 override"
            );
            result.message = if new.enabled {
                "已保存并启用；下次执行时间已重新计算，到期任务可能立即开始。"
            } else {
                "已保存并关闭自动检索；正在运行的扫描不会中断，仍可手动更新。"
            }
            .into();
            Ok(result)
        })();
        match apply {
            Ok(value) => Ok(value),
            Err(error) => {
                let rollback = (|| -> Result<()> {
                    if let Some(bytes) = backup {
                        self.write(&path, &bytes)?;
                    } else {
                        match fs::remove_file(&path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => return Err(e.into()),
                        }
                    }
                    self.control.run(&["daemon-reload"])?;
                    self.control
                        .run(&[if old.enabled { "enable" } else { "disable" }, TIMER])?;
                    self.control
                        .run(&[if old.active { "restart" } else { "stop" }, TIMER])?;
                    Ok(())
                })();
                match rollback {
                    Ok(()) => bail!("保存失败，已恢复原配置：{error:#}"),
                    Err(rollback) => bail!(
                        "保存失败：{error:#}；恢复原配置也失败：{rollback:#}；请检查 systemctl cat {TIMER}"
                    ),
                }
            }
        }
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        fs::create_dir_all(&self.directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
        fs::File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
