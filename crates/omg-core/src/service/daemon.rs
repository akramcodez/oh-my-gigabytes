//! Scan-only background service loop. It owns a single kernel lock, serves a closed IPC protocol and
//! shares `scan.lock` with foreground scans. It depends on no cleanup module and no operation here
//! can modify scanned data.
use super::{
    config::ServiceConfig,
    ipc::{self, Operation, Response},
    notify, schedule,
};
use crate::{Index, ScanOptions, ScanReport, index, now, platform, scanner};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const BUSY_RETRY_SECS: i64 = 300;
const YIELD_HOLD: Duration = Duration::from_secs(120);
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

#[derive(Clone)]
pub struct DaemonOptions {
    pub config: ServiceConfig,
    pub tick: Duration,
    pub heartbeat: Duration,
    pub startup_delay: Duration,
    pub power_supply: PathBuf,
}
impl DaemonOptions {
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            tick: Duration::from_millis(500),
            heartbeat: Duration::from_secs(5),
            startup_delay: Duration::from_secs(30),
            power_supply: POWER_SUPPLY.into(),
        }
    }
}

struct Active {
    handle: JoinHandle<Result<ScanReport>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    full: bool,
}
struct Runtime {
    phase: &'static str,
    failures: u32,
    next_run: i64,
    last_scan: Option<i64>,
    last_error: Option<String>,
    active: Option<Active>,
    scan_requested: bool,
    yield_until: Instant,
    stop: bool,
    seed: u64,
    priority_applied: bool,
    notifications_sent: u64,
    rejected_peers: u64,
    battery: Option<bool>,
}
impl Runtime {
    fn snapshot(&self) -> serde_json::Value {
        json!({
            "phase": self.phase,
            "scan_active": self.active.is_some(),
            "scan_progress_entries": self.active.as_ref().map(|a| a.progress.load(Ordering::Relaxed) & !scanner::FINISHING),
            "failures": self.failures,
            "next_run": self.next_run,
            "last_scan": self.last_scan,
            "last_error": self.last_error,
            "priority_lowered": self.priority_applied,
            "battery": match self.battery { Some(true) => "discharging", Some(false) => "not discharging", None => "unknown" },
            "notifications_sent": self.notifications_sent,
            "rejected_peers": self.rejected_peers,
            "protocol": ipc::PROTOCOL,
            "operations": ["status", "scan", "yield", "stop"],
        })
    }
    fn persist(&self, index: &Index, nonce: &str, started: i64) -> Result<()> {
        index.db.execute(
            "INSERT INTO service_state VALUES(1,?,?,?,?,?,?,?,?,?) ON CONFLICT(singleton) DO UPDATE SET nonce=excluded.nonce,pid=excluded.pid,started=excluded.started,heartbeat=excluded.heartbeat,phase=excluded.phase,failures=excluded.failures,next_run=excluded.next_run,last_scan=excluded.last_scan,last_error=excluded.last_error",
            params![nonce, std::process::id(), started, now(), self.phase, self.failures, self.next_run, self.last_scan, self.last_error],
        )?;
        Ok(())
    }
    fn next_seed(&mut self) -> u64 {
        self.seed = self.seed.wrapping_add(0x1234_5678_9ABC_DEF1);
        self.seed
    }
}

fn random_nonce() -> (String, u64) {
    use std::io::Read;
    let mut bytes = [0u8; 8];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if ok.is_err() {
        bytes = (now() as u64 ^ u64::from(std::process::id())).to_le_bytes();
    }
    let value = u64::from_le_bytes(bytes);
    (format!("{value:016x}"), value)
}

