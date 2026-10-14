//! Keyboard-first terminal UI. A worker thread (`worker.rs`) owns the index; this thread only
//! handles keys, keeps an in-memory `AppState`, and redraws when something changed.
mod render;
mod theme;
mod worker;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use omg_core::{
    Index, ScanOptions, ScanReport,
    cleanup::DeletePlan,
    platform,
    rollup::{Child, Totals},
};
use render::{render, row_count, visible_children, visible_findings};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};
use worker::{Request, Response, Summary};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Overview,
    Browse,
    Findings,
    Trash,
}
impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Browse, Tab::Findings, Tab::Trash];
    pub fn name(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Browse => "Browse",
            Tab::Findings => "Findings",
            Tab::Trash => "OMG Trash",
        }
    }
}

pub struct ScanView {
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub started: Instant,
    /// When traversal ended and the finishing phase began.
    pub finishing_since: Option<Instant>,
}

/// A scan run by another process, polled from the database.
pub struct OtherScan {
    pub progress: u64,
    pub total: u64,
    pub walking_secs: f64,
    pub finishing_secs: Option<f64>,
}

pub struct DirView {
    pub children: Vec<Child>,
    pub marks: Vec<worker::RowMark>,
    pub total: Option<Totals>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Warn,
    Danger,
}

#[derive(Clone)]
pub enum Action {
    Scan(bool),
    Go(Tab),
    Help,
    Quit,
    Mark,
    DeleteNow,
    DeleteSelected,
    EmptyTrash,
    Unmark,
    Preview,
    Open,
    /// The single "are you sure?" confirmation (warnings are shown in the same dialog).
    Confirm(Box<DeletePlan>),
    /// Dismissed a plan that was created by "delete now": take the item back out of OMG Trash.
    /// Cancel a delete confirmation: take back items it added, and tell the backend to discard the plan.
    CancelPlan {
        ids: Vec<i64>,
        fingerprint: String,
    },
    AcknowledgePreview,
    ToggleAutoTrash,
}

pub struct Choice {
    pub label: String,
    pub key: Option<char>,
    pub action: Action,
    pub tone: Tone,
}

pub struct Menu {
    pub title: String,
    pub body: Vec<(String, Tone)>,
    pub choices: Vec<Choice>,
    pub cursor: usize,
    pub tone: Tone,
}

#[derive(Default)]
pub struct AppState {
    pub tab: Tab,
    pub summary: Option<Box<Summary>>,
    pub dirs: HashMap<Vec<u8>, DirView>,
    pub dir: Vec<u8>,
    pub cursor: usize,
    pub details: bool,
    pub tier: usize,
    pub plan: Option<omg_core::planning::Plan>,
    pub menu: Option<Menu>,
    pub selected: BTreeSet<i64>,
    pub help: bool,
    pub message: String,
    pub scan: Option<ScanView>,
    pub other: Option<OtherScan>,
    pub watching: bool,
    pub busy: Option<String>,
    pub tick: usize,
    pub now: i64,
    pub wsl: bool,
}

impl AppState {
    pub fn absolute(&self, relative: &[u8]) -> String {
        let rel = theme::clean(relative);
        match self.summary.as_ref().and_then(|s| s.root.clone()) {
            Some(root) => root.join(&rel).display().to_string(),
            None => rel,
        }
    }
    pub fn shorten_home(&self, path: &str) -> String {
        let home = self
            .summary
            .as_ref()
            .and_then(|s| s.home.as_ref())
            .and_then(|h| h.to_str())
            .map(str::to_owned);
        match home {
            Some(home) if path == home => "~".into(),
            Some(home) if path.strip_prefix(&home).is_some_and(|r| r.starts_with('/')) => {
                format!("~{}", &path[home.len()..])
            }
            _ => path.into(),
        }
    }
    pub fn current_dir(&self) -> Option<&DirView> {
        self.dirs.get(&self.dir)
    }
    /// The (root-relative path, estimated bytes) the cursor points at in Browse or Findings.
    fn target(&self) -> Option<(Vec<u8>, u64)> {
        match self.tab {
            Tab::Browse => visible_children(self)
                .get(self.cursor)
                .map(|c| (c.path.clone(), c.allocated)),
            Tab::Findings => visible_findings(self)
                .get(self.cursor)
                .map(|f| (f.entry.path.clone(), f.estimate)),
            _ => None,
        }
    }
    fn marked_ids(&self) -> Vec<i64> {
        self.summary
            .as_ref()
            .map(|s| s.marked.iter().map(|m| m.id).collect())
            .unwrap_or_default()
    }
}

