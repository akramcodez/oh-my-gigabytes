//! Pure rendering from `AppState`; this module never touches SQLite (see the render test).
use super::{AppState, Tab, theme::*};
use omg_core::{bytes, catalog::Finding, planning::Plan};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
};
use std::sync::atomic::Ordering;

type Lines = Vec<Line<'static>>;

fn name_of(raw: &[u8]) -> String {
    let text = clean(raw);
    text.rsplit('/').next().unwrap_or(&text).to_string()
}

pub fn visible_findings(state: &AppState) -> Vec<&Finding> {
    let tiers = ["", "review", "info", "protected"];
    let needle = state.filter.to_lowercase();
    state
        .findings
        .iter()
        .filter(|f| state.tier == 0 || f.effective_tier == tiers[state.tier])
        .filter(|f| {
            needle.is_empty()
                || clean(&f.entry.path).to_lowercase().contains(&needle)
                || f.rule.name.to_lowercase().contains(&needle)
        })
        .collect()
}
pub fn visible_children(state: &AppState) -> Vec<&omg_core::rollup::Child> {
    let needle = state.filter.to_lowercase();
    state
        .children
        .iter()
        .filter(|c| needle.is_empty() || clean(&c.path).to_lowercase().contains(&needle))
        .collect()
}
pub fn row_count(state: &AppState) -> usize {
    match state.tab {
        Tab::Overview => 0,
        Tab::Browse => visible_children(state).len(),
        Tab::Findings => visible_findings(state).len(),
        Tab::History => state.history.len(),
        Tab::Ignores => state.ignores.len(),
    }
}

pub fn render(frame: &mut Frame<'_>, state: &AppState, theme: &Theme) {
    let area = frame.area();
    if area.width < 50 || area.height < 15 {
        frame.render_widget(
            Paragraph::new("OMG! Terminal too small — resize to at least 50×15 (q quits)."),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    header(frame, rows[0], state, theme);
    tabs(frame, rows[1], state, theme);
    let body: Lines = if state.help {
        help(theme)
    } else if let Some(plan) = &state.plan {
        plan_view(plan, state.confirm, theme)
    } else {
        match state.tab {
            Tab::Overview => overview(state, theme, rows[2].width as usize),
            Tab::Browse => browse(
                state,
                theme,
                rows[2].width as usize,
                rows[2].height as usize,
            ),
            Tab::Findings => findings(
                state,
                theme,
                rows[2].width as usize,
                rows[2].height as usize,
            ),
            Tab::History => history(state, theme),
            Tab::Ignores => ignores(state, theme),
        }
    };
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), rows[2]);
    frame.render_widget(Paragraph::new(status_line(state, theme)), rows[3]);
    prompt(frame, rows[4], state, theme);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ? for shortcuts · / for commands · q to quit · cleanup is disabled",
            theme.dim(),
        ))),
        rows[5],
    );
}

fn header(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let word = "✦ OMG!";
    let mut title: Vec<Span> = vec![Span::raw(" ")];
    title.extend(
        word.chars()
            .enumerate()
            .map(|(i, c)| Span::styled(c.to_string(), theme.gradient(i, word.chars().count()))),
    );
    title.push(Span::styled("  oh-my-gigabytes", theme.bold()));
    title.push(Span::styled(
        format!("  v{}", env!("CARGO_PKG_VERSION")),
        theme.dim(),
    ));
    let mut lines = vec![Line::from(title)];
    let Some(status) = &state.status else {
        lines.push(Line::from(Span::styled(
            "  Nothing scanned yet. Type /scan and let OMG count your gigabytes.",
            theme.dim(),
        )));
        lines.push(Line::from(""));
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme.violet()),
            ),
            area,
        );
        return;
    };
    let scope = status
        .root
        .as_deref()
        .map(clean)
        .map(|p| state.shorten_home(&p))
        .unwrap_or_default();
    let label = match status.last_report.as_ref().map(|r| r.state.as_str()) {
        Some("partial") => Span::styled(" · partial (some folders unreadable)", theme.warn()),
        Some("running") => Span::styled(" · scanning", theme.cyan()),
        _ => Span::raw(""),
    };
    lines.push(Line::from(vec![
        Span::styled(format!("  {scope}"), theme.cyan()),
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
    let gauge_width = (area.width as usize).saturating_sub(48).clamp(10, 40);
    let line = match (status.capacity, status.available) {
        (Some(cap), Some(free)) if cap > 0 => {
            let used = cap.saturating_sub(free) as f64 / cap as f64;
            let (caption, style) = if used >= 0.9 {
                ("OMG, it's full!", theme.danger())
            } else if used >= 0.7 {
                ("getting chunky", theme.warn())
            } else {
                ("plenty of room", theme.good())
            };
            let filled = bar(used, gauge_width).trim_end().to_string();
            Line::from(vec![
                Span::styled("  disk ", theme.dim()),
                Span::styled(filled.clone(), style),
                Span::styled(
                    "░".repeat(gauge_width.saturating_sub(filled.chars().count())),
                    theme.dim(),
                ),
                Span::styled(
                    format!(" {:.0}% used · {} free · ", used * 100.0, bytes(free)),
                    theme.dim(),
                ),
                Span::styled(caption, style),
            ])
        }
        _ => Line::from(Span::styled("  disk usage unknown", theme.dim())),
    };
    lines.push(line);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme.violet()),
        ),
        area,
    );
}

