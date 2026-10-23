use super::*;
use std::sync::{Arc, Mutex};

const BASE: &str = "[Timer]\nOnBootSec=5min\nOnUnitInactiveSec=3h\nRandomizedDelaySec=1min\n";

#[derive(Default)]
struct State {
    enabled: bool,
    active: bool,
    calls: Vec<Vec<String>>,
    fail_once: Option<String>,
}
struct Fake {
    directory: PathBuf,
    state: Arc<Mutex<State>>,
}
impl TimerControl for Fake {
    fn run(&self, args: &[&str]) -> Result<String> {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(args.iter().map(|s| s.to_string()).collect());
        if state.fail_once.as_deref() == Some(args[0]) {
            state.fail_once = None;
            bail!("injected failure");
        }
        match args[0] {
            "cat" => Ok(format!(
                "{BASE}\n{}",
                fs::read_to_string(self.directory.join(OVERRIDE)).unwrap_or_default()
            )),
            "show" if args.contains(&"--property=UnitFileState") => {
                Ok(if state.enabled { "enabled" } else { "disabled" }.into())
            }
            "show" if args.contains(&"--property=ActiveState") => {
                Ok(if state.active { "active" } else { "inactive" }.into())
            }
            "show" => Ok("0".into()),
            "daemon-reload" => Ok(String::new()),
            "enable" => {
                state.enabled = true;
                Ok(String::new())
            }
            "disable" => {
                state.enabled = false;
                if args.contains(&"--now") {
                    state.active = false;
                }
                Ok(String::new())
            }
            "restart" => {
                state.active = true;
                Ok(String::new())
            }
            "stop" => {
                state.active = false;
                Ok(String::new())
            }
            _ => bail!("unexpected fake command"),
        }
    }
}

fn fixture() -> (tempfile::TempDir, ScheduleManager, Arc<Mutex<State>>) {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(State {
        enabled: true,
        active: true,
        ..Default::default()
    }));
    let manager = ScheduleManager {
        directory: dir.path().into(),
        control: Box::new(Fake {
            directory: dir.path().into(),
            state: state.clone(),
        }),
        lock: Mutex::new(()),
    };
    (dir, manager, state)
}

#[test]
fn durations_limits_and_effective_override_are_validated() {
    assert_eq!(parse_duration("1h30m").unwrap(), 5400);
    assert_eq!(parse_duration("30 minutes").unwrap(), 1800);
    assert_eq!(parse_duration("0s").unwrap(), 0);
    assert_eq!(effective_values(BASE).unwrap(), (10800, 300, 60));
    assert_eq!(
        effective_values(&format!(
            "{BASE}\n[Timer]\nOnBootSec=\nOnBootSec=10m\nOnUnitInactiveSec=1h"
        ))
        .unwrap(),
        (3600, 600, 60)
    );
    assert_eq!(
        effective_values(&format!("{BASE}\nOnUnitInactiveSec=\nOnUnitInactiveSec=2h")).unwrap(),
        (7200, 0, 60)
    );
    assert_eq!(
        duration_seconds("5h 21min 33.966766s").unwrap(),
        19293.966766
    );
    for bad in [
        "",
        "-1h",
        "NaN",
        "1h\n[Service]",
        "1s; reboot",
        "1month",
        "0.2s",
    ] {
        assert!(parse_duration(bad).is_err(), "{bad}");
    }
    let (_dir, manager, state) = fixture();
    let old = manager.read(false).unwrap();
    for seconds in [0, 1, 59, 30 * 86400 + 1, u64::MAX] {
        assert!(
            old.patched(&ScanSettingsPatch {
                interval_secs: Some(seconds),
                ..Default::default()
            })
            .is_err()
        );
    }
    assert!(
        old.patched(&ScanSettingsPatch {
            jitter_secs: Some(3601),
            ..Default::default()
        })
        .is_err()
    );
    state.lock().unwrap().calls.clear();
    assert!(
        manager
            .update(&ScanSettingsPatch {
                interval_secs: Some(1),
                ..Default::default()
            })
            .is_err()
    );
    assert!(!manager.directory.join(OVERRIDE).exists());
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c[0] == "daemon-reload")
    );
}

#[test]
fn save_disable_and_reenable_are_persistent_without_touching_scan_service() {
    let (_dir, manager, state) = fixture();
    let result = manager
        .update(&ScanSettingsPatch {
            interval_secs: Some(3600),
            boot_delay_secs: Some(600),
            jitter_secs: Some(10),
            enabled: Some(false),
        })
        .unwrap();
    assert_eq!(result.interval_secs, 3600);
    assert!(!result.enabled && !result.active);
    assert_eq!(
        effective_values(&format!(
            "{BASE}\n{}",
            fs::read_to_string(manager.directory.join(OVERRIDE)).unwrap()
        ))
        .unwrap(),
        (3600, 600, 10)
    );
    let result = manager
        .update(&ScanSettingsPatch {
            enabled: Some(true),
            ..Default::default()
        })
        .unwrap();
    assert!(result.enabled && result.active);
    assert_eq!(result.interval_secs, 3600);
    assert_eq!(manager.read(false).unwrap().boot_delay_secs, 600);
    assert!(!manager.read(false).unwrap().can_edit);
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .flatten()
            .any(|arg| arg == "usage-report-scan.service")
    );
}

#[test]
fn unchanged_settings_do_not_restart_or_write() {
    let (_dir, manager, state) = fixture();
    let result = manager
        .update(&ScanSettingsPatch {
            interval_secs: Some(10800),
            ..Default::default()
        })
        .unwrap();
    assert!(result.message.contains("未变化"));
    assert!(!manager.directory.join(OVERRIDE).exists());
    assert!(!state.lock().unwrap().calls.iter().any(|c| matches!(
        c[0].as_str(),
        "restart" | "enable" | "disable" | "daemon-reload"
    )));
}

#[test]
fn apply_failure_restores_previous_file_and_timer_state() {
    for existing in [false, true] {
        let (_dir, manager, state) = fixture();
        let old = b"[Timer]\nOnUnitInactiveSec=\nOnUnitInactiveSec=2h\n";
        if existing {
            fs::write(manager.directory.join(OVERRIDE), old).unwrap();
        }
        state.lock().unwrap().fail_once = Some("restart".into());
        let error = manager
            .update(&ScanSettingsPatch {
                interval_secs: Some(3600),
                ..Default::default()
            })
            .unwrap_err();
        assert!(error.to_string().contains("已恢复原配置"));
        if existing {
            assert_eq!(fs::read(manager.directory.join(OVERRIDE)).unwrap(), old);
        } else {
            assert!(!manager.directory.join(OVERRIDE).exists());
        }
        {
            let state = state.lock().unwrap();
            assert!(state.enabled && state.active);
        }
        assert_eq!(
            manager.read(false).unwrap().interval_secs,
            if existing { 7200 } else { 10800 }
        );
    }
}
