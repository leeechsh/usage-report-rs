use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{MetadataExt, symlink},
        net::UnixStream,
    },
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use usage_report::{db, model::*, scan, service};

fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    let database = temp.path().join("db/index.db");
    (temp, root, database)
}

#[test]
fn hardlinks_sparse_files_symlinks_and_invalid_names() {
    let (_temp, root, database) = fixture();
    fs::create_dir(root.join("a")).unwrap();
    fs::create_dir(root.join("b")).unwrap();
    fs::write(root.join("a/数据😀"), vec![1_u8; 8192]).unwrap();
    fs::hard_link(root.join("a/数据😀"), root.join("b/alias")).unwrap();
    let file = fs::File::create(root.join("sparse")).unwrap();
    file.set_len(1 << 30).unwrap();
    symlink("/", root.join("never-follow")).unwrap();
    use std::os::unix::ffi::OsStringExt;
    let invalid = root.join(std::ffi::OsString::from_vec(vec![b'a', 0xff, b'\n', 0x1b]));
    fs::write(&invalid, b"z").unwrap();
    let run = scan::scan(&database, &root, true).unwrap();
    assert_eq!(run.errors, 0);
    let conn = db::open(&database, false).unwrap();
    let uid = unsafe { libc::geteuid() };
    let page = db::tree(&conn, uid, None, None, 0, 200).unwrap();
    let expected: [&std::path::Path; 7] = [
        &root,
        &root.join("a"),
        &root.join("b"),
        &root.join("a/数据😀"),
        &root.join("sparse"),
        &root.join("never-follow"),
        &invalid,
    ];
    let allocated: i64 = expected
        .iter()
        .map(|p| fs::symlink_metadata(p).unwrap().blocks() as i64 * 512)
        .sum();
    assert_eq!(page.totals.allocated, allocated);
    assert_eq!(page.totals.inodes, 7);
    assert_eq!(page.totals.paths, 8);
    assert!(page.totals.apparent > 1 << 29);
    assert!(
        page.entries
            .iter()
            .any(|e| e.name.contains("\\n\\u{1b}") && !e.name.contains('\u{1b}'))
    );
    let mut aliases = 0;
    for dir in page.entries.iter().filter(|e| e.directory) {
        for entry in db::tree(&conn, uid, None, Some(dir.id), 0, 200)
            .unwrap()
            .entries
        {
            if entry.alias {
                aliases += 1;
                assert_eq!(entry.allocated, 0);
            }
        }
    }
    assert_eq!(aliases, 1);
}

#[test]
fn generations_publish_atomically_and_only_last_two_remain() {
    let (_temp, root, database) = fixture();
    fs::write(root.join("first"), b"a").unwrap();
    let first = scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, true).unwrap();
    conn.execute(
        "INSERT INTO runs(root,started,state,privileged) VALUES('/bad',1,'building',0)",
        [],
    )
    .unwrap();
    assert_eq!(db::latest_run(&conn, false).unwrap().unwrap().id, first.id);
    drop(conn);
    let second = scan::scan(&database, &root, true).unwrap();
    let third = scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, false).unwrap();
    assert_eq!(db::latest_run(&conn, false).unwrap().unwrap().id, third.id);
    assert!(db::run_by_id(&conn, second.id).is_ok());
    assert!(db::run_by_id(&conn, first.id).is_err());
}

#[test]
fn mixed_uid_navigation_and_pagination() {
    let (_temp, root, database) = fixture();
    fs::write(root.join("file"), b"a").unwrap();
    let run = scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, true).unwrap();
    let uid = unsafe { libc::geteuid() };
    let other = uid + 1000;
    // Simulate ownership variants without requiring root/chown in the test runner.
    conn.execute(
        "UPDATE nodes SET uid=?1 WHERE gen=?2 AND id=1",
        rusqlite::params![other, run.id],
    )
    .unwrap();
    let page = db::tree(&conn, uid, None, None, 0, 1).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].owner, uid);
    let empty = db::tree(&conn, other, None, None, 0, 200).unwrap();
    assert!(empty.entries.is_empty());
    let unowned_file = page.entries[0].id;
    assert!(db::tree(&conn, uid, None, Some(unowned_file), 0, 200).is_err());
}

