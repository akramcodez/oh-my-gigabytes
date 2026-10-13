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
