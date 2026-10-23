use super::{ConfigForm, age, limit, percent, progress_text_in};
use crate::{i18n::Language, model::*, tr};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Padding, Paragraph, Row, Table, TableState, Wrap,
    },
};

const BG: Color = Color::Rgb(10, 16, 24);
const PANEL: Color = Color::Rgb(17, 26, 38);
const ALT: Color = Color::Rgb(14, 22, 33);
const EDGE: Color = Color::Rgb(44, 62, 80);
const TEXT: Color = Color::Rgb(220, 230, 240);
const MUTED: Color = Color::Rgb(130, 150, 170);
const CYAN: Color = Color::Rgb(80, 205, 230);
const GREEN: Color = Color::Rgb(100, 215, 170);
const YELLOW: Color = Color::Rgb(240, 195, 100);
const RED: Color = Color::Rgb(250, 115, 125);
const PURPLE: Color = Color::Rgb(205, 155, 245);
const SELECTED: Color = Color::Rgb(29, 57, 75);

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().bg(PANEL).fg(TEXT))
        .padding(Padding::horizontal(1))
        .title(title.into().style(Style::default().fg(MUTED)))
}

fn bold(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn muted(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(MUTED))
}

fn quota_color(q: &Quota) -> Color {
    match q.status() {
        "达到硬限制" => RED,
        "超过软限制" => PURPLE,
        "接近硬限制" | "接近软限制" => YELLOW,
        _ => GREEN,
    }
}

fn bar(value: i64, total: i64, width: usize, color: Color) -> Line<'static> {
    let ratio = if total > 0 {
        value as f64 / total as f64
    } else {
        0.0
    };
    let used = (ratio.clamp(0.0, 1.0) * width as f64).round() as usize;
    Line::from(vec![
        Span::styled("━".repeat(used), Style::default().fg(color)),
        Span::styled("━".repeat(width - used), Style::default().fg(EDGE)),
    ])
}

fn number(value: impl Into<String>) -> Cell<'static> {
    Cell::from(Text::from(value.into()).alignment(Alignment::Right))
}

fn status_lines(o: &Overview, progress: bool, width: u16, lang: Language) -> Vec<Line<'static>> {
    let groups = if progress {
        if let Some(r) = o.scanning.as_ref().or(o.latest.as_ref()) {
            let end = if r.finished > 0 { r.finished } else { now() };
            let measured = (if r.finished > 0 {
                r.finished
            } else if r.updated > 0 {
                r.updated
            } else {
                now()
            } - r.started)
                .max(1);
            vec![
                Line::from(vec![
                    muted(if o.scanning.is_some() {
                        lang.text("扫描中 ", "Scanning ")
                    } else {
                        lang.text("已完成 ", "Completed ")
                    }),
                    bold(tr!(lang, "{} 项", "{} entries", r.entries), CYAN),
                ]),
                Line::from(vec![
                    muted(lang.text("耗时 ", "Elapsed ")),
                    bold(tr!(lang, "{} 秒", "{} s", (end - r.started).max(0)), TEXT),
                ]),
                Line::from(vec![
                    muted(lang.text("平均 ", "Average ")),
                    bold(
                        tr!(
                            lang,
                            "{:.0} 项/秒",
                            "{:.0} entries/s",
                            r.entries as f64 / measured as f64
                        ),
                        GREEN,
                    ),
                ]),
            ]
        } else {
            vec![Line::from(muted(lang.text(
                "尚无索引，等待首次扫描",
                "No index; waiting for the first scan",
            )))]
        }
    } else {
        let warnings = o.quotas.iter().filter(|q| q.status() != "正常").count();
        let accent = if o.capacity > 0 && o.used as f64 / o.capacity as f64 >= 0.9 {
            YELLOW
        } else {
            CYAN
        };
        let index = if let Some(r) = &o.scanning {
            Line::from(vec![
                bold(lang.text("扫描中 ", "Scanning "), CYAN),
                bold(tr!(lang, "{} 项", "{} entries", r.entries), TEXT),
                muted(lang.text(" · p 查看", " · p details")),
            ])
        } else if let Some(r) = &o.latest {
            Line::from(vec![
                bold(tr!(lang, "索引 #{}", "Index #{}", r.id), GREEN),
                muted(format!(" · {}", age(r.finished, lang))),
            ])
        } else {
            Line::from(muted(
                lang.text("索引尚无 · p 查看", "No index · p details"),
            ))
        };
        vec![
            Line::from(vec![
                muted(lang.text("容量 ", "Disk ")),
                bold(human(o.used), TEXT),
                muted(format!(" / {} ", human(o.capacity))),
                bold(percent(o.used, o.capacity), accent),
            ]),
            Line::from(vec![
                muted(lang.text("可用 ", "Free ")),
                bold(human(o.available), TEXT),
            ]),
            Line::from(vec![
                bold(tr!(lang, "{} 用户", "{} users", o.quotas.len()), CYAN),
                muted(" · "),
                bold(
                    tr!(lang, "{warnings} 关注", "{warnings} alerts"),
                    if warnings > 0 { YELLOW } else { GREEN },
                ),
            ]),
            index,
        ]
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line = Line::default();
    for group in groups {
        if !line.spans.is_empty() && line.width() + 3 + group.width() > width as usize {
            lines.push(line);
            line = Line::default();
        }
        if !line.spans.is_empty() {
            line.spans.push(muted(" │ "));
        }
        line.spans.extend(group.spans);
    }
    if !line.spans.is_empty() {
        lines.push(line);
    }
    lines
}