#[test]
fn authorization_uses_peer_uid_and_explicit_admins() {
    let request = Request::Tree {
        uid: 1004,
        generation: None,
        node: None,
        offset: 0,
        limit: 200,
    };
    assert!(service::authorize(&request, 1004, &[]).is_ok());
    assert!(service::authorize(&request, 1005, &[]).is_err());
    assert!(service::authorize(&request, 1005, &[1005]).is_ok());
    assert!(service::authorize(&request, 0, &[]).is_ok());
    assert!(service::authorize(&Request::Overview, 1005, &[]).is_ok());
    assert!(service::authorize(&Request::Refresh, 1005, &[]).is_err());
    assert!(service::authorize(&Request::Refresh, 1005, &[1005]).is_ok());
    let update = Request::UpdateSettings {
        patch: ScanSettingsPatch {
            interval_secs: Some(3600),
            ..Default::default()
        },
    };
    assert!(service::authorize(&Request::Settings, 1005, &[]).is_ok());
    assert!(service::authorize(&update, 1005, &[]).is_err());
    assert!(service::authorize(&update, 1005, &[1005]).is_ok());
    assert!(service::authorize(&update, 0, &[]).is_ok());
}

#[test]
fn quota_status_keeps_legacy_priority_and_thresholds() {
    let q = Quota {
        soft: 80,
        hard: 100,
        used: 80,
        ..Default::default()
    };
    assert_eq!(q.status(), "超过软限制");
    assert_eq!(
        Quota {
            used: 64,
            ..q.clone()
        }
        .status(),
        "接近软限制"
    );
    assert_eq!(
        Quota {
            used: 100,
            ..q.clone()
        }
        .status(),
        "达到硬限制"
    );
    assert_eq!(
        Quota {
            used: 63,
            ..q.clone()
        }
        .status(),
        "正常"
    );
    assert_eq!(
        Quota {
            soft: 200,
            hard: 100,
            used: 80,
            ..q
        }
        .status(),
        "接近硬限制"
    );
}

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn real_socket_queries_read_database_without_rescanning() {
    let (_temp, root, database) = fixture();
    fs::write(root.join("cached"), b"data").unwrap();
    scan::scan(&database, &root, true).unwrap();
    let socket = database.parent().unwrap().join("report.sock");
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_usage-report"))
            .arg("serve")
            .arg("--unprivileged")
            .arg("--db")
            .arg(&database)
            .arg("--root")
            .arg(&root)
            .arg("--socket")
            .arg(&socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !socket.exists() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(root.join("cached")).unwrap();
    let uid = unsafe { libc::geteuid() };
    let query = Request::Tree {
        uid,
        generation: None,
        node: None,
        offset: 0,
        limit: 200,
    };
    let Payload::Tree(page) = service::request(&socket, &query).unwrap() else {
        panic!("wrong response")
    };
    assert!(page.entries.iter().any(|e| e.name == "cached"));
    let update = Request::UpdateSettings {
        patch: ScanSettingsPatch {
            interval_secs: Some(3600),
            ..Default::default()
        },
    };
    let error = service::request(&socket, &update).unwrap_err().to_string();
    assert!(error.contains("只有管理员") || error.contains("演示服务不能修改"));
    if uid != 0 {
        let denied = Request::Tree {
            uid: uid + 1,
            generation: None,
            node: None,
            offset: 0,
            limit: 200,
        };
        assert!(service::request(&socket, &denied).is_err());
        assert!(service::request(&socket, &Request::Refresh).is_err());
    }
    let cli = Command::new(env!("CARGO_BIN_EXE_usage-report"))
        .arg(uid.to_string())
        .arg("--socket")
        .arg(&socket)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(value["type"], "tree");
    for (language, expected) in [("en", "Allocated"), ("zh", "占用")] {
        let plain = Command::new(env!("CARGO_BIN_EXE_usage-report"))
            .arg(uid.to_string())
            .args(["--plain", "--lang", language, "--socket"])
            .arg(&socket)
            .output()
            .unwrap();
        assert!(plain.status.success());
        let output = String::from_utf8(plain.stdout).unwrap();
        assert!(output.contains(expected) && output.contains("cached"));
        let localized_json = Command::new(env!("CARGO_BIN_EXE_usage-report"))
            .arg(uid.to_string())
            .args(["--json", "--lang", language, "--socket"])
            .arg(&socket)
            .output()
            .unwrap();
        assert!(localized_json.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&localized_json.stdout).unwrap(),
            value
        );
    }
    for flag in ["--progress", "--status"] {
        let output = Command::new(env!("CARGO_BIN_EXE_usage-report"))
            .args([flag, "--plain", "--socket"])
            .arg(&socket)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Last completed"));
    }
    let mut watcher = Server(
        Command::new(env!("CARGO_BIN_EXE_usage-report"))
            .args(["--watch", "--json", "--socket"])
            .arg(&socket)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut reader = std::io::BufReader::new(watcher.0.stdout.take().unwrap());
    let mut progress = String::new();
    reader.read_line(&mut progress).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&progress).unwrap()["latest"].is_object());
    unsafe {
        libc::kill(watcher.0.id() as i32, libc::SIGINT);
    }
    assert!(watcher.0.wait().unwrap().success());
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream.write_all(b"not-json\n").unwrap();
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut line)
        .unwrap();
    assert!(
        serde_json::from_str::<Response>(&line)
            .unwrap()
            .error
            .is_some()
    );
}

