#![cfg(target_os = "linux")]
use omg_core::{
    Index, ScanOptions, scanner,
    service::{
        ServiceConfig,
        daemon::{self, DaemonOptions},
        ipc::{self, Operation, Response},
        notify::{self, Action, Phase, Policy, State},
        schedule,
        unit::{self, Spec, Support, Systemd},
    },
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn private_state() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
fn scan(index: &mut Index, root: &Path) -> omg_core::ScanReport {
    scanner::scan(
        index,
        &ScanOptions {
            root: root.to_path_buf(),
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap()
}
fn wait_until(what: &str, seconds: u64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

// ---------- configuration ----------

#[test]
fn configuration_is_strict_bounded_and_cannot_name_actions() {
    assert_eq!(ServiceConfig::parse("").unwrap(), ServiceConfig::default());
    assert!(!ServiceConfig::default().notifications.enabled);
    for forged in [
        "cleanup = true",
        "[actions]\ntrash = true",
        "[schedule]\ncommand = \"rm -rf ~\"",
        "[resources]\nelevate = true",
        "[notifications]\nenabled = true\nlow_free_bytes = 1\nrecovery_free_bytes = 2\nexec = \"x\"",
        "version = 2",
    ] {
        assert!(ServiceConfig::parse(forged).is_err(), "accepted: {forged}");
    }
    for bad in [
        "[schedule]\ninterval_minutes = 1",
        "[schedule]\njitter_percent = 90",
        "[schedule]\nbackoff_base_minutes = 60\nbackoff_max_minutes = 30",
        "[resources]\nworkers = 8",
        "[resources]\nnice = -5",
        "[notifications]\nenabled = true",
        "[notifications]\nenabled = true\nlow_free_bytes = 10\nrecovery_free_bytes = 10",
        "[retention]\nmax_samples = 1",
    ] {
        assert!(ServiceConfig::parse(bad).is_err(), "accepted: {bad}");
    }
    assert!(ServiceConfig::parse(&"#".repeat(20_000)).is_err());
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        ServiceConfig::load(&dir.path().join("missing.toml")).unwrap(),
        ServiceConfig::default()
    );
    let real = dir.path().join("real.toml");
    fs::write(&real, "[schedule]\ninterval_minutes = 60\n").unwrap();
    assert_eq!(
        ServiceConfig::load(&real)
            .unwrap()
            .schedule
            .interval_minutes,
        60
    );
    symlink(&real, dir.path().join("link.toml")).unwrap();
    assert!(ServiceConfig::load(&dir.path().join("link.toml")).is_err());
    assert!(ServiceConfig::load(dir.path()).is_err());
}

// ---------- notification hysteresis ----------

const POLICY: Policy = Policy {
    low: 100,
    recovery: 200,
    dwell_secs: 600,
    cooldown_secs: 86_400,
};
fn drive(state: &mut State, samples: impl Iterator<Item = (u64, i64)>) -> usize {
    samples
        .filter(|(available, at)| notify::step(state, &POLICY, *available, *at) == Action::Notify)
        .count()
}

#[test]
fn oscillation_around_the_threshold_never_notifies() {
    let mut state = State::new(0);
    let samples = (0..2000).map(|i| (if i % 2 == 0 { 90 } else { 110 }, i * 60));
    assert_eq!(drive(&mut state, samples), 0);
}

#[test]
fn sustained_low_notifies_once_and_stays_quiet_until_recovery() {
    let mut state = State::new(0);
    // Low from t=0 for three days, sampled every minute.
    let count = drive(&mut state, (0..4320).map(|i| (50, i * 60)));
    assert_eq!(count, 1);
    assert_eq!(state.phase, Phase::Notified);
    // Inside the hysteresis band (low..recovery) the state does not re-arm.
    assert_eq!(
        drive(&mut state, [(150, 300_000), (90, 300_060)].into_iter()),
        0
    );
    assert_eq!(state.phase, Phase::Notified);
    // Recovery re-arms; a re-crossing inside the cooldown waits for the cooldown while still low.
    let t = 4320 * 60;
    assert_eq!(drive(&mut state, [(500, t)].into_iter()), 0);
    assert_eq!(state.phase, Phase::Normal);
    let notified_at = state.last_notified.unwrap();
    let relow = (0..1500).map(|i| (50, t + 60 + i * 60));
    let fired: Vec<i64> = relow
        .filter(|(a, at)| notify::step(&mut state, &POLICY, *a, *at) == Action::Notify)
        .map(|(_, at)| at)
        .collect();
    assert_eq!(fired.len(), 1);
    assert!(fired[0] - notified_at >= POLICY.cooldown_secs);
}

#[test]
fn clock_moving_backwards_cannot_cause_early_notifications() {
    let mut state = State::new(1_000_000);
    assert_eq!(
        drive(&mut state, (0..20).map(|i| (50, 1_000_000 + i * 60))),
        1
    );
    // Wall clock jumps back by a day; nothing fires immediately.
    let back = 1_000_000 - 86_400;
    assert_eq!(
        drive(
            &mut state,
            [(500, back), (50, back + 60), (50, back + 1200)].into_iter()
        ),
        0
    );
}

#[test]
fn notification_text_and_persistence_are_privacy_safe() {
    let text = notify::message(3 * 1024 * 1024 * 1024);
    assert!(text.contains("3.00 GiB"));
    assert!(!text.contains('/') && !text.contains('~'));
    let state = private_state();
    let index = Index::open(state.path()).unwrap();
    let saved = State {
        phase: Phase::Notified,
        since: 42,
        last_notified: Some(40),
    };
    index.save_notification_state("vol", &saved).unwrap();
    drop(index);
    let index = Index::open(state.path()).unwrap();
    assert_eq!(index.notification_state("vol").unwrap(), saved);
    let settings = omg_core::service::config::Notifications {
        enabled: true,
        low_free_bytes: 100,
        recovery_free_bytes: 200,
        dwell_minutes: 1,
        cooldown_hours: 1,
        ..Default::default()
    };
    let off = omg_core::service::config::Notifications::default();
    assert!(
        daemon::evaluate_volume(&index, &off, "v2", 1, 0)
            .unwrap()
            .is_none()
    );
    assert!(
        daemon::evaluate_volume(&index, &settings, "v2", 1, 0)
            .unwrap()
            .is_none()
    );
    let text = daemon::evaluate_volume(&index, &settings, "v2", 1, 100).unwrap();
    assert!(text.is_some_and(|t| !t.contains("v2")));
}

// ---------- schedule helpers ----------

#[test]
fn backoff_is_exponential_capped_and_jitter_never_runs_early() {
    let secs: Vec<u64> = (1..=8)
        .map(|n| schedule::backoff_secs(n, 15, 120))
        .collect();
    assert_eq!(secs[..4], [900, 1800, 3600, 7200]);
    assert!(secs[4..].iter().all(|s| *s == 7200));
    assert_eq!(schedule::backoff_secs(u32::MAX, 15, 120), 7200);
    for seed in 0..500 {
        let value = schedule::jittered(3600, 10, seed);
        assert!((3600..=3960).contains(&value));
    }
    assert_eq!(schedule::jittered(3600, 0, 7), 3600);
    let distinct: std::collections::HashSet<_> =
        (0..200).map(|s| schedule::jittered(3600, 10, s)).collect();
    assert!(distinct.len() > 20);
}

#[test]
fn battery_detection_reads_power_supply_tree() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(schedule::on_battery(&dir.path().join("missing")), None);
    assert_eq!(schedule::on_battery(dir.path()), None);
    let mains = dir.path().join("AC");
    fs::create_dir(&mains).unwrap();
    fs::write(mains.join("type"), "Mains\n").unwrap();
    assert_eq!(schedule::on_battery(dir.path()), None);
    let battery = dir.path().join("BAT0");
    fs::create_dir(&battery).unwrap();
    fs::write(battery.join("type"), "Battery\n").unwrap();
    fs::write(battery.join("status"), "Charging\n").unwrap();
    assert_eq!(schedule::on_battery(dir.path()), Some(false));
    fs::write(battery.join("status"), "Discharging\n").unwrap();
    assert_eq!(schedule::on_battery(dir.path()), Some(true));
}

// ---------- trends ----------

#[test]
fn trends_explain_growth_and_retention_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("a/sub")).unwrap();
    fs::create_dir_all(root.path().join("b")).unwrap();
    fs::write(root.path().join("a/sub/one.bin"), vec![1u8; 1 << 20]).unwrap();
    fs::write(root.path().join("b/two.bin"), vec![2u8; 1 << 20]).unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    assert_eq!(index.trend(30).unwrap().samples, 0);
    assert_eq!(scan(&mut index, root.path()).state, "complete");
    fs::write(root.path().join("a/sub/grown.bin"), vec![3u8; 8 << 20]).unwrap();
    assert_eq!(scan(&mut index, root.path()).state, "complete");
    let trend = index.trend(30).unwrap();
    assert_eq!(trend.samples, 2);
    assert!(trend.indexed_allocated_change.unwrap() >= 8 << 20);
    assert!(trend.available_change.is_some());
    let top = &trend.growers[0];
    assert!(top.path == b"a" || top.path == b"a/sub");
    assert!(top.change.unwrap() >= 8 << 20);
    assert!(trend.growers.iter().all(|g| g.path != b"b"));
    assert!(trend.note.contains("uncertain"));
    assert!(index.trend(0).is_err());

    // Count and age bounds, with snapshot rows following their sample.
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    let insert = "INSERT INTO volume_samples(at,root,capacity,available,scan_state) VALUES(?,?,1,1,'complete')";
    for i in 0..30 {
        db.execute(
            insert,
            rusqlite::params![omg_core::now() - 1000 + i, b"/r".to_vec()],
        )
        .unwrap();
    }
    db.execute(
        insert,
        rusqlite::params![omg_core::now() - 400 * 86_400, b"/r".to_vec()],
    )
    .unwrap();
    index.prune_samples(365, 10).unwrap();
    assert_eq!(index.trend(3650).unwrap().samples, 10);
}

