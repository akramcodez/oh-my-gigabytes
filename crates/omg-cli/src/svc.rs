//! Service control surface. Everything here is scan-only: the commands register, start, stop and
//! query a per-user service, or ask it for a low-priority scan. None can start a cleanup.
use super::{Cli, Command, ServiceCommand};
use anyhow::{Result, bail};
use omg_core::{
    Index, ScanOptions, ScanReport, platform, scanner,
    service::{
        ServiceConfig,
        daemon::{self, DaemonOptions},
        ipc::{self, Operation},
        unit::{Spec, Support, Systemd},
    },
};
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn state_of(cli: &Cli) -> Result<PathBuf> {
    platform::absolute(&match &cli.state_dir {
        Some(path) => path.clone(),
        None => platform::default_state()?,
    })
}
fn config_path(cli: &Cli) -> Result<PathBuf> {
    match &cli.config {
        Some(path) => platform::absolute(path),
        None => platform::default_config(),
    }
}
fn spec(cli: &Cli) -> Result<Spec> {
    Ok(Spec {
        executable: std::env::current_exe()?,
        state_dir: state_of(cli)?,
        config: cli.config.as_deref().map(platform::absolute).transpose()?,
    })
}

/// Read-only registration facts for `omg doctor`.
pub fn registration() -> Result<serde_json::Value> {
    let systemd = Systemd::discover()?;
    let expected = std::env::current_exe().ok().map(|executable| Spec {
        executable,
        state_dir: platform::default_state().unwrap_or_default(),
        config: None,
    });
    Ok(systemd.describe(expected.as_ref().filter(|s| s.state_dir.is_absolute())))
}

fn approve(spec: &Spec, systemd: &Systemd, yes: bool) -> Result<()> {
    println!("omg will register a per-user systemd service (no root, no lingering):");
    println!("  unit file : {}", systemd.unit_path().display());
    println!("  runs      : {} service run", spec.executable.display());
    println!("  state     : {}", spec.state_dir.display());
    println!("It only refreshes the local metadata index at low priority. It cannot delete or");
    println!(
        "move files and there is no service cleanup command. Remove it with `omg service uninstall`."
    );
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("registration needs your approval; re-run with --yes to approve non-interactively");
    }
    print!("Register this service? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("registration declined; nothing was changed");
    }
    Ok(())
}

pub fn control(cli: &Cli) -> Result<()> {
    let command = match &cli.command {
        Some(Command::Start) => ServiceCommand::Start { yes: false },
        Some(Command::Stop) => ServiceCommand::Stop,
        Some(Command::Service { command }) => match command {
            ServiceCommand::Install { enable, yes } => ServiceCommand::Install {
                enable: *enable,
                yes: *yes,
            },
            ServiceCommand::Uninstall => ServiceCommand::Uninstall,
            ServiceCommand::Start { yes } => ServiceCommand::Start { yes: *yes },
            ServiceCommand::Stop => ServiceCommand::Stop,
            ServiceCommand::Status => ServiceCommand::Status,
            ServiceCommand::Scan => ServiceCommand::Scan,
            ServiceCommand::Run => ServiceCommand::Run,
        },
        _ => unreachable!(),
    };
    let state = state_of(cli)?;
    match command {
        ServiceCommand::Run => run(cli, &state),
        ServiceCommand::Install { enable, yes } => {
            let (systemd, spec) = (Systemd::discover()?, spec(cli)?);
            if let Support::Unsupported(reason) = systemd.support() {
                bail!("per-user service is unsupported here: {reason}");
            }
            approve(&spec, &systemd, yes)?;
            systemd.install(&spec, enable)?;
            println!(
                "registered {}; start it with `omg start`",
                systemd.unit_path().display()
            );
            Ok(())
        }
        ServiceCommand::Uninstall => {
            let systemd = Systemd::discover()?;
            if systemd.uninstall()? {
                println!(
                    "service stopped and unregistered; local index, history and ignores were kept"
                );
            } else {
                println!("no omg service registration found");
            }
            Ok(())
        }
        ServiceCommand::Start { yes } => {
            let systemd = Systemd::discover()?;
            if let Support::Unsupported(reason) = systemd.support() {
                bail!(
                    "per-user service is unsupported here: {reason}. Run `omg service run` under a supervisor of your choice"
                );
            }
            if !systemd.registered() {
                let spec = spec(cli)?;
                approve(&spec, &systemd, yes)?;
                systemd.install(&spec, false)?;
            }
            systemd.start()?;
            // `systemctl start` returns once the process is forked; confirm it actually serves.
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if ipc::request(&state, Operation::Status).is_ok_and(|r| r.ok) {
                    println!("service started and responding; details: `omg service status`");
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            bail!(
                "systemd started the unit but the service is not responding; see `journalctl --user -u {}` and `omg service status`",
                systemd.unit
            )
        }
        ServiceCommand::Stop => {
            let systemd = Systemd::discover()?;
            if systemd.registered() && matches!(systemd.support(), Support::Available) {
                systemd.stop()?;
            } else {
                let reply = ipc::request(&state, Operation::Stop)?;
                if !reply.ok {
                    bail!("service refused stop: {}", reply.error.unwrap_or_default());
                }
            }
            println!("service stop requested");
            Ok(())
        }
        ServiceCommand::Scan => {
            let reply = ipc::request(&state, Operation::Scan)?;
            if !reply.ok {
                bail!("service refused scan: {}", reply.error.unwrap_or_default());
            }
            println!("scan requested; the service runs it at low priority");
            Ok(())
        }
        ServiceCommand::Status => {
            let mut report = serde_json::json!({"registration": registration()?});
            if state.join("index.sqlite").exists() {
                let index = Index::open(&state)?;
                report["health"] = daemon::health(&index)?;
            } else {
                report["health"] =
                    serde_json::json!({"state": "stopped", "detail": "no index yet"});
            }
            if let Ok(live) = ipc::request(&state, Operation::Status) {
                report["live"] = live.data.unwrap_or_default();
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
    }
}

fn run(cli: &Cli, state: &std::path::Path) -> Result<()> {
    let config = ServiceConfig::load(&config_path(cli)?)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = shutdown.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    eprintln!("omg-service: starting (scan-only; no cleanup capability)");
    daemon::run(state, DaemonOptions::new(config), shutdown)
}

/// Foreground scan that asks a running service to yield instead of failing or duplicating work.
pub fn scan_with_handoff(
    index: &mut Index,
    options: &ScanOptions,
    cancel: Arc<AtomicBool>,
) -> Result<ScanReport> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut asked = false;
    loop {
        match scanner::scan(index, options, cancel.clone(), Arc::new(AtomicU64::new(0))) {
            Err(error)
                if error.downcast_ref::<omg_core::index::Busy>().is_some()
                    && Instant::now() < deadline
                    && !cancel.load(Ordering::Relaxed) =>
            {
                if !asked {
                    asked = true;
                    match ipc::request(&index.state, Operation::Yield) {
                        Ok(reply) if reply.ok => {
                            eprintln!(
                                "omg: background scan in progress; asked the service to yield"
                            )
                        }
                        _ => return Err(error),
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            other => return other,
        }
    }
}