fn acquire_service_lock(state: &Path) -> Result<std::fs::File> {
    // A concurrent status probe holds a shared lock for microseconds; retry briefly before failing.
    for attempt in 0..20 {
        match index::lock(state, "service.lock") {
            Ok(file) => return Ok(file),
            Err(error) if error.downcast_ref::<index::Busy>().is_some() && attempt < 19 => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) if error.downcast_ref::<index::Busy>().is_some() => {
                bail!("the omg background service is already running for this profile")
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn setting_i64(index: &Index, key: &str) -> Result<Option<i64>> {
    let value: Option<Vec<u8>> = index
        .db
        .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(value.and_then(|v| String::from_utf8(v).ok()?.parse().ok()))
}
fn set_setting(index: &Index, key: &str, value: i64) -> Result<()> {
    index.db.execute(
        "INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value.to_string().into_bytes()],
    )?;
    Ok(())
}
fn last_complete_end(index: &Index) -> Result<Option<i64>> {
    Ok(index
        .db
        .query_row(
            "SELECT ended FROM scans WHERE id=(SELECT scan_id FROM active WHERE singleton=1)",
            [],
            |r| r.get(0),
        )
        .optional()?)
}
fn sanitize(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect()
}

/// Evaluate one free-space observation for the scanned volume; returns notification text to deliver.
pub fn evaluate_volume(
    index: &Index,
    settings: &super::config::Notifications,
    volume: &str,
    available: u64,
    at: i64,
) -> Result<Option<String>> {
    if !settings.enabled {
        return Ok(None);
    }
    let policy = notify::Policy {
        low: settings.low_free_bytes,
        recovery: settings.recovery_free_bytes,
        dwell_secs: i64::try_from(settings.dwell_minutes * 60)?,
        cooldown_secs: i64::try_from(settings.cooldown_hours * 3600)?,
    };
    let mut state = index.notification_state(volume)?;
    let action = notify::step(&mut state, &policy, available, at);
    index.save_notification_state(volume, &state)?;
    Ok((action == notify::Action::Notify).then(|| notify::message(available)))
}

fn volume_key(root: &[u8]) -> String {
    root.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_volume(rt: &mut Runtime, index: &Index, options: &DaemonOptions) {
    if !options.config.notifications.enabled {
        return;
    }
    let result = (|| -> Result<Option<String>> {
        let Some(scan) = index.options()? else {
            return Ok(None);
        };
        #[cfg(target_os = "linux")]
        let (_, available) = platform::linux::volume(&platform::linux::root(&scan.root)?)?;
        #[cfg(not(target_os = "linux"))]
        let available = 0;
        evaluate_volume(
            index,
            &options.config.notifications,
            &volume_key(&platform::raw(&scan.root)?),
            available,
            now(),
        )
    })();
    match result {
        Ok(Some(text)) => {
            rt.notifications_sent += 1;
            eprintln!("omg-service: notification: {text}");
            if options.config.notifications.desktop
                && let Err(error) = notify::deliver_desktop(&text)
            {
                eprintln!("omg-service: desktop delivery unavailable: {error}");
            }
        }
        Ok(None) => (),
        Err(error) => eprintln!("omg-service: volume check failed: {}", sanitize(&error)),
    }
}

fn start_scan(state: &Path, options: &DaemonOptions, full: bool) -> Active {
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU64::new(0));
    let (state, config) = (state.to_path_buf(), options.config.clone());
    let (c, p) = (cancel.clone(), progress.clone());
    let handle = std::thread::spawn(move || -> Result<ScanReport> {
        let mut index = Index::open(&state)?;
        let Some(saved) = index.options()? else {
            bail!("no scan root is configured; run `omg scan --root PATH` once");
        };
        scanner::scan(
            &mut index,
            &ScanOptions {
                workers: config.resources.workers,
                full,
                publish_partial: false,
                ..saved
            },
            c,
            p,
        )
    });
    Active {
        handle,
        cancel,
        progress,
        full,
    }
}

fn finish_scan(rt: &mut Runtime, index: &Index, options: &DaemonOptions, done: Active) {
    let cfg = &options.config;
    let interval = cfg.schedule.interval_minutes * 60;
    let at = now();
    // We asked it to stop (foreground handoff or shutdown): whatever it returned is not a failure.
    let cancelled = done.cancel.load(Ordering::Relaxed);
    let outcome = done
        .handle
        .join()
        .unwrap_or_else(|_| Err(anyhow::anyhow!("scan thread panicked")));
    let seed = rt.next_seed();
    if cancelled {
        rt.phase = "idle";
        rt.next_run = at + schedule::jittered(interval, cfg.schedule.jitter_percent, seed) as i64;
        return;
    }
    match outcome {
        Ok(report) if matches!(report.state.as_str(), "complete" | "partial") => {
            rt.failures = 0;
            rt.last_scan = Some(at);
            rt.last_error = (report.state == "partial").then(|| {
                format!(
                    "partial coverage: {} access/consistency issues",
                    report.errors
                )
            });
            rt.phase = "idle";
            if report.state == "complete" && done.full {
                let _ = set_setting(index, "service_last_full", at);
            }
            let _ = index.prune_samples(cfg.retention.sample_days, cfg.retention.max_samples);
            rt.next_run =
                at + schedule::jittered(interval, cfg.schedule.jitter_percent, seed) as i64;
        }
        Ok(report) if report.state == "cancelled" => {
            // Foreground handoff or shutdown: not a failure, no backoff.
            rt.phase = "idle";
            rt.next_run =
                at + schedule::jittered(interval, cfg.schedule.jitter_percent, seed) as i64;
        }
        Ok(_) => fail(rt, cfg, at, seed, "scan did not complete".into()),
        Err(error) if error.downcast_ref::<index::Busy>().is_some() => {
            // A foreground scan owns the lock; coalesce into it instead of competing.
            rt.phase = "idle";
            rt.next_run = at + BUSY_RETRY_SECS;
        }
        Err(error) => fail(rt, cfg, at, seed, sanitize(&error)),
    }
}
fn fail(rt: &mut Runtime, cfg: &ServiceConfig, at: i64, seed: u64, message: String) {
    rt.failures += 1;
    rt.last_error = Some(message);
    rt.phase = if rt.failures >= 3 {
        "unhealthy"
    } else {
        "degraded"
    };
    let wait = schedule::backoff_secs(
        rt.failures,
        cfg.schedule.backoff_base_minutes,
        cfg.schedule.backoff_max_minutes,
    );
    rt.next_run = at + schedule::jittered(wait, cfg.schedule.jitter_percent, seed) as i64;
}

/// Heartbeat/state writes can lose a race with a scan commit; that must not stop the service.
fn save(rt: &Runtime, index: &Index, nonce: &str, started: i64) {
    if let Err(error) = rt.persist(index, nonce, started) {
        eprintln!("omg-service: state update failed: {}", sanitize(&error));
    }
}

/// Run until `shutdown` is set or a `stop` request arrives. Refuses to run with elevated privileges.
pub fn run(state: &Path, options: DaemonOptions, shutdown: Arc<AtomicBool>) -> Result<()> {
    options.config.validate()?;
    if rustix::process::geteuid().is_root() {
        bail!("the omg background service never runs as root; start it as your own user");
    }
    let state = platform::absolute(state)?;
    let index = Index::open(&state)?; // validates private state directory ownership and schema
    let _owner = acquire_service_lock(&state)?;
    let (nonce, seed) = random_nonce();
    let started = now();
    let cfg = &options.config;
    let interval = (cfg.schedule.interval_minutes * 60) as i64;
    let first_due = last_complete_end(&index)?.map_or(started, |t| t + interval);
    let mut rt = Runtime {
        phase: "idle",
        failures: 0,
        next_run: first_due.max(started + options.startup_delay.as_secs() as i64),
        last_scan: last_complete_end(&index)?,
        last_error: None,
        active: None,
        scan_requested: false,
        yield_until: Instant::now(),
        stop: false,
        seed,
        priority_applied: rustix::process::setpriority_process(None, cfg.resources.nice).is_ok(),
        notifications_sent: 0,
        rejected_peers: 0,
        battery: schedule::on_battery(&options.power_supply),
    };
    let mut ipc = ipc::Server::bind(&state)?;
    rt.persist(&index, &nonce, started)?;
    let mut last_heartbeat = Instant::now();
    let mut next_volume_check = Instant::now() + Duration::from_secs(5);
    while !shutdown.load(Ordering::Relaxed) && !rt.stop {
        let handled = ipc.poll(|op| match op {
            Operation::Status => Response::ok(Some(rt.snapshot())),
            Operation::Scan => {
                rt.scan_requested = true;
                Response::ok(None)
            }
            Operation::Yield => {
                if let Some(active) = &rt.active {
                    active.cancel.store(true, Ordering::Relaxed);
                }
                rt.yield_until = Instant::now() + YIELD_HOLD;
                Response::ok(None)
            }
            Operation::Stop => {
                rt.stop = true;
                Response::ok(None)
            }
        });
        if let Err(error) = handled {
            eprintln!("omg-service: ipc error: {}", sanitize(&error));
        }
        rt.rejected_peers = ipc.rejected_peers;
        if rt.active.as_ref().is_some_and(|a| a.handle.is_finished()) {
            if let Some(done) = rt.active.take() {
                finish_scan(&mut rt, &index, &options, done);
            }
            check_volume(&mut rt, &index, &options);
            save(&rt, &index, &nonce, started);
        }
        if rt.active.is_none() && !rt.stop && Instant::now() >= rt.yield_until {
            if rt.scan_requested {
                rt.battery = schedule::on_battery(&options.power_supply);
            }
            let paused = cfg.schedule.pause_on_battery && rt.battery == Some(true);
            let due = now() >= rt.next_run && !paused;
            if rt.scan_requested || due {
                let at = now();
                let gap = last_complete_end(&index)?.is_none_or(|t| at - t > interval * 2);
                let reconcile = setting_i64(&index, "service_last_full")?
                    .is_none_or(|t| at - t >= (cfg.schedule.reconcile_hours * 3600) as i64);
                rt.scan_requested = false;
                rt.active = Some(start_scan(&state, &options, gap || reconcile));
                rt.phase = "scanning";
                save(&rt, &index, &nonce, started);
            }
        }
        if Instant::now() >= next_volume_check {
            check_volume(&mut rt, &index, &options);
            next_volume_check =
                Instant::now() + Duration::from_secs(cfg.resources.volume_check_minutes * 60);
        }
        // While scanning, state writes contend with the scan's transactions and could delay IPC
        // handling (foreground handoff needs a prompt answer), so heartbeat less often then.
        let beat = if rt.active.is_some() {
            options.heartbeat.max(Duration::from_secs(15))
        } else {
            options.heartbeat
        };
        if last_heartbeat.elapsed() >= beat {
            // Sysfs is read at heartbeat cadence, not every tick, to keep idle wakeups cheap.
            rt.battery = schedule::on_battery(&options.power_supply);
            save(&rt, &index, &nonce, started);
            last_heartbeat = Instant::now();
        }
        std::thread::sleep(options.tick);
    }
    if let Some(active) = rt.active.take() {
        active.cancel.store(true, Ordering::Relaxed);
        let _ = active.handle.join();
    }
    rt.phase = "stopped";
    save(&rt, &index, &nonce, started);
    Ok(())
}

struct StateRow {
    heartbeat: i64,
    phase: String,
    failures: u32,
    next_run: Option<i64>,
    last_scan: Option<i64>,
    last_error: Option<String>,
}

/// Truthful health from the kernel lock and heartbeat, never from a PID alone.
pub fn health(index: &Index) -> Result<serde_json::Value> {
    let row: Option<StateRow> = index
        .db
        .query_row(
            "SELECT heartbeat,phase,failures,next_run,last_scan,last_error FROM service_state WHERE singleton=1",
            [],
            |r| {
                Ok(StateRow {
                    heartbeat: r.get(0)?,
                    phase: r.get(1)?,
                    failures: r.get(2)?,
                    next_run: r.get(3)?,
                    last_scan: r.get(4)?,
                    last_error: r.get(5)?,
                })
            },
        )
        .optional()?;
    let held = index::lock_held(&index.state, "service.lock");
    let Some(row) = row else {
        return Ok(json!({"state": "stopped", "detail": "service has never run for this profile"}));
    };
    let age = (now() - row.heartbeat).max(0);
    let (state, detail) = if !held {
        if row.phase == "stopped" {
            ("stopped", "stopped cleanly")
        } else {
            (
                "stopped",
                "process is not running (last run ended without a clean stop)",
            )
        }
    } else if age > 60 {
        (
            "unhealthy",
            "service holds its lock but the heartbeat is stale",
        )
    } else if row.failures >= 3 {
        ("unhealthy", "repeated scan failures; backing off")
    } else if row.failures >= 1 {
        ("degraded", "recent scan failure; backing off")
    } else {
        ("running", "healthy")
    };
    Ok(
        json!({"state": state, "detail": detail, "phase": row.phase, "heartbeat_age_secs": age,
        "failures": row.failures, "next_run": row.next_run, "last_scan": row.last_scan, "last_error": row.last_error}),
    )
}