#[test]
fn large_directories_are_paged_with_stable_order() {
    let (_temp, root, database) = fixture();
    for n in 0..215 {
        fs::write(root.join(format!("file-{n:04}")), b"data").unwrap();
    }
    scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, false).unwrap();
    let uid = unsafe { libc::geteuid() };
    let first = db::tree(&conn, uid, None, None, 0, 200).unwrap();
    let second = db::tree(&conn, uid, Some(first.run.id), None, 200, 200).unwrap();
    assert_eq!(first.entries.len(), 200);
    assert!(first.more);
    assert_eq!(second.entries.len(), 15);
    assert!(!second.more);
    assert!(
        first
            .entries
            .iter()
            .all(|a| second.entries.iter().all(|b| a.id != b.id))
    );
    assert_eq!(
        db::tree(&conn, uid, None, None, 0, 1000)
            .unwrap()
            .entries
            .len(),
        215
    );
}

#[test]
fn database_symlinks_and_concurrent_scans_are_rejected() {
    let (_temp, root, database) = fixture();
    scan::scan(&database, &root, true).unwrap();
    let link = database.with_extension("symlink");
    symlink(&database, &link).unwrap();
    assert!(db::open(&link, true).is_err());
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("{}.scan.lock", database.display()))
        .unwrap();
    use std::os::fd::AsRawFd;
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert!(
        scan::scan(&database, &root, true)
            .unwrap_err()
            .to_string()
            .contains("已有扫描")
    );
}

#[test]
fn schema_one_upgrade_keeps_existing_index_and_is_idempotent() {
    let (_temp, root, database) = fixture();
    fs::write(root.join("retained"), b"old data").unwrap();
    let original = scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, true).unwrap();
    conn.execute_batch("ALTER TABLE runs DROP COLUMN updated; ALTER TABLE runs DROP COLUMN current_path; PRAGMA user_version=1;").unwrap();
    // Old server records still deserialize with progress defaults.
    let legacy = serde_json::json!({"id":1,"root":"/","started":1,"finished":2,"state":"ready","entries":1,"errors":0,"vanished":0,"skipped":0,"root_node":1,"privileged":true,"message":""});
    let old: Run = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.updated, 0);
    db::initialize(&conn).unwrap();
    db::initialize(&conn).unwrap();
    let retained = db::latest_run(&conn, false).unwrap().unwrap();
    assert_eq!(retained.id, original.id);
    assert_eq!(retained.entries, original.entries);
    assert_eq!(retained.updated, 0);
    let tree = db::tree(&conn, unsafe { libc::geteuid() }, None, None, 0, 200).unwrap();
    assert_eq!(tree.entries[0].name, "retained");
}

