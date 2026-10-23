use crate::model::*;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::OpenOptions,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::Path,
    time::Duration,
};

pub fn open(path: &Path, write: bool) -> Result<Connection> {
    if write {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let parent = std::ffi::CString::new(parent.as_os_str().as_bytes())?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        ensure!(
            unsafe { libc::statfs(parent.as_ptr(), stat.as_mut_ptr()) } == 0,
            "无法识别数据库文件系统"
        );
        ensure!(
            unsafe { stat.assume_init() }.f_type != 0x6969,
            "WAL 数据库不能存放在 NFS 上"
        );
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let info = file.metadata()?;
        ensure!(
            info.is_file() && info.uid() == unsafe { libc::geteuid() },
            "数据库必须是当前服务属主拥有的普通文件"
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let conn = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .context("打开索引数据库")?;
    conn.busy_timeout(Duration::from_secs(10))?;
    if write {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-65536; PRAGMA temp_store=FILE;")?;
    }
    Ok(conn)
}

pub fn initialize(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!((0..=2).contains(&version), "不支持的数据库版本 {version}");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs(
          id INTEGER PRIMARY KEY, root TEXT NOT NULL, started INTEGER NOT NULL,
          finished INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL,
          entries INTEGER NOT NULL DEFAULT 0, errors INTEGER NOT NULL DEFAULT 0,
          vanished INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0,
          root_node INTEGER NOT NULL DEFAULT 1, privileged INTEGER NOT NULL, message TEXT NOT NULL DEFAULT '');
         CREATE TABLE IF NOT EXISTS nodes(
          gen INTEGER NOT NULL, id INTEGER NOT NULL, parent INTEGER, name BLOB NOT NULL,
          uid INTEGER NOT NULL, kind INTEGER NOT NULL, allocated INTEGER NOT NULL, apparent INTEGER NOT NULL,
          alias INTEGER NOT NULL, error INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(gen,id)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS inodes(
          gen INTEGER NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL, uid INTEGER NOT NULL,
          primary_node INTEGER NOT NULL, PRIMARY KEY(gen,dev,ino)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS usage(
          gen INTEGER NOT NULL, uid INTEGER NOT NULL, node INTEGER NOT NULL, parent INTEGER,
          allocated INTEGER NOT NULL, apparent INTEGER NOT NULL, paths INTEGER NOT NULL, inodes INTEGER NOT NULL,
          PRIMARY KEY(gen,uid,node)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS usage_children ON usage(gen,uid,parent,allocated DESC,node);
         CREATE TABLE IF NOT EXISTS quotas(
          uid INTEGER PRIMARY KEY, used INTEGER NOT NULL, soft INTEGER NOT NULL, hard INTEGER NOT NULL, grace INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
         "
    )?;
    if version < 2 {
        // Recheck after acquiring the write lock: serve and scan may start together.
        let transaction =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let locked_version: i64 = transaction.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if locked_version < 2 {
            transaction.execute_batch(
                "
            ALTER TABLE runs ADD COLUMN updated INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE runs ADD COLUMN current_path TEXT NOT NULL DEFAULT '';
            PRAGMA user_version=2;",
            )?;
        }
        transaction.commit()?;
    }
    Ok(())
}

pub fn set_meta(conn: &Connection, key: &str, value: impl ToString) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

fn meta(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .unwrap_or_default()
}

fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        root: r.get(1)?,
        started: r.get(2)?,
        finished: r.get(3)?,
        state: r.get(4)?,
        entries: r.get(5)?,
        errors: r.get(6)?,
        vanished: r.get(7)?,
        skipped: r.get(8)?,
        root_node: r.get(9)?,
        privileged: r.get(10)?,
        message: r.get(11)?,
        updated: r.get(12).unwrap_or(0),
        current_path: r.get(13).unwrap_or_default(),
    })
}

pub fn latest_run(conn: &Connection, building: bool) -> Result<Option<Run>> {
    Ok(conn
        .query_row(
            "SELECT * FROM runs WHERE state=?1 ORDER BY id DESC LIMIT 1",
            [if building { "building" } else { "ready" }],
            run_row,
        )
        .optional()?)
}

pub fn run_by_id(conn: &Connection, id: i64) -> Result<Run> {
    conn.query_row(
        "SELECT * FROM runs WHERE id=?1 AND state='ready'",
        [id],
        run_row,
    )
    .context("该索引版本已过期或尚未完成，请按 r 读取最新索引")
}

pub fn overview(conn: &Connection) -> Result<Overview> {
    // A short read transaction keeps quota rows and timestamps from different refreshes apart.
    conn.execute_batch("BEGIN")?;
    let result = (|| {
        let latest = latest_run(conn, false)?;
        let generation = latest.as_ref().map(|r| r.id).unwrap_or(-1);
        let mut statement = conn.prepare(
            "SELECT q.uid,q.used,q.soft,q.hard,q.grace,u.allocated
             FROM quotas q LEFT JOIN usage u ON u.gen=?1 AND u.uid=q.uid AND u.node=1
             WHERE q.soft>0 OR q.hard>0 ORDER BY q.used DESC,q.uid",
        )?;
        let quotas = statement
            .query_map([generation], |r| {
                let uid = r.get::<_, u32>(0)?;
                Ok(Quota {
                    uid,
                    name: user_name(uid),
                    used: r.get(1)?,
                    soft: r.get(2)?,
                    hard: r.get(3)?,
                    grace: r.get(4)?,
                    indexed: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Overview {
            quotas,
            latest,
            scanning: latest_run(conn, true)?,
            quota_updated: meta(conn, "quota_updated").parse().unwrap_or(0),
            quota_error: meta(conn, "quota_error"),
            root: meta(conn, "root"),
            source: meta(conn, "source"),
            filesystem: meta(conn, "filesystem"),
            capacity: meta(conn, "capacity").parse().unwrap_or(0),
            used: meta(conn, "used").parse().unwrap_or(0),
            available: meta(conn, "available").parse().unwrap_or(0),
        })
    })();
    conn.execute_batch("ROLLBACK")?;
    result
}

pub fn tree(
    conn: &Connection,
    uid: u32,
    generation: Option<i64>,
    node: Option<i64>,
    offset: u32,
    limit: u32,
) -> Result<TreePage> {
    conn.execute_batch("BEGIN")?;
    let result = (|| {
        let run = match generation {
            Some(id) => run_by_id(conn, id)?,
            None => latest_run(conn, false)?.context("尚无已完成索引，请先运行扫描任务")?,
        };
        let node = node.unwrap_or(run.root_node);
        let node_info: (Option<i64>, bool) = conn.query_row(
            "SELECT parent,kind=1 FROM nodes WHERE gen=?1 AND id=?2",
            params![run.id, node],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(node_info.1, "该条目不是目录");
        let totals = conn.query_row("SELECT allocated,apparent,paths,inodes FROM usage WHERE gen=?1 AND uid=?2 AND node=?3", params![run.id,uid,node], |r| {
            Ok(Totals{allocated:r.get(0)?,apparent:r.get(1)?,paths:r.get(2)?,inodes:r.get(3)?})
        }).optional()?;
        ensure!(
            node == run.root_node || totals.is_some(),
            "该目录不包含目标 UID 的已知条目"
        );
        let limit = limit.clamp(1, 500);
        let mut statement = conn.prepare(
            "SELECT n.id,n.name,n.kind,n.uid,u.allocated,u.apparent,u.paths,n.alias,n.error
             FROM usage u JOIN nodes n ON n.gen=u.gen AND n.id=u.node
             WHERE u.gen=?1 AND u.uid=?2 AND u.parent=?3
             ORDER BY u.allocated DESC,u.node LIMIT ?4 OFFSET ?5",
        )?;
        let mut entries = statement
            .query_map(params![run.id, uid, node, limit + 1, offset], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    name: safe_name(&r.get::<_, Vec<u8>>(1)?),
                    directory: r.get::<_, i64>(2)? == 1,
                    owner: r.get(3)?,
                    allocated: r.get(4)?,
                    apparent: r.get(5)?,
                    paths: r.get(6)?,
                    alias: r.get(7)?,
                    error: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = entries.len() > limit as usize;
        entries.truncate(limit as usize);
        let mut path_parts = Vec::new();
        let mut current = Some(node);
        while let Some(id) = current {
            ensure!(path_parts.len() < 1024, "无效的目录层级");
            let (parent, name): (Option<i64>, Vec<u8>) = conn.query_row(
                "SELECT parent,name FROM nodes WHERE gen=?1 AND id=?2",
                params![run.id, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            path_parts.push(safe_name(&name));
            current = parent;
        }
        path_parts.reverse();
        let path = path_parts.join("/").replace("//", "/");
        Ok(TreePage {
            run,
            uid,
            user: user_name(uid),
            node,
            parent: node_info.0,
            path,
            totals: totals.unwrap_or_default(),
            entries,
            offset,
            more,
        })
    })();
    conn.execute_batch("ROLLBACK")?;
    result
}

pub fn prune(conn: &Connection) -> Result<()> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM runs WHERE state!='building' AND id NOT IN (SELECT id FROM runs WHERE state='ready' ORDER BY id DESC LIMIT 2)")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        for table in ["usage", "nodes", "inodes"] {
            // Bounded deletion transactions let quota refreshes interleave.
            loop {
                let sql = format!(
                    "DELETE FROM {table} WHERE gen=?1 AND ({}) IN (SELECT {} FROM {table} WHERE gen=?1 LIMIT 10000)",
                    if table == "usage" {
                        "uid,node"
                    } else if table == "nodes" {
                        "id"
                    } else {
                        "dev,ino"
                    },
                    if table == "usage" {
                        "uid,node"
                    } else if table == "nodes" {
                        "id"
                    } else {
                        "dev,ino"
                    }
                );
                if conn.execute(&sql, [id])? == 0 {
                    break;
                }
            }
        }
        conn.execute("DELETE FROM runs WHERE id=?1", [id])?;
    }
    Ok(())
}
