use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use omg_core::{
    Index, ScanOptions, bytes, catalog::Finding, display_path, planning::Plan, platform,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Paragraph},
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

type Refresh = Option<std::thread::JoinHandle<Result<omg_core::ScanReport>>>;

#[derive(Default)]
struct View {
    offset: usize,
    cursor: usize,
    search: String,
    searching: bool,
    details: bool,
    history: bool,
    ignores: bool,
    tier: usize,
    category: usize,
    minimum: u64,
    selected: BTreeSet<String>,
    plan: Option<Plan>,
    confirmation: bool,
}

fn visible(mut findings: Vec<Finding>, view: &View) -> Vec<Finding> {
    let tiers = ["", "review", "info", "protected"];
    let categories = ["", "package_caches", "large_files"];
    findings.retain(|f| {
        (view.search.is_empty()
            || display_path(&f.entry.path)
                .to_lowercase()
                .contains(&view.search.to_lowercase())
            || f.rule
                .name
                .to_lowercase()
                .contains(&view.search.to_lowercase()))
            && (view.tier == 0 || f.effective_tier == tiers[view.tier])
            && (view.category == 0 || f.rule.category == categories[view.category])
            && f.entry.allocated >= view.minimum
    });
    findings.sort_by(|a, b| {
        a.rule
            .category
            .cmp(&b.rule.category)
            .then(b.entry.allocated.cmp(&a.entry.allocated))
    });
    findings
}

