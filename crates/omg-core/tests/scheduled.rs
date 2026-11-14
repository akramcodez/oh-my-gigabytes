//! Daily scan registration on macOS, against a fake `launchctl` and a temporary LaunchAgents
//! folder, so the real user session is never touched.
#![cfg(target_os = "macos")]
use omg_core::scheduled::{LABEL, Scheduler, Spec};
use std::{fs, os::unix::fs::PermissionsExt};

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
    };
    assert!(!scheduler.state().unwrap().registered);
    let spec = Spec {
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