fn overview_table(
    frame: &mut Frame,
    area: Rect,
    o: &Overview,
    state: &mut TableState,
    lang: Language,
) {
    let extra = if lang == Language::English { 4 } else { 0 };
    let wide = area.width >= 112 + extra;
    let medium = area.width >= 78;
    let show_bar = medium && (lang == Language::Chinese || area.width >= 90);
    let tiny = area.width < 50 + extra;
    let soft_percent = area.width >= 130 + extra;
    let widths = if tiny {
        vec![Constraint::Min(8), Constraint::Length(12)]
    } else {
        let mut w = vec![Constraint::Min(14), Constraint::Length(12)];
        if wide {
            w.push(Constraint::Length(12));
        }
        if medium {
            w.push(Constraint::Length(12));
        }
        if wide {
            w.push(Constraint::Length(12));
        }
        if soft_percent {
            w.push(Constraint::Length(7));
        }
        if show_bar {
            w.push(Constraint::Length(12));
        }
        w.extend([Constraint::Length(7), Constraint::Length(12 + extra)]);
        w
    };
    let mut headers = vec![
        Cell::from(lang.text("用户", "User")),
        number(lang.text("已用空间", "Used")),
    ];
    if !tiny {
        if wide {
            headers.push(number(lang.text("软限制", "Soft limit")));
        }
        if medium {
            headers.push(number(lang.text("硬限制", "Hard limit")));
        }
        if wide {
            headers.push(number(lang.text("硬限剩余", "Remaining")));
        }
        if soft_percent {
            headers.push(number(lang.text("软占比", "Soft %")));
        }
        if show_bar {
            headers.push(Cell::from(lang.text("硬配额用量", "Hard quota")));
        }
        headers.extend([
            number(lang.text("硬占比", "Hard %")),
            Cell::from(lang.text("状态", "Status")),
        ]);
    }
    let rows = o.quotas.iter().enumerate().map(|(i, q)| {
        let mut cells = vec![Cell::from(q.name.clone()), number(human(q.used))];
        if !tiny {
            if wide {
                cells.push(number(limit(q.soft)));
            }
            if medium {
                cells.push(number(limit(q.hard)));
            }
            if wide {
                cells.push(number(if q.hard > 0 {
                    human((q.hard - q.used).max(0))
                } else {
                    "—".into()
                }));
            }
            if soft_percent {
                cells.push(number(percent(q.used, q.soft)));
            }
            if show_bar {
                cells.push(Cell::from(bar(q.used, q.hard, 12, quota_color(q))));
            }
            cells.push(number(percent(q.used, q.hard)).style(Style::default().fg(quota_color(q))));
            cells.push(
                Cell::from(lang.status(q.status()).to_owned())
                    .style(Style::default().fg(quota_color(q))),
            );
        }
        Row::new(cells).style(Style::default().bg(if i % 2 == 0 { PANEL } else { ALT }))
    });
    let selected = if o.quotas.is_empty() {
        0
    } else {
        state.selected().unwrap_or(0).min(o.quotas.len() - 1) + 1
    };
    let block = panel(if area.width >= 80 {
        tr!(
            lang,
            " 配额总览 · 按已用空间排序 · {selected}/{} ",
            " Quotas · sorted by used · {selected}/{} ",
            o.quotas.len()
        )
    } else {
        tr!(
            lang,
            " 用户配额 · {selected}/{} ",
            " Quotas · {selected}/{} ",
            o.quotas.len()
        )
    });
    if o.quotas.is_empty() {
        frame.render_widget(
            Paragraph::new(lang.text(
                "\n 暂无配额记录\n 请查看底部采集提示，或使用 --status 检查服务状态。",
                "\n No quota records\n Check the notice below or use --status.",
            ))
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: false })
            .block(block),
            area,
        );
        return;
    }
    frame.render_stateful_widget(
        Table::new(rows, widths)
            .column_spacing(1)
            .header(
                Row::new(headers)
                    .style(Style::default().fg(MUTED))
                    .bottom_margin(1),
            )
            .row_highlight_style(Style::default().bg(SELECTED).add_modifier(Modifier::BOLD))
            .highlight_symbol("▸ ")
            .block(block),
        area,
        state,
    );
}

