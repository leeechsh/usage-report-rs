use anyhow::{Result, ensure};
use clap::{ArgGroup, CommandFactory, FromArgMatches, Parser};
use std::{io::IsTerminal, path::PathBuf};
use usage_report::{db, i18n::Language, model::*, quota, scan, schedule, service, tr, tui};

#[derive(Parser)]
#[command(
    version,
    author,
    about = "Linux user quotas and cached per-UID disk usage (read-only file browser)"
    ,group(ArgGroup::new("scan_config").args(["config","scan_interval","boot_delay","scan_jitter","auto_scan"]).multiple(true).conflicts_with_all(["user","watch","refresh","progress","status"]))
)]
struct Args {
    /// Username/UID; admin commands: serve, scan, quota-refresh
    user: Option<String>,
    /// Interface language; press l in the TUI to switch English/Chinese
    #[arg(long,value_enum,default_value_t=Language::English)]
    lang: Language,
    #[arg(long, default_value = "/var/lib/usage-report/index.db")]
    db: PathBuf,
    #[arg(long, default_value = "/run/usage-report/report.sock")]
    socket: PathBuf,
    #[arg(long, default_value = "/")]
    root: PathBuf,
    /// Plain text output; enabled automatically outside a terminal
    #[arg(long)]
    plain: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    status: bool,
    /// Open index progress; use --plain for a single text report
    #[arg(long, conflicts_with = "refresh")]
    progress: bool,
    /// Print progress every second (supports --json); Ctrl-C to stop
    #[arg(long, conflicts_with = "refresh")]
    watch: bool,
    /// Administrator: request a background index rebuild
    #[arg(long)]
    refresh: bool,
    /// Show scan settings; administrators may update them with the options below
    #[arg(long)]
    config: bool,
    /// Administrator: interval after the previous scan, e.g. 30m, 3h, 1d (1m-30d)
    #[arg(long)]
    scan_interval: Option<String>,
    /// Administrator: initial scan delay after boot, e.g. 5m (0-1d)
    #[arg(long)]
    boot_delay: Option<String>,
    /// Administrator: randomized scan delay, e.g. 1m (0-1h)
    #[arg(long)]
    scan_jitter: Option<String>,
    /// Administrator: enable/disable automatic scans; running scans continue
    #[arg(long,value_parser=["on","off"])]
    auto_scan: Option<String>,
    /// Explicitly allow a limited unprivileged demo
    #[arg(long)]
    unprivileged: bool,
    /// Administrator allowed to query every UID and edit scan settings (serve only)
    #[arg(long)]
    admin_uid: Vec<String>,
    #[arg(long, default_value_t = 30)]
    quota_interval: u64,
}

fn main() -> Result<()> {
    let args = parse_args();
    let lang = args.lang;
    execute(args).map_err(|error| anyhow::anyhow!("{}", lang.diagnostic(&format!("{error:#}"))))
}

fn parse_args() -> Args {
    let raw = std::env::args_os().collect::<Vec<_>>();
    let chinese = raw.iter().any(|s| s == "--lang=zh")
        || raw.windows(2).any(|v| v[0] == "--lang" && v[1] == "zh");
    let mut cmd = Args::command();
    if chinese {
        cmd = cmd.about("Linux 用户配额总览与按 UID 缓存占用树（文件只读）");
        for (id, help) in [
            ("user", "用户名/UID；管理命令：serve、scan、quota-refresh"),
            ("lang", "界面语言；TUI 中按 l 切换中英文"),
            ("plain", "纯文本输出，非终端时默认启用"),
            ("progress", "查看索引进度；--plain 单次输出"),
            ("watch", "每秒输出进度，可配合 --json；Ctrl-C 退出"),
            ("refresh", "管理员请求后台重建索引，不等待完成"),
            ("config", "查看检索配置；管理员可结合以下参数修改"),
            (
                "scan_interval",
                "管理员设置扫描完成后的间隔，例如 30m、3h、1d（1 分钟至 30 天）",
            ),
            (
                "boot_delay",
                "管理员设置开机后首次扫描延迟，例如 5m（0 至 1 天）",
            ),
            ("scan_jitter", "管理员设置随机延迟，例如 1m（0 至 1 小时）"),
            (
                "auto_scan",
                "管理员开启/关闭自动扫描；关闭不影响正在运行的扫描",
            ),
            ("unprivileged", "显式允许非特权演示，不代表全盘扫描"),
            (
                "admin_uid",
                "可查询所有 UID 和修改检索配置的管理员（仅服务端）",
            ),
        ] {
            cmd = cmd.mut_arg(id, |arg| arg.help(help));
        }
    }
    Args::from_arg_matches(&cmd.get_matches_from(raw)).unwrap_or_else(|error| error.exit())
}

