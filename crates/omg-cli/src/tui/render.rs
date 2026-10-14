//! Pure rendering from `AppState`; this module never touches SQLite.
use super::{AppState, Menu, Tab, Tone, theme::*};
use omg_core::{bytes, catalog::Finding, planning::Plan, rollup::Child};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Wrap},
};
use std::sync::atomic::Ordering;

type Lines = Vec<Line<'static>>;

fn name_of(raw: &[u8]) -> String {
    let text = clean(raw);
    text.rsplit('/').next().unwrap_or(&text).to_string()
}

pub fn visible_findings(state: &AppState) -> Vec<&Finding> {
    let tiers = ["", "review", "info", "protected"];
    state
        .summary
        .as_ref()
        .map(|s| {
            s.findings
                .iter()
                .filter(|f| state.tier == 0 || f.effective_tier == tiers[state.tier])
                .collect()
        })
        .unwrap_or_default()
}
pub fn visible_children(state: &AppState) -> Vec<&Child> {
    state
        .current_dir()
        .map(|d| d.children.iter().collect())
        .unwrap_or_default()
}
pub fn row_count(state: &AppState) -> usize {
    match state.tab {
        Tab::Overview => 0,
        Tab::Browse => visible_children(state).len(),
        Tab::Findings => visible_findings(state).len(),
        Tab::Trash => state.summary.as_ref().map_or(0, |s| s.marked.len()),
    }
}

fn tone(theme: &Theme, tone: Tone) -> Style {
    match tone {
        Tone::Normal => Style::new(),
        Tone::Warn => theme.warn(),
        Tone::Danger => theme.danger(),
    }
}

pub fn render(frame: &mut Frame<'_>, state: &AppState, theme: &Theme) {
    let area = frame.area();
    if area.width < 50 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("OMG! Terminal too small — resize to at least 50×16 (q quits)."),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    header(frame, rows[0], state, theme);
    tabs(frame, rows[1], state, theme);
    let (width, height) = (rows[2].width as usize, rows[2].height as usize);
    let body: Lines = if state.help {
        help(theme, state.wsl)
    } else if let Some(plan) = &state.plan {
        plan_view(plan, theme)
    } else {
        match state.tab {
            Tab::Overview => overview(state, theme, width),
            Tab::Browse => browse(state, theme, width, height),
            Tab::Findings => findings(state, theme, width, height),
            Tab::Trash => trash(state, theme, width, height),
        }
    };
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), rows[2]);
    frame.render_widget(Paragraph::new(status_line(state, theme)), rows[3]);
    options_bar(frame, rows[4], state, theme);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  m options · ? shortcuts · 1-4 views · q quit",
            theme.dim(),
        ))),
        rows[5],
    );
    if let Some(menu) = &state.menu {
        menu_overlay(frame, area, menu, theme);
    }
}

fn header(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let inner = area.width.saturating_sub(4) as usize;
    let rows = banner(inner.saturating_sub(24).max(13));
    let brand_row = |row: &str| -> Vec<Span<'static>> {
        vec![
            Span::raw(" "),
            Span::styled(row.to_string(), theme.brand().patch(theme.bold())),
        ]
    };
    let mut first = brand_row(&rows[0]);
    first.push(Span::styled(
        format!("   v{}", env!("CARGO_PKG_VERSION")),
        theme.dim(),
    ));
    let mut second = brand_row(&rows[1]);
    second.push(Span::styled(
        "   find it · explain it · you decide",
        theme.dim(),
    ));
    let mut lines = vec![Line::from(first), Line::from(second)];
    match state.summary.as_ref().map(|s| &s.status) {
        None => {
            lines.push(Line::from(Span::styled(
                "  Loading your index…",
                theme.dim(),
            )));
            lines.push(Line::from(""));
        }
        Some(status) if status.root.is_none() => {
            lines.push(Line::from(Span::styled(
                "  Nothing scanned yet — press r to count your gigabytes.",
                theme.dim(),
            )));
            lines.push(Line::from(""));
        }
        Some(status) => {
            let scope = status
                .root
                .as_deref()
                .map(clean)
                .map(|p| state.shorten_home(&p))
                .unwrap_or_default();
            let label = match status.last_report.as_ref().map(|r| r.state.as_str()) {
                Some("partial") => {
                    Span::styled(" · partial (some folders unreadable)", theme.warn())
                }
                Some("running") => Span::styled(" · a scan is running", theme.brand()),
                _ => Span::raw(""),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {scope}"), Style::new()),
                Span::styled(
                    format!(
                        "  ·  scanned {}  ·  {} files  ·  {} indexed",
                        ago(state.now, status.last_successful),
                        short_count(status.entries),
                        bytes(status.unique_allocated)
                    ),
                    theme.dim(),
                ),
                label,
            ]));
            let gauge = inner.saturating_sub(70).clamp(10, 36);
            // Live numbers (read fresh from the filesystem) win over the scan-time sample.
            let disk = summary_volume(state).or_else(|| {
                status
                    .capacity
                    .zip(status.available)
                    .map(|(c, a)| (c, c.saturating_sub(a)))
            });
            lines.push(match disk {
                Some((cap, used_bytes)) if cap > 0 => {
                    let used = used_bytes as f64 / cap as f64;
                    let style = if used >= 0.9 {
                        theme.danger()
                    } else if used >= 0.7 {
                        theme.warn()
                    } else {
                        theme.brand()
                    };
                    let filled = bar(used, gauge).trim_end().to_string();
                    Line::from(vec![
                        Span::styled("  disk ", theme.dim()),
                        Span::styled(filled.clone(), style),
                        Span::styled(
                            "░".repeat(gauge.saturating_sub(filled.chars().count())),
                            theme.dim(),
                        ),
                        Span::styled(
                            format!(" {} used of {}", bytes(used_bytes), bytes(cap)),
                            Style::new().patch(theme.bold()),
                        ),
                    ])
                }
                _ => Line::from(Span::styled("  disk usage unknown", theme.dim())),
            });
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme.dim()),
        ),
        area,
    );
}

