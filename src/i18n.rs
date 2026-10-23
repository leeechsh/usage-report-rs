use clap::ValueEnum;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum Language {
    #[default]
    #[value(name = "en")]
    English,
    #[value(name = "zh")]
    Chinese,
}

impl Language {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::English => Self::Chinese,
            Self::Chinese => Self::English,
        };
    }
    pub fn text(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Chinese => zh,
            Self::English => en,
        }
    }
    pub fn status(self, status: &str) -> &str {
        if self == Self::Chinese {
            return status;
        }
        match status {
            "正常" => "OK",
            "接近软限制" => "Near soft limit",
            "接近硬限制" => "Near hard limit",
            "超过软限制" => "Over soft limit",
            "达到硬限制" => "Hard limit hit",
            _ => status,
        }
    }
    /// Only application diagnostics are translated; paths and user data never pass here.
    pub fn diagnostic(self, message: &str) -> String {
        if self == Self::Chinese {
            return message.into();
        }
        let translated = match message {
            "未排期" => "Not scheduled",
            "尚无" => "N/A",
            "读取配额设备" => "Read quota device",
            "检查内核用户配额是否启用（Q_GETINFO）" => {
                "Check kernel user quota activation (Q_GETINFO)"
            }
            "读取内核用户配额（Q_GETNEXTQUOTA/Q_GETQUOTA）" => {
                "Read kernel user quotas (Q_GETNEXTQUOTA/Q_GETQUOTA)"
            }
            "受限演示：只能读取服务属主自己的配额" => {
                "Limited demo: only the service owner's quota is available"
            }
            "配额溢出" => "Quota overflow",
            "内核配额枚举未向前推进" => "Kernel quota enumeration did not advance",
            "findmnt 无法识别文件系统" => "findmnt could not identify the filesystem",
            "无文件系统设备" => "Missing filesystem device",
            "无文件系统挂载点" => "Missing filesystem mount point",
            "statvfs 失败" => "statvfs failed",
            "只有管理员可以更新索引或修改检索配置" => {
                "Only administrators can rebuild the index or change scan settings"
            }
            "权限不足：普通用户只能浏览自己的 UID" => {
                "Permission denied: users can only browse their own UID"
            }
            "演示服务不能修改系统检索配置" => {
                "Demo services cannot change system scan settings"
            }
            "演示服务不能启动系统扫描任务" => {
                "Demo services cannot start system scans"
            }
            "启动扫描任务失败" => "Could not start the scan service",
            "已请求后台更新；当前缓存仍可浏览。usage-report --status 可查看进度。" => {
                "Background rebuild requested; the current cache is still available. Use usage-report --status for progress."
            }
            "无法识别 Unix socket 对端" => "Could not identify the Unix socket peer",
            "请求过长或未终止" => "Request too large or unterminated",
            "查询服务不属于 root 或当前用户" => {
                "Query service is not owned by root or the current user"
            }
            "服务响应过大" => "Service response is too large",
            "服务返回空响应" => "Service returned an empty response",
            "服务返回无效的检索配置" => "Service returned invalid scan settings",
            "无效的树响应" => "Invalid tree response",
            "无效的总览响应" => "Invalid overview response",
            "无效的配置响应" => "Invalid settings response",
            "服务不支持检索配置，请升级后台服务" => {
                "The service does not support scan settings; upgrade the backend"
            }
            "非交互终端请使用 --plain 或 --json" => {
                "Use --plain or --json outside an interactive terminal"
            }
            "需要 root 权限" => "Root privileges required",
            "--watch 查看全局索引进度，不接受用户名" => {
                "--watch displays global indexing progress and does not accept a user"
            }
            "时间格式应为 30m、3h、1d 或 1h30m" => {
                "Use a duration such as 30m, 3h, 1d or 1h30m"
            }
            "无效的时间数值" => "Invalid duration value",
            "不支持的时间单位" => "Unsupported duration unit",
            "时间值无效或过大" => "Duration is invalid or too large",
            "请使用整数秒" => "Use whole seconds",
            "检索间隔须在 1 分钟到 30 天之间" => {
                "Scan interval must be between 1 minute and 30 days"
            }
            "开机延迟须在 0 秒到 1 天之间" => {
                "Boot delay must be between 0 seconds and 1 day"
            }
            "随机延迟不能超过 1 小时或检索间隔" => {
                "Jitter must not exceed 1 hour or the scan interval"
            }
            "定时器存在其他触发规则，请先由系统管理员整理配置" => {
                "The timer has additional trigger rules; ask a system administrator to review them"
            }
            "定时器存在多重触发规则或缺少检索间隔，请检查 systemctl cat usage-report-scan.timer" => {
                "Multiple timer triggers or missing scan interval; check systemctl cat usage-report-scan.timer"
            }
            "按上次扫描完成时间计时；调度精度另有约 1 分钟浮动。" => {
                "The interval starts when the previous scan finishes; scheduling accuracy adds about 1 minute."
            }
            "配置锁不可用" => "Settings lock is unavailable",
            "配置未变化，无需重载定时器。" => {
                "Settings unchanged; the timer was not restarted."
            }
            "定时器生效值不符，请检查其他 override" => {
                "Effective timer values do not match; check other overrides"
            }
            "已保存并启用；下次执行时间已重新计算，到期任务可能立即开始。" => {
                "Saved and enabled; the next run was recalculated. An overdue scan may start immediately."
            }
            "已保存并关闭自动检索；正在运行的扫描不会中断，仍可手动更新。" => {
                "Saved and disabled automatic scans. Running scans continue; manual rebuilds remain available."
            }
            "动态目录遍历；非文件系统快照" => {
                "Live directory traversal; not a filesystem snapshot"
            }
            "非特权受限演示，可能缺失不可访问目录" => {
                "Unprivileged demo; inaccessible directories may be missing"
            }
            "前次扫描被中断；未发布" => "Previous scan interrupted; not published",
            "该索引版本已过期或尚未完成，请按 r 读取最新索引" => {
                "This index generation expired or is incomplete; press r to load the latest index"
            }
            "尚无已完成索引，请先运行扫描任务" => {
                "No completed index; run a scan first"
            }
            "该条目不是目录" => "This entry is not a directory",
            "该目录不包含目标 UID 的已知条目" => {
                "This directory has no known entries for the requested UID"
            }
            "无效的目录层级" => "Invalid directory hierarchy",
            "打开索引数据库" => "Open index database",
            "无法识别数据库文件系统" => "Could not identify the database filesystem",
            "WAL 数据库不能存放在 NFS 上" => "The WAL database cannot be stored on NFS",
            "数据库必须是当前服务属主拥有的普通文件" => {
                "The database must be a regular file owned by the service user"
            }
            "已有扫描任务在运行" => "A scan is already running",
            "扫描起点必须是目录" => "The scan root must be a directory",
            "无法读取扫描根目录" => "Could not read the scan root",
            "清理旧索引" => "Prune old index generations",
            "完整扫描需要 sudo；演示可显式使用 --unprivileged" => {
                "Full scans require root; use --unprivileged for a limited demo"
            }
            "系统服务需要 root；演示可指定 --unprivileged" => {
                "The system service requires root; use --unprivileged for a demo"
            }
            _ => {
                if let Some(rest) = message
                    .strip_prefix("读取检索配置失败：")
                    .and_then(|s| s.strip_suffix("；请确认后台已升级"))
                {
                    return format!(
                        "Could not read scan settings: {}; make sure the backend is upgraded",
                        self.diagnostic(rest)
                    );
                }
                if let Some(source) = message.strip_prefix("配额源不是块设备：") {
                    return format!("Quota source is not a block device: {source}");
                }
                if let Some(version) = message.strip_prefix("不支持的数据库版本 ") {
                    return format!("Unsupported database version {version}");
                }
                if let Some(name) = message.strip_prefix("用户不存在：") {
                    return format!("Unknown user: {name}");
                }
                if let Some(rest) = message
                    .strip_prefix("无法连接 ")
                    .and_then(|s| s.strip_suffix("；请先启动 usage-report 服务"))
                {
                    return format!(
                        "Cannot connect to {rest}; start the usage-report service first"
                    );
                }
                for (zh, en) in [
                    (
                        "保存失败，已恢复原配置：",
                        "Save failed; previous settings restored: ",
                    ),
                    ("保存失败：", "Save failed: "),
                ] {
                    if let Some(rest) = message.strip_prefix(zh) {
                        return format!("{en}{}", self.diagnostic(rest));
                    }
                }
                if message.contains(": ") {
                    return message
                        .split(": ")
                        .map(|s| self.diagnostic(s))
                        .collect::<Vec<_>>()
                        .join(": ");
                }
                return message.into();
            }
        };
        translated.into()
    }
}

