//! Render the actual TUI widgets from the live read-only service to reviewable files.
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
    widgets::TableState,
};
use std::{fmt::Write as _, path::Path};
use usage_report::{i18n::Language, model::*, service, tui};

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn rgb(c: Color, default: &str) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#000000".into(),
        Color::White => "#ffffff".into(),
        _ => default.into(),
    }
}

fn save(
    out: &Path,
    name: &str,
    size: (u16, u16),
    o: &Overview,
    tree: Option<&TreePage>,
    progress: bool,
    lang: Language,
) -> Result<()> {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1))?;
    let mut state = TableState::default().with_selected(Some(0));
    terminal.draw(|frame| tui::draw_localized(frame, o, tree, progress, &mut state, "", lang))?;
    let buffer = terminal.backend().buffer();
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#0a1018\"/>",
        size.0 as u32 * 10,
        size.1 as u32 * 20,
        size.0 as u32 * 10,
        size.1 as u32 * 20
    );
    let mut text = String::new();
    for y in 0..size.1 {
        for x in 0..size.0 {
            let cell = &buffer[(x, y)];
            // A wide character occupies the following buffer cell on real terminals.
            let background =
                if x > 0 && ratatui::text::Span::raw(buffer[(x - 1, y)].symbol()).width() == 2 {
                    buffer[(x - 1, y)].bg
                } else {
                    cell.bg
                };
            let bg = rgb(background, "#0a1018");
            write!(
                svg,
                "<rect x=\"{}\" y=\"{}\" width=\"10\" height=\"20\" fill=\"{bg}\"/>",
                x as u32 * 10,
                y as u32 * 20
            )?;
        }
        for x in 0..size.0 {
            let cell = &buffer[(x, y)];
            text.push_str(cell.symbol());
            if cell.symbol().trim().is_empty() {
                continue;
            }
            write!(
                svg,
                "<text x=\"{}\" y=\"{}\" fill=\"{}\" font-family=\"DejaVu Sans Mono, Noto Sans CJK SC, monospace\" font-size=\"16\" font-weight=\"{}\">{}</text>",
                x as u32 * 10,
                y as u32 * 20 + 15,
                rgb(cell.fg, "#dce6f0"),
                if cell.modifier.contains(Modifier::BOLD) {
                    "bold"
                } else {
                    "normal"
                },
                escape(cell.symbol())
            )?;
        }
        text.push('\n');
    }
    svg.push_str("</svg>");
    std::fs::write(out.join(format!("{name}.svg")), svg)?;
    std::fs::write(out.join(format!("{name}.txt")), text)?;
    println!("{name}: {}×{} rendered", size.0, size.1);
    Ok(())
}

fn main() -> Result<()> {
    let output = std::env::args().nth(1).unwrap_or("var/ui-preview".into());
    let out = Path::new(&output);
    let lang = if std::env::args().nth(2).as_deref() == Some("zh") {
        Language::Chinese
    } else {
        Language::English
    };
    std::fs::create_dir_all(out)?;
    let socket = Path::new("/run/usage-report/report.sock");
    let Payload::Overview(o) = service::request(socket, &Request::Overview)? else {
        anyhow::bail!("overview expected")
    };
    for (name, size) in [
        ("overview-wide", (176, 62)),
        ("overview-medium", (120, 40)),
        ("overview-compact", (80, 30)),
        ("overview-narrow", (40, 24)),
        ("small-terminal", (24, 10)),
    ] {
        save(out, name, size, &o, None, false, lang)?;
    }
    let Payload::Tree(tree) = service::request(
        socket,
        &Request::Tree {
            uid: unsafe { libc::geteuid() },
            generation: None,
            node: None,
            offset: 0,
            limit: 200,
        },
    )?
    else {
        anyhow::bail!("tree expected")
    };
    save(out, "tree", (120, 40), &o, Some(&tree), false, lang)?;
    save(out, "progress", (120, 32), &o, None, true, lang)?;
    let mut active = o.clone();
    active.scanning = Some(Run {
        id: 2,
        root: "/".into(),
        started: now() - 40,
        updated: now() - 1,
        entries: 6_240_000,
        state: "building".into(),
        current_path: "/home/演示用户/数据".into(),
        ..Default::default()
    });
    save(out, "progress-demo", (120, 32), &active, None, true, lang)?;
    Ok(())
}