fn tabs(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let mut spans = vec![Span::raw(" ")];
    let marked = state.summary.as_ref().map_or(0, |s| s.marked.len());
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let mut name = if area.width < 84 {
            tab.name()
                .split(' ')
                .next_back()
                .unwrap_or("")
                .chars()
                .take(5)
                .collect()
        } else {
            tab.name().to_string()
        };
        if *tab == Tab::Trash && marked > 0 {
            name.push_str(&format!(" ({marked})"));
        }
        if *tab == state.tab {
            spans.push(Span::styled(format!("● {name}"), theme.selected()));
        } else {
            spans.push(Span::styled(format!("{} {name}", i + 1), theme.dim()));
        }
        spans.push(Span::raw("  "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn status_line(state: &AppState, theme: &Theme) -> Line<'static> {
    let frame = FRAMES[state.tick % FRAMES.len()];
    let progress_line =
        |label: String, done: u64, total: u64, left: f64, finishing: bool, rate: f64| {
            let detail = if finishing {
                format!(
                    "finishing up: comparing, totalling folders, saving · ~{} left",
                    duration(left)
                )
            } else if total > 0 && done > 0 {
                let pct = (done as f64 / total as f64).min(1.0);
                format!(
                    "{} {} / ~{} · ≤{:.0}% · {:.0}/s · ~{} left",
                    bar(pct, 12),
                    count(done),
                    short_count(total),
                    pct * 100.0,
                    rate,
                    duration(left)
                )
            } else {
                format!("{} entries · {:.0}/s", count(done), rate)
            };
            Line::from(vec![
                Span::styled(format!(" {frame} {label}… "), theme.accent()),
                Span::styled(detail, theme.dim()),
            ])
        };
    if let Some(scan) = &state.scan {
        let raw = scan.progress.load(Ordering::Relaxed);
        let done = raw & !omg_core::scanner::FINISHING;
        let secs = scan.started.elapsed().as_secs_f64().max(0.001);
        let finishing = scan.finishing_since.map(|t| t.elapsed().as_secs_f64());
        let walking = finishing.map_or(secs, |f| secs - f);
        let left = omg_core::scanner::eta(done, scan.total, walking, finishing);
        let verb = VERBS[(secs as usize / 4) % VERBS.len()];
        let mut line = progress_line(
            verb.to_string(),
            done,
            scan.total,
            left,
            finishing.is_some(),
            done as f64 / walking.max(0.001),
        );
        line.spans.push(Span::styled(" (esc stops)", theme.dim()));
        return line;
    }
    if let Some(other) = &state.other {
        let left = omg_core::scanner::eta(
            other.progress,
            other.total,
            other.walking_secs,
            other.finishing_secs,
        );
        return progress_line(
            "Another omg scan is running".into(),
            other.progress,
            other.total,
            left,
            other.finishing_secs.is_some(),
            other.progress as f64 / other.walking_secs.max(1.0),
        );
    }
    if let Some(busy) = &state.busy {
        return Line::from(vec![
            Span::styled(format!(" {frame} {busy}… "), theme.accent()),
            Span::styled(state.message.clone(), theme.dim()),
        ]);
    }
    let style = if state.message.starts_with('⚠') {
        theme.warn()
    } else {
        theme.dim()
    };
    Line::from(Span::styled(format!(" {}", state.message), style))
}

fn options_bar(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let options: &[(&str, &str)] = if state.menu.is_some() {
        &[("↑↓", "choose"), ("⏎", "select"), ("esc", "cancel")]
    } else if state.plan.is_some() {
        &[("⏎", "record preview"), ("esc", "back")]
    } else {
        match state.tab {
            Tab::Overview => &[
                ("r", "scan"),
                ("2", "browse"),
                ("3", "findings"),
                ("4", "OMG Trash"),
                ("m", "options"),
            ],
            Tab::Browse => &[
                ("⏎", "open"),
                ("⌫", "up"),
                ("␣", "add to OMG Trash"),
                ("d", "delete…"),
                ("m", "options"),
            ],
            Tab::Findings => &[
                ("⏎", "details"),
                ("␣", "add to OMG Trash"),
                ("d", "delete…"),
                ("p", "preview"),
                ("t", "tier"),
            ],
            Tab::Trash => &[
                ("␣", "select"),
                ("d", "delete selected…"),
                ("D", "empty all…"),
                ("u", "remove from list"),
            ],
        }
    };
    let mut spans = vec![Span::raw(" ")];
    for (key, label) in options {
        let danger = label.starts_with("delete") || label.starts_with("empty");
        spans.push(Span::styled(
            format!(" {key} "),
            if danger {
                theme.danger()
            } else {
                theme.accent()
            }
            .patch(theme.bold()),
        ));
        spans.push(Span::styled(format!("{label}   "), theme.dim()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme.dim()),
        ),
        area,
    );
}

fn menu_overlay(frame: &mut Frame<'_>, area: Rect, menu: &Menu, theme: &Theme) {
    let width = (area.width.saturating_sub(6)).min(76);
    let height = (menu.body.len() + menu.choices.len() + 4)
        .min(area.height.saturating_sub(2) as usize) as u16;
    let rect = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let mut lines: Lines = menu
        .body
        .iter()
        .map(|(text, t)| Line::from(Span::styled(format!(" {text}"), tone(theme, *t))))
        .collect();
    if !menu.body.is_empty() {
        lines.push(Line::from(""));
    }
    for (i, choice) in menu.choices.iter().enumerate() {
        let selected = i == menu.cursor;
        let label_style = if selected {
            theme.selected()
        } else {
            Style::new()
        }
        .patch(tone(theme, choice.tone));
        lines.push(Line::from(vec![
            Span::styled(if selected { " ❯ " } else { "   " }, theme.selected()),
            Span::styled(format!("{}. ", i + 1), theme.dim()),
            Span::styled(choice.label.clone(), label_style),
            Span::styled(
                choice
                    .key
                    .map(|k| {
                        format!(
                            "   {}",
                            if k == ' ' {
                                "space".into()
                            } else {
                                k.to_string()
                            }
                        )
                    })
                    .unwrap_or_default(),
                theme.dim(),
            ),
        ]));
    }
    let border = match menu.tone {
        Tone::Danger => theme.danger(),
        Tone::Warn => theme.warn(),
        Tone::Normal => theme.accent(),
    };
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border)
                .title(Span::styled(
                    format!(" {} ", menu.title),
                    border.patch(theme.bold()),
                )),
        ),
        rect,
    );
}

