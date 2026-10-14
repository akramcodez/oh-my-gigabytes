use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use omg_core::{Index, ScanOptions, platform};
#[cfg(target_os = "linux")]
mod svc;
use std::{
    io::IsTerminal,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
mod tui;

#[derive(Parser)]
#[command(
    name = "omg",
    version,
    about = "Local-first storage intelligence. Analysis-only development build."
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Service configuration file (default: $XDG_CONFIG_HOME/omg/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Scan {
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        full: bool,
        /// Publish the result even if some directories were unreadable (labelled partial)
        #[arg(long)]
        accept_gaps: bool,
        /// Don't add regenerable cache folders to OMG Trash after this scan
        #[arg(long)]
        no_suggest: bool,
        /// Scanner threads (default: up to 8, based on CPU count)
        #[arg(long)]
        workers: Option<usize>,
        #[arg(long)]
        exclude: Vec<PathBuf>,
    },
    Status,
    /// Show OMG Trash (saved paths waiting for your decision; nothing has been deleted)
    Trash {
        /// Recompute scan suggestions from the current index first (no rescan)
        #[arg(long)]
        refresh: bool,
    },
    /// Finish removing rows of superseded scans (housekeeping; scans only prune for a few seconds)
    Prune,
    Doctor,
    /// Start the scan-only background service (asks before registering it)
    Start,
    /// Stop the background service
    Stop,
    /// Background service registration and control (never cleans up files)
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Observed free-space and growth history from local scans
    Trends {
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
    /// Largest subdirectories and files under a path in the scanned scope (subtree totals)
    Tree {
        /// Path inside the scan root (absolute or root-relative); default is the root
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 25)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    Findings {
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    Ignores {
        #[arg(long)]
        remove: Option<i64>,
    },
    Cleanup {
        #[arg(long, required = true)]
        dry_run: bool,
        #[arg(long,required=true,num_args=1..)]
        candidate: Vec<String>,
    },
    History,
}
// Only the Linux service controller (svc.rs) reads these fields.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Subcommand)]
enum ServiceCommand {
    /// Register a per-user systemd service (explicit approval required)
    Install {
        /// Also start it at login
        #[arg(long)]
        enable: bool,
        /// Approve registration non-interactively
        #[arg(long)]
        yes: bool,
    },
    /// Stop and remove the per-user registration; local state is kept
    Uninstall,
    Start {
        #[arg(long)]
        yes: bool,
    },
    Stop,
    Status,
    /// Ask a running service for a low-priority scan
    Scan,
    /// Run the service in the foreground (used by the registered unit)
    Run,
}
#[derive(Subcommand)]
enum CatalogCommand {
    Status,
    Import {
        payload: PathBuf,
        signature: PathBuf,
    },
    Rollback,
}
/// Live one-line progress on stderr (terminals only); the JSON report on stdout is unaffected.
struct Meter {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Meter {
    fn finish(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
fn progress_meter(root: &std::path::Path, progress: Arc<std::sync::atomic::AtomicU64>) -> Meter {
    let stop = Arc::new(AtomicBool::new(false));
    if !std::io::stderr().is_terminal() {
        return Meter { stop, handle: None };
    }
    let total = omg_core::scanner::estimate_entries(root).unwrap_or(0);
    let flag = stop.clone();
    let handle = std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let frames = ["·", "✢", "✳", "✶", "✻", "✽"];
        let mut tick = 0usize;
        let mut walked: Option<f64> = None;
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            let raw = progress.load(std::sync::atomic::Ordering::Relaxed);
            let done = raw & !omg_core::scanner::FINISHING;
            let secs = started.elapsed().as_secs_f64().max(0.001);
            let finishing = raw & omg_core::scanner::FINISHING != 0;
            if finishing && walked.is_none() {
                walked = Some(secs);
            }
            let left = omg_core::scanner::eta(
                done,
                total,
                walked.unwrap_or(secs),
                walked.map(|w| secs - w),
            );
            let detail = if finishing {
                format!(
                    "finishing up: comparing, totalling folders, saving · {done} entries · ~{left:.0}s left"
                )
            } else if total > 0 && done > 0 {
                format!(
                    "{} / ~{} · ≤{:.0}% · {:.0}/s · ~{:.0}s left",
                    done,
                    total,
                    (done as f64 / total as f64 * 100.0).min(100.0),
                    done as f64 / secs,
                    left
                )
            } else {
                format!("{done} entries · {:.0}/s", done as f64 / secs)
            };
            eprint!("\r\x1b[2K{} Weighing gigabytes… {detail}", frames[tick % 6]);
            tick += 1;
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        eprint!("\r\x1b[2K");
    });
    Meter {
        stop,
        handle: Some(handle),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("omg: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    #[cfg(target_os = "linux")]
    if matches!(
        cli.command,
        Some(Command::Start | Command::Stop | Command::Service { .. })
    ) {
        return svc::control(&cli);
    }
    #[cfg(not(target_os = "linux"))]
    if matches!(
        cli.command,
        Some(Command::Start | Command::Stop | Command::Service { .. })
    ) {
        bail!("background service is not supported on this platform");
    }
    if matches!(cli.command, Some(Command::Doctor)) && !platform::supported() {
        println!("omg: this platform is not certified; native scanning and cleanup disabled");
        return Ok(());
    }
    let state = cli.state_dir.unwrap_or(platform::default_state()?);
    if matches!(cli.command, Some(Command::Doctor)) {
        let diagnosis = omg_core::index::diagnose(&state)?;
        println!("omg: {}", serde_json::to_string_pretty(&diagnosis)?);
        let scope = if omg_core::PERMANENT_DELETE {
            "(Linux: ext-family, tmpfs, Btrfs; unknown filesystems refused)"
        } else {
            "(not included in this analysis-only build)"
        };
        println!(
            "permanent delete: {} {scope}; OS Trash: {}; cloud/FUSE/remote profiles excluded",
            omg_core::capability::platform_permanent_delete().as_str(),
            omg_core::capability::os_trash().as_str()
        );
        #[cfg(target_os = "linux")]
        println!(
            "service: {}",
            serde_json::to_string_pretty(&svc::registration()?)?
        );
        return Ok(());
    }
    // Refuse before opening state: a nonterminal run must not create or migrate anything.
    if cli.command.is_none()
        && (!std::io::stdin().is_terminal() || !std::io::stdout().is_terminal())
    {
        bail!("interactive TUI requires a terminal; use omg status or omg scan --root PATH");
    }
    let mut index = Index::open(&state)?;
    match cli.command {
        Some(Command::Scan {
            root,
            full,
            accept_gaps,
            no_suggest,
            workers,
            exclude,
        }) => {
            let saved = index.options()?;
            let root = root
                .or_else(|| saved.as_ref().map(|o| o.root.clone()))
                .unwrap_or(platform::home()?);
            let exclusions = if exclude.is_empty() {
                saved.map(|o| o.exclusions).unwrap_or_default()
            } else {
                exclude
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = cancel.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            let options = ScanOptions {
                workers: workers.unwrap_or_else(omg_core::scanner::default_workers),
                root,
                exclusions,
                full,
                publish_partial: accept_gaps,
            };
            let progress = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let meter = progress_meter(&options.root, progress.clone());
            #[cfg(target_os = "linux")]
            let report = svc::scan_with_handoff(&mut index, &options, cancel, progress);
            #[cfg(not(target_os = "linux"))]
            let report = omg_core::scanner::scan(&mut index, &options, cancel, progress);
            meter.finish();
            let report = report?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            match (report.state.as_str(), accept_gaps) {
                ("complete", _) => (),
                ("partial", true) => eprintln!(
                    "omg: published as partial: {} directories could not be read (permission or changes during scan)",
                    report.errors
                ),
                ("partial", false) => bail!(
                    "scan finished with {} unreadable/changed directories and was not published; re-run with --accept-gaps to publish it labelled partial",
                    report.errors
                ),
                _ => bail!("scan incomplete; last valid index retained"),
            }
            if !no_suggest && index.auto_trash()? {
                let listed = index.suggest(&platform::home()?)?;
                eprintln!(
                    "omg: {listed} cache folder(s) are listed in OMG Trash (nothing deleted; review with `omg trash`)"
                );
            }
        }
        Some(Command::Status) => {
            #[allow(unused_mut)]
            let mut value = serde_json::to_value(index.status()?)?;
            // Estimate only: deduplicated finding totals, not reclaimable or freed space.
            value["explained_estimate_bytes"] = index
                .findings(1000, 0, false)?
                .iter()
                .filter(|f| f.effective_tier != "protected")
                .map(|f| f.estimate)
                .sum::<u64>()
                .into();
            #[cfg(target_os = "linux")]
            {
                value["service"] = omg_core::service::daemon::health(&index)?;
            }
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Some(Command::Trash { refresh }) => {
            if refresh {
                index.suggest(&platform::home()?)?;
            }
            let items: Vec<_> = index
                .marked()?
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "path": omg_core::display_path(&m.path),
                        "size": omg_core::bytes(m.allocated),
                        "allocated": m.allocated,
                        "source": m.source,
                        "warning": m.warning,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        Some(Command::Prune) => {
            let done = index.prune_stale(std::time::Duration::from_secs(600))?;
            println!(
                "{}",
                if done {
                    "nothing stale remains"
                } else {
                    "more stale data remains; run `omg prune` again"
                }
            );
        }
        Some(Command::Tree {
            path,
            limit,
            offset,
        }) => {
            let status = index.status()?;
            let root = platform::path(
                &status
                    .root
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("no scan yet; run `omg scan`"))?,
            )?;
            if !index.rollups_available()? {
                bail!("directory totals are unavailable for this index; run `omg scan` once");
            }
            let relative = match &path {
                None => PathBuf::new(),
                Some(p) if p.is_absolute() => p
                    .strip_prefix(&root)
                    .map_err(|_| anyhow::anyhow!("path is outside the scanned root"))?
                    .to_path_buf(),
                Some(p) => p.clone(),
            };
            if !relative.as_os_str().is_empty() && !omg_core::policy::safe_relative(&relative) {
                bail!("path must be inside the scanned root without `..` components");
            }
            let raw = platform::raw(&relative)?;
            let totals = index
                .subtree(&raw)?
                .ok_or_else(|| anyhow::anyhow!("not a scanned directory"))?;
            let children: Vec<_> = index
                .children(&raw, limit, offset)?
                .into_iter()
                .map(|c| {
                    serde_json::json!({
                        "path": omg_core::display_path(&c.path),
                        "kind": c.kind,
                        "allocated": c.allocated,
                        "allocated_human": omg_core::bytes(c.allocated),
                        "apparent": c.apparent,
                        "files": c.files,
                        "directories": c.dirs,
                        "protected_or_excluded": c.protected,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "directory": omg_core::display_path(&raw),
                    "totals": totals,
                    "scan_state": status.last_report.as_ref().map(|r| r.state.clone()),
                    "last_successful": status.last_successful,
                    "note": "Allocated bytes from scan metadata; hard links counted once; shared/cloned blocks unknown; excluded entries are counted but not traversed.",
                    "children": children,
                }))?
            );
        }
        Some(Command::Trends { days }) => {
            println!("{}", serde_json::to_string_pretty(&index.trend(days)?)?)
        }
        Some(Command::Cleanup {
            dry_run: _,
            candidate,
        }) => {
            let plan = index.preview(&candidate)?;
            index.record_preview(&plan)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Some(Command::History) => println!("{}", serde_json::to_string_pretty(&index.history()?)?),
        Some(Command::Findings { offset }) => println!(
            "{}",
            serde_json::to_string_pretty(&index.findings(1000, offset, false)?)?
        ),
        Some(Command::Ignores { remove }) => {
            if let Some(id) = remove {
                index.remove_ignore(id)?;
            }
            println!("{}", serde_json::to_string_pretty(&index.ignores()?)?);
        }
        Some(Command::Catalog { command }) => match command {
            CatalogCommand::Status => {
                let (catalog, digest) = index.catalog()?;
                println!(
                    "catalog {} digest {} (inventory-only)",
                    catalog.version, digest
                );
            }
            CatalogCommand::Import { payload, signature } => {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(payload)?
                    .take((omg_core::catalog::MAX_CATALOG + 1) as u64)
                    .read_to_string(&mut text)?;
                let mut sig = vec![];
                std::fs::File::open(signature)?
                    .take(65)
                    .read_to_end(&mut sig)?;
                println!(
                    "activated verified catalog {}",
                    index.import_catalog(&text, &sig)?
                );
            }
            CatalogCommand::Rollback => {
                index.rollback_catalog()?;
                println!("rolled back to previous verified catalog");
            }
        },
        None => {
            tui::run(index)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