fn sidebar(frame: &mut Frame, area: Rect, o: &Overview, state: &TableState, lang: Language) {
    let sections = Layout::vertical([Constraint::Length(14), Constraint::Min(5)])
        .spacing(1)
        .split(area);
    let mut lines = Vec::new();
    if let Some(q) = o.quotas.get(state.selected().unwrap_or(0)) {
        lines.extend([
            Line::default(),
            Line::from(bold(q.name.clone(), CYAN)),
            Line::from(muted(format!("UID {}", q.uid))),
            Line::default(),
            Line::from(bold(lang.status(q.status()), quota_color(q))),
            bar(q.used, q.hard, 24, quota_color(q)),
            Line::from(tr!(
                lang,
                "硬限使用 {}",
                "Hard used {}",
                percent(q.used, q.hard)
            )),
            Line::default(),
            Line::from(muted(tr!(
                lang,
                "索引占用 {}",
                "Indexed {}",
                q.indexed
                    .map(human)
                    .unwrap_or(lang.text("尚无", "N/A").into())
            ))),
            Line::from(muted(
                lang.text("Enter 浏览该用户文件树", "Enter to browse this user"),
            )),
        ]);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(lang.text(" 当前用户 ", " Selected user "))),
        sections[0],
    );
    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            muted(lang.text("配额总占用  ", "Quota total  ")),
            bold(human(o.quotas.iter().map(|q| q.used).sum()), TEXT),
        ]),
        Line::default(),
    ];
    for label in [
        "正常",
        "接近软限制",
        "接近硬限制",
        "超过软限制",
        "达到硬限制",
    ] {
        let count = o.quotas.iter().filter(|q| q.status() == label).count();
        let color = match label {
            "正常" => GREEN,
            "超过软限制" => PURPLE,
            "达到硬限制" => RED,
            _ => YELLOW,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}  ", lang.status(label)),
                Style::default().fg(color),
            ),
            bold(count.to_string(), TEXT),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).block(panel(lang.text(" 状态分布 ", " Status summary "))),
        sections[1],
    );
}

