use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use omg_core::{Index, ScanOptions, bytes, display_path, platform};
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Paragraph},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

type Refresh = Option<std::thread::JoinHandle<Result<omg_core::ScanReport>>>;
pub fn run(index: Index) -> Result<()> {
    ratatui::run(|terminal| {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let mut refresh: Refresh = None;
        let mut message = "Read-only inventory; estimates are not physical space freed".to_string();
        let mut offset: usize = 0;
        let mut first = true;
        let result = (|| -> Result<()> {
            loop {
                if refresh.as_ref().is_some_and(|h| h.is_finished()) {
                    let outcome = refresh.take().unwrap().join();
                    message = match outcome {
                        Ok(Ok(report)) => format!(
                            "Refresh {}: {} entries, {} errors",
                            report.state, report.entries, report.errors
                        ),
                        _ => "Refresh failed; last valid snapshot retained".into(),
                    };
                }
                let status = index.status()?;
                let entries = index.entries(50, offset)?;
                terminal.draw(|frame| {
                    let area=frame.area();
                    if area.width<50 || area.height<12 {frame.render_widget(Paragraph::new("Terminal too small; resize. q quit / c cancel"),area);return;}
                    let regions=Layout::vertical([Constraint::Length(8),Constraint::Min(2),Constraint::Length(3)]).split(area);
                    let overview=format!("Scope: {}\nLast successful: {:?}; latest attempt: {}\nScoped allocated estimate: {}; apparent: {}\nVolume capacity: {}; available sample: {}\nCoverage: {}\nRefresh entries: {}",status.root.as_deref().map(display_path).unwrap_or("first run — home scope".into()),status.last_successful,status.last_report.as_ref().map(|r|r.state.as_str()).unwrap_or("not scanned"),bytes(status.unique_allocated),bytes(status.unique_apparent),status.capacity.map(bytes).unwrap_or("unknown".into()),status.available.map(bytes).unwrap_or("unknown".into()),status.scope_note,progress.load(Ordering::Relaxed));
                    frame.render_widget(Paragraph::new(overview).block(Block::bordered().title("OMG — analysis only")),regions[0]);
                    let rows=entries.iter().map(|e|format!("{}  {}  {}{}",bytes(e.allocated),e.kind,display_path(&e.path),if e.excluded.is_some(){" [excluded]"}else{""})).collect::<Vec<_>>().join("\n");
                    frame.render_widget(Paragraph::new(rows).block(Block::bordered().title("Largest indexed entries (directory size is own metadata)")),regions[1]);
                    frame.render_widget(Paragraph::new(format!("{message}\nr/R full reconciliation | j/k scroll | c cancel | q quit")),regions[2]);
                })?;
                if first {
                    first = false;
                    if status
                        .last_successful
                        .is_none_or(|t| omg_core::now() - t > 3600)
                    {
                        start_refresh(&index, &cancel, &progress, &mut refresh)?;
                    }
                }
                if event::poll(Duration::from_millis(100))?
                    && let Event::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') => cancel.store(true, Ordering::Relaxed),
                        KeyCode::Char('r' | 'R') if refresh.is_none() => {
                            start_refresh(&index, &cancel, &progress, &mut refresh)?
                        }
                        KeyCode::Char('j') | KeyCode::Down => offset += 1,
                        KeyCode::Char('k') | KeyCode::Up => offset = offset.saturating_sub(1),
                        _ => (),
                    }
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