// ---------- IPC ----------

fn serve(
    state: &Path,
    calls: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let mut server = ipc::Server::bind(state).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            server
                .poll(|op| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    if op == Operation::Stop {
                        stop.store(true, Ordering::Relaxed);
                    }
                    Response::ok(Some(serde_json::json!({"op": format!("{op:?}")})))
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}
fn raw(state: &Path, payload: &[u8]) -> String {
    let mut stream = UnixStream::connect(ipc::socket_path(state)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let _ = stream.write_all(payload);
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    out
}

#[test]
fn ipc_is_a_closed_scan_only_protocol() {
    let state = private_state();
    let calls = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let thread = serve(state.path(), calls.clone(), stop.clone());
    let socket = fs::metadata(ipc::socket_path(state.path())).unwrap();
    assert_eq!(socket.permissions().mode() & 0o777, 0o600);

    let sentinel = state.path().join("user-data.txt");
    fs::write(&sentinel, "keep").unwrap();
    let forged = [
        r#"{"v":1,"op":"cleanup"}"#,
        r#"{"v":1,"op":"delete","path":"/home"}"#,
        r#"{"v":1,"op":"approve"}"#,
        r#"{"v":1,"op":"trash"}"#,
        r#"{"v":1,"op":"plan"}"#,
        r#"{"v":1,"op":"status","extra":"cleanup"}"#,
        r#"{"v":99,"op":"status"}"#,
        r#"{"op":"status"}"#,
        "not json",
    ];
    for message in forged {
        let reply = raw(state.path(), format!("{message}\n").as_bytes());
        let reply: Response = serde_json::from_str(reply.trim()).unwrap();
        assert!(!reply.ok, "accepted forged message {message}");
    }
    // Oversize and unterminated input is bounded and rejected without hanging the service.
    let reply = raw(state.path(), &vec![b'x'; 100_000]);
    assert!(reply.is_empty() || !serde_json::from_str::<Response>(reply.trim()).unwrap().ok);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "forged input reached the handler"
    );
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "keep");

    for (op, name) in [
        (Operation::Status, "Status"),
        (Operation::Scan, "Scan"),
        (Operation::Yield, "Yield"),
    ] {
        let reply = ipc::request(state.path(), op).unwrap();
        assert!(reply.ok);
        assert_eq!(reply.data.unwrap()["op"], name);
    }
    assert!(ipc::request(state.path(), Operation::Stop).unwrap().ok);
    thread.join().unwrap();
    assert!(
        !ipc::socket_path(state.path()).exists(),
        "socket removed on drop"
    );
}