fn tabs(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let name = if area.width < 70 {
            tab.name()
                .chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_default()
        } else {
            tab.name().to_string()
        };
        if *tab == state.tab {
            spans.push(Span::styled(format!("● {name}"), theme.selected()));
        } else {
            spans.push(Span::styled(format!("{} {name}", i + 1), theme.dim()));
        }
        spans.push(Span::raw("   "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn status_line(state: &AppState, theme: &Theme) -> Line<'static> {
    if let Some(scan) = &state.scan {
        let done = scan.progress.load(Ordering::Relaxed);
        let secs = scan.started.elapsed().as_secs_f64().max(0.001);
        let rate = done as f64 / secs;
        let verb = VERBS[(secs as usize / 4) % VERBS.len()];
        let frame = FRAMES[state.tick % FRAMES.len()];
        let detail = if scan.total > 0 && done > 0 {
            let pct = (done as f64 / scan.total as f64).min(1.0);
            format!(
                "{} {} / ~{} · ≤{:.0}% · {:.0}/s · ~{} left (esc to stop)",
                bar(pct, 12),
                count(done),
                short_count(scan.total),
                pct * 100.0,
                rate,
                duration((scan.total.saturating_sub(done)) as f64 / rate.max(1.0))
            )
        } else {
            format!("{} entries · {:.0}/s (esc to stop)", count(done), rate)
        };
        return Line::from(vec![
            Span::styled(format!(" {frame} {verb}… "), theme.accent()),
            Span::styled(detail, theme.dim()),
        ]);
    }
    Line::from(Span::styled(format!(" {}", state.message), theme.dim()))
}

fn prompt(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let content = if state.prompt || !state.input.is_empty() {
        Line::from(vec![
            Span::styled("❯ ", theme.accent()),
            Span::raw(state.input.clone()),
            Span::styled(if state.prompt { "█" } else { "" }, theme.accent()),
        ])
    } else if !state.filter.is_empty() {
        Line::from(vec![
            Span::styled("❯ ", theme.accent()),
            Span::styled(
                format!("filter: {}  (/clear to reset)", state.filter),
                theme.cyan(),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("❯ ", theme.dim()),
            Span::styled(
                "Try \"/scan\", \"/browse\", or press / and type to filter",
                theme.dim(),
            ),
        ])
    };
    frame.render_widget(
        Paragraph::new(content).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(if state.prompt {
                    theme.accent()
                } else {
                    theme.dim()
                }),
        ),
        area,
    );
}

fn overview(state: &AppState, theme: &Theme, width: usize) -> Lines {
    let mut out: Lines = vec![Line::from("")];
    if state.status.is_none() || state.status.as_ref().is_some_and(|s| s.root.is_none()) {
        out.push(Line::from(Span::styled(
            "  No index yet. Type /scan — a scan only reads metadata and never changes a file.",
            theme.dim(),
        )));
        return out;
    }
    out.push(Line::from(Span::styled("  Biggest folders", theme.bold())));
    if !state.rollups {
        out.push(Line::from(Span::styled(
            "  Folder totals need a fresh scan (type /scan).",
            theme.dim(),
        )));
    }
    let max = state
        .top
        .iter()
        .map(|c| c.allocated)
        .max()
        .unwrap_or(1)
        .max(1);
    let bar_width = (width.saturating_sub(34)).clamp(8, 28);
    for child in state.top.iter().take(8) {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {} ", bar(child.allocated as f64 / max as f64, bar_width)),
                theme.gradient(1, 2),
            ),
            Span::styled(format!("{:>10}  ", bytes(child.allocated)), theme.bold()),
            Span::raw(truncate_middle(
                &name_of(&child.path),
                width.saturating_sub(bar_width + 18),
            )),
            Span::styled(
                if child.kind == "directory" { "/" } else { "" },
                theme.dim(),
            ),
        ]));
    }
    out.push(Line::from(""));
    let count_tier = |t: &str| {
        state
            .findings
            .iter()
            .filter(|f| f.effective_tier == t)
            .count()
    };
    let explained: u64 = state
        .findings
        .iter()
        .filter(|f| f.effective_tier != "protected")
        .map(|f| f.estimate)
        .sum();
    out.push(Line::from(vec![
        Span::styled("  Findings  ", theme.bold()),
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
        Span::styled(
            format!("   ≈ {} explained (estimate)", bytes(explained)),
            theme.dim(),
        ),
    ]));
    if let Some(change) = state.trend_delta {
        let (sign, style) = if change >= 0 {
            ("+", theme.warn())
        } else {
            ("−", theme.good())
        };
        out.push(Line::from(vec![
            Span::styled("  Since earlier scans  ", theme.bold()),
            Span::styled(
                format!(
                    "{sign}{} indexed (30 days, observed)",
                    bytes(change.unsigned_abs())
                ),
                style,
            ),
        ]));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "  Cleanup is disabled — OMG finds it, explains it, and leaves every decision to you.",
        theme.dim(),
    )));
    out
}