fn render(
    frame: &mut Frame<'_>,
    index: &Index,
    view: &View,
    findings: &[Finding],
    message: &str,
    progress: u64,
) -> Result<()> {
    let area = frame.area();
    if area.width < 50 || area.height < 14 {
        frame.render_widget(
            Paragraph::new("Terminal too small. Resize; q quit / c cancel. Execution disabled."),
            area,
        );
        return Ok(());
    }
    let regions = Layout::vertical([
        Constraint::Length(7),
        Constraint::Min(4),
        Constraint::Length(4),
    ])
    .split(area);
    let status = index.status()?;
    let header = format!(
        "Scope {} | last success {:?} | latest {}\nInventory estimate: {} allocated / {} apparent\nVolume capacity {} | available {} (sample {:?})\nPotentially actionable estimate: 0 B | selected actionable estimate: 0 B\nNo qualified cleanup handler; {} selected for explanation preview only | refresh {}",
        status
            .root
            .as_deref()
            .map(display_path)
            .unwrap_or("home first-run scope".into()),
        status.last_successful,
        status
            .last_report
            .as_ref()
            .map(|r| r.state.as_str())
            .unwrap_or("unscanned"),
        bytes(status.unique_allocated),
        bytes(status.unique_apparent),
        status.capacity.map(bytes).unwrap_or("unknown".into()),
        status.available.map(bytes).unwrap_or("unknown".into()),
        status.volume_sample_at,
        view.selected.len(),
        progress
    );
    frame.render_widget(
        Paragraph::new(header)
            .block(Block::bordered().title("OMG — local review (cleanup disabled)")),
        regions[0],
    );
    let (title, content) = if let Some(plan) = &view.plan {
        (
            "Exact read-only plan",
            format!(
                "{}\n{} targets; {} estimate; actionable 0 B\n{}\n{}",
                plan.fingerprint,
                plan.items.len(),
                bytes(plan.estimated),
                plan.items
                    .iter()
                    .map(|i| format!(
                        "{}\n{} | {}\nWarnings: {}",
                        display_path(&i.path),
                        i.action,
                        i.decision,
                        i.warnings.join("; ")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                if view.confirmation {
                    "CANCEL DEFAULT — Esc cancels. y acknowledges preview only; no operation can execute."
                } else {
                    "Enter: review acknowledgement | Esc: back; filesystem execution unavailable"
                }
            ),
        )
    } else if view.history {
        (
            "Local history",
            index
                .history()?
                .iter()
                .map(|h| format!("{} {} {} {}", h.id, h.created, h.kind, h.outcome))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else if view.ignores {
        (
            "Ignore scopes (u removes selected row)",
            index
                .ignores()?
                .iter()
                .enumerate()
                .map(|(n, i)| {
                    format!(
                        "{} {} rule {} path {} subtree {}",
                        if n == view.cursor { ">" } else { " " },
                        i.id,
                        i.rule,
                        display_path(&i.path),
                        i.subtree
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else if view.details {
        ("Explanation / evidence / consequences",findings.get(view.cursor).map(|f|format!("{}\n{}\nTier {} | action {}\n{}\nEvidence {}\nEffects {}\nRegeneration {}\nWarnings {}\nSource {}",f.rule.name,display_path(&f.entry.path),f.effective_tier,f.action,f.reason,f.evidence,f.rule.side_effects,f.rule.regeneration,f.rule.warnings.join("; "),f.rule.source)).unwrap_or("No finding selected".into()))
    } else {
        let rows = findings
            .iter()
            .enumerate()
            .map(|(n, f)| {
                format!(
                    "{} {} | {} | {} | {}",
                    if n == view.cursor { ">" } else { " " },
                    f.rule.category,
                    bytes(f.entry.allocated),
                    f.effective_tier,
                    display_path(&f.entry.path)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        (
            "Findings grouped by category (paged)",
            if rows.is_empty() {
                "No matching findings in this metadata page. Page with ]/[ or refresh; not proof of no findings elsewhere.".into()
            } else {
                rows
            },
        )
    };
    frame.render_widget(
        Paragraph::new(content).block(Block::bordered().title(title)),
        regions[1],
    );
    let legend = format!(
        "q quit | c cancel | Esc back | r/R reconcile | p preview\nj/k navigate | ]/[ page | Enter detail | / search {}\nt tier | g category | m size | i/I ignores | h history\n{message}",
        view.search
    );
    frame.render_widget(Paragraph::new(legend), regions[2]);
    Ok(())
}

pub fn run(index: Index) -> Result<()> {
    ratatui::run(|terminal| {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let mut refresh: Refresh = None;
        let mut message="Scoped coverage excludes protected/cloud locations; snapshots and shared blocks unknown".into();
        let mut view = View::default();
        let mut first = true;
        let result = (|| -> Result<()> {
            loop {
                if refresh.as_ref().is_some_and(|h| h.is_finished()) {
                    message = match refresh.take().unwrap().join() {
                        Ok(Ok(r)) => format!(
                            "Refresh {}: {} entries, {} errors",
                            r.state, r.entries, r.errors
                        ),
                        _ => "Refresh failed; prior valid inventory retained".into(),
                    };
                    view.selected.clear();
                    view.plan = None;
                    view.confirmation = false;
                }
                let findings = visible(index.findings(500, view.offset, false)?, &view);
                let row_count = if view.ignores {
                    index.ignores()?.len()
                } else {
                    findings.len()
                };
                view.cursor = view.cursor.min(row_count.saturating_sub(1));
                let mut draw_error = None;
                terminal.draw(|frame| {
                    if let Err(error) = render(
                        frame,
                        &index,
                        &view,
                        &findings,
                        &message,
                        progress.load(Ordering::Relaxed),
                    ) {
                        draw_error = Some(error)
                    }
                })?;
                if let Some(error) = draw_error {
                    return Err(error);
                }
                if first {
                    first = false;
                    if index
                        .status()?
                        .last_successful
                        .is_none_or(|t| omg_core::now() - t > 3600)
                    {
                        start_refresh(&index, &cancel, &progress, &mut refresh)?;
                    }
                }
                if !event::poll(Duration::from_millis(100))? {
                    continue;
                }
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if view.searching {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => view.searching = false,
                        KeyCode::Backspace => {
                            view.search.pop();
                        }
                        KeyCode::Char(c) if !c.is_control() && view.search.len() < 256 => {
                            view.search.push(c)
                        }
                        _ => (),
                    };
                    continue;
                }
                match key.code {
                    KeyCode::Char('q')=>break,
                    KeyCode::Esc=>{view.details=false;view.history=false;view.ignores=false;view.plan=None;view.confirmation=false;},
                    KeyCode::Char('c')=>cancel.store(true,Ordering::Relaxed),
                    KeyCode::Char('r'|'R') if refresh.is_none()=>start_refresh(&index,&cancel,&progress,&mut refresh)?,
                    KeyCode::Char('j')|KeyCode::Down=>view.cursor=(view.cursor+1).min(row_count.saturating_sub(1)),
                    KeyCode::Char('k')|KeyCode::Up=>view.cursor=view.cursor.saturating_sub(1),
                    KeyCode::Char(']')=>{view.offset+=500;view.cursor=0;},KeyCode::Char('[')=>{view.offset=view.offset.saturating_sub(500);view.cursor=0;},
                    KeyCode::Char('/')=>view.searching=true,KeyCode::Char('t')=>view.tier=(view.tier+1)%4,
                    KeyCode::Char('g')=>view.category=(view.category+1)%3,KeyCode::Char('m')=>view.minimum=if view.minimum==0{100*1024*1024}else{0},
                    KeyCode::Char('h')=>{view.history = !view.history;view.ignores=false;},KeyCode::Char('I')=>{view.ignores = !view.ignores;view.history=false;},
                    KeyCode::Char('u') if view.ignores=>{if let Some(item)=index.ignores()?.get(view.cursor){index.remove_ignore(item.id)?;message="Ignore removed; no target selected".into();}},
                    KeyCode::Char('i')=>{if let Some(finding)=findings.get(view.cursor){index.ignore_finding(finding,false)?;message="Ignored exact rule/path/scope. I reviews scopes; u removes.".into();}},
                    KeyCode::Char(' ')=>message="Selection denied: all current handlers are informational; tier never authorizes cleanup".into(),
                    KeyCode::Char('p')=>{if let Some(finding)=findings.get(view.cursor){match index.preview(std::slice::from_ref(&finding.id)){Ok(plan)=>{view.plan=Some(plan);view.confirmation=false;},Err(e)=>message=e.to_string()}}},
                    KeyCode::Enter if view.plan.is_some()=>view.confirmation=true,
                    KeyCode::Char('y') if view.confirmation=>{if let Some(plan)=&view.plan{index.record_preview(plan)?;message="Preview acknowledged and logged. 0 mutations; 0 physical bytes freed claimed.".into();}view.confirmation=false;view.plan=None;},
                    KeyCode::Enter=>view.details = !view.details,
                    KeyCode::Char('o')=>{if let Some(finding)=findings.get(view.cursor){let root=index.status()?.root.map(|raw|platform::path(&raw)).transpose()?.ok_or_else(||anyhow::anyhow!("no active root"))?;
                        message=match omg_core::planning::open_location(&root,&finding.entry){Ok(())=>"Requested native file manager location".into(),Err(e)=>e.to_string()};}},
                    _=>(),
                }
            }
            Ok(())
        })();
        cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = refresh {
            let _ = handle.join();
        }
        result
    })
}
fn start_refresh(
    index: &Index,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<AtomicU64>,
    handle: &mut Refresh,
) -> Result<()> {
    let options = index.options()?.unwrap_or(ScanOptions {
        root: platform::home()?,
        workers: 2,
        exclusions: vec![],
        full: true,
    });
    let state = index.state.clone();
    let cancel = cancel.clone();
    let progress = progress.clone();
    cancel.store(false, Ordering::Relaxed);
    progress.store(0, Ordering::Relaxed);
    *handle = Some(std::thread::spawn(move || {
        let mut index = Index::open(&state)?;
        omg_core::scanner::scan(&mut index, &options, cancel, progress)
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "linux")]
    fn narrow_and_standard_terminal_rendering_keeps_controls() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let index = Index::open(state.path()).unwrap();
        for (width, height) in [(40, 10), (60, 20), (80, 24), (120, 40)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal
                .draw(|f| render(f, &index, &View::default(), &[], "fixture", 0).unwrap())
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("quit"));
        }
    }
}
