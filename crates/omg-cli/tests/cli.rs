use std::process::Command;
#[test]
#[cfg(target_os = "linux")]
fn doctor_does_not_initialize_or_migrate_state() {
    let fixture = tempfile::tempdir().unwrap();
    let state = fixture.path().join("not-created");
    let binary = env!("CARGO_BIN_EXE_omg");
    let result = Command::new(binary)
        .arg("--state-dir")
        .arg(&state)
        .arg("doctor")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(!state.exists());
    let future = fixture.path().join("future");
    std::fs::create_dir(&future).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&future, std::fs::Permissions::from_mode(0o700)).unwrap();
    // SQL fixture produced by a helper path is covered by core diagnosis tests separately.
    std::fs::write(future.join("index.sqlite"), b"corrupt fixture").unwrap();
    let result = Command::new(binary)
        .arg("--state-dir")
        .arg(&future)
        .arg("doctor")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read(future.join("index.sqlite")).unwrap(),
        b"corrupt fixture"
    );
}
#[test]
#[cfg(target_os = "linux")]
fn scan_status_findings_and_dry_run_roundtrip() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let state = fixture.path().join("state");
    std::fs::create_dir(&root).unwrap();
    let target = root.join("large.zip");
    std::fs::File::create(&target)
        .unwrap()
        .set_len(101 * 1024 * 1024)
        .unwrap();
    let binary = env!("CARGO_BIN_EXE_omg");
    let invoke = |args: &[&str]| {
        Command::new(binary)
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .output()
            .unwrap()
    };
    let scan = Command::new(binary)
        .arg("--state-dir")
        .arg(&state)
        .arg("scan")
        .arg("--root")
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    assert!(invoke(&["status"]).status.success());
    let output = invoke(&["findings"]);
    assert!(output.status.success());
    let findings: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = findings[0]["id"].as_str().unwrap();
    let preview = invoke(&["cleanup", "--dry-run", "--candidate", id]);
    assert!(preview.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(plan["actionable"], 0);
    assert_eq!(plan["dry_run"], true);
    assert_eq!(std::fs::metadata(&target).unwrap().len(), 101 * 1024 * 1024);
    let history = invoke(&["history"]);
    let value: serde_json::Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(value[0]["outcome"], "evaluated-no-mutation");
}
#[test]
fn dry_run_requires_explicit_targets_and_no_unattended_execution_flag() {
    let binary = env!("CARGO_BIN_EXE_omg");
    for arguments in [
        vec!["cleanup", "--dry-run"],
        vec!["cleanup", "--candidate", "anything"],
        vec!["cleanup", "--yes"],
        vec!["cleanup", "--force"],
    ] {
        let result = Command::new(binary).args(arguments).output().unwrap();
        assert!(!result.status.success());
    }
    assert!(
        Command::new(binary)
            .arg("--help")
            .output()
            .unwrap()
            .status
            .success()
    );
}
