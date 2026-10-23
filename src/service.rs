use crate::{db, model::*, quota, schedule::ScheduleManager};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub fn authorize(request: &Request, peer: u32, admins: &[u32]) -> Result<()> {
    if matches!(request, Request::Refresh | Request::UpdateSettings { .. }) {
        ensure!(
            peer == 0 || admins.contains(&peer),
            "只有管理员可以更新索引或修改检索配置"
        );
    }
    if let Request::Tree { uid, .. } = request {
        ensure!(
            peer == 0 || peer == *uid || admins.contains(&peer),
            "权限不足：普通用户只能浏览自己的 UID"
        );
    }
    Ok(())
}

fn peer_uid(stream: &UnixStream) -> Result<u32> {
    let mut credential = unsafe { std::mem::zeroed::<libc::ucred>() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credential as *mut libc::ucred).cast(),
                &mut len,
            )
        } == 0,
        "无法识别 Unix socket 对端"
    );
    Ok(credential.uid)
}

pub fn redact_progress(payload: &mut Payload, peer: u32, admins: &[u32]) {
    if peer == 0 || admins.contains(&peer) {
        return;
    }
    match payload {
        Payload::Overview(o) | Payload::Status(o) => {
            for run in [&mut o.latest, &mut o.scanning].into_iter().flatten() {
                run.current_path.clear();
            }
        }
        Payload::Tree(t) => t.run.current_path.clear(),
        Payload::Settings(s) => s.can_edit = false,
        Payload::Message(_) => {}
    }
}

fn handle(
    mut stream: UnixStream,
    database: &Path,
    admins: &[u32],
    schedule: &ScheduleManager,
    system_mode: bool,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let result = (|| -> Result<Payload> {
        let uid = peer_uid(&stream)?;
        let mut line = Vec::new();
        BufReader::new((&stream).take(65537)).read_until(b'\n', &mut line)?;
        ensure!(
            line.len() <= 65536 && line.last() == Some(&b'\n'),
            "请求过长或未终止"
        );
        let request: Request = serde_json::from_slice(&line)?;
        authorize(&request, uid, admins)?;
        let conn = db::open(database, false)?;
        let mut payload = match request {
            Request::Settings => Payload::Settings(
                schedule.read(system_mode && (uid == 0 || admins.contains(&uid)))?,
            ),
            Request::UpdateSettings { patch } => {
                ensure!(system_mode, "演示服务不能修改系统检索配置");
                let result = schedule.update(&patch)?;
                eprintln!(
                    "管理员 UID {uid} 修改检索配置：间隔 {}s，开机延迟 {}s，随机延迟 {}s，自动 {}",
                    result.interval_secs,
                    result.boot_delay_secs,
                    result.jitter_secs,
                    result.enabled
                );
                Payload::Settings(result)
            }
            Request::Refresh => {
                ensure!(system_mode, "演示服务不能启动系统扫描任务");
                let status = std::process::Command::new("/usr/bin/systemctl")
                    .args(["start", "--no-block", "usage-report-scan.service"])
                    .status()?;
                ensure!(status.success(), "启动扫描任务失败");
                Payload::Message(
                    "已请求后台更新；当前缓存仍可浏览。usage-report --status 可查看进度。".into(),
                )
            }
            Request::Overview => Payload::Overview(db::overview(&conn)?),
            Request::Status => Payload::Status(db::overview(&conn)?),
            Request::Tree {
                uid,
                generation,
                node,
                offset,
                limit,
            } => Payload::Tree(db::tree(&conn, uid, generation, node, offset, limit)?),
        };
        redact_progress(&mut payload, uid, admins);
        Ok(payload)
    })();
    let response = match result {
        Ok(payload) => Response {
            payload: Some(payload),
            error: None,
        },
        Err(error) => Response {
            payload: None,
            error: Some(format!("{error:#}")),
        },
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn request(socket: &Path, request: &Request) -> Result<Payload> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("无法连接 {}；请先启动 usage-report 服务", socket.display()))?;
    let server = peer_uid(&stream)?;
    ensure!(
        server == 0 || server == unsafe { libc::geteuid() },
        "查询服务不属于 root 或当前用户"
    );
    stream.set_read_timeout(Some(Duration::from_secs(
        if matches!(request, Request::UpdateSettings { .. }) {
            90
        } else {
            30
        },
    )))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    let mut response = Vec::new();
    BufReader::new(stream.take(4 * 1024 * 1024 + 1)).read_until(b'\n', &mut response)?;
    ensure!(response.len() <= 4 * 1024 * 1024, "服务响应过大");
    let response: Response = serde_json::from_slice(&response)?;
    if let Some(error) = response.error {
        anyhow::bail!(error);
    }
    response.payload.context("服务返回空响应")
}

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn serve(
    database: &Path,
    socket: &Path,
    root: &Path,
    admins: Vec<u32>,
    unprivileged: bool,
    quota_interval: u64,
) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0 || unprivileged,
        "系统服务需要 root；演示可指定 --unprivileged"
    );
    if let Some(parent) = database.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut conn = db::open(database, true)?;
    db::initialize(&conn)?;
    if let Err(error) = quota::refresh(&mut conn, root) {
        db::set_meta(&conn, "quota_error", format!("{error:#}"))?;
    }
    if let Ok(meta) = fs::symlink_metadata(socket) {
        ensure!(
            meta.file_type().is_socket() && meta.uid() == unsafe { libc::geteuid() },
            "拒绝覆盖非本服务所有的 socket 路径"
        );
        ensure!(
            UnixStream::connect(socket).is_err(),
            "该 socket 已有服务在运行"
        );
        fs::remove_file(socket)?;
    }
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket)?;
    let _guard = SocketGuard(socket.to_owned());
    fs::set_permissions(socket, fs::Permissions::from_mode(0o666))?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let active = Arc::new(AtomicUsize::new(0));
    let admins = Arc::new(admins);
    let schedule = Arc::new(ScheduleManager::default());
    let system_mode = !unprivileged && unsafe { libc::geteuid() } == 0;
    let mut last_quota = Instant::now();
    eprintln!(
        "只读查询服务：{}；DB：{}",
        socket.display(),
        database.display()
    );
    while !stop.load(Ordering::Relaxed) {
        if last_quota.elapsed() >= Duration::from_secs(quota_interval.max(5)) {
            if let Err(error) = quota::refresh(&mut conn, root) {
                let _ = db::set_meta(&conn, "quota_error", format!("{error:#}"));
            }
            last_quota = Instant::now();
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if active.load(Ordering::Relaxed) >= 32 {
                    continue;
                }
                active.fetch_add(1, Ordering::Relaxed);
                let database = database.to_owned();
                let admins = admins.clone();
                let active = active.clone();
                let schedule = schedule.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &database, &admins, &schedule, system_mode);
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
