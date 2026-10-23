use crate::{db, model::*};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use std::{ffi::CString, os::unix::fs::FileTypeExt, path::Path};

#[repr(C)]
#[derive(Default)]
struct NextQuota {
    hard: u64,
    soft: u64,
    space: u64,
    ihard: u64,
    isoft: u64,
    inodes: u64,
    btime: u64,
    itime: u64,
    valid: u32,
    id: u32,
}

fn read_quotas(source: &str) -> Result<Vec<Quota>> {
    ensure!(
        std::fs::metadata(source)
            .context("读取配额设备")?
            .file_type()
            .is_block_device(),
        "配额源不是块设备：{source}"
    );
    let special = CString::new(source)?;
    let privileged = unsafe { libc::geteuid() } == 0;
    if privileged {
        // Check quota activation separately: ESRCH can also mean quotas are disabled.
        #[repr(C)]
        #[derive(Default)]
        struct QuotaInfo {
            bgrace: u64,
            igrace: u64,
            flags: u32,
            valid: u32,
        }
        let mut info = QuotaInfo::default();
        let code = unsafe {
            libc::quotactl(
                (0x800005_u32 << 8) as libc::c_int,
                special.as_ptr(),
                0,
                (&mut info as *mut QuotaInfo).cast(),
            )
        };
        if code != 0 {
            return Err(std::io::Error::last_os_error())
                .context("检查内核用户配额是否启用（Q_GETINFO）");
        }
    }
    collect_quotas(privileged, unsafe { libc::geteuid() }, |id, info| {
        let operation = if privileged {
            0x800009_u32
        } else {
            0x800007_u32
        };
        let code = unsafe {
            libc::quotactl(
                (operation << 8) as libc::c_int,
                special.as_ptr(),
                id as libc::c_int,
                (info as *mut NextQuota).cast(),
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    })
}

fn collect_quotas(
    privileged: bool,
    own_uid: u32,
    mut read: impl FnMut(u32, &mut NextQuota) -> std::io::Result<()>,
) -> Result<Vec<Quota>> {
    let mut result = Vec::new();
    let mut cursor = 0_u32;
    loop {
        let mut info = NextQuota::default();
        let id = if privileged { cursor } else { own_uid };
        if let Err(error) = read(id, &mut info) {
            if privileged && matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) {
                break;
            }
            return Err(error).context("读取内核用户配额（Q_GETNEXTQUOTA/Q_GETQUOTA）");
        }
        let uid = if privileged { info.id } else { id };
        if info.soft > 0 || info.hard > 0 {
            result.push(Quota {
                uid,
                name: user_name(uid),
                used: i64::try_from(info.space)?,
                soft: i64::try_from(info.soft.checked_mul(1024).context("配额溢出")?)?,
                hard: i64::try_from(info.hard.checked_mul(1024).context("配额溢出")?)?,
                grace: i64::try_from(info.btime)?,
                indexed: None,
            });
        }
        if !privileged || uid == u32::MAX {
            break;
        }
        ensure!(uid >= cursor, "内核配额枚举未向前推进");
        cursor = uid + 1;
    }
    Ok(result)
}

pub fn refresh(conn: &mut Connection, root: &Path) -> Result<()> {
    let output = std::process::Command::new("/usr/bin/findmnt")
        .args(["-J", "-T"])
        .arg(root)
        .args(["-o", "SOURCE,FSTYPE,TARGET"])
        .output()?;
    ensure!(output.status.success(), "findmnt 无法识别文件系统");
    let mount: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let fs = &mount["filesystems"][0];
    let source = fs["source"].as_str().context("无文件系统设备")?;
    let target = fs["target"].as_str().context("无文件系统挂载点")?;
    let source = source.split('[').next().unwrap_or(source);
    let mut vfs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let croot = CString::new(std::os::unix::ffi::OsStrExt::as_bytes(root.as_os_str()))?;
    ensure!(
        unsafe { libc::statvfs(croot.as_ptr(), vfs.as_mut_ptr()) } == 0,
        "statvfs 失败"
    );
    let vfs = unsafe { vfs.assume_init() };
    let quotas = read_quotas(source);
    let transaction = conn.transaction()?;
    db::set_meta(&transaction, "root", target)?;
    db::set_meta(&transaction, "source", source)?;
    db::set_meta(
        &transaction,
        "filesystem",
        fs["fstype"].as_str().unwrap_or(""),
    )?;
    db::set_meta(
        &transaction,
        "capacity",
        vfs.f_blocks.saturating_mul(vfs.f_frsize),
    )?;
    db::set_meta(
        &transaction,
        "used",
        (vfs.f_blocks - vfs.f_bfree).saturating_mul(vfs.f_frsize),
    )?;
    db::set_meta(
        &transaction,
        "available",
        vfs.f_bavail.saturating_mul(vfs.f_frsize),
    )?;
    match quotas {
        Ok(quotas) => {
            transaction.execute("DELETE FROM quotas", [])?;
            for q in quotas {
                transaction.execute(
                    "INSERT INTO quotas VALUES(?1,?2,?3,?4,?5)",
                    params![q.uid, q.used, q.soft, q.hard, q.grace],
                )?;
            }
            db::set_meta(&transaction, "quota_updated", now())?;
            db::set_meta(
                &transaction,
                "quota_error",
                if unsafe { libc::geteuid() } != 0 {
                    "受限演示：只能读取服务属主自己的配额"
                } else {
                    ""
                },
            )?;
        }
        Err(error) => {
            db::set_meta(&transaction, "quota_error", format!("{error:#}"))?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