fn overview(state: &AppState, theme: &Theme, width: usize) -> Lines {
    let mut out: Lines = vec![Line::from("")];
    let Some(summary) = &state.summary else {
        out.push(Line::from(Span::styled("  Loading…", theme.dim())));
        return out;
    };
    if summary.status.root.is_none() {
        out.push(Line::from(Span::styled(
            "  No index yet. Press r — scanning only reads metadata and never changes a file.",
            theme.dim(),
        )));
        return out;
    }
    out.push(Line::from(Span::styled("  Biggest folders", theme.bold())));
    if !summary.rollups {
        out.push(Line::from(Span::styled(
            "  Folder totals need a fresh scan (press r).",
            theme.dim(),
        )));
    }
    let total = summary.top.iter().map(|c| c.allocated).sum::<u64>().max(1);
    let max = summary
        .top
        .iter()
        .map(|c| c.allocated)
        .max()
        .unwrap_or(1)
        .max(1);
    let bar_width = (width.saturating_sub(44)).clamp(8, 30);
    for child in summary.top.iter().take(8) {
        let filled = bar(child.allocated as f64 / max as f64, bar_width)
            .trim_end()
            .to_string();
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(filled.clone(), theme.brand()),
            Span::styled(
                "·".repeat(bar_width.saturating_sub(filled.chars().count())),
                theme.dim(),
            ),
            Span::styled(format!(" {:>10}", bytes(child.allocated)), theme.bold()),
            Span::styled(
                format!(" {:>4.0}%  ", child.allocated as f64 / total as f64 * 100.0),
                theme.dim(),
            ),
            Span::raw(truncate_middle(
                &name_of(&child.path),
                width.saturating_sub(bar_width + 22),
            )),
            Span::styled(
                if child.kind == "directory" { "/" } else { "" },
                theme.dim(),
            ),
        ]));
    }
    out.push(Line::from(""));
    let count_tier = |t: &str| {
        summary
            .findings
            .iter()
            .filter(|f| f.effective_tier == t)
            .count()
    };
    let explained: u64 = summary
        .findings
        .iter()
        .filter(|f| f.effective_tier != "protected")
        .map(|f| f.estimate)
        .sum();
    out.push(Line::from(vec![
        Span::styled("  Findings   ", theme.bold()),
        Span::styled(
            format!("● {} review  ", count_tier("review")),
            theme.tier("review"),
        ),
        Span::styled(
            format!("● {} info  ", count_tier("info")),
            theme.tier("info"),
        ),
        Span::styled(
            format!("● {} protected", count_tier("protected")),
            theme.tier("protected"),
        ),
        Span::styled(format!("   ≈ {} explained", bytes(explained)), theme.dim()),
    ]));
    let marked: u64 = summary.marked.iter().map(|m| m.allocated).sum();
    out.push(Line::from(vec![
        Span::styled("  OMG Trash  ", theme.bold()),
        Span::styled(
            format!(
                "{} item(s) · ≈ {} waiting for your decision",
                summary.marked.len(),
                bytes(marked)
            ),
            theme.dim(),
        ),
    ]));
    if let Some(change) = summary.trend_delta {
        let (sign, style) = if change >= 0 {
            ("+", theme.warn())
        } else {
            ("−", theme.good())
        };
        out.push(Line::from(vec![
            Span::styled("  Growth     ", theme.bold()),
            Span::styled(
                format!(
                    "{sign}{} indexed in this scope over 30 days",
                    bytes(change.unsigned_abs())
                ),
                style,
            ),
        ]));
    }
    out
}