#[test]
fn ipc_works_when_the_state_directory_path_exceeds_unix_socket_limits() {
    let base = tempfile::tempdir().unwrap();
    let deep = base
        .path()
        .join("d".repeat(60))
        .join("e".repeat(60))
        .join("state");
    fs::create_dir_all(&deep).unwrap();
    fs::set_permissions(&deep, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(deep.as_os_str().len() > 130);
    let stop = Arc::new(AtomicBool::new(false));
    let thread = serve(&deep, Arc::new(AtomicU64::new(0)), stop);
    assert!(ipc::request(&deep, Operation::Status).unwrap().ok);
    assert!(ipc::request(&deep, Operation::Stop).unwrap().ok);
    thread.join().unwrap();
    assert!(!ipc::socket_path(&deep).exists());
    // A symlinked state directory is refused instead of followed.
    let link = base.path().join("link");
    symlink(&deep, &link).unwrap();
    assert!(ipc::Server::bind(&link).is_err());
}

#[test]
fn ipc_refuses_to_replace_non_sockets() {
    let state = private_state();
    fs::write(ipc::socket_path(state.path()), "not a socket").unwrap();
    assert!(ipc::Server::bind(state.path()).is_err());
    assert_eq!(
        fs::read(ipc::socket_path(state.path())).unwrap(),
        b"not a socket"
    );
}

// ---------- daemon ----------

struct Service {
    state: PathBuf,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}
impl Service {
    fn start(state: &Path, mut options: DaemonOptions) -> Self {
        options.tick = Duration::from_millis(20);
        options.heartbeat = Duration::from_millis(100);
        options.startup_delay = Duration::ZERO;
        let shutdown = Arc::new(AtomicBool::new(false));
        let (path, flag) = (state.to_path_buf(), shutdown.clone());
        let thread = std::thread::spawn(move || daemon::run(&path, options, flag));
        wait_until("socket", 10, || ipc::socket_path(state).exists());
        Self {
            state: state.to_path_buf(),
            shutdown,
            thread: Some(thread),
        }
    }
    fn live(&self) -> serde_json::Value {
        ipc::request(&self.state, Operation::Status)
            .unwrap()
            .data
            .unwrap()
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn seeded(files: usize) -> (tempfile::TempDir, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    for d in 0..(files / 1000).max(1) {
        let dir = root.path().join(format!("d{d}"));
        fs::create_dir(&dir).unwrap();
        for f in 0..files.min(1000) {
            fs::write(dir.join(format!("f{f}")), b"x").unwrap();
        }
    }
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    assert_eq!(scan(&mut index, root.path()).state, "complete");
    (root, state)
}

#[test]
fn service_runs_requested_scans_reports_health_and_stops_cleanly() {
    let (root, state) = seeded(10);
    let index = Index::open(state.path()).unwrap();
    assert_eq!(daemon::health(&index).unwrap()["state"], "stopped");
    let mut service = Service::start(state.path(), DaemonOptions::new(ServiceConfig::default()));
    let before = service.live()["last_scan"].as_i64();
    wait_until("healthy", 10, || {
        daemon::health(&index).unwrap()["state"] == "running"
    });
    // A second service for the same profile is refused by the kernel lock.
    let second = daemon::run(
        state.path(),
        DaemonOptions::new(ServiceConfig::default()),
        Arc::new(AtomicBool::new(true)),
    );
    assert!(second.unwrap_err().to_string().contains("already running"));
    fs::write(root.path().join("new.bin"), vec![9u8; 1 << 20]).unwrap();
    assert!(ipc::request(state.path(), Operation::Scan).unwrap().ok);
    wait_until("requested scan", 20, || {
        let live = service.live();
        !live["scan_active"].as_bool().unwrap()
            && live["last_scan"].as_i64() >= before
            && index.trend(1).unwrap().samples >= 2
    });
    let live = service.live();
    assert_eq!(live["failures"], 0);
    assert_eq!(
        live["operations"],
        serde_json::json!(["status", "scan", "yield", "stop"])
    );
    assert!(
        fs::read(root.path().join("new.bin")).unwrap().len() == 1 << 20,
        "scan never modifies data"
    );
    service.stop().unwrap();
    let health = daemon::health(&index).unwrap();
    assert_eq!(health["state"], "stopped");
    assert_eq!(health["detail"], "stopped cleanly");
    assert!(!ipc::socket_path(state.path()).exists());
}

#[test]
fn failed_scans_back_off_and_report_degraded_health() {
    let (root, state) = seeded(10);
    let index = Index::open(state.path()).unwrap();
    let mut service = Service::start(state.path(), DaemonOptions::new(ServiceConfig::default()));
    fs::remove_dir_all(root.path()).unwrap(); // saved scan root disappears
    assert!(ipc::request(state.path(), Operation::Scan).unwrap().ok);
    wait_until("failure recorded", 20, || service.live()["failures"] == 1);
    let live = service.live();
    assert_eq!(live["phase"], "degraded");
    let wait = live["next_run"].as_i64().unwrap() - omg_core::now();
    assert!((14 * 60..=17 * 60).contains(&wait), "backoff was {wait}s");
    assert_eq!(daemon::health(&index).unwrap()["state"], "degraded");
    service.stop().unwrap();
}

#[test]
fn scheduled_scans_pause_on_battery_but_explicit_requests_run() {
    let (_root, state) = seeded(10);
    // Make the index stale so the schedule is immediately due.
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute("UPDATE scans SET ended=ended-?", [7 * 86_400])
        .unwrap();
    drop(db);
    let power = tempfile::tempdir().unwrap();
    let battery = power.path().join("BAT0");
    fs::create_dir(&battery).unwrap();
    fs::write(battery.join("type"), "Battery").unwrap();
    fs::write(battery.join("status"), "Discharging").unwrap();
    let mut options = DaemonOptions::new(ServiceConfig::default());
    options.power_supply = power.path().into();
    let mut service = Service::start(state.path(), options);
    std::thread::sleep(Duration::from_millis(1200));
    let live = service.live();
    assert_eq!(live["battery"], "discharging");
    assert_eq!(live["scan_active"], false);
    let stale = live["last_scan"].as_i64().unwrap();
    assert!(
        omg_core::now() - stale > 6 * 86_400,
        "scheduled scan ran on battery"
    );
    assert!(ipc::request(state.path(), Operation::Scan).unwrap().ok);
    wait_until("explicit scan on battery", 20, || {
        service.live()["last_scan"].as_i64().unwrap() > stale + 86_400
    });
    service.stop().unwrap();
}

#[test]
fn foreground_scan_takes_over_from_a_running_service_scan_without_corruption() {
    let (_root, state) = seeded(30_000);
    let mut index = Index::open(state.path()).unwrap();
    let root: PathBuf = platform_root(&index);
    let mut service = Service::start(state.path(), DaemonOptions::new(ServiceConfig::default()));
    assert!(ipc::request(state.path(), Operation::Scan).unwrap().ok);
    // Entries recorded means the service scan is past lock acquisition and mid-traversal.
    wait_until("service scan in flight", 20, || {
        service.live()["scan_progress_entries"]
            .as_u64()
            .unwrap_or(0)
            > 0
    });
    // Foreground sees contention, asks the service to yield, then completes its own scan.
    let options = ScanOptions {
        root,
        workers: 2,
        exclusions: vec![],
        full: true,
        publish_partial: false,
    };
    let mut yielded = false;
    let report = loop {
        match scanner::scan(
            &mut index,
            &options,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        ) {
            Ok(report) => break report,
            Err(error) if error.downcast_ref::<omg_core::index::Busy>().is_some() => {
                if !yielded {
                    yielded = true;
                    assert!(ipc::request(state.path(), Operation::Yield).unwrap().ok);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("{error:#}"),
        }
    };
    assert!(yielded, "service scan never held the lock");
    assert_eq!(report.state, "complete");
    wait_until("service idle", 20, || {
        service.live()["scan_active"] == false
    });
    assert_eq!(
        service.live()["failures"],
        0,
        "cancelled handoff must not count as a failure"
    );
    service.stop().unwrap();
    let status = index.status().unwrap();
    assert!(status.entries >= 30_000);
    assert_eq!(status.last_report.unwrap().state, "complete");
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    let check: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let running: i64 = db
        .query_row(
            "SELECT count(*) FROM scans WHERE state='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(running, 0);
}
fn platform_root(index: &Index) -> PathBuf {
    omg_core::platform::path(&index.status().unwrap().root.unwrap()).unwrap()
}

// ---------- systemd registration ----------

fn fake_systemctl(dir: &Path, state_output: &str) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let script = dir.join("systemctl");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$*\" in *is-system-running*) echo '{state_output}';; esac\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    (script, log)
}
fn installed_binary(dir: &Path) -> PathBuf {
    let binary = dir.join("omg bin");
    fs::copy("/bin/true", &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

#[test]
fn unit_rendering_quotes_arguments_and_rejects_unsafe_values() {
    assert_eq!(unit::quote("a b").unwrap(), "\"a b\"");
    assert_eq!(
        unit::quote("100%$HOME\"\\").unwrap(),
        "\"100%%$$HOME\\\"\\\\\""
    );
    assert!(unit::quote("line\nbreak").is_err());
    let spec = Spec {
        executable: "/opt/omg dir/omg".into(),
        state_dir: "/home/u/.local/state/omg 100%".into(),
        config: None,
    };
    let text = unit::render(&spec).unwrap();
    assert!(text.starts_with("# Managed by omg"));
    assert!(text.contains(
        "ExecStart=\"/opt/omg dir/omg\" \"--state-dir\" \"/home/u/.local/state/omg 100%%\" \"service\" \"run\""
    ));
    assert!(text.contains("WantedBy=default.target") && text.contains("NoNewPrivileges=yes"));
    assert!(!text.contains("system.slice") && !text.to_lowercase().contains("cleanup"));
    for bad in ["relative/omg", "/a/../b/omg", "/bad\nname/omg"] {
        let spec = Spec {
            executable: bad.into(),
            state_dir: "/s".into(),
            config: None,
        };
        assert!(unit::render(&spec).is_err(), "rendered {bad}");
    }
}

#[test]
fn registration_is_explicit_per_user_and_never_overwrites_foreign_units() {
    let dir = tempfile::tempdir().unwrap();
    let binary = installed_binary(dir.path());
    let (script, log) = fake_systemctl(dir.path(), "running");
    let units = dir.path().join("units");
    let systemd = Systemd {
        systemctl: script,
        unit_dir: units.clone(),
        unit: "omg-test.service".into(),
    };
    let spec = Spec {
        executable: binary.clone(),
        state_dir: dir.path().join("state"),
        config: None,
    };
    assert_eq!(systemd.support(), Support::Available);
    assert!(!systemd.registered());
    assert!(systemd.start().is_err(), "start must not auto-register");
    systemd.install(&spec, false).unwrap();
    assert!(systemd.registered());
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.lines().all(|l| l.starts_with("--user ")),
        "non --user call: {calls}"
    );
    assert!(calls.contains("daemon-reload") && !calls.contains("enable"));
    systemd.start().unwrap();
    systemd.stop().unwrap();
    let described = systemd.describe(Some(&spec));
    assert_eq!(described["managed_by_omg"], true);
    assert_eq!(
        described["registration_differs_from_current_install"],
        false
    );
    assert_eq!(described["orphaned_binary"], false);
    // A moved binary is reported as an orphaned registration.
    fs::remove_file(&binary).unwrap();
    assert_eq!(systemd.describe(None)["orphaned_binary"], true);
    let binary = installed_binary(dir.path());
    let moved = Spec {
        state_dir: dir.path().join("other"),
        ..Spec {
            executable: binary.clone(),
            state_dir: PathBuf::new(),
            config: None,
        }
    };
    assert_eq!(
        systemd.describe(Some(&moved))["registration_differs_from_current_install"],
        true
    );

    systemd
        .install(
            &Spec {
                executable: binary.clone(),
                ..spec.clone()
            },
            true,
        )
        .unwrap();
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("enable omg-test.service")
    );
    assert!(systemd.uninstall().unwrap());
    assert!(!systemd.registered());
    assert!(!systemd.uninstall().unwrap());

    // A unit that omg did not write is never overwritten or removed.
    fs::write(systemd.unit_path(), "[Service]\nExecStart=/bin/true\n").unwrap();
    assert!(systemd.install(&spec, false).is_err());
    assert!(systemd.uninstall().is_err());
    assert_eq!(
        fs::read_to_string(systemd.unit_path()).unwrap(),
        "[Service]\nExecStart=/bin/true\n"
    );
}

#[test]
fn unsupported_environments_are_reported_not_faked() {
    let dir = tempfile::tempdir().unwrap();
    let binary = installed_binary(dir.path());
    let spec = Spec {
        executable: binary,
        state_dir: dir.path().join("s"),
        config: None,
    };
    let (script, _) = fake_systemctl(dir.path(), "offline");
    let offline = Systemd {
        systemctl: script,
        unit_dir: dir.path().join("u"),
        unit: "omg-test.service".into(),
    };
    assert!(matches!(offline.support(), Support::Unsupported(r) if r.contains("offline")));
    assert!(
        offline
            .install(&spec, false)
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
    assert!(!offline.unit_path().exists());
    let missing = Systemd {
        systemctl: dir.path().join("nope"),
        unit_dir: dir.path().join("u"),
        unit: "omg-test.service".into(),
    };
    assert!(matches!(missing.support(), Support::Unsupported(r) if r.contains("not installed")));
}

#[test]
fn executables_must_be_private_regular_files() {
    let dir = tempfile::tempdir().unwrap();
    let binary = installed_binary(dir.path());
    unit::validate_executable(&binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(
        unit::validate_executable(&binary).is_err(),
        "group-writable"
    );
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        unit::validate_executable(&binary).is_err(),
        "not executable"
    );
    let target = installed_binary(dir.path());
    let link = dir.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(unit::validate_executable(&link).is_err(), "symlink");
    assert!(unit::validate_executable(dir.path()).is_err(), "directory");
}

// ---------- structural guard: the service can only observe ----------

#[test]
fn service_logic_contains_no_deletion_or_cleanup_calls() {
    for (name, source) in [
        ("daemon", include_str!("../src/service/daemon.rs")),
        ("notify", include_str!("../src/service/notify.rs")),
        ("trend", include_str!("../src/service/trend.rs")),
        ("schedule", include_str!("../src/service/schedule.rs")),
        ("config", include_str!("../src/service/config.rs")),
    ] {
        for forbidden in [
            "remove_file",
            "remove_dir",
            "rename(",
            "planning",
            "Trash",
            "unlink",
            "authoriz",
        ] {
            assert!(!source.contains(forbidden), "{name} references {forbidden}");
        }
    }
}
