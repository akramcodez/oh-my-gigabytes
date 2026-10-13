//! Keyboard-first terminal UI. All data is loaded into `AppState` on start, after a scan and when
//! the view changes; rendering (see `render.rs`) never queries SQLite, so large indexes stay fluid.
mod render;
mod theme;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use omg_core::{
    Index, ScanOptions, ScanReport, Status,
    catalog::{Finding, Ignore},
    planning::{History, Plan},
    platform,
    rollup::{Child, Totals},
};
use render::{render, row_count, visible_children, visible_findings};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Browse,
    Findings,
    History,
    Ignores,
}
impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Overview,
        Tab::Browse,
        Tab::Findings,
        Tab::History,
        Tab::Ignores,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Browse => "Browse",
            Tab::Findings => "Findings",
            Tab::History => "History",
            Tab::Ignores => "Ignores",
        }
    }
}

pub struct ScanView {
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub started: Instant,
}

#[derive(Default)]
pub struct AppState {
    pub tab: Tab,
    pub status: Option<Status>,
    pub root: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub findings: Vec<Finding>,
    pub top: Vec<Child>,
    pub dir: Vec<u8>,
    pub children: Vec<Child>,
    pub dir_total: Option<Totals>,
    pub rollups: bool,
    pub history: Vec<History>,
    pub ignores: Vec<Ignore>,
    pub trend_delta: Option<i64>,
    pub cursor: usize,
    pub details: bool,
    pub plan: Option<Plan>,
    pub confirm: bool,
    pub input: String,
    pub prompt: bool,
    pub filter: String,
    pub tier: usize,
    pub help: bool,
    pub message: String,
    pub scan: Option<ScanView>,
    pub tick: usize,
    pub now: i64,
}

impl AppState {
    pub fn absolute(&self, relative: &[u8]) -> String {
        let rel = theme::clean(relative);
        match &self.root {
            Some(root) => root.join(&rel).display().to_string(),
            None => rel,
        }
    }
    pub fn shorten_home(&self, path: &str) -> String {
        match self.home.as_ref().and_then(|h| h.to_str()) {
            Some(home) if path == home => "~".into(),
            Some(home) if path.strip_prefix(home).is_some_and(|r| r.starts_with('/')) => {
                format!("~{}", &path[home.len()..])
            }
            _ => path.into(),
        }
    }
    /// Reload everything the views need. This is the only place the UI reads the index.
    fn reload(&mut self, index: &Index) -> Result<()> {
        self.now = omg_core::now();
        let status = index.status()?;
        self.root = status.root.as_deref().and_then(|r| platform::path(r).ok());
        self.home = platform::home().ok();
        self.rollups = index.rollups_available()?;
        self.status = Some(status);
        self.findings = index.findings(1000, 0, false)?;
        self.top = if self.rollups {
            index.children(b"", 8, 0)?
        } else {
            vec![]
        };
        self.children = if self.rollups {
            index.children(&self.dir, 500, 0)?
        } else {
            vec![]
        };
        self.dir_total = if self.rollups {
            index.subtree(&self.dir)?
        } else {
            None
        };
        self.history = index.history()?;
        self.ignores = index.ignores()?;
        self.trend_delta = index
            .trend(30)
            .ok()
            .and_then(|t| t.indexed_allocated_change);
        self.cursor = self.cursor.min(row_count(self).saturating_sub(1));
        Ok(())
    }
}

type Job = std::thread::JoinHandle<Result<ScanReport>>;

fn start_scan(
    index: &Index,
    state: &mut AppState,
    cancel: &Arc<AtomicBool>,
    job: &mut Option<Job>,
    full: bool,
) -> Result<()> {
    if job.is_some() {
        state.message = "A scan is already running.".into();
        return Ok(());
    }
    let mut options = index.options()?.unwrap_or(ScanOptions {
        root: platform::home()?,
        workers: 2,
        exclusions: vec![],
        full: true,
        publish_partial: false,
    });
    options.workers = omg_core::scanner::default_workers();
    options.full = full || options.full;
    // The interactive user sees the "partial" label, so unreadable folders must not hide results.
    options.publish_partial = true;
    let progress = Arc::new(AtomicU64::new(0));
    cancel.store(false, Ordering::Relaxed);
    state.scan = Some(ScanView {
        progress: progress.clone(),
        total: omg_core::scanner::estimate_entries(&options.root).unwrap_or(0),
        started: Instant::now(),
    });
    let (path, flag) = (index.state.clone(), cancel.clone());
    *job = Some(std::thread::spawn(move || {
        let mut index = Index::open(&path)?;
        omg_core::scanner::scan(&mut index, &options, flag, progress)
    }));
    Ok(())
}