type Job = std::thread::JoinHandle<Result<(ScanReport, Option<usize>)>>;

struct App {
    state: AppState,
    tx: Sender<Request>,
    cancel: Arc<AtomicBool>,
    job: Option<Job>,
    state_path: std::path::PathBuf,
}

impl App {
    fn request(&mut self, request: Request, busy: Option<&str>) {
        if let Some(label) = busy {
            self.state.busy = Some(label.into());
        }
        let _ = self.tx.send(request);
    }
    fn start_scan(&mut self, full: bool) -> Result<()> {
        if self.job.is_some() {
            self.state.message = "A scan is already running.".into();
            return Ok(());
        }
        if omg_core::index::lock_held(&self.state_path, "scan.lock") {
            self.state.message =
                "Another omg scan is running; showing the last published results until it finishes.".into();
            self.state.watching = true;
            self.request(Request::PollRunning, None);
            return Ok(());
        }
        let index = Index::open(&self.state_path)?;
        let mut options = index.options()?.unwrap_or(ScanOptions {
            root: platform::home()?,
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        });
        drop(index);
        options.workers = omg_core::scanner::default_workers();
        options.full = full || options.full;
        // The interactive user sees the "partial" label, so unreadable folders must not hide results.
        options.publish_partial = true;
        let progress = Arc::new(AtomicU64::new(0));
        self.cancel.store(false, Ordering::Relaxed);
        self.state.scan = Some(ScanView {
            progress: progress.clone(),
            total: omg_core::scanner::estimate_entries(&options.root).unwrap_or(0),
            started: Instant::now(),
            finishing_since: None,
        });
        let (path, flag) = (self.state_path.clone(), self.cancel.clone());
        self.job = Some(std::thread::spawn(move || {
            let mut index = Index::open(&path)?;
            let report = omg_core::scanner::scan(&mut index, &options, flag, progress)?;
            // Scan-time suggestions: regenerable caches go into OMG Trash (a list, not a deletion).
            let published = matches!(report.state.as_str(), "complete" | "partial");
            let suggested = if published && index.auto_trash()? {
                Some(index.suggest(&platform::home()?)?)
            } else {
                None
            };
            Ok((report, suggested))
        }));
        Ok(())
    }
    fn open_menu(&mut self) {
        let s = &self.state;
        let mut choices = vec![];
        let mut push = |label: &str, key: Option<char>, action: Action, tone: Tone| {
            choices.push(Choice {
                label: label.into(),
                key,
                action,
                tone,
            })
        };
        if s.target().is_some() {
            push(
                "Add to / remove from OMG Trash (saves the path only)",
                Some(' '),
                Action::Mark,
                Tone::Normal,
            );
            if omg_core::PERMANENT_DELETE {
                push(
                    "Delete permanently…",
                    Some('d'),
                    Action::DeleteNow,
                    Tone::Danger,
                );
            }
        }
        if s.tab == Tab::Findings && s.target().is_some() {
            push("Dry-run preview", Some('p'), Action::Preview, Tone::Normal);
            push(
                "Show in file manager",
                Some('o'),
                Action::Open,
                Tone::Normal,
            );
        }
        if s.tab == Tab::Trash && !s.marked_ids().is_empty() && omg_core::PERMANENT_DELETE {
            push(
                "Delete selected permanently…",
                Some('d'),
                Action::DeleteSelected,
                Tone::Danger,
            );
            push(
                "Empty OMG Trash permanently…",
                Some('D'),
                Action::EmptyTrash,
                Tone::Danger,
            );
            push(
                "Remove from OMG Trash",
                Some('u'),
                Action::Unmark,
                Tone::Normal,
            );
        }
        push("Scan now", Some('r'), Action::Scan(false), Tone::Normal);
        let auto = s.summary.as_ref().is_some_and(|x| x.auto_trash);
        push(
            if auto {
                "Auto-add caches to OMG Trash on scan: ON (turn off)"
            } else {
                "Auto-add caches to OMG Trash on scan: OFF (turn on)"
            },
            None,
            Action::ToggleAutoTrash,
            Tone::Normal,
        );
        push("Full rescan", Some('R'), Action::Scan(true), Tone::Normal);
        for tab in Tab::ALL {
            if tab != s.tab {
                push(
                    &format!("Go to {}", tab.name()),
                    None,
                    Action::Go(tab),
                    Tone::Normal,
                );
            }
        }
        push("Keyboard shortcuts", Some('?'), Action::Help, Tone::Normal);
        push("Quit", Some('q'), Action::Quit, Tone::Normal);
        self.state.menu = Some(Menu {
            title: "Options".into(),
            body: vec![],
            choices,
            cursor: 0,
            tone: Tone::Normal,
        });
    }
    fn confirm_delete(&mut self, plan: DeletePlan, added_now: bool) {
        let s = &self.state;
        let mut body = vec![(
            format!(
                "{} item(s) · ≈ {} · this cannot be undone",
                plan.items.len(),
                omg_core::bytes(plan.estimated)
            ),
            Tone::Danger,
        )];
        for item in plan.items.iter().take(6) {
            body.push((
                format!(
                    "  {}  ({})",
                    s.shorten_home(&s.absolute(&item.path)),
                    omg_core::bytes(item.allocated)
                ),
                Tone::Normal,
            ));
        }
        if plan.items.len() > 6 {
            body.push((
                format!("  … and {} more", plan.items.len() - 6),
                Tone::Normal,
            ));
        }
        if let Some(warning) = plan.items.iter().find_map(|i| i.warning.clone()) {
            body.push((format!("⚠ {warning}"), Tone::Warn));
        }
        for (path, reason) in plan.denied.iter().take(4) {
            body.push((
                format!("skipped {}: {reason}", theme::clean(path)),
                Tone::Warn,
            ));
        }
        let undo: Vec<i64> = if added_now {
            plan.items.iter().map(|i| i.id).collect()
        } else {
            vec![]
        };
        let mut choices = vec![Choice {
            label: "Cancel".into(),
            key: None,
            action: Action::CancelPlan {
                ids: undo,
                fingerprint: plan.fingerprint.clone(),
            },
            tone: Tone::Normal,
        }];
        if !plan.items.is_empty() {
            choices.push(Choice {
                label: "Yes, delete permanently".into(),
                key: None,
                action: Action::Confirm(Box::new(plan)),
                tone: Tone::Danger,
            });
        }
        self.state.menu = Some(Menu {
            title: "Are you sure?".into(),
            body,
            choices,
            cursor: 0,
            tone: Tone::Danger,
        });
    }
    fn run_action(&mut self, action: Action) -> Result<bool> {
        self.state.menu = None;
        if matches!(
            action,
            Action::DeleteNow | Action::DeleteSelected | Action::EmptyTrash | Action::Confirm(_)
        ) && !omg_core::PERMANENT_DELETE
        {
            self.state.message = "Permanent delete is not available in this build.".into();
            return Ok(false);
        }
        match action {
            Action::Quit => return Ok(true),
            Action::CancelPlan { ids, fingerprint } => {
                if !ids.is_empty() {
                    self.request(Request::Unmark(ids), None);
                }
                self.request(Request::DiscardPlan(fingerprint), None);
                self.state.message = "Cancelled. Nothing was deleted.".into();
            }
            Action::Scan(full) => self.start_scan(full)?,
            Action::Go(tab) => {
                self.state.tab = tab;
                self.state.cursor = 0;
                self.state.details = false;
            }
            Action::Help => self.state.help = true,
            // Space toggles: a saved item is taken back out of OMG Trash.
            Action::Mark => {
                if let Some((path, allocated)) = self.state.target() {
                    let saved = self
                        .state
                        .summary
                        .as_ref()
                        .and_then(|s| s.marked.iter().find(|m| m.path == path))
                        .map(|m| m.id);
                    match saved {
                        Some(id) => self.request(Request::Unmark(vec![id]), None),
                        None => self.request(Request::Mark { path, allocated }, None),
                    }
                }
            }
            Action::DeleteNow => {
                if let Some((path, allocated)) = self.state.target() {
                    self.request(Request::DeleteNow { path, allocated }, Some("Checking"));
                }
            }
            Action::DeleteSelected => {
                let ids: Vec<i64> = if self.state.selected.is_empty() {
                    self.state
                        .summary
                        .as_ref()
                        .and_then(|s| s.marked.get(self.state.cursor))
                        .map(|m| vec![m.id])
                        .unwrap_or_default()
                } else {
                    self.state.selected.iter().copied().collect()
                };
                if !ids.is_empty() {
                    self.request(Request::PrepareDelete(ids), Some("Checking"));
                }
            }
            Action::EmptyTrash => {
                let ids = self.state.marked_ids();
                if !ids.is_empty() {
                    self.request(Request::PrepareDelete(ids), Some("Checking"));
                }
            }
            Action::Unmark => {
                let ids: Vec<i64> = if self.state.selected.is_empty() {
                    self.state
                        .summary
                        .as_ref()
                        .and_then(|s| s.marked.get(self.state.cursor))
                        .map(|m| vec![m.id])
                        .unwrap_or_default()
                } else {
                    std::mem::take(&mut self.state.selected)
                        .into_iter()
                        .collect()
                };
                if !ids.is_empty() {
                    self.request(Request::Unmark(ids), None);
                }
            }
            Action::Preview => {
                if let Some(f) = visible_findings(&self.state).get(self.state.cursor) {
                    let id = f.id.clone();
                    self.request(Request::Preview(id), Some("Preparing preview"));
                }
            }
            Action::AcknowledgePreview => {
                if let Some(plan) = self.state.plan.take() {
                    self.request(Request::RecordPreview(Box::new(plan)), None);
                }
            }
            Action::Open => {
                let target = visible_findings(&self.state)
                    .get(self.state.cursor)
                    .map(|f| f.entry.clone());
                let root = self.state.summary.as_ref().and_then(|s| s.root.clone());
                if let (Some(entry), Some(root)) = (target, root) {
                    self.state.message = match omg_core::planning::open_location(&root, &entry) {
                        Ok(()) => "Asked the file manager to show it.".into(),
                        Err(error) => error.to_string(),
                    };
                }
            }
            // One confirmation is enough: warnings were already shown in that dialog.
            Action::Confirm(plan) => self.request(Request::Delete(plan), Some("Deleting")),
            Action::ToggleAutoTrash => {
                let on = !self.state.summary.as_ref().is_some_and(|x| x.auto_trash);
                self.request(Request::SetAutoTrash(on), None);
            }
        }
        Ok(false)
    }
    fn on_response(&mut self, response: Response) {
        let s = &mut self.state;
        match response {
            Response::Summary(summary) => {
                s.busy = None;
                s.summary = Some(summary);
                s.selected.retain(|id| {
                    s.summary
                        .as_ref()
                        .is_some_and(|m| m.marked.iter().any(|x| x.id == *id))
                });
                s.cursor = s.cursor.min(row_count(s).saturating_sub(1));
            }
            Response::Running(running) => match running {
                Some(r) => {
                    let now = omg_core::now();
                    let since_phase = r.phase_at.map(|t| (now - t).max(0) as f64);
                    s.other = Some(OtherScan {
                        progress: r.progress,
                        total: r.estimated_total,
                        walking_secs: if r.finishing {
                            ((r.phase_at.unwrap_or(now) - r.started).max(1)) as f64
                        } else {
                            ((now - r.started).max(1)) as f64
                        },
                        finishing_secs: if r.finishing { since_phase } else { None },
                    });
                }
                None => {
                    if s.watching && s.other.take().is_some() {
                        s.message = "The other scan finished; showing its results.".into();
                        s.dirs.clear();
                        let dir = s.dir.clone();
                        let _ = self.tx.send(Request::Summary);
                        let _ = self.tx.send(Request::Dir(dir));
                    }
                    s.watching = false;
                }
            },
            Response::Dir {
                path,
                children,
                marks,
                total,
            } => {
                s.busy = None;
                s.dirs.insert(
                    path,
                    DirView {
                        children,
                        marks,
                        total,
                    },
                );
                s.cursor = s.cursor.min(row_count(s).saturating_sub(1));
            }
            Response::Plan(plan) => {
                s.busy = None;
                s.plan = Some(*plan);
            }
            Response::DeletePlan { plan, added_now } => {
                s.busy = None;
                self.confirm_delete(*plan, added_now);
            }
            Response::Deleted(report) => {
                s.busy = None;
                s.dirs.clear();
                s.selected.clear();
                let observed = report
                    .observed_free_change
                    .map(|d| {
                        format!(
                            " · free space {}{}",
                            if d >= 0 { "+" } else { "−" },
                            omg_core::bytes(d.unsigned_abs())
                        )
                    })
                    .unwrap_or_default();
                let stop = if report.cancelled {
                    " · cancelled: remaining items were not started"
                } else {
                    ""
                };
                let stale = if report.needs_reconcile {
                    " · some entries were removed from a partly deleted folder; run a scan to refresh totals"
                } else {
                    ""
                };
                s.message = format!(
                    "Deleted {} item(s), ≈ {}{observed}{stop}{stale}{}{}",
                    report.deleted.len(),
                    omg_core::bytes(report.estimated_bytes),
                    if report.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " · {} skipped: {}",
                            report.skipped.len(),
                            report.skipped[0].1
                        )
                    },
                    if s.wsl {
                        " · WSL: Windows regains it only after the virtual disk is compacted (see ?)"
                    } else {
                        ""
                    }
                );
                let dir = s.dir.clone();
                let _ = self.tx.send(Request::Dir(dir));
            }
            Response::Info(text) => {
                s.busy = None;
                s.message = text;
            }
            Response::Error(text) => {
                s.busy = None;
                s.message = format!("⚠ {text}");
            }
        }
    }
    fn enter_dir(&mut self, path: Vec<u8>) {
        self.state.dir = path.clone();
        self.state.cursor = 0;
        if !self.state.dirs.contains_key(&path) {
            self.request(Request::Dir(path), Some("Loading"));
        }
    }
    /// Returns true to quit.
    fn handle_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(true);
        }
        if let Some(menu) = &mut self.state.menu {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    let cancel = menu
                        .choices
                        .first()
                        .map(|c| c.action.clone())
                        .filter(|a| matches!(a, Action::CancelPlan { .. }));
                    self.state.menu = None;
                    if let Some(action) = cancel {
                        return self.run_action(action);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => menu.cursor = menu.cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    menu.cursor = (menu.cursor + 1).min(menu.choices.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let action = menu.choices[menu.cursor].action.clone();
                    return self.run_action(action);
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let n = c as usize - '1' as usize;
                    if let Some(choice) = menu.choices.get(n) {
                        let action = choice.action.clone();
                        return self.run_action(action);
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.state.help {
            self.state.help = false;
            return Ok(false);
        }
        if let Some(_plan) = &self.state.plan {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => {
                    return self.run_action(Action::AcknowledgePreview);
                }
                KeyCode::Esc | KeyCode::Char('q') => self.state.plan = None,
                _ => {}
            }
            return Ok(false);
        }
        let rows = row_count(&self.state);
        let tab = self.state.tab;
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => self.state.help = true,
            KeyCode::Char('m') | KeyCode::Char(':') | KeyCode::Char('/') => self.open_menu(),
            KeyCode::Esc => {
                if self.state.scan.is_some() {
                    self.cancel.store(true, Ordering::Relaxed);
                    self.state.message =
                        "Stopping the scan… the previous index stays valid.".into();
                } else if self.state.details {
                    self.state.details = false;
                } else if tab == Tab::Browse && !self.state.dir.is_empty() {
                    self.go_up();
                }
            }
            KeyCode::Char(c @ '1'..='4') => self
                .run_action(Action::Go(Tab::ALL[c as usize - '1' as usize]))
                .map(|_| ())?,
            KeyCode::Tab => {
                let next = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0) + 1;
                self.run_action(Action::Go(Tab::ALL[next % Tab::ALL.len()]))?;
            }
            KeyCode::BackTab => {
                let i = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
                self.run_action(Action::Go(
                    Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()],
                ))?;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.cursor = (self.state.cursor + 1).min(rows.saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.cursor = self.state.cursor.saturating_sub(1)
            }
            KeyCode::PageDown => {
                self.state.cursor = (self.state.cursor + 10).min(rows.saturating_sub(1))
            }
            KeyCode::PageUp => self.state.cursor = self.state.cursor.saturating_sub(10),
            KeyCode::Char('r') => self.start_scan(false)?,
            KeyCode::Char('R') => self.start_scan(true)?,
            KeyCode::Enter | KeyCode::Right if tab == Tab::Browse => {
                let next = visible_children(&self.state)
                    .get(self.state.cursor)
                    .filter(|c| c.kind == "directory")
                    .map(|c| c.path.clone());
                if let Some(path) = next {
                    self.enter_dir(path);
                }
            }
            KeyCode::Backspace | KeyCode::Left if tab == Tab::Browse => self.go_up(),
            KeyCode::Enter if tab == Tab::Findings => self.state.details = !self.state.details,
            KeyCode::Char(' ') if matches!(tab, Tab::Browse | Tab::Findings) => {
                self.run_action(Action::Mark).map(|_| ())?
            }
            KeyCode::Char(' ') if tab == Tab::Trash => {
                if let Some(id) = self
                    .state
                    .summary
                    .as_ref()
                    .and_then(|s| s.marked.get(self.state.cursor))
                    .map(|m| m.id)
                {
                    if !self.state.selected.remove(&id) {
                        self.state.selected.insert(id);
                    }
                    self.state.cursor = (self.state.cursor + 1).min(rows.saturating_sub(1));
                }
            }
            KeyCode::Char('d') if matches!(tab, Tab::Browse | Tab::Findings) => {
                self.run_action(Action::DeleteNow).map(|_| ())?
            }
            KeyCode::Char('d') if tab == Tab::Trash => {
                self.run_action(Action::DeleteSelected).map(|_| ())?
            }
            KeyCode::Char('D') if tab == Tab::Trash => {
                self.run_action(Action::EmptyTrash).map(|_| ())?
            }
            KeyCode::Char('u') if tab == Tab::Trash => {
                self.run_action(Action::Unmark).map(|_| ())?
            }
            KeyCode::Char('t') if tab == Tab::Findings => {
                self.state.tier = (self.state.tier + 1) % 4;
                self.state.cursor = 0;
            }
            KeyCode::Char('p') if tab == Tab::Findings => {
                self.run_action(Action::Preview).map(|_| ())?
            }
            KeyCode::Char('o') if tab == Tab::Findings => {
                self.run_action(Action::Open).map(|_| ())?
            }
            _ => {}
        }
        Ok(false)
    }
    fn go_up(&mut self) {
        if self.state.dir.is_empty() {
            return;
        }
        let cut = self.state.dir.iter().rposition(|b| *b == b'/').unwrap_or(0);
        let mut parent = self.state.dir.clone();
        parent.truncate(cut);
        self.enter_dir(parent);
    }
}

