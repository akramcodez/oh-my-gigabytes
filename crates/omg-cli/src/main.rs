use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use omg_core::{Index, ScanOptions, platform};
use std::{
    io::IsTerminal,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
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
    Start,
    Stop,
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
    if matches!(cli.command, Some(Command::Start | Command::Stop)) {
        bail!("background service not implemented; no service installed");
    }
    if matches!(cli.command, Some(Command::Doctor)) && !platform::supported() {
        println!("omg: this platform is not certified; native scanning and cleanup disabled");
        return Ok(());
    }
    let state = cli.state_dir.unwrap_or(platform::default_state()?);
    if matches!(cli.command, Some(Command::Doctor)) {
        let diagnosis = omg_core::index::diagnose(&state)?;
        println!("omg: {}", serde_json::to_string_pretty(&diagnosis)?);
        println!("cleanup disabled; service not installed; cloud/FUSE/remote profiles excluded");
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
            let report = omg_core::scanner::scan(
                &mut index,
                &ScanOptions {
                    root,
                    workers,
                    exclusions,
                    full,
                },
                cancel,
                Arc::new(AtomicU64::new(0)),
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.state != "complete" {
                bail!("scan incomplete; last valid index retained");
            }
        }
        Some(Command::Status) => println!("{}", serde_json::to_string_pretty(&index.status()?)?),
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
            println!(
                "cleanup disabled; service not installed; cloud/FUSE/remote filesystems excluded"
            );
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
