use ratatui::{Terminal, backend::TestBackend, widgets::TableState};
use std::{fs, process::Command};
use usage_report::{db, i18n::Language, model::*, scan, tui};

fn text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        let mut skip = 0;
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let value = buffer[(x, y)].symbol();
            skip = ratatui::text::Span::raw(value).width().saturating_sub(1);
            text.push_str(value);
        }
        text.push('\n');
    }
    text
}

#[test]
fn language_rendering_keeps_entity_names_and_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("硬限制"), b"data").unwrap();
    let database = temp.path().join("index.db");
    let run = scan::scan(&database, &root, true).unwrap();
    let conn = db::open(&database, false).unwrap();
    let tree = db::tree(&conn, unsafe { libc::geteuid() }, None, None, 0, 200).unwrap();
    let mut o = Overview {
        latest: Some(run),
        capacity: 1000,
        used: 400,
        available: 500,
        quotas: vec![Quota {
            uid: 1004,
            name: "用户目录".into(),
            used: 10,
            hard: 100,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut state = TableState::default().with_selected(Some(0));
    for lang in [Language::English, Language::Chinese, Language::English] {
        terminal
            .draw(|frame| tui::draw_localized(frame, &o, None, false, &mut state, "", lang))
            .unwrap();
        let output = text(&terminal);
        assert!(output.contains("用户目录"));
        assert!(output.contains(if lang == Language::English {
            "Quota overview"
        } else {
            "配额总览"
        }));
        assert_eq!(state.selected(), Some(0));
        terminal
            .draw(|frame| tui::draw_localized(frame, &o, Some(&tree), false, &mut state, "", lang))
            .unwrap();
        let output = text(&terminal);
        assert!(output.contains("硬限制"));
        assert!(output.contains(if lang == Language::English {
            "File / directory"
        } else {
            "文件 / 目录"
        }));
    }
    o.scanning = Some(Run {
        id: 2,
        state: "building".into(),
        root: "/目录".into(),
        started: now() - 20,
        updated: now() - 1,
        entries: 2000,
        current_path: "/home/用户目录".into(),
        ..Default::default()
    });
    let en = tui::progress_text(&o);
    let zh = tui::progress_text_in(&o, Language::Chinese);
    assert!(en.contains("Index #2 building"));
    assert!(zh.contains("索引 #2 构建中"));
    assert!(en.contains("/home/用户目录") && zh.contains("/home/用户目录"));
}

#[test]
fn command_line_language_defaults_to_english_and_validates_options() {
    let binary = env!("CARGO_BIN_EXE_usage-report");
    let en = Command::new(binary)
        .arg("--help")
        .env("LANG", "zh_CN.UTF-8")
        .output()
        .unwrap();
    assert!(en.status.success());
    let en = String::from_utf8(en.stdout).unwrap();
    assert!(en.contains("Linux user quotas"));
    assert!(en.contains("[default: en]"));
    let zh = Command::new(binary)
        .args(["--lang", "zh", "--help"])
        .output()
        .unwrap();
    assert!(zh.status.success());
    assert!(String::from_utf8(zh.stdout).unwrap().contains("界面语言"));
    let invalid = Command::new(binary)
        .args(["--lang", "fr"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
}