fn browse(state: &AppState, theme: &Theme, width: usize, height: usize) -> Lines {
    let mut crumbs = vec![Span::styled("  / ", theme.dim())];
    for part in clean(&state.dir).split('/').filter(|p| !p.is_empty()) {
        crumbs.push(Span::styled(part.to_string(), theme.cyan()));
        crumbs.push(Span::styled(" / ", theme.dim()));
    }
    if let Some(total) = &state.dir_total {
        crumbs.push(Span::styled(
            format!(" {} total", bytes(total.allocated)),
            theme.bold(),
        ));
    }
    let mut out: Lines = vec![Line::from(crumbs), Line::from("")];
    if !state.rollups {
        out.push(Line::from(Span::styled(
            "  Folder totals unavailable — type /scan.",
            theme.dim(),
        )));
        return out;
    }
    let rows = visible_children(state);
    if rows.is_empty() {
        out.push(Line::from(Span::styled(
            "  (empty, or nothing matches the filter)",
            theme.dim(),
        )));
    }
    let max = rows.iter().map(|c| c.allocated).max().unwrap_or(1).max(1);
    let bar_width = (width.saturating_sub(40)).clamp(6, 24);
    let visible = height.saturating_sub(3).max(1);
    let start = state.cursor.saturating_sub(visible.saturating_sub(1));
    for (n, child) in rows.iter().enumerate().skip(start).take(visible) {
        let selected = n == state.cursor;
        let lock = if child.protected > 0 { "  🔒" } else { "" };
        let detail = if child.kind == "directory" {
            format!("/  {} files", short_count(child.files))
        } else {
            String::new()
        };
        out.push(Line::from(vec![
            Span::styled(if selected { "❯ " } else { "  " }, theme.selected()),
            Span::styled(
                bar(child.allocated as f64 / max as f64, bar_width),
                theme.gradient(n.min(9), 10),
            ),
            Span::styled(format!(" {:>10}  ", bytes(child.allocated)), theme.bold()),
            Span::styled(
                truncate_middle(&name_of(&child.path), width.saturating_sub(bar_width + 34)),
                if selected {
                    theme.selected()
                } else {
                    Default::default()
                },
            ),
            Span::styled(format!("{detail}{lock}"), theme.dim()),
        ]));
    }
    out
}

fn findings(state: &AppState, theme: &Theme, width: usize, height: usize) -> Lines {
    let rows = visible_findings(state);
    let mut out: Lines = vec![Line::from("")];
    if rows.is_empty() {
        out.push(Line::from(Span::styled(
            "  No findings match. OMG only reports known developer caches and large files it can explain.",
            theme.dim(),
        )));
        return out;
    }
    if state.details
        && let Some(f) = rows.get(state.cursor)
    {
        return details(f, theme);
    }
    let visible = (height.saturating_sub(2) / 2).max(1);
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
            Span::raw(truncate_middle(
                &state.shorten_home(&state.absolute(&f.entry.path)),
                width.saturating_sub(28),
            )),
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
        line(
            "Tier",
            format!(
                "{} · action {} (information only)",
                f.effective_tier, f.action
            ),
        ),
        line("Why", f.reason.clone()),
        line("Evidence", f.evidence.clone()),
        line("Effects", f.rule.side_effects.clone()),
        line("Regenerates", f.rule.regeneration.clone()),
        line("Warnings", f.rule.warnings.join("; ")),
        line("Source", f.rule.source.clone()),
        Line::from(""),
        Line::from(Span::styled(
            "  Enter or Esc to go back · p dry-run preview · i ignore this scope",
            theme.dim(),
        )),
    ]
}

