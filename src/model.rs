use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Run {
    pub id: i64,
    pub root: String,
    pub started: i64,
    pub finished: i64,
    pub state: String,
    pub entries: i64,
    pub errors: i64,
    pub vanished: i64,
    pub skipped: i64,
    pub root_node: i64,
    pub privileged: bool,
    pub message: String,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub current_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Totals {
    pub allocated: i64,
    pub apparent: i64,
    pub paths: i64,
    pub inodes: i64,
}

impl Totals {
    pub fn add(&mut self, other: &Self) {
        self.allocated += other.allocated;
        self.apparent += other.apparent;
        self.paths += other.paths;
        self.inodes += other.inodes;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Quota {
    pub uid: u32,
    pub name: String,
    pub used: i64,
    pub soft: i64,
    pub hard: i64,
    pub grace: i64,
    pub indexed: Option<i64>,
}

impl Quota {
    pub fn status(&self) -> &'static str {
        if self.hard > 0 && self.used >= self.hard {
            "达到硬限制"
        } else if self.soft > 0 && self.used >= self.soft {
            "超过软限制"
        } else if self.hard > 0 && (self.used as i128) * 100 >= (self.hard as i128) * 80 {
            "接近硬限制"
        } else if self.soft > 0 && (self.used as i128) * 100 >= (self.soft as i128) * 80 {
            "接近软限制"
        } else {
            "正常"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Overview {
    pub quotas: Vec<Quota>,
    pub quota_updated: i64,
    pub quota_error: String,
    pub root: String,
    pub source: String,
    pub filesystem: String,
    pub capacity: i64,
    pub used: i64,
    pub available: i64,
    pub latest: Option<Run>,
    pub scanning: Option<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub name: String,
    pub directory: bool,
    pub owner: u32,
    pub allocated: i64,
    pub apparent: i64,
    pub paths: i64,
    pub alias: bool,
    pub error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreePage {
    pub run: Run,
    pub uid: u32,
    pub user: String,
    pub node: i64,
    pub parent: Option<i64>,
    pub path: String,
    pub totals: Totals,
    pub entries: Vec<Entry>,
    pub offset: u32,
    pub more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Overview,
    Status,
    Refresh,
    Settings,
    UpdateSettings {
        patch: ScanSettingsPatch,
    },
    Tree {
        uid: u32,
        generation: Option<i64>,
        node: Option<i64>,
        offset: u32,
        limit: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Payload {
    Overview(Overview),
    Tree(TreePage),
    Status(Overview),
    Message(String),
    Settings(ScanSettings),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScanSettingsPatch {
    pub interval_secs: Option<u64>,
    pub boot_delay_secs: Option<u64>,
    pub jitter_secs: Option<u64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSettings {
    pub interval_secs: u64,
    pub boot_delay_secs: u64,
    pub jitter_secs: u64,
    pub enabled: bool,
    pub active: bool,
    pub next_run: Option<i64>,
    pub can_edit: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub payload: Option<Payload>,
    pub error: Option<String>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn human(bytes: i64) -> String {
    let value = bytes.max(0) as f64;
    for (unit, divisor) in [
        ("TiB", 2_f64.powi(40)),
        ("GiB", 2_f64.powi(30)),
        ("MiB", 2_f64.powi(20)),
        ("KiB", 1024.),
    ] {
        if value >= divisor {
            return format!("{:.2} {unit}", value / divisor);
        }
    }
    format!("{} B", bytes.max(0))
}

pub fn safe_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

pub fn user_name(uid: u32) -> String {
    let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 65536];
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            &mut record,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code == 0 && !result.is_null() {
        safe_name(unsafe { std::ffi::CStr::from_ptr(record.pw_name).to_bytes() })
    } else {
        uid.to_string()
    }
}

pub fn resolve_uid(value: &str) -> anyhow::Result<u32> {
    if let Ok(uid) = value.parse::<u32>() {
        return Ok(uid);
    }
    let name = std::ffi::CString::new(value)?;
    let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 65536];
    let code = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            &mut record,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    anyhow::ensure!(code == 0 && !result.is_null(), "用户不存在：{value}");
    Ok(record.pw_uid)
}