pub fn run(index: Index) -> Result<()> {
    let theme = theme::Theme::detect();
    let state_path = index.state.clone();
    let has_scan = index.status()?.root.is_some();
    drop(index);
    let (tx, rx) = worker::spawn(state_path.clone());
    let _ = tx.send(Request::Summary);
    let _ = tx.send(Request::Dir(Vec::new()));
    // Housekeeping off the UI thread: drop rows of superseded scans.
    let prune_path = state_path.clone();
    std::thread::spawn(move || {
        if let Ok(index) = Index::open(&prune_path) {
            let _ = index.prune_stale(Duration::from_secs(120));
        }
    });
    let mut app = App {
        state: AppState {
            message:
                "Welcome to OMG — it finds it, explains it, and you decide. Press m for options."
                    .into(),
            busy: Some("Loading".into()),
            wsl: std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .is_ok_and(|v| v.to_lowercase().contains("microsoft")),
            ..Default::default()
        },
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        job: None,
        state_path,
    };
    if omg_core::index::lock_held(&app.state_path, "scan.lock") {
        app.state.watching = true;
        app.state.message = "Another omg scan is running; results refresh when it finishes.".into();
        app.request(Request::PollRunning, None);
    } else if !has_scan {
        app.start_scan(false)?;
    }
    ratatui::run(|terminal| {
        let mut dirty = true;
        let started = Instant::now();
        let mut last_poll = Instant::now();
        let result = (|| -> Result<()> {
            loop {
                while let Ok(response) = rx.try_recv() {
                    app.on_response(response);
                    dirty = true;
                }
                if app.job.as_ref().is_some_and(|j| j.is_finished()) {
                    app.state.message = match app.job.take().map(|j| j.join()) {
                        Some(Ok(Ok((r, suggested)))) => format!(
                            "Scan {}: {} entries, {} unreadable folders.{}",
                            r.state,
                            theme::count(r.entries),
                            r.errors,
                            suggested
                                .map(|n| format!(
                                    " {n} cache folder(s) waiting in OMG Trash (press 4)."
                                ))
                                .unwrap_or_default()
                        ),
                        Some(Ok(Err(e))) => {
                            format!("⚠ Scan failed: {e:#}. The previous index is untouched.")
                        }
                        _ => "⚠ Scan failed; the previous index is untouched.".into(),
                    };
                    app.state.scan = None;
                    app.state.dirs.clear();
                    let dir = app.state.dir.clone();
                    app.request(Request::Summary, None);
                    app.request(Request::Dir(dir), None);
                    dirty = true;
                }
                if let Some(scan) = &mut app.state.scan
                    && scan.finishing_since.is_none()
                    && scan.progress.load(Ordering::Relaxed) & omg_core::scanner::FINISHING != 0
                {
                    scan.finishing_since = Some(Instant::now());
                }
                if app.state.watching && last_poll.elapsed() >= Duration::from_millis(800) {
                    last_poll = Instant::now();
                    app.request(Request::PollRunning, None);
                }
                let animating =
                    app.state.scan.is_some() || app.state.busy.is_some() || app.state.watching;
                // Spinner speed follows the clock, so the loop can poll quickly for worker replies.
                let tick = (started.elapsed().as_millis() / 100) as usize;
                if animating && tick != app.state.tick {
                    app.state.tick = tick;
                    dirty = true;
                }
                if dirty {
                    app.state.now = omg_core::now();
                    terminal.draw(|frame| render(frame, &app.state, &theme))?;
                    dirty = false;
                }
                if !event::poll(Duration::from_millis(if animating { 15 } else { 250 }))? {
                    continue;
                }
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        dirty = true;
                        if app.handle_key(key)? {
                            break;
                        }
                    }
                    Event::Resize(..) => dirty = true,
                    _ => {}
                }
            }
            Ok(())
        })();
        app.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = app.job.take() {
            let _ = handle.join();
        }
        result
    })
}

#[cfg(test)]
mod tests;
