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

/// Run explicitly with `OMG_BTRFS_TEST_DIR=/path/on/btrfs cargo test -p omg-cli btrfs_cli_profile -- --ignored`.
#[test]
#[ignore = "requires an existing writable Btrfs path in OMG_BTRFS_TEST_DIR"]
#[cfg(target_os = "linux")]
fn btrfs_cli_profile_scans_and_previews_without_mutation() {
    use std::os::unix::fs::PermissionsExt;

    let base = std::env::var_os("OMG_BTRFS_TEST_DIR")
        .expect("set OMG_BTRFS_TEST_DIR to an existing writable Btrfs directory");
    let base = std::path::Path::new(&base);
    let fs_type = Command::new("stat")
        .args(["-f", "--format=%T"])
        .arg(base)
        .output()
        .expect("GNU stat is needed to verify the native fixture filesystem");
    assert!(fs_type.status.success());
    assert_eq!(String::from_utf8_lossy(&fs_type.stdout).trim(), "btrfs");

    let fixture = tempfile::Builder::new()
        .prefix("omg-btrfs-cli-")
        .tempdir_in(base)
        .unwrap();
    let state = fixture.path().join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = fixture.path().join("scan-root");
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
    let scan = invoke(&["scan", "--root", root.to_str().unwrap()]);
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let status = invoke(&["status"]);
    assert!(status.status.success());
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(
        status["scope_note"]
            .as_str()
            .unwrap()
            .contains("not reclaimable-space estimates")
    );
    assert!(invoke(&["tree"]).status.success());

    let findings = invoke(&["findings"]);
    assert!(findings.status.success());
    let findings: serde_json::Value = serde_json::from_slice(&findings.stdout).unwrap();
    let candidate = findings[0]["id"].as_str().unwrap();
    let preview = invoke(&["cleanup", "--dry-run", "--candidate", candidate]);
    assert!(preview.status.success());
    assert_eq!(std::fs::metadata(&target).unwrap().len(), 101 * 1024 * 1024);
    assert!(invoke(&["history"]).status.success());
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

#[cfg(target_os = "linux")]
mod service {
    use std::{
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    const BINARY: &str = env!("CARGO_BIN_EXE_omg");

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        state: PathBuf,
        config_home: PathBuf,
    }
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let (root, state, config_home) = (
            dir.path().join("root"),
            dir.path().join("state"),
            dir.path().join("config"),
        );
        for path in [&root, &config_home] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(root.join("data.bin"), vec![7u8; 1 << 20]).unwrap();
        Fixture {
            _dir: dir,
            root,
            state,
            config_home,
        }
    }
    fn omg(fx: &Fixture) -> Command {
        let mut command = Command::new(BINARY);
        command
            .arg("--state-dir")
            .arg(&fx.state)
            .env("XDG_CONFIG_HOME", &fx.config_home)
            .env("HOME", &fx.config_home);
        command
    }
    fn json(output: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
    }
    struct Daemon(Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for {what}");
    }

    #[test]
    fn status_and_trends_work_without_a_service() {
        let fx = fixture();
        let scan = omg(&fx)
            .arg("scan")
            .arg("--root")
            .arg(&fx.root)
            .output()
            .unwrap();
        assert!(scan.status.success());
        let status = json(&omg(&fx).arg("status").output().unwrap());
        assert_eq!(status["service"]["state"], "stopped");
        assert!(status["entries"].as_u64().unwrap() >= 1);
        let trends = json(&omg(&fx).args(["trends", "--days", "7"]).output().unwrap());
        assert_eq!(trends["samples"], 1);
        assert!(trends["note"].as_str().unwrap().contains("uncertain"));
        assert!(
            !omg(&fx)
                .args(["trends", "--days", "0"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let health = json(&omg(&fx).args(["service", "status"]).output().unwrap());
        assert_eq!(health["health"]["state"], "stopped");
    }

    #[test]
    fn registration_requires_explicit_approval_and_writes_nothing_without_it() {
        let fx = fixture();
        let out = omg(&fx).arg("start").stdin(Stdio::null()).output().unwrap();
        assert!(!out.status.success());
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            message.contains("approval") || message.contains("unsupported"),
            "{message}"
        );
        assert!(
            !fx.config_home.join("systemd").exists(),
            "unit written without approval"
        );
        let out = omg(&fx)
            .args(["service", "install"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(!fx.config_home.join("systemd").exists());
        // Uninstall with nothing registered is a harmless no-op that keeps state.
        let out = omg(&fx).args(["service", "uninstall"]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn invalid_or_forged_configuration_stops_the_service_before_touching_state() {
        let fx = fixture();
        let config = fx.config_home.join("bad.toml");
        for forged in ["cleanup = true\n", "[schedule]\ninterval_minutes = 1\n"] {
            std::fs::write(&config, forged).unwrap();
            let out = omg(&fx)
                .arg("--config")
                .arg(&config)
                .args(["service", "run"])
                .output()
                .unwrap();
            assert!(!out.status.success(), "{forged}");
            assert!(!fx.state.exists(), "state created despite invalid config");
        }
    }

    #[test]
    fn running_service_coexists_with_foreground_commands_and_stops_cleanly() {
        let fx = fixture();
        assert!(
            omg(&fx)
                .arg("scan")
                .arg("--root")
                .arg(&fx.root)
                .output()
                .unwrap()
                .status
                .success()
        );
        let mut child = omg(&fx)
            .args(["service", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let daemon = Daemon(child);
        wait_for("service socket", || fx.state.join("service.sock").exists());
        wait_for("running health", || {
            let out = omg(&fx).args(["service", "status"]).output().unwrap();
            out.status.success() && json(&out)["health"]["state"] == "running"
        });
        let live = json(&omg(&fx).args(["service", "status"]).output().unwrap());
        assert_eq!(
            live["live"]["operations"],
            serde_json::json!(["status", "scan", "yield", "stop"])
        );
        assert_eq!(live["live"]["priority_lowered"], true);

        // A second service is refused; a foreground scan and a scan request both work.
        let second = omg(&fx).args(["service", "run"]).output().unwrap();
        assert!(!second.status.success());
        assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));
        assert!(
            omg(&fx)
                .arg("scan")
                .arg("--root")
                .arg(&fx.root)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            omg(&fx)
                .args(["service", "scan"])
                .output()
                .unwrap()
                .status
                .success()
        );
        wait_for("requested scan recorded", || {
            json(&omg(&fx).arg("trends").output().unwrap())["samples"]
                .as_u64()
                .unwrap()
                >= 3
        });

        // Stop through IPC (nothing is registered with systemd in this fixture).
        assert!(omg(&fx).arg("stop").output().unwrap().status.success());
        let mut daemon = daemon;
        wait_for("clean exit", || daemon.0.try_wait().unwrap().is_some());
        assert!(daemon.0.try_wait().unwrap().unwrap().success());
        let mut log = String::new();
        std::io::Read::read_to_string(&mut { stderr }, &mut log).unwrap();
        assert!(log.contains("scan-only; no cleanup capability"));
        let health = json(&omg(&fx).args(["service", "status"]).output().unwrap());
        assert_eq!(health["health"]["detail"], "stopped cleanly");
        assert!(!fx.state.join("service.sock").exists());
        assert_eq!(
            std::fs::read(fx.root.join("data.bin")).unwrap().len(),
            1 << 20
        );
        assert_eq!(
            std::fs::metadata(&fx.state).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn doctor_reports_registration_without_changing_anything() {
        let fx = fixture();
        assert!(
            omg(&fx)
                .arg("scan")
                .arg("--root")
                .arg(&fx.root)
                .output()
                .unwrap()
                .status
                .success()
        );
        let out = omg(&fx).arg("doctor").output().unwrap();
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("service:") && text.contains("\"registered\": false"),
            "{text}"
        );
        assert!(!Path::new(&fx.config_home.join("systemd")).exists());
    }
}

#[test]
#[cfg(target_os = "linux")]
fn tree_reports_subtree_sizes_and_rejects_escapes() {
    let fixture = tempfile::tempdir().unwrap();
    let (root, state) = (fixture.path().join("root"), fixture.path().join("state"));
    std::fs::create_dir_all(root.join("big/inner")).unwrap();
    std::fs::create_dir(root.join("small")).unwrap();
    std::fs::write(root.join("big/inner/data"), vec![1u8; 2 << 20]).unwrap();
    std::fs::write(root.join("small/data"), vec![1u8; 4096]).unwrap();
    let omg = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_omg"))
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .output()
            .unwrap()
    };
    // Before any scan there is nothing to report.
    assert!(!omg(&["tree"]).status.success());
    assert!(
        omg(&["scan", "--root", root.to_str().unwrap()])
            .status
            .success()
    );
    let json = |out: &std::process::Output| -> serde_json::Value {
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let top = json(&omg(&["tree"]));
    assert_eq!(top["children"][0]["path"], "\"big\"");
    assert!(top["children"][0]["allocated"].as_u64().unwrap() >= 2 << 20);
    assert!(top["totals"]["allocated"].as_u64().unwrap() >= 2 << 20);
    let inner = json(&omg(&["tree", "big"]));
    assert_eq!(inner["children"][0]["path"], "\"big/inner\"");
    let absolute = json(&omg(&[
        "tree",
        root.join("big").to_str().unwrap(),
        "--limit",
        "1",
    ]));
    assert_eq!(absolute["children"].as_array().unwrap().len(), 1);
    assert!(!omg(&["tree", "../escape"]).status.success());
    assert!(!omg(&["tree", "/etc"]).status.success());
    assert!(!omg(&["tree", "no-such-dir"]).status.success());
    let status = json(&omg(&["status"]));
    assert!(status["explained_estimate_bytes"].is_u64());
}

/// Without a terminal the interactive TUI must refuse with a clear message, not hang or draw.
/// Stdin and stdout are pipes here, so the binary sees no terminal.
#[test]
#[cfg(target_os = "linux")]
fn interactive_tui_refuses_without_a_terminal_and_changes_nothing() {
    let fixture = tempfile::tempdir().unwrap();
    let state = fixture.path().join("state");
    let result = Command::new(env!("CARGO_BIN_EXE_omg"))
        .arg("--state-dir")
        .arg(&state)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("interactive TUI requires a terminal"),
        "{stderr}"
    );
    assert!(!state.exists(), "refusal must not create state");
}