fn execute(args: Args) -> Result<()> {
    let lang = args.lang;
    unsafe {
        libc::umask(0o077);
    }
    if args.config
        || args.scan_interval.is_some()
        || args.boot_delay.is_some()
        || args.scan_jitter.is_some()
        || args.auto_scan.is_some()
    {
        let patch = ScanSettingsPatch {
            interval_secs: args
                .scan_interval
                .as_deref()
                .map(schedule::parse_duration)
                .transpose()?,
            boot_delay_secs: args
                .boot_delay
                .as_deref()
                .map(schedule::parse_duration)
                .transpose()?,
            jitter_secs: args
                .scan_jitter
                .as_deref()
                .map(schedule::parse_duration)
                .transpose()?,
            enabled: args.auto_scan.as_deref().map(|s| s == "on"),
        };
        let request = if patch == ScanSettingsPatch::default() {
            Request::Settings
        } else {
            Request::UpdateSettings { patch }
        };
        let response = service::request(&args.socket, &request)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else if let Payload::Settings(settings) = response {
            tui::settings_text_in(&settings, lang);
        } else {
            anyhow::bail!("服务返回无效的检索配置");
        }
        return Ok(());
    }
    match args.user.as_deref() {
        Some("scan") => {
            let run = scan::scan(&args.db, &args.root, args.unprivileged)?;
            println!("{}", serde_json::to_string_pretty(&run)?);
            return Ok(());
        }
        Some("serve") => {
            let admins = args
                .admin_uid
                .iter()
                .map(|s| resolve_uid(s))
                .collect::<Result<Vec<_>>>()?;
            return service::serve(
                &args.db,
                &args.socket,
                &args.root,
                admins,
                args.unprivileged,
                args.quota_interval,
            );
        }
        Some("quota-refresh") => {
            ensure!(
                unsafe { libc::geteuid() } == 0 || args.unprivileged,
                "需要 root 权限"
            );
            let mut conn = db::open(&args.db, true)?;
            db::initialize(&conn)?;
            quota::refresh(&mut conn, &args.root)?;
            let overview = db::overview(&conn)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&overview)?);
            } else {
                tui::overview_text_in(&overview, lang);
            }
            return Ok(());
        }
        _ => {}
    }
    let uid = args.user.as_deref().map(resolve_uid).transpose()?;
    if args.watch {
        ensure!(uid.is_none(), "--watch 查看全局索引进度，不接受用户名");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = stop.clone();
        ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            if let Payload::Status(o) = service::request(&args.socket, &Request::Status)? {
                if args.json {
                    println!("{}", serde_json::to_string(&o)?);
                } else {
                    println!("[{}]\n{}", now(), tui::progress_text_in(&o, lang));
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        return Ok(());
    }
    if args.json || args.plain || args.status || args.refresh || !std::io::stdout().is_terminal() {
        let request = if args.refresh {
            Request::Refresh
        } else if args.status || args.progress {
            Request::Status
        } else if let Some(uid) = uid {
            Request::Tree {
                uid,
                generation: None,
                node: None,
                offset: 0,
                limit: 200,
            }
        } else {
            Request::Overview
        };
        let response = service::request(&args.socket, &request)?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            match response {
                Payload::Overview(o) => tui::overview_text_in(&o, lang),
                Payload::Tree(t) => tui::tree_text_in(&t, lang),
                Payload::Status(o) => {
                    println!("{}", tui::progress_text_in(&o, lang));
                    if args.status {
                        println!(
                            "{}",
                            tr!(
                                lang,
                                "配额更新：{}；错误：{}",
                                "Quota updated: {}; error: {}",
                                o.quota_updated,
                                lang.diagnostic(&o.quota_error)
                            )
                        );
                        println!(
                            "{}: {}",
                            lang.text("最近完成", "Last completed"),
                            serde_json::to_string_pretty(&o.latest)?
                        );
                        println!(
                            "{}: {}",
                            lang.text("正在构建", "Building"),
                            serde_json::to_string_pretty(&o.scanning)?
                        );
                    }
                }
                Payload::Message(message) => println!("{}", lang.diagnostic(&message)),
                Payload::Settings(settings) => tui::settings_text_in(&settings, lang),
            }
        }
        Ok(())
    } else {
        tui::run(&args.socket, uid, args.progress, lang)
    }
}