fn browse(state: &AppState, theme: &Theme, width: usize, height: usize) -> Lines {
    let mut crumbs = vec![Span::styled("  ", theme.dim())];
    let root = state
        .summary
        .as_ref()
        .and_then(|s| s.root.as_ref())
        .map(|r| state.shorten_home(&r.display().to_string()))
        .unwrap_or_else(|| "/".into());
    crumbs.push(Span::styled(root, Style::new()));
    for part in clean(&state.dir).split('/').filter(|p| !p.is_empty()) {
        crumbs.push(Span::styled(" › ", theme.dim()));
        crumbs.push(Span::styled(part.to_string(), Style::new()));
    }
    let Some(view) = state.current_dir() else {
        return vec![
            Line::from(crumbs),
            Line::from(""),
            Line::from(Span::styled("  Loading…", theme.dim())),
        ];
    };
    if let Some(total) = &view.total {
        crumbs.push(Span::styled(
            format!(
                "   {} · {} files",
                bytes(total.allocated),
                short_count(total.files)
            ),
            theme.bold(),
        ));
    }
    let mut out: Lines = vec![Line::from(crumbs), Line::from("")];
    if view.children.is_empty() {
        out.push(Line::from(Span::styled(
            "  (empty, or folder totals need a fresh scan)",
            theme.dim(),
        )));
        return out;
    }
    let marked: Vec<&[u8]> = state
        .summary
        .as_ref()
        .map(|s| s.marked.iter().map(|m| m.path.as_slice()).collect())
        .unwrap_or_default();
    let max = view
        .children
        .iter()
        .map(|c| c.allocated)
        .max()
        .unwrap_or(1)
        .max(1);
    let bar_width = (width.saturating_sub(46)).clamp(6, 24);
    let visible = height.saturating_sub(2).max(1);
    let start = state.cursor.saturating_sub(visible.saturating_sub(1));
    for (n, child) in view.children.iter().enumerate().skip(start).take(visible) {
        let selected = n == state.cursor;
        let flag = if marked.contains(&child.path.as_slice()) {
            Span::styled(" ✗ in OMG Trash", theme.danger())
        } else {
            match view.marks.get(n) {
                Some(super::worker::RowMark::Locked(_)) => Span::styled(" 🔒", theme.dim()),
                Some(super::worker::RowMark::Warn(_)) => Span::styled(" ⚠", theme.warn()),
                Some(super::worker::RowMark::Regenerable) => {
                    Span::styled(" ♻ regenerable", theme.good())
                }
                _ => Span::raw(""),
            }
        };
        let filled = bar(child.allocated as f64 / max as f64, bar_width)
            .trim_end()
            .to_string();
        out.push(Line::from(vec![
            Span::styled(if selected { "❯ " } else { "  " }, theme.selected()),
            Span::styled(filled.clone(), theme.brand()),
            Span::styled(
                " ".repeat(bar_width.saturating_sub(filled.chars().count())),
                Style::new(),
            ),
            Span::styled(format!(" {:>10}  ", bytes(child.allocated)), theme.bold()),
            Span::styled(
                format!(
                    "{}{}",
                    truncate_middle(&name_of(&child.path), width.saturating_sub(bar_width + 40)),
                    if child.kind == "directory" { "/" } else { "" }
                ),
                if selected {
                    theme.selected()
                } else {
                    Style::new()
                },
            ),
            Span::styled(
                if child.kind == "directory" {
                    format!("  {} files", short_count(child.files))
                } else {
                    String::new()
                },
                theme.dim(),
            ),
            flag,
        ]));
    }
    out
}

