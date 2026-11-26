//! Job registration against a fake `launchctl` or `systemctl` and a temporary folder, so the real
//! user session is never touched.
#![cfg(unix)]
use omg_core::scheduled::{Job, LABEL, Scheduler, Spec};
use std::{fs, os::unix::fs::PermissionsExt};

#[cfg(target_os = "macos")]
#[test]
fn enable_writes_a_marked_agent_and_disable_only_removes_ours() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let tool = dir.path().join("launchctl");
    fs::write(
        &tool,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let agents = dir.path().join("LaunchAgents");
    let scheduler = Scheduler {
        tool: tool.clone(),
        agents: agents.clone(),
        job: Job::Scan,
    };
    assert!(!scheduler.state().unwrap().registered);
    let spec = Spec {
        job: Job::Scan,
        executable: tool.clone(),
        state_dir: dir.path().join("state"),
        hour: 3,
    };
    scheduler.enable(&spec).unwrap();
    let plist = fs::read_to_string(agents.join(format!("{LABEL}.plist"))).unwrap();
    assert!(plist.contains("<string>scan</string>"));
    assert!(plist.contains("<integer>3</integer>"));
    assert!(scheduler.state().unwrap().registered);
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains("bootstrap gui/"), "{calls}");
    assert!(scheduler.disable().unwrap());
    assert!(!agents.join(format!("{LABEL}.plist")).exists());
    assert!(!scheduler.disable().unwrap());

    fs::write(agents.join(format!("{LABEL}.plist")), "someone else's").unwrap();
    assert!(scheduler.disable().is_err());
    assert!(scheduler.enable(&spec).is_err());
    assert_eq!(
        fs::read_to_string(agents.join(format!("{LABEL}.plist"))).unwrap(),
        "someone else's"
    );

    let writable = dir.path().join("writable");
    fs::write(&writable, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o777)).unwrap();
    fs::remove_file(agents.join(format!("{LABEL}.plist"))).unwrap();
    let unsafe_spec = Spec {
        executable: writable,
        ..spec
    };
    assert!(scheduler.enable(&unsafe_spec).is_err());
}

fn fake_tool(dir: &std::path::Path, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let log = dir.join("calls.log");
    let tool = dir.join(name);
    fs::write(
        &tool,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    (tool, log)
}

#[cfg(target_os = "linux")]
#[test]
fn auto_delete_timer_is_written_marked_and_only_ours_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (tool, log) = fake_tool(dir.path(), "systemctl");
    let units = dir.path().join("systemd/user");
    let scheduler = Scheduler {
        tool: tool.clone(),
        agents: units.clone(),
        job: Job::AutoDelete,
    };
    assert!(!scheduler.state().unwrap().registered);
    let spec = Spec {
        job: Job::AutoDelete,
        executable: tool.clone(),
        state_dir: dir.path().join("state"),
        hour: 0,
    };
    scheduler.enable(&spec).unwrap();
    let service = fs::read_to_string(units.join("omg-auto-delete.service")).unwrap();
    assert!(service.contains("\"auto-delete\" \"run\""));
    assert!(units.join("omg-auto-delete.timer").exists());
    assert!(scheduler.state().unwrap().registered);
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("--user enable --now omg-auto-delete.timer"),
        "{calls}"
    );
    assert!(scheduler.disable().unwrap());
    assert!(!units.join("omg-auto-delete.timer").exists());
    assert!(!scheduler.disable().unwrap());

    fs::write(units.join("omg-auto-delete.timer"), "someone else's").unwrap();
    assert!(scheduler.disable().is_err());
    assert!(scheduler.enable(&spec).is_err());
    assert!(!units.join("omg-auto-delete.service").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn auto_delete_agent_is_separate_from_the_daily_scan() {
    let dir = tempfile::tempdir().unwrap();
    let (tool, _) = fake_tool(dir.path(), "launchctl");
    let agents = dir.path().join("LaunchAgents");
    let auto = Scheduler {
        tool: tool.clone(),
        agents: agents.clone(),
        job: Job::AutoDelete,
    };
    let scan = Scheduler {
        tool: tool.clone(),
        agents: agents.clone(),
        job: Job::Scan,
    };
    let spec = |job| Spec {
        job,
        executable: tool.clone(),
        state_dir: dir.path().join("state"),
        hour: 3,
    };
    scan.enable(&spec(Job::Scan)).unwrap();
    auto.enable(&spec(Job::AutoDelete)).unwrap();
    assert!(auto.enable(&spec(Job::Scan)).is_err());
    let plist = fs::read_to_string(agents.join("dev.omg.auto-delete.plist")).unwrap();
    assert!(plist.contains("<string>auto-delete</string>"));
    assert!(auto.disable().unwrap());
    assert!(scan.state().unwrap().registered);
    assert!(!auto.state().unwrap().registered);
}