fn tree_table(frame: &mut Frame, area: Rect, t: &TreePage, state: &mut TableState, lang: Language) {
    let wide = area.width >= 95;
    let medium = area.width >= 65;
    let mut widths = vec![Constraint::Min(12), Constraint::Length(12)];
    let mut header = vec![
        Cell::from(lang.text("文件 / 目录", "File / directory")),
        number(lang.text("实际占用", "Allocated")),
    ];
    if medium {
        widths.push(Constraint::Length(17));
        header.push(Cell::from(lang.text("目录占比", "Share")));
    }
    if wide {
        widths.extend([Constraint::Length(10), Constraint::Length(10)]);
        header.extend([
            number(lang.text("路径数", "Paths")),
            number(lang.text("属主 UID", "Owner UID")),
        ]);
    }
    let rows = t.entries.iter().enumerate().map(|(i, e)| {
        let mut cells = vec![
            Cell::from(Line::from(vec![
                Span::styled(
                    if e.directory { "▸ " } else { "· " },
                    Style::default().fg(if e.directory { CYAN } else { MUTED }),
                ),
                Span::styled(
                    e.name.clone(),
                    Style::default().fg(if e.directory { CYAN } else { TEXT }),
                ),
                muted(if e.alias {
                    lang.text(" [硬链接]", " [hard link]")
                } else {
                    ""
                }),
                Span::styled(if e.error { " !" } else { "" }, Style::default().fg(RED)),
            ])),
            number(human(e.allocated)),
        ];
        if medium {
            let mut ratio = bar(e.allocated, t.totals.allocated, 10, CYAN);
            ratio.spans.push(muted(format!(
                " {:>5}",
                percent(e.allocated, t.totals.allocated)
            )));
            cells.push(Cell::from(ratio));
        }
        if wide {
            cells.extend([number(e.paths.to_string()), number(e.owner.to_string())]);
        }
        Row::new(cells).style(Style::default().bg(if i % 2 == 0 { PANEL } else { ALT }))
    });
    let end = t.offset as usize + t.entries.len();
    let block = panel(tr!(
        lang,
        " {} · UID {} · 第 {}–{} 项{} ",
        " {} · UID {} · entries {}–{}{} ",
        t.user,
        t.uid,
        if end == 0 { 0 } else { t.offset + 1 },
        end,
        if t.more {
            lang.text(" · PgDn 下一页", " · PgDn more")
        } else {
            ""
        }
    ));
    let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(bold(t.path.clone(), CYAN)),
            Line::from(muted(tr!(
                lang,
                "实际 {}  ·  逻辑 {}  ·  {} 路径  ·  {} inode",
                "Allocated {}  ·  Apparent {}  ·  {} paths  ·  {} inodes",
                human(t.totals.allocated),
                human(t.totals.apparent),
                t.totals.paths,
                t.totals.inodes
            ))),
        ])
        .wrap(Wrap { trim: false }),
        parts[0],
    );
    if t.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(lang.text("\n 此目录下没有目标 UID 的已索引条目。\n ← 返回上级，r 读取最新索引。","\n No indexed entries for this UID in this directory.\n Press Left to go back, r for the latest index."))
                .style(Style::default().fg(MUTED))
                .block(block),
            parts[1],
        );
    } else {
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .column_spacing(2)
                .header(
                    Row::new(header)
                        .style(Style::default().fg(MUTED))
                        .bottom_margin(1),
                )
                .row_highlight_style(Style::default().bg(SELECTED).add_modifier(Modifier::BOLD))
                .highlight_symbol("▸ ")
                .block(block),
            parts[1],
            state,
        );
    }
}

/// Render only cached data. This function does not query or mutate the filesystem.
pub fn draw(
    frame: &mut Frame,
    o: &Overview,
    tree: Option<&TreePage>,
    progress: bool,
    state: &mut TableState,
    error: &str,
) {
    draw_localized(frame, o, tree, progress, state, error, Language::English);
}

