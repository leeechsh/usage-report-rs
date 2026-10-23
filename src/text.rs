use crate::{i18n::Language, model::*, schedule, tr};

pub fn overview_text(o: &Overview) {
    overview_text_in(o, Language::English);
}
pub fn overview_text_in(o: &Overview, lang: Language) {
    println!(
        "{}",
        tr!(
            lang,
            "用户磁盘配额报告\n挂载点：{}  设备：{}  文件系统：{}",
            "User disk quota report\nMount: {}  Device: {}  Filesystem: {}",
            o.root,
            o.source,
            o.filesystem
        )
    );
    println!(
        "{}",
        tr!(
            lang,
            "文件系统容量：{} / {}，可用 {}",
            "Filesystem: {} / {}, available {}",
            human(o.used),
            human(o.capacity),
            human(o.available)
        )
    );
    println!(
        "{}",
        tr!(
            lang,
            "配额更新：{}  索引：{}",
            "Quota updated: {}  Index: {}",
            age(o.quota_updated, lang),
            o.latest
                .as_ref()
                .map(|r| format!("#{} {}", r.id, age(r.finished, lang)))
                .unwrap_or(lang.text("尚无索引", "No index").into())
        )
    );
    if !o.quota_error.is_empty() {
        println!(
            "{}: {}",
            lang.text("配额提示", "Quota notice"),
            lang.diagnostic(&o.quota_error)
        );
    }
    println!(
        "{:<20} {:>13} {:>13} {:>13} {:>13} {:>8} {:>8}  {}",
        lang.text("用户", "User"),
        lang.text("已用", "Used"),
        lang.text("软限制", "Soft limit"),
        lang.text("硬限制", "Hard limit"),
        lang.text("硬限剩余", "Hard remain"),
        lang.text("软占比", "Soft %"),
        lang.text("硬占比", "Hard %"),
        lang.text("状态", "Status")
    );
    for q in &o.quotas {
        println!(
            "{:<20} {:>13} {:>13} {:>13} {:>13} {:>8} {:>8}  {}",
            q.name,
            human(q.used),
            limit(q.soft),
            limit(q.hard),
            if q.hard > 0 {
                human((q.hard - q.used).max(0))
            } else {
                "-".into()
            },
            percent(q.used, q.soft),
            percent(q.used, q.hard),
            lang.status(q.status())
        );
    }
    println!(
        "\n{}",
        tr!(
            lang,
            "有配额用户：{}；总占用：{}",
            "Users with quotas: {}; total used: {}",
            o.quotas.len(),
            human(o.quotas.iter().map(|q| q.used).sum())
        )
    );
    for status in [
        "正常",
        "接近软限制",
        "接近硬限制",
        "超过软限制",
        "达到硬限制",
    ] {
        println!(
            "{}: {}",
            lang.status(status),
            o.quotas.iter().filter(|q| q.status() == status).count()
        );
    }
}

pub fn tree_text(t: &TreePage) {
    tree_text_in(t, Language::English);
}
pub fn tree_text_in(t: &TreePage, lang: Language) {
    println!(
        "{}",
        tr!(
            lang,
            "{} / UID {}；索引 #{}，{}；{}",
            "{} / UID {}; index #{}, {}; {}",
            t.user,
            t.uid,
            t.run.id,
            age(t.run.finished, lang),
            t.path
        )
    );
    println!(
        "{}",
        tr!(
            lang,
            "占用 {}；逻辑大小 {}；{} 个路径；{} 个唯一 inode",
            "Allocated {}; apparent {}; {} paths; {} unique inodes",
            human(t.totals.allocated),
            human(t.totals.apparent),
            t.totals.paths,
            t.totals.inodes
        )
    );
    println!(
        "{}",
        tr!(
            lang,
            "扫描错误 {}，消失 {}；{}",
            "Scan errors {}, vanished {}; {}",
            t.run.errors,
            t.run.vanished,
            lang.diagnostic(&t.run.message)
        )
    );
    for e in &t.entries {
        println!(
            "{:>13}  {}{}{}{}",
            human(e.allocated),
            if e.directory { "/" } else { "" },
            e.name,
            if e.alias {
                lang.text(" [硬链接引用]", " [hard link]")
            } else {
                ""
            },
            if e.error {
                lang.text(" [读取不完整]", " [incomplete]")
            } else {
                ""
            }
        );
    }
    if t.more {
        println!(
            "{}",
            lang.text(
                "（仅显示本页；交互界面 PageDown 可翻页）",
                "(Only this page is shown; use PageDown in the TUI for more.)"
            )
        );
    }
}