fn history(state: &AppState, theme: &Theme) -> Lines {
    let mut out: Lines = vec![Line::from("")];
    if state.history.is_empty() {
        out.push(Line::from(Span::styled(
            "  No previews yet. Press p on a finding for a dry-run.",
            theme.dim(),
        )));
    }
    for (n, h) in state.history.iter().enumerate() {
        out.push(Line::from(vec![
            Span::styled(
                if n == state.cursor { "❯ " } else { "  " },
                theme.selected(),
            ),
            Span::styled(
                format!("{:<9}", ago(state.now, Some(h.created))),
                theme.dim(),
            ),
            Span::raw(format!("{}  ", h.kind)),
            Span::styled(h.outcome.clone(), theme.good()),
        ]));
    }
    out
}

fn ignores(state: &AppState, theme: &Theme) -> Lines {
    let mut out: Lines = vec![Line::from("")];
    if state.ignores.is_empty() {
        out.push(Line::from(Span::styled(
            "  No ignored scopes. Press i on a finding to ignore it.",
            theme.dim(),
        )));
    }
    for (n, i) in state.ignores.iter().enumerate() {
        out.push(Line::from(vec![
            Span::styled(
                if n == state.cursor { "❯ " } else { "  " },
                theme.selected(),
            ),
            Span::styled(format!("{}  ", i.rule), theme.cyan()),
            Span::raw(clean(&i.path)),
            Span::styled(
                if i.subtree { "  (whole subtree)" } else { "" },
                theme.dim(),
            ),
        ]));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "  u removes the selected ignore",
        theme.dim(),
    )));
    out
}

fn plan_view(plan: &Plan, confirm: bool, theme: &Theme) -> Lines {
    let mut out: Lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Dry-run plan (nothing will be changed)",
            theme.accent().patch(theme.bold()),
        )),
        Line::from(Span::styled(format!("  {}", plan.fingerprint), theme.dim())),
        Line::from(format!(
            "  {} target(s) · {} estimate · actionable 0 B",
            plan.items.len(),
            bytes(plan.estimated)
        )),
        Line::from(""),
    ];
    for item in &plan.items {
        out.push(Line::from(Span::styled(
            format!("  {}", clean(&item.path)),
            theme.cyan(),
        )));
        out.push(Line::from(format!(
            "    {} | {}",
            item.action, item.decision
        )));
        out.push(Line::from(Span::styled(
            format!("    Warnings: {}", item.warnings.join("; ")),
            theme.dim(),
        )));
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        if confirm {
            "  Cancel is the default — Esc cancels. y records this preview only; no operation can execute."
        } else {
            "  Enter: review acknowledgement · Esc: back · execution is unavailable in this build"
        },
        theme.warn(),
    )));
    out
}

fn help(theme: &Theme) -> Lines {
    let row = |key: &str, text: &str| {
        Line::from(vec![
            Span::styled(format!("  {key:<14}"), theme.accent()),
            Span::raw(text.to_string()),
        ])
    };
    vec![
        Line::from(""),
        Line::from(Span::styled("  Shortcuts", theme.bold())),
        row(
            "1-5 / Tab",
            "switch view: Overview, Browse, Findings, History, Ignores",
        ),
        row("j k ↑ ↓", "move"),
        row("Enter → ", "open folder (Browse) · details (Findings)"),
        row("Backspace ←", "up one folder"),
        row("r", "rescan   (esc stops a scan)"),
        row(
            "p",
            "dry-run preview of the selected finding (never executes)",
        ),
        row(
            "i  /  u",
            "ignore a finding  /  remove an ignore (Ignores view)",
        ),
        row("o", "show location in the file manager"),
        row("t", "cycle tier filter in Findings"),
        row(
            "/",
            "command bar: /scan /scan --full /browse /findings /history /search <text> /clear /quit",
        ),
        row("?  q", "this help  /  quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Cleanup is disabled. OMG explains; you decide.",
            theme.dim(),
        )),
    ]
}