pub fn draw_localized(
    frame: &mut Frame,
    o: &Overview,
    tree: Option<&TreePage>,
    progress: bool,
    state: &mut TableState,
    error: &str,
    lang: Language,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        frame.area(),
    );
    let outer = frame.area();
    let width = outer.width.min(176);
    let area = Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y,
        width,
        outer.height,
    );
    let area = area.inner(ratatui::layout::Margin::new(u16::from(width >= 60), 0));
    if area.height < 16 || area.width < 32 {
        frame.render_widget(
            Paragraph::new(lang.text(
                "usage-report\n终端空间不足，请放大窗口\n或使用 --plain\nq 退出",
                "usage-report\nTerminal too small; resize\nor use --plain\nq quit · l 中文",
            ))
            .style(Style::default().fg(CYAN))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let warning = if !error.is_empty() {
        error
    } else {
        &o.quota_error
    };
    let tree_warning = tree.map(|t| {
        tr!(
            lang,
            "索引 #{} · 读取错误 {} · 消失 {}{}",
            "Index #{} · Read errors {} · Vanished {}{}",
            t.run.id,
            t.run.errors,
            t.run.vanished,
            if t.run.privileged {
                ""
            } else {
                lang.text(" · 非特权受限扫描", " · unprivileged scan")
            }
        )
    });
    let footer_height = if !warning.is_empty() { 5 } else { 3 };
    let status = status_lines(o, progress, area.width.saturating_sub(2), lang);
    let status_height = status
        .iter()
        .map(|line| {
            line.width()
                .div_ceil(area.width.saturating_sub(2).max(1) as usize)
                .max(1)
        })
        .sum::<usize>() as u16;
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(status_height),
        Constraint::Min(3),
        Constraint::Length(footer_height),
    ])
    .spacing(1)
    .split(area);
    let page = if progress {
        lang.text("索引进度", "Index progress")
    } else if tree.is_some() {
        lang.text("用户目录", "User files")
    } else {
        lang.text("配额总览", "Quota overview")
    };
    let brand = vec![bold("usage-report", CYAN), muted("  /  "), bold(page, TEXT)];
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(brand),
            Line::from(muted(format!(
                "{}  ·  {}  ·  {}",
                if o.root.is_empty() { "/" } else { &o.root },
                o.source,
                o.filesystem
            ))),
        ]),
        parts[0],
    );
    if area.width >= 100 {
        let badge = Rect::new(parts[0].right() - 28, parts[0].y, 28, 1);
        frame.render_widget(
            Paragraph::new(muted(
                lang.text("leeechsh · 文件只读", "leeechsh · files read-only"),
            ))
            .alignment(Alignment::Right),
            badge,
        );
    }
    frame.render_widget(
        Paragraph::new(status).wrap(Wrap { trim: false }).block(
            Block::default()
                .style(Style::default().bg(PANEL).fg(TEXT))
                .padding(Padding::horizontal(1)),
        ),
        parts[1],
    );
    if progress {
        let block = panel(if o.scanning.is_some() {
            lang.text(
                " 正在建立索引 · 每秒刷新 ",
                " Building index · refreshes every second ",
            )
        } else {
            lang.text(" 索引已就绪 ", " Index ready ")
        });
        frame.render_widget(
            Paragraph::new(format!("\n{}", progress_text_in(o, lang)))
                .style(Style::default().fg(TEXT))
                .wrap(Wrap { trim: false })
                .block(block),
            parts[2],
        );
    } else if let Some(t) = tree {
        tree_table(frame, parts[2], t, state, lang);
    } else if area.width >= 154 && parts[2].height >= 27 {
        let body = Layout::horizontal([Constraint::Min(80), Constraint::Length(31)])
            .spacing(1)
            .split(parts[2]);
        overview_table(frame, body[0], o, state, lang);
        sidebar(frame, body[1], o, state, lang);
    } else {
        overview_table(frame, parts[2], o, state, lang);
    }
    let help = if progress {
        lang.text(
            "p 返回原页面   c 检索配置   Esc 总览   r 刷新   q 退出  l EN",
            "p Back  c Settings  Esc Overview  r Refresh  l 中文  q Quit",
        )
    } else if area.width >= 100 {
        lang.text("↑↓ 选择  Enter 进入  ← 返回  PgUp/PgDn 翻页  r 更新  p 进度  c 检索配置  Esc 总览  q 退出  l EN","↑↓ Select  Enter Open  ← Back  PgUp/Dn Pages  r Reload  p Progress  c Settings  l 中文  q Quit")
    } else if area.width >= 65 {
        lang.text(
            "↑↓ 选择  ↵ 进入  ← 返回  p 进度  c 配置  r 更新  q 退出  l EN",
            "↑↓ Select  ↵ Open  ← Back  p Progress  c Settings  l 中文  q Quit",
        )
    } else {
        lang.text(
            "↑↓ 选择  ↵ 进入  c 配置  q 退出  l EN",
            "↑↓ Select  ↵ Open  c Settings  l 中文  q Quit",
        )
    };
    let mut lines = Vec::new();
    if !warning.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("! {}", lang.diagnostic(warning)),
            Style::default().fg(YELLOW),
        )));
    }
    lines.push(Line::from(bold(help, CYAN)));
    let detail = tree_warning.unwrap_or_else(|| {
        if !progress && let Some(q) = o.quotas.get(state.selected().unwrap_or(0)) {
            return tr!(
                lang,
                "{} · 软限 {} · 硬限 {} · 索引 {}",
                "{} · Soft {} · Hard {} · Indexed {}",
                q.name,
                limit(q.soft),
                limit(q.hard),
                q.indexed
                    .map(human)
                    .unwrap_or(lang.text("尚无", "N/A").into())
            );
        }
        tr!(
            lang,
            "配额更新 {}   ·   浏览缓存，不触发扫描",
            "Quota updated {}   ·   Browsing cached data",
            age(o.quota_updated, lang)
        )
    });
    lines.push(Line::from(muted(detail)));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), parts[3]);
}