fn findings(state: &AppState, theme: &Theme, width: usize, height: usize) -> Lines {
    let rows = visible_findings(state);
    let mut out: Lines = vec![Line::from(Span::styled(
        format!(
            "  tier filter: {}",
            ["all", "review", "info", "protected"][state.tier]
        ),
        theme.dim(),
    ))];
    if state.summary.is_none() {
        out.push(Line::from(Span::styled("  Loading…", theme.dim())));
        return out;
    }
    if rows.is_empty() {
        out.push(Line::from(Span::styled(
            "  No findings here. OMG reports known developer caches and large files.",
            theme.dim(),
        )));
        return out;
    }
    if state.details
        && let Some(f) = rows.get(state.cursor)
    {
        return details(f, theme);
    }
    let visible = (height.saturating_sub(1) / 2).max(1);
    let start = state.cursor.saturating_sub(visible.saturating_sub(1));
    for (n, f) in rows.iter().enumerate().skip(start).take(visible) {
        let selected = n == state.cursor;
        out.push(Line::from(vec![
            Span::styled(if selected { "❯ " } else { "  " }, theme.selected()),
            Span::styled(
                format!("● {:<9}", f.effective_tier),
                theme.tier(&f.effective_tier),
            ),
            Span::styled(format!("{:>10}  ", bytes(f.estimate)), theme.bold()),
            Span::styled(
                truncate_middle(
                    &state.shorten_home(&state.absolute(&f.entry.path)),
                    width.saturating_sub(28),
                ),
                if selected {
                    theme.selected()
                } else {
                    Style::new()
                },
            ),
        ]));
        out.push(Line::from(Span::styled(
            format!(
                "      {}",
                truncate_middle(&f.reason, width.saturating_sub(8))
            ),
            theme.dim(),
        )));
    }
    out
}

fn details(f: &Finding, theme: &Theme) -> Lines {
    let line = |label: &str, text: String| {
        Line::from(vec![
            Span::styled(format!("  {label:<13}"), theme.dim()),
            Span::raw(text),
        ])
    };
    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", f.rule.name),
            theme.accent().patch(theme.bold()),
        )),
        line("Tier", f.effective_tier.clone()),
        line("Why", f.reason.clone()),
        line("Evidence", f.evidence.clone()),
        line("Effects", f.rule.side_effects.clone()),
        line("Regenerates", f.rule.regeneration.clone()),
        line("Warnings", f.rule.warnings.join("; ")),
        line("Source", f.rule.source.clone()),
        Line::from(""),
        Line::from(Span::styled(
            "  ⏎ back · space add to OMG Trash · d delete… · p dry-run preview",
            theme.dim(),
        )),
    ]
}