pub fn progress_text(o: &Overview) -> String {
    progress_text_in(o, Language::English)
}
pub fn progress_text_in(o: &Overview, lang: Language) -> String {
    let mut lines = Vec::new();
    if let Some(r) = &o.scanning {
        let elapsed = (now() - r.started).max(0);
        let measured = (if r.updated > 0 { r.updated } else { now() } - r.started).max(1);
        lines.push(tr!(
            lang,
            "索引 #{} 构建中 | 根目录 {}",
            "Index #{} building | Root {}",
            r.id,
            r.root
        ));
        lines.push(tr!(
            lang,
            "已处理 {} 项 | 耗时 {} 秒 | 平均 {:.0} 项/秒",
            "Processed {} entries | Elapsed {} s | Average {:.0} entries/s",
            r.entries,
            elapsed,
            r.entries as f64 / measured as f64
        ));
        lines.push(tr!(
            lang,
            "读取错误 {} | 消失 {} | 跳过 {}",
            "Read errors {} | Vanished {} | Skipped {}",
            r.errors,
            r.vanished,
            r.skipped
        ));
        if r.updated > 0 {
            lines.push(tr!(
                lang,
                "进度更新：{}",
                "Progress updated: {}",
                age(r.updated, lang)
            ));
            if now() - r.updated > 10 {
                lines.push(lang.text("超过 10 秒未更新：可能正在等待 I/O、数据库或扫描已中断；请检查扫描服务。","No update for over 10 s: possibly waiting for I/O or the database, or the scan was interrupted. Check the scan service.").into());
            }
        } else {
            lines.push(lang.text("旧扫描器未提供更新时间；下次扫描将记录完整进度。","The old scanner did not report update times; the next scan will record full progress.").into());
        }
        if !r.current_path.is_empty() {
            lines.push(tr!(
                lang,
                "当前目录：{}",
                "Current directory: {}",
                r.current_path
            ));
        }
        lines.push(lang.text("扫描期间目录会变化，不提供未经校准的百分比或剩余时间。","Directories may change during scanning; no estimated percentage or remaining time is shown.").into());
    } else {
        lines.push(
            lang.text(
                "当前无构建中的索引；浏览直接读取已完成缓存。",
                "No index is building; browsing reads the completed cache.",
            )
            .into(),
        );
    }
    if let Some(r) = &o.latest {
        lines.push(tr!(
            lang,
            "最近完成：#{} | {} 项 | 耗时 {} 秒 | {}",
            "Last completed: #{} | {} entries | Elapsed {} s | {}",
            r.id,
            r.entries,
            (r.finished - r.started).max(0),
            age(r.finished, lang)
        ));
        lines.push(tr!(
            lang,
            "完成时读取错误 {} | 消失 {} | 跳过 {}",
            "Completed with read errors {} | Vanished {} | Skipped {}",
            r.errors,
            r.vanished,
            r.skipped
        ));
    } else {
        lines.push(
            lang.text("尚无已完成索引。", "No completed index yet.")
                .into(),
        );
    }
    lines.join("\n")
}

pub fn settings_text(s: &ScanSettings) {
    settings_text_in(s, Language::English);
}
pub fn settings_text_in(s: &ScanSettings, lang: Language) {
    println!(
        "{}",
        tr!(
            lang,
            "自动检索配置\n自动检索：{}；定时器：{}\n检索间隔：{}（上次扫描结束后）\n开机延迟：{}\n随机延迟：{}\n下次执行：{}\n修改权限：{}\n{}",
            "Scan settings\nAutomatic scans: {}; timer: {}\nInterval: {} (after the previous scan finishes)\nBoot delay: {}\nJitter: {}\nNext run: {}\nAccess: {}\n{}",
            if s.enabled {
                lang.text("开启", "On")
            } else {
                lang.text("关闭", "Off")
            },
            if s.active {
                lang.text("运行中", "Active")
            } else {
                lang.text("未运行", "Inactive")
            },
            schedule::format_duration(s.interval_secs),
            schedule::format_duration(s.boot_delay_secs),
            schedule::format_duration(s.jitter_secs),
            lang.diagnostic(&schedule::next_run_text(s.next_run)),
            if s.can_edit {
                lang.text("管理员可修改", "Administrator: editable")
            } else {
                lang.text("只读", "Read only")
            },
            lang.diagnostic(&s.message)
        )
    );
}

pub(super) fn age(timestamp: i64, lang: Language) -> String {
    if timestamp == 0 {
        return lang.text("尚无数据", "N/A").into();
    }
    let seconds = (now() - timestamp).max(0);
    if seconds < 60 {
        tr!(lang, "{seconds} 秒前", "{seconds}s ago")
    } else if seconds < 3600 {
        tr!(lang, "{} 分钟前", "{}m ago", seconds / 60)
    } else if seconds < 86400 {
        tr!(lang, "{} 小时前", "{}h ago", seconds / 3600)
    } else {
        tr!(lang, "{} 天前", "{}d ago", seconds / 86400)
    }
}
pub(super) fn limit(value: i64) -> String {
    if value > 0 { human(value) } else { "-".into() }
}
pub(super) fn percent(value: i64, limit: i64) -> String {
    if limit > 0 {
        format!("{:.1}%", value as f64 / limit as f64 * 100.)
    } else {
        "-".into()
    }
}