pub(super) fn draw_settings(frame: &mut Frame, form: &ConfigForm, lang: Language) {
    let outer = frame.area();
    if outer.width < 60 || outer.height < 22 {
        frame.render_widget(Clear, outer);
        frame.render_widget(Paragraph::new(lang.text("自动检索配置\n请将终端放大到至少 60×22\n也可使用 usage-report --config\nEsc / q 返回","Scan settings\nResize to at least 60×22\nor use usage-report --config\nEsc / q back · l 中文")).style(Style::default().fg(CYAN).bg(PANEL)).wrap(Wrap{trim:false}),outer);
        return;
    }
    let width = outer.width.saturating_sub(2).min(86);
    let height = outer.height.saturating_sub(2).min(23);
    let area = Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let mut lines = vec![
        Line::default(),
        Line::from(bold(
            if form.settings.can_edit {
                lang.text("管理员配置", "Administrator settings")
            } else {
                lang.text(
                    "只读 · 需要管理员权限才能修改",
                    "Read only · administrator access required",
                )
            },
            if form.settings.can_edit { GREEN } else { MUTED },
        )),
        Line::from(muted(tr!(
            lang,
            "下次执行：{}",
            "Next run: {}",
            lang.diagnostic(&crate::schedule::next_run_text(form.settings.next_run))
        ))),
        Line::default(),
    ];
    for (i, label) in [
        lang.text("检索间隔", "Interval"),
        lang.text("开机延迟", "Boot delay"),
        lang.text("随机延迟", "Jitter"),
        lang.text("自动检索", "Auto scans"),
    ]
    .into_iter()
    .enumerate()
    {
        let value = if i == 3 {
            if form.enabled {
                lang.text("开启", "On")
            } else {
                lang.text("关闭", "Off")
            }
            .into()
        } else {
            form.values[i].clone()
        };
        let label_padding = " ".repeat(12usize.saturating_sub(Line::from(label).width()));
        let mut line = Line::from(format!(
            "{} {label}{label_padding}{value}{}",
            if i == form.field { "▸" } else { " " },
            if i == form.field && i < 3 && form.settings.can_edit {
                "▏"
            } else {
                ""
            }
        ));
        if i == form.field {
            line = line.style(
                Style::default()
                    .bg(SELECTED)
                    .fg(CYAN)
                    .add_modifier(Modifier::BOLD),
            );
        }
        lines.push(line);
        if width >= 76 && height >= 23 {
            lines.push(Line::default());
        }
    }
    lines.extend([
        Line::from(muted(lang.text(
            "时间示例：30m / 3h / 1d；间隔 1 分钟至 30 天",
            "Examples: 30m / 3h / 1d; interval: 1 minute to 30 days",
        ))),
        Line::from(muted(lang.text(
            "保存后重算下次执行时间，到期任务可能立即开始。",
            "Saving recalculates the next run; an overdue scan may start now.",
        ))),
        Line::from(muted(lang.text(
            "关闭自动检索不影响正在扫描的任务或手动更新。",
            "Disabling auto scans keeps running scans and manual rebuilds.",
        ))),
        Line::default(),
        Line::from(bold(
            if form.settings.can_edit {
                lang.text(
                    "↑↓/Tab 选项  ←→/空格 开关  Enter 保存  Esc 返回  l EN",
                    "↑↓/Tab Field  ←→/Space Toggle  Enter Save  Esc Back  l 中文",
                )
            } else {
                lang.text("Esc / q 返回  l EN", "Esc / q Back · l 中文")
            },
            CYAN,
        )),
    ]);
    if !form.message.is_empty() {
        lines.push(Line::from(Span::styled(
            lang.diagnostic(&form.message),
            Style::default().fg(if form.failed { RED } else { GREEN }),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(lang.text(" 自动检索配置 ", " Scan settings "))),
        area,
    );
}