fn trash(state: &AppState, theme: &Theme, width: usize, height: usize) -> Lines {
    let Some(summary) = &state.summary else {
        return vec![
            Line::from(""),
            Line::from(Span::styled("  Loading…", theme.dim())),
        ];
    };
    let total: u64 = summary.marked.iter().map(|m| m.allocated).sum();
    let mut out: Lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  {} item(s) · ≈ {}", summary.marked.len(), bytes(total)),
                theme.bold(),
            ),
            Span::styled(
                "   saved paths only — nothing has been moved or deleted",
                theme.dim(),
            ),
        ]),
        Line::from(""),
    ];
    if summary.marked.is_empty() {
        out.push(Line::from(Span::styled(
            "  Empty. In Browse or Findings press space to add something you want gone.",
            theme.dim(),
        )));
        return out;
    }
    let visible = height.saturating_sub(2).max(1);
    let start = state.cursor.saturating_sub(visible.saturating_sub(1));
    for (n, item) in summary.marked.iter().enumerate().skip(start).take(visible) {
        let selected = n == state.cursor;
        let check = if state.selected.contains(&item.id) {
            "◉ "
        } else {
            "○ "
        };
        out.push(Line::from(vec![
            Span::styled(if selected { "❯ " } else { "  " }, theme.selected()),
            Span::styled(check, theme.danger()),
            Span::styled(format!("{:>10}  ", bytes(item.allocated)), theme.bold()),
            Span::styled(
                truncate_middle(
                    &state.shorten_home(&state.absolute(&item.path)),
                    width.saturating_sub(30),
                ),
                if selected {
                    theme.selected()
                } else {
                    Style::new()
                },
            ),
            Span::styled(
                if item.warning.is_some() {
                    "  ⚠ in a Git repo"
                } else {
                    ""
                },
                theme.warn(),
            ),
        ]));
    }
    out
}

fn plan_view(plan: &Plan, theme: &Theme) -> Lines {
    let mut out: Lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Dry-run preview — nothing will change",
            theme.accent().patch(theme.bold()),
        )),
        Line::from(format!(
            "  {} target(s) · {} estimate",
            plan.items.len(),
            bytes(plan.estimated)
        )),
        Line::from(""),
    ];
    for item in &plan.items {
        out.push(Line::from(Span::styled(
            format!("  {}", clean(&item.path)),
            theme.brand(),
        )));
        out.push(Line::from(Span::styled(
            format!("    {}", item.decision),
            theme.dim(),
        )));
    }
    out
}

fn help(theme: &Theme, wsl: bool) -> Lines {
    let row = |key: &str, text: &str| {
        Line::from(vec![
            Span::styled(format!("  {key:<12}"), theme.accent()),
            Span::raw(text.to_string()),
        ])
    };
    let mut out = vec![
        Line::from(""),
        Line::from(Span::styled("  Shortcuts (any key closes)", theme.bold())),
        row("m", "options menu for the selected item and the app"),
        row("1-4  Tab", "Overview · Browse · Findings · OMG Trash"),
        row("j k ↑ ↓", "move        PgUp/PgDn jump"),
        row("⏎ →  ⌫ ←", "open folder / details  ·  up one folder"),
        row(
            "space",
            "add to OMG Trash (saves the path; nothing changes on disk)",
        ),
        row("d", "delete permanently… (always asks; cannot be undone)"),
        row("D", "in OMG Trash: delete everything listed…"),
        row("u", "remove from OMG Trash"),
        row("r  R", "scan  ·  full rescan        esc stops a scan"),
        row("p i o t", "preview · show in file manager · tier filter"),
        Line::from(""),
        Line::from(Span::styled(
            "  Protected forever: .git, credentials, databases, disk images, anything outside your home.",
            theme.dim(),
        )),
    ];
    if wsl {
        out.push(Line::from(Span::styled(
            "  WSL: deleting here frees space inside Linux; Windows (C:) gets it back only after the WSL virtual disk",
            theme.warn(),
        )));
        out.push(Line::from(Span::styled(
            "  is compacted, e.g. in PowerShell: wsl --manage <distro> --set-sparse true  (or Optimize-VHD).",
            theme.warn(),
        )));
    }
    out
}

fn summary_volume(state: &AppState) -> Option<(u64, u64)> {
    state.summary.as_ref().and_then(|s| s.volume)
}