#[test]
fn published_progress_survives_checkpoints_and_respects_privacy() {
    let (_temp, root, database) = fixture();
    scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, true).unwrap();
    conn.execute("INSERT INTO runs(root,started,state,privileged,entries,updated,current_path) VALUES('/',?1,'building',1,2000,?2,'/home/private')", rusqlite::params![now()-20,now()-2]).unwrap();
    let reader = db::open(&database, false).unwrap();
    let o = db::overview(&reader).unwrap();
    assert_eq!(o.scanning.as_ref().unwrap().entries, 2000);
    assert!(usage_report::tui::progress_text(&o).contains("2000 entries"));
    assert!(usage_report::tui::progress_text(&o).contains("entries/s"));
    for mut payload in [Payload::Overview(o.clone()), Payload::Status(o)] {
        service::redact_progress(&mut payload, 12345, &[]);
        assert!(
            !serde_json::to_string(&payload)
                .unwrap()
                .contains("/home/private")
        );
    }
    let mut admin = Payload::Status(db::overview(&reader).unwrap());
    service::redact_progress(&mut admin, 1004, &[1004]);
    assert!(
        serde_json::to_string(&admin)
            .unwrap()
            .contains("/home/private")
    );
    let mut t = db::tree(&reader, unsafe { libc::geteuid() }, None, None, 0, 200).unwrap();
    t.run.current_path = "/home/private".into();
    let mut tree = Payload::Tree(t);
    service::redact_progress(&mut tree, 12345, &[]);
    assert!(
        !serde_json::to_string(&tree)
            .unwrap()
            .contains("/home/private")
    );
    assert!(usage_report::tui::progress_text(&db::overview(&reader).unwrap()).contains("building"));
}

#[test]
fn scan_records_final_progress_and_uncertain_heartbeat_is_explicit() {
    let (_temp, root, database) = fixture();
    for n in 0..2100 {
        fs::write(root.join(format!("{n}")), b"").unwrap();
    }
    let r = scan::scan(&database, &root, true).unwrap();
    assert_eq!(r.entries, 2101);
    assert_eq!(r.updated, r.finished);
    assert!(r.current_path.is_empty());
    let mut o = Overview {
        scanning: Some(Run {
            started: now() - 40,
            updated: now() - 20,
            state: "building".into(),
            ..Default::default()
        }),
        latest: Some(r),
        ..Default::default()
    };
    assert!(usage_report::tui::progress_text(&o).contains("possibly waiting"));
    o.scanning = None;
    let text = usage_report::tui::progress_text(&o);
    assert!(text.contains("No index is building"));
    assert!(text.contains("2101 entries"));
}

#[test]
fn live_scan_exposes_progress_while_previous_generation_remains_readable() {
    let (_temp, root, database) = fixture();
    fs::write(root.join("original"), b"data").unwrap();
    let previous = scan::scan(&database, &root, true).unwrap();
    for n in 0..30_000 {
        fs::hard_link(root.join("original"), root.join(format!("alias-{n}"))).unwrap();
    }
    let reader = db::open(&database, false).unwrap();
    let scan_db = database.clone();
    let scan_root = root.clone();
    let worker = std::thread::spawn(move || scan::scan(&scan_db, &scan_root, true).unwrap());
    let mut observed = false;
    let start = Instant::now();
    while !worker.is_finished() && start.elapsed() < Duration::from_secs(20) {
        let overview = db::overview(&reader).unwrap();
        if let Some(r) = overview.scanning.filter(|r| r.entries >= 2000) {
            assert!(r.updated >= r.started);
            assert!(!r.current_path.is_empty());
            assert_eq!(overview.latest.unwrap().id, previous.id);
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let completed = worker.join().unwrap();
    assert!(observed, "未观察到扫描过程中已提交的进度");
    assert_eq!(completed.entries, 30_002);
    assert_eq!(
        db::latest_run(&reader, false).unwrap().unwrap().id,
        completed.id
    );
}
