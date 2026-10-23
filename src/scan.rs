use crate::{db, model::*};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::{CStr, CString, OsString},
    fs::{self, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::OpenOptionsExt,
        },
    },
    path::{Path, PathBuf},
    time::Instant,
};

struct Directory(*mut libc::DIR);
struct ScanLock(fs::File);
impl Drop for ScanLock {
    fn drop(&mut self) {
        // Explicit unlock also releases a temporarily fork-inherited file description.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}
impl Directory {
    fn open(parent: i32, name: &[u8], expected: &libc::stat) -> io::Result<Self> {
        let name = CString::new(name)?;
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut actual = unsafe { std::mem::zeroed::<libc::stat>() };
        let valid = unsafe { libc::fstat(fd, &mut actual) } == 0
            && actual.st_dev == expected.st_dev
            && actual.st_ino == expected.st_ino;
        if !valid {
            unsafe { libc::close(fd) };
            return Err(io::Error::from_raw_os_error(libc::ESTALE));
        }
        let dir = unsafe { libc::fdopendir(fd) };
        if dir.is_null() {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        Ok(Self(dir))
    }
    fn fd(&self) -> i32 {
        unsafe { libc::dirfd(self.0) }
    }
    fn next(&self) -> io::Result<Option<Vec<u8>>> {
        loop {
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(self.0) };
            if entry.is_null() {
                let e = io::Error::last_os_error();
                return if e.raw_os_error().unwrap_or(0) == 0 {
                    Ok(None)
                } else {
                    Err(e)
                };
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                return Ok(Some(name.to_vec()));
            }
        }
    }
    fn metadata(&self, name: &[u8]) -> io::Result<libc::stat> {
        let name = CString::new(name)?;
        let mut st = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe { libc::fstatat(self.fd(), name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(st)
    }
}

fn unescape(value: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'\\'
            && i + 3 < value.len()
            && value[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            output
                .push((value[i + 1] - b'0') * 64 + (value[i + 2] - b'0') * 8 + value[i + 3] - b'0');
            i += 4;
        } else {
            output.push(value[i]);
            i += 1;
        }
    }
    output
}

pub fn mountpoints(root: &Path) -> Result<HashSet<PathBuf>> {
    let contents = fs::read("/proc/self/mountinfo")?;
    Ok(contents
        .split(|b| *b == b'\n')
        .filter_map(|line| line.split(|b| *b == b' ').nth(4))
        .map(|raw| PathBuf::from(OsString::from_vec(unescape(raw))))
        .filter(|p| p != root && p.starts_with(root))
        .collect())
}

struct Scanner<'a> {
    conn: &'a Connection,
    run: Run,
    mounts: HashSet<PathBuf>,
    excludes: HashSet<PathBuf>,
    device: u64,
    next: i64,
    pending: usize,
    last_log: Instant,
    last_checkpoint: Instant,
    transaction_open: bool,
}
impl Scanner<'_> {
    fn checkpoint(&mut self, reopen: bool) -> Result<()> {
        self.run.updated = now();
        self.conn.execute(
            "UPDATE runs SET entries=?1,errors=?2,vanished=?3,skipped=?4,updated=?6,current_path=?7 WHERE id=?5",
            params![
                self.run.entries,
                self.run.errors,
                self.run.vanished,
                self.run.skipped,
                self.run.id,
                self.run.updated,
                self.run.current_path
            ],
        )?;
        self.conn.execute_batch("COMMIT")?;
        self.transaction_open = false;
        self.pending = 0;
        self.last_checkpoint = Instant::now();
        if self.last_log.elapsed().as_secs() >= 3 {
            eprintln!(
                "索引 #{}：{} 项，耗时 {} 秒，读取错误 {}，消失 {}，目录 {}",
                self.run.id,
                self.run.entries,
                now() - self.run.started,
                self.run.errors,
                self.run.vanished,
                self.run.current_path
            );
            self.last_log = Instant::now();
        }
        if reopen {
            self.conn.execute_batch("BEGIN IMMEDIATE")?;
            self.transaction_open = true;
        }
        Ok(())
    }
    fn error(&mut self, path: &Path, error: io::Error) {
        if error.raw_os_error() == Some(libc::ENOENT) {
            self.run.vanished += 1;
        } else {
            self.run.errors += 1;
        }
        if self.run.errors + self.run.vanished <= 10 {
            eprintln!("跳过 {}：{}", safe_name(path.as_os_str().as_bytes()), error);
        }
    }
    fn usage(&self, uid: u32, node: i64, parent: Option<i64>, total: &Totals) -> Result<()> {
        self.conn
            .prepare_cached("INSERT INTO usage VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?
            .execute(params![
                self.run.id,
                uid,
                node,
                parent,
                total.allocated,
                total.apparent,
                total.paths,
                total.inodes
            ])?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        path: &Path,
        name: &[u8],
        parentfd: i32,
        parent: Option<i64>,
        st: libc::stat,
        depth: usize,
    ) -> Result<BTreeMap<u32, Totals>> {
        self.run.entries += 1;
        if st.st_dev != self.device || self.excludes.contains(path) {
            self.run.skipped += 1;
            return Ok(BTreeMap::new());
        }
        let directory = st.st_mode & libc::S_IFMT == libc::S_IFDIR;
        let id = self.next;
        self.next += 1;
        let dev = i64::try_from(st.st_dev)?;
        let ino = i64::try_from(st.st_ino)?;
        let inserted = self
            .conn
            .prepare_cached("INSERT OR IGNORE INTO inodes VALUES(?1,?2,?3,?4,?5)")?
            .execute(params![self.run.id, dev, ino, st.st_uid, id])?
            != 0;
        let uid = if inserted {
            st.st_uid
        } else {
            self.conn
                .prepare_cached("SELECT uid FROM inodes WHERE gen=?1 AND dev=?2 AND ino=?3")?
                .query_row(params![self.run.id, dev, ino], |r| r.get(0))?
        };
        if directory && !inserted {
            self.run.skipped += 1;
            return Ok(BTreeMap::new());
        }
        let total = Totals {
            allocated: if inserted {
                st.st_blocks.max(0).saturating_mul(512)
            } else {
                0
            },
            apparent: if inserted { st.st_size.max(0) } else { 0 },
            paths: 1,
            inodes: i64::from(inserted),
        };
        let kind = if directory {
            1
        } else if st.st_mode & libc::S_IFMT == libc::S_IFREG {
            0
        } else {
            2
        };
        self.conn
            .prepare_cached("INSERT INTO nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0)")?
            .execute(params![
                self.run.id,
                id,
                parent,
                name,
                uid,
                kind,
                total.allocated,
                total.apparent,
                !inserted
            ])?;
        let mut totals = BTreeMap::from([(uid, total)]);
        self.pending += 1;
        if self.pending >= 2000 || self.last_checkpoint.elapsed().as_secs() >= 1 {
            self.run.current_path = safe_name(
                if directory {
                    path
                } else {
                    path.parent().unwrap_or(path)
                }
                .as_os_str()
                .as_bytes(),
            );
            self.checkpoint(true)?;
        }
        if directory {
            let mut failed = false;
            if depth > 256 {
                self.error(path, io::Error::from_raw_os_error(libc::ELOOP));
                failed = true;
            } else {
                match Directory::open(parentfd, name, &st) {
                    Err(error) => {
                        self.error(path, error);
                        failed = true;
                    }
                    Ok(dir) => loop {
                        if self.last_checkpoint.elapsed().as_secs() >= 1 {
                            self.run.current_path = safe_name(path.as_os_str().as_bytes());
                            self.checkpoint(true)?;
                        }
                        let child = match dir.next() {
                            Ok(Some(name)) => name,
                            Ok(None) => break,
                            Err(error) => {
                                self.error(path, error);
                                failed = true;
                                break;
                            }
                        };
                        let childpath = path.join(OsString::from_vec(child.clone()));
                        // Crucially, do not stat known NFS mounts before pruning them.
                        if self.mounts.contains(&childpath) || self.excludes.contains(&childpath) {
                            self.run.skipped += 1;
                            continue;
                        }
                        let metadata = match dir.metadata(&child) {
                            Ok(st) => st,
                            Err(error) => {
                                self.error(&childpath, error);
                                failed = true;
                                continue;
                            }
                        };
                        for (uid, child_total) in
                            self.walk(&childpath, &child, dir.fd(), Some(id), metadata, depth + 1)?
                        {
                            totals.entry(uid).or_default().add(&child_total);
                        }
                    },
                }
            }
            if failed {
                self.conn.execute(
                    "UPDATE nodes SET error=1 WHERE gen=?1 AND id=?2",
                    params![self.run.id, id],
                )?;
            }
        }
        for (&uid, total) in &totals {
            self.usage(uid, id, parent, total)?;
        }
        Ok(totals)
    }
}

pub fn scan(database: &Path, root: &Path, unprivileged: bool) -> Result<Run> {
    let privileged = unsafe { libc::geteuid() } == 0;
    ensure!(
        privileged || unprivileged,
        "完整扫描需要 sudo；演示可显式使用 --unprivileged"
    );
    let root = root.canonicalize()?;
    ensure!(root.is_dir(), "扫描起点必须是目录");
    if let Some(parent) = database.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = PathBuf::from(format!("{}.scan.lock", database.display()));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock_path)?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "已有扫描任务在运行"
    );
    let _scan_lock = ScanLock(lock);
    let conn = db::open(database, true)?;
    db::initialize(&conn)?;
    conn.execute(
        "UPDATE runs SET state='failed',message='前次扫描被中断；未发布' WHERE state='building'",
        [],
    )?;
    conn.execute(
        "INSERT INTO runs(root,started,state,privileged,updated) VALUES(?1,?2,'building',?3,?2)",
        params![root.to_string_lossy(), now(), privileged],
    )?;
    let id = conn.last_insert_rowid();
    let croot = CString::new(root.as_os_str().as_bytes())?;
    let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
    ensure!(
        unsafe { libc::lstat(croot.as_ptr(), &mut metadata) } == 0,
        "无法读取扫描根目录"
    );
    let database = database.canonicalize()?;
    let mut excludes = HashSet::from([database.clone(), lock_path.canonicalize()?]);
    for suffix in ["-wal", "-shm"] {
        excludes.insert(PathBuf::from(format!("{}{suffix}", database.display())));
    }
    let run = Run {
        id,
        root: root.to_string_lossy().into_owned(),
        started: now(),
        privileged,
        state: "building".into(),
        root_node: 1,
        current_path: safe_name(root.as_os_str().as_bytes()),
        ..Default::default()
    };
    let mut scanner = Scanner {
        conn: &conn,
        run,
        mounts: mountpoints(&root)?,
        excludes,
        device: metadata.st_dev,
        next: 1,
        pending: 0,
        last_log: Instant::now(),
        last_checkpoint: Instant::now(),
        transaction_open: true,
    };
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = scanner.walk(
        &root,
        root.as_os_str().as_bytes(),
        libc::AT_FDCWD,
        None,
        metadata,
        0,
    );
    if let Err(error) = result {
        if scanner.transaction_open {
            let _ = conn.execute_batch("ROLLBACK");
        }
        conn.execute(
            "UPDATE runs SET state='failed',finished=?1,message=?2 WHERE id=?3",
            params![now(), format!("{error:#}"), id],
        )?;
        return Err(error);
    }
    scanner.checkpoint(false)?;
    conn.execute(
        "UPDATE runs SET state='ready',finished=?1,updated=?1,current_path='',message=?2 WHERE id=?3",
        params![
            now(),
            if privileged {
                "动态目录遍历；非文件系统快照"
            } else {
                "非特权受限演示，可能缺失不可访问目录"
            },
            id
        ],
    )?;
    let run = db::run_by_id(&conn, id)?;
    db::prune(&conn).context("清理旧索引")?;
    eprintln!(
        "索引 #{} 已发布：{} 项，耗时 {} 秒，错误 {}，消失 {}",
        id,
        run.entries,
        run.finished - run.started,
        run.errors,
        run.vanished
    );
    Ok(run)
}