fn run_command(
    index: &Index,
    state: &mut AppState,
    cancel: &Arc<AtomicBool>,
    job: &mut Option<Job>,
    line: &str,
) -> Result<bool> {
    let mut parts = line.trim().splitn(2, ' ');
    let command = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    state.cursor = 0;
    match command {
        "/scan" => start_scan(index, state, cancel, job, rest == "--full")?,
        "/overview" => state.tab = Tab::Overview,
        "/browse" => state.tab = Tab::Browse,
        "/findings" => state.tab = Tab::Findings,
        "/history" => state.tab = Tab::History,
        "/ignores" => state.tab = Tab::Ignores,
        "/search" => state.filter = rest.chars().take(128).collect(),
        "/clear" => state.filter.clear(),
        "/help" => state.help = true,
        "/quit" | "/q" | "/exit" => return Ok(true),
        other if other.starts_with('/') => {
            state.message = format!("Unknown command {other}. Try /help.");
        }
        text => state.filter = text.chars().take(128).collect(),
    }
    Ok(false)
}

const COMMANDS: [&str; 10] = [
    "/scan",
    "/scan --full",
    "/browse",
    "/findings",
    "/history",
    "/ignores",
    "/overview",
    "/search ",
    "/clear",
    "/help",
];

fn go_up(index: &Index, state: &mut AppState) -> Result<()> {
    if state.dir.is_empty() {
        return Ok(());
    }
    let cut = state.dir.iter().rposition(|b| *b == b'/').unwrap_or(0);
    state.dir.truncate(cut);
    state.cursor = 0;
    state.reload(index)
}

/// Returns true to quit.
fn handle_key(
    index: &Index,
    state: &mut AppState,
    cancel: &Arc<AtomicBool>,
    job: &mut Option<Job>,
    key: event::KeyEvent,
) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }
    if state.prompt {
        match key.code {
            KeyCode::Esc => {
                state.prompt = false;
                state.input.clear();
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut state.input);
                state.prompt = false;
                if !line.trim().is_empty() && run_command(index, state, cancel, job, &line)? {
                    return Ok(true);
                }
                state.reload(index)?;
            }
            KeyCode::Tab => {
                if let Some(c) = COMMANDS
                    .iter()
                    .find(|c| c.starts_with(state.input.as_str()))
                {
                    state.input = (*c).to_string();
                }
            }
            KeyCode::Backspace => {
                state.input.pop();
            }
            KeyCode::Char(c) if !c.is_control() && state.input.chars().count() < 200 => {
                state.input.push(c)
            }
            _ => (),
        }
        return Ok(false);
    }
    let rows = row_count(state);
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('?') => state.help = !state.help,
        KeyCode::Char('/') => {
            state.prompt = true;
            state.input = "/".into();
        }
        KeyCode::Esc => {
            if state.help {
                state.help = false;
            } else if state.scan.is_some() {
                cancel.store(true, Ordering::Relaxed);
                state.message = "Stopping the scan… the previous index stays valid.".into();
            } else if state.plan.is_some() {
                state.plan = None;
                state.confirm = false;
            } else if state.details {
                state.details = false;
            } else if !state.filter.is_empty() {
                state.filter.clear();
            } else if state.tab == Tab::Browse {
                go_up(index, state)?;
            }
        }
        KeyCode::Char(c @ '1'..='5') => {
            state.tab = Tab::ALL[c as usize - '1' as usize];
            state.cursor = 0;
            state.details = false;
            state.help = false;
        }
        KeyCode::Tab => {
            let next = Tab::ALL.iter().position(|t| *t == state.tab).unwrap_or(0) + 1;
            state.tab = Tab::ALL[next % Tab::ALL.len()];
            state.cursor = 0;
            state.details = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.cursor = (state.cursor + 1).min(rows.saturating_sub(1))
        }
        KeyCode::Char('k') | KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Char('r') if state.scan.is_none() => start_scan(index, state, cancel, job, false)?,
        KeyCode::Char('R') if state.scan.is_none() => start_scan(index, state, cancel, job, true)?,
        KeyCode::Enter | KeyCode::Right if state.plan.is_some() => state.confirm = true,
        KeyCode::Char('y') if state.confirm => {
            if let Some(plan) = state.plan.take() {
                index.record_preview(&plan)?;
                state.message =
                    "Preview recorded. 0 mutations made; no space freed or claimed.".into();
            }
            state.confirm = false;
            state.history = index.history()?;
        }
        KeyCode::Enter | KeyCode::Right if state.tab == Tab::Browse => {
            let next = visible_children(state)
                .get(state.cursor)
                .filter(|c| c.kind == "directory")
                .map(|c| c.path.clone());
            if let Some(path) = next {
                state.dir = path;
                state.cursor = 0;
                state.reload(index)?;
            }
        }
        KeyCode::Backspace | KeyCode::Left if state.tab == Tab::Browse => go_up(index, state)?,
        KeyCode::Enter if state.tab == Tab::Findings => state.details = !state.details,
        KeyCode::Char('t') if state.tab == Tab::Findings => {
            state.tier = (state.tier + 1) % 4;
            state.cursor = 0;
        }
        KeyCode::Char('p') if state.tab == Tab::Findings => {
            let id = visible_findings(state)
                .get(state.cursor)
                .map(|f| f.id.clone());
            if let Some(id) = id {
                match index.preview(std::slice::from_ref(&id)) {
                    Ok(plan) => {
                        state.plan = Some(plan);
                        state.confirm = false;
                    }
                    Err(error) => state.message = error.to_string(),
                }
            }
        }
        KeyCode::Char('i') if state.tab == Tab::Findings => {
            let finding = visible_findings(state)
                .get(state.cursor)
                .map(|f| (*f).clone());
            if let Some(finding) = finding {
                index.ignore_finding(&finding, false)?;
                state.message =
                    "Ignored this exact rule and path. See the Ignores view to undo.".into();
                state.reload(index)?;
            }
        }
        KeyCode::Char('u') if state.tab == Tab::Ignores => {
            if let Some(item) = state.ignores.get(state.cursor) {
                index.remove_ignore(item.id)?;
                state.message = "Ignore removed.".into();
                state.reload(index)?;
            }
        }
        KeyCode::Char('o') if state.tab == Tab::Findings => {
            let target = visible_findings(state)
                .get(state.cursor)
                .map(|f| f.entry.clone());
            if let (Some(entry), Some(root)) = (target, state.root.clone()) {
                state.message = match omg_core::planning::open_location(&root, &entry) {
                    Ok(()) => "Asked the file manager to show this location.".into(),
                    Err(error) => error.to_string(),
                };
            }
        }
        _ => (),
    }
    Ok(false)
}

