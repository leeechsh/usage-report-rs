use crate::{i18n::Language, model::*, service};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, widgets::TableState};
#[path = "ui.rs"]
mod ui;
use crate::schedule;
use std::{
    io::{self, IsTerminal},
    path::Path,
    time::{Duration, Instant},
};
pub use ui::{draw, draw_localized};

#[path = "text.rs"]
mod text;
use text::{age, limit, percent};
pub use text::{
    overview_text, overview_text_in, progress_text, progress_text_in, settings_text,
    settings_text_in, tree_text, tree_text_in,
};
struct TerminalGuard;
struct ConfigForm {
    settings: ScanSettings,
    values: [String; 3],
    enabled: bool,
    field: usize,
    message: String,
    failed: bool,
}

impl ConfigForm {
    fn new(settings: ScanSettings) -> Self {
        Self {
            values: [
                schedule::format_duration(settings.interval_secs),
                schedule::format_duration(settings.boot_delay_secs),
                schedule::format_duration(settings.jitter_secs),
            ],
            enabled: settings.enabled,
            settings,
            field: 0,
            message: String::new(),
            failed: false,
        }
    }
    fn patch(&self) -> Result<ScanSettingsPatch> {
        let patch = ScanSettingsPatch {
            interval_secs: Some(schedule::parse_duration(&self.values[0])?),
            boot_delay_secs: Some(schedule::parse_duration(&self.values[1])?),
            jitter_secs: Some(schedule::parse_duration(&self.values[2])?),
            enabled: Some(self.enabled),
        };
        self.settings.patched(&patch)?;
        Ok(patch)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn get_tree(
    socket: &Path,
    uid: u32,
    generation: Option<i64>,
    node: Option<i64>,
    offset: u32,
) -> Result<TreePage> {
    match service::request(
        socket,
        &Request::Tree {
            uid,
            generation,
            node,
            offset,
            limit: 200,
        },
    )? {
        Payload::Tree(t) => Ok(t),
        _ => anyhow::bail!("无效的树响应"),
    }
}

pub fn run(socket: &Path, uid: Option<u32>, show_progress: bool, mut lang: Language) -> Result<()> {
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "非交互终端请使用 --plain 或 --json"
    );
    let mut overview = match service::request(socket, &Request::Overview)? {
        Payload::Overview(o) => o,
        _ => anyhow::bail!("无效的总览响应"),
    };
    let mut tree = uid
        .map(|uid| get_tree(socket, uid, None, None, 0))
        .transpose()?;
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut state = TableState::default().with_selected(Some(0));
    let mut error = String::new();
    let mut last_refresh = Instant::now();
    let mut progress = show_progress;
    let mut config: Option<ConfigForm> = None;
    loop {
        if last_refresh.elapsed() >= Duration::from_secs(if progress { 1 } else { 5 }) {
            match service::request(socket, &Request::Overview) {
                Ok(Payload::Overview(o)) => overview = o,
                Err(e) => error = e.to_string(),
                _ => {}
            }
            last_refresh = Instant::now();
        }
        terminal.draw(|frame| {
            draw_localized(
                frame,
                &overview,
                tree.as_ref(),
                progress,
                &mut state,
                &error,
                lang,
            );
            if let Some(form) = &config {
                ui::draw_settings(frame, form, lang);
            }
        })?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        if key.code == KeyCode::Char('l')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            lang.toggle();
            continue;
        }
        if let Some(form) = &mut config {
            let size = terminal.size()?;
            if size.width < 60 || size.height < 22 {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    config = None;
                }
                continue;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    config = None;
                }
                KeyCode::Down | KeyCode::Tab => {
                    form.field = (form.field + 1) % 4;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    form.field = (form.field + 3) % 4;
                }
                KeyCode::Enter if form.settings.can_edit => {
                    let result = form.patch().and_then(|patch| {
                        service::request(socket, &Request::UpdateSettings { patch })
                    });
                    match result {
                        Ok(Payload::Settings(settings)) => {
                            let message = settings.message.clone();
                            *form = ConfigForm::new(settings);
                            form.message = message;
                        }
                        Ok(_) => {
                            form.message = "无效的配置响应".into();
                            form.failed = true;
                        }
                        Err(e) => {
                            form.message = format!("{e:#}");
                            form.failed = true;
                        }
                    }
                }
                _ if !form.settings.can_edit => {}
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 3 => {
                    form.enabled = !form.enabled;
                }
                KeyCode::Backspace if form.field < 3 => {
                    form.values[form.field].pop();
                }
                KeyCode::Char(ch)
                    if form.field < 3
                        && (ch.is_ascii_alphanumeric() || ch == ' ')
                        && form.values[form.field].len() < 32 =>
                {
                    form.values[form.field].push(ch);
                }
                _ => {}
            }
            continue;
        }
        let length = tree
            .as_ref()
            .map(|t| t.entries.len())
            .unwrap_or(overview.quotas.len());
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('q') => break,
            KeyCode::Char('c') => match service::request(socket, &Request::Settings) {
                Ok(Payload::Settings(settings)) => {
                    config = Some(ConfigForm::new(settings));
                    error.clear();
                }
                Ok(_) => error = "服务不支持检索配置，请升级后台服务".into(),
                Err(e) => error = format!("读取检索配置失败：{e:#}；请确认后台已升级"),
            },
            KeyCode::Char('p') => {
                progress = !progress;
                last_refresh = Instant::now() - Duration::from_secs(5);
            }
            KeyCode::Esc if progress => {
                progress = false;
                tree = None;
                state.select(Some(0));
                error.clear();
            }
            KeyCode::Char('r') if progress => {
                last_refresh = Instant::now() - Duration::from_secs(5);
            }
            _ if progress => {}
            KeyCode::Down | KeyCode::Char('j') => state.select(Some(
                (state.selected().unwrap_or(0) + 1).min(length.saturating_sub(1)),
            )),
            KeyCode::Up | KeyCode::Char('k') => {
                state.select(Some(state.selected().unwrap_or(0).saturating_sub(1)))
            }
            KeyCode::Esc => {
                tree = None;
                state.select(Some(0));
                error.clear();
            }
            KeyCode::Enter | KeyCode::Right => {
                let selected = state.selected().unwrap_or(0);
                let next = if let Some(t) = &tree {
                    t.entries
                        .get(selected)
                        .filter(|e| e.directory)
                        .map(|e| (t.uid, Some(t.run.id), Some(e.id), 0))
                } else {
                    overview
                        .quotas
                        .get(selected)
                        .map(|q| (q.uid, None, None, 0))
                };
                if let Some((uid, generation, node, offset)) = next {
                    match get_tree(socket, uid, generation, node, offset) {
                        Ok(t) => {
                            tree = Some(t);
                            state.select(Some(0));
                            error.clear();
                        }
                        Err(e) => error = e.to_string(),
                    }
                }
            }
            KeyCode::Left | KeyCode::Backspace => {
                if let Some(t) = &tree {
                    if let Some(parent) = t.parent {
                        match get_tree(socket, t.uid, Some(t.run.id), Some(parent), 0) {
                            Ok(t) => {
                                tree = Some(t);
                                state.select(Some(0));
                                error.clear();
                            }
                            Err(e) => error = e.to_string(),
                        }
                    } else {
                        tree = None;
                        state.select(Some(0));
                    }
                }
            }
            KeyCode::PageDown | KeyCode::PageUp => {
                if let Some(t) = &tree {
                    let offset = if key.code == KeyCode::PageDown && t.more {
                        t.offset + 200
                    } else if key.code == KeyCode::PageUp {
                        t.offset.saturating_sub(200)
                    } else {
                        t.offset
                    };
                    match get_tree(socket, t.uid, Some(t.run.id), Some(t.node), offset) {
                        Ok(t) => {
                            tree = Some(t);
                            state.select(Some(0));
                            error.clear();
                        }
                        Err(e) => error = e.to_string(),
                    }
                }
            }
            KeyCode::Char('r') => {
                if let Some(t) = &tree {
                    match get_tree(socket, t.uid, None, None, 0) {
                        Ok(t) => {
                            tree = Some(t);
                            state.select(Some(0));
                            error.clear();
                        }
                        Err(e) => error = e.to_string(),
                    }
                }
                match service::request(socket, &Request::Overview) {
                    Ok(Payload::Overview(o)) => overview = o,
                    Err(e) => error = e.to_string(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