#[macro_export]
macro_rules! tr {
    ($lang:expr,$zh:literal,$en:literal $(,$arg:expr)* $(,)?) => {
        match $lang {
            $crate::i18n::Language::Chinese=>format!($zh $(,$arg)*),
            $crate::i18n::Language::English=>format!($en $(,$arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_toggle_status_and_diagnostic_boundaries() {
        let mut lang = Language::default();
        assert_eq!(lang, Language::English);
        assert_eq!(lang.status("接近软限制"), "Near soft limit");
        assert_eq!(
            lang.diagnostic(
                "读取内核用户配额（Q_GETNEXTQUOTA/Q_GETQUOTA）: No such file or directory"
            ),
            "Read kernel user quotas (Q_GETNEXTQUOTA/Q_GETQUOTA): No such file or directory"
        );
        assert_eq!(
            lang.diagnostic("/data/用户目录/硬限制.jsonl"),
            "/data/用户目录/硬限制.jsonl"
        );
        lang.toggle();
        assert_eq!(lang, Language::Chinese);
        assert_eq!(lang.status("接近软限制"), "接近软限制");
        lang.toggle();
        assert_eq!(lang, Language::English);
        let count = 42;
        assert_eq!(
            crate::tr!(lang, "{count} 用户", "{count} users"),
            "42 users"
        );
    }
}