pub fn run(index: Index) -> Result<()> {
    let theme = theme::Theme::detect();
    ratatui::run(|terminal| {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut job: Option<Job> = None;
        let mut state = AppState {
            message: "Welcome to OMG — it finds it, explains it, and lets you decide.".into(),
            ..Default::default()
        };
        state.reload(&index)?;
        // Housekeeping off the UI thread: drop rows of superseded scans.
        let prune_path = index.state.clone();
        std::thread::spawn(move || {
            if let Ok(index) = Index::open(&prune_path) {
                let _ = index.prune_stale(Duration::from_secs(120));
            }
        });
        let stale = state
            .status
            .as_ref()
            .is_none_or(|s| s.last_successful.is_none_or(|t| omg_core::now() - t > 3600));
        if stale {
            start_scan(&index, &mut state, &cancel, &mut job, false)?;
        }
        let result = (|| -> Result<()> {
            loop {
                state.tick = state.tick.wrapping_add(1);
                if job.as_ref().is_some_and(|j| j.is_finished()) {
                    state.message = match job.take().map(|j| j.join()) {
                        Some(Ok(Ok(r))) => format!(
                            "Scan {}: {} entries, {} unreadable.",
                            r.state,
                            theme::count(r.entries),
                            r.errors
                        ),
                        Some(Ok(Err(e))) => {
                            format!("Scan failed: {e:#}. The previous index is untouched.")
                        }
                        _ => "Scan failed; the previous index is untouched.".into(),
                    };
                    state.scan = None;
                    state.plan = None;
                    state.confirm = false;
                    state.reload(&index)?;
                }
                terminal.draw(|frame| render(frame, &state, &theme))?;
                if !event::poll(Duration::from_millis(80))? {
                    continue;
                }
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if handle_key(&index, &mut state, &cancel, &mut job, key)? {
                    break;
                }
            }
            Ok(())
        })();
        cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = job {
            let _ = handle.join();
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(state: &AppState, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    state,
                    &theme::Theme {
                        mode: theme::Mode::Mono,
                    },
                )
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    /// Rendering takes only `AppState`: there is no index to query, which is the point.
    #[test]
    fn every_view_renders_at_standard_sizes_without_an_index() {
        for (width, height) in [(50, 15), (60, 20), (80, 24), (120, 40)] {
            for tab in Tab::ALL {
                let state = AppState {
                    tab,
                    ..Default::default()
                };
                let text = draw(&state, width, height);
                assert!(
                    text.contains("OMG"),
                    "{tab:?} at {width}x{height}",
                    tab = tab.name()
                );
                assert!(text.contains("quit"), "{} at {width}x{height}", tab.name());
            }
        }
        assert!(draw(&AppState::default(), 40, 10).contains("too small"));
    }

    #[test]
    fn data_views_show_sizes_escaped_names_and_scan_progress() {
        let hostile = Child {
            path: b"dir/evil\x1b[31mname\n".to_vec(),
            kind: "directory".into(),
            allocated: 5 << 30,
            apparent: 5 << 30,
            files: 1200,
            dirs: 3,
            protected: 1,
        };
        let state = AppState {
            tab: Tab::Browse,
            rollups: true,
            children: vec![hostile.clone()],
            top: vec![hostile],
            dir_total: Some(Totals {
                allocated: 6 << 30,
                ..Default::default()
            }),
            scan: Some(ScanView {
                progress: Arc::new(AtomicU64::new(500)),
                total: 1000,
                started: Instant::now(),
            }),
            ..Default::default()
        };
        let text = draw(&state, 100, 30);
        assert!(text.contains("5.00 GiB"));
        assert!(
            !text.contains('\u{1b}'),
            "terminal escapes must not reach the screen"
        );
        assert!(
            text.contains("Weighing") || text.contains("Sniffing") || text.contains("Counting")
        );
        assert!(text.contains("50%"));
        assert!(
            draw(
                &AppState {
                    help: true,
                    ..Default::default()
                },
                100,
                30
            )
            .contains("Shortcuts")
        );
    }

    #[test]
    fn helpers_format_humanly() {
        assert_eq!(theme::count(1_234_567), "1,234,567");
        assert_eq!(theme::short_count(1_650_000), "1.65M");
        assert_eq!(theme::ago(1000, Some(1000 - 180)), "3m ago");
        assert_eq!(theme::ago(1000, None), "never");
        assert_eq!(theme::bar(0.5, 4), "██  ");
        assert_eq!(theme::clean(b"a\x1bb"), "a\\u{1b}b");
    }

    /// `cargo test -p omg-cli preview_screens -- --ignored --nocapture` prints sample screens.
    #[test]
    #[ignore]
    fn preview_screens() {
        let child = |name: &str, gib: f64, files: u64, protected: u64| Child {
            path: name.as_bytes().to_vec(),
            kind: "directory".into(),
            allocated: (gib * (1u64 << 30) as f64) as u64,
            apparent: 0,
            files,
            dirs: 2,
            protected,
        };
        let mut state = AppState {
            rollups: true,
            root: Some("/home/demo".into()),
            home: Some("/home/demo".into()),
            now: 10_000,
            top: vec![
                child(".cache", 22.4, 480_000, 0),
                child("projects", 14.1, 90_000, 3),
                child(".rustup", 6.2, 12_000, 0),
                child("Downloads", 3.4, 120, 0),
            ],
            status: Some(Status {
                root: Some(b"/home/demo".to_vec()),
                last_successful: Some(9_820),
                last_report: None,
                entries: 1_556_000,
                unique_apparent: 0,
                unique_allocated: 69_074_563_072,
                capacity: Some(1_081_101_176_832),
                available: Some(939_947_728_896),
                volume_sample_at: None,
                scope_note: String::new(),
            }),
            trend_delta: Some(1_288_490_188),
            message: "Welcome to OMG".into(),
            ..Default::default()
        };
        state.children = state.top.clone();
        let theme = theme::Theme {
            mode: theme::Mode::Mono,
        };
        for tab in [Tab::Overview, Tab::Browse] {
            state.tab = tab;
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 28)).unwrap();
            terminal.draw(|f| render(f, &state, &theme)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            for y in 0..28 {
                let line: String = (0..100)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect();
                println!("{}", line.trim_end());
            }
        }
    }
}
