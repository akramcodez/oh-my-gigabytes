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
        #[arg(long, default_value_t = 2)]
        workers: usize,
        #[arg(long)]
        exclude: Vec<PathBuf>,
    },
    Status,
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
        println!("cleanup disabled; cloud/FUSE/remote profiles excluded");
        #[cfg(target_os = "linux")]
        println!(
            "service: {}",
            serde_json::to_string_pretty(&svc::registration()?)?
        );
        return Ok(());
    }
    let mut index = Index::open(&state)?;
    match cli.command {
        Some(Command::Scan {
            root,
            full,
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
                root,
                workers,
                exclusions,
                full,
            };
            #[cfg(target_os = "linux")]
            let report = svc::scan_with_handoff(&mut index, &options, cancel)?;
            #[cfg(not(target_os = "linux"))]
            let report = omg_core::scanner::scan(
                &mut index,
                &options,
                cancel,
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.state != "complete" {
                bail!("scan incomplete; last valid index retained");
            }
        }
        Some(Command::Status) => {
            #[allow(unused_mut)]
            let mut value = serde_json::to_value(index.status()?)?;
            #[cfg(target_os = "linux")]
            {
                value["service"] = omg_core::service::daemon::health(&index)?;
            }
            println!("{}", serde_json::to_string_pretty(&value)?);
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
        Some(Command::Doctor) => {
            println!(
                "omg: Linux metadata backend; SQLite {}; private local index opened",
                omg_core::index::sqlite_version()
            );
            println!("cleanup disabled; cloud/FUSE/remote filesystems excluded");
            println!(
                "No content reads or network requests during scanning; full reconciliation on refresh."
            );
        }
        None => {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                bail!(
                    "interactive TUI requires a terminal; use omg status or omg scan --root PATH"
                );
            }
            tui::run(index)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
