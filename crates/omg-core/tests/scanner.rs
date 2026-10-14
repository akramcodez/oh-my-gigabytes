#![cfg(target_os = "linux")]
use omg_core::{Index, ScanOptions, platform, scanner};
use std::{
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

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

fn private_state() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[test]
fn metadata_accounting_reconciliation_and_exclusions() {
    let fixture = tempfile::tempdir().unwrap();
    let state = private_state();
    fs::create_dir(fixture.path().join("nested")).unwrap();
    fs::write(fixture.path().join("nested/artifact"), b"original").unwrap();
    fs::hard_link(
        fixture.path().join("nested/artifact"),
        fixture.path().join("shared"),
    )
    .unwrap();
    let sparse = fs::File::create(fixture.path().join("sparse")).unwrap();
    sparse.set_len(8 * 1024 * 1024).unwrap();
    fs::create_dir(fixture.path().join(".ssh")).unwrap();
    fs::write(fixture.path().join(".ssh/secret"), b"private fixture").unwrap();
    symlink("nested", fixture.path().join("link")).unwrap();
    let invalid = fixture
        .path()
        .join(std::ffi::OsStr::from_bytes(b"name-\xff"));
    fs::write(&invalid, b"native").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    let first = scan(&mut index, fixture.path());
    assert_eq!(first.state, "complete");
    assert_eq!(first.excluded, 2);
    let entries = index.entries(1000, 0).unwrap();
    assert!(!entries.iter().any(|e| e.path == b".ssh/secret"));
    assert!(!entries.iter().any(|e| e.path == b"link/artifact"));
    assert!(entries.iter().any(|e| e.path == b"name-\xff"));
    let artifact = entries
        .iter()
        .find(|e| e.path == b"nested/artifact")
        .unwrap();
    let shared = entries.iter().find(|e| e.path == b"shared").unwrap();
    assert_eq!(artifact.identity, shared.identity);
    let mut identities = std::collections::BTreeSet::new();
    let mut allocation = 0;
    for entry in &entries {
        if entry.excluded.is_none() && identities.insert(&entry.identity) {
            allocation += entry.allocated;
        }
    }
    assert_eq!(index.status().unwrap().unique_allocated, allocation);
    let sparse = entries.iter().find(|e| e.path == b"sparse").unwrap();
    assert!(sparse.apparent > sparse.allocated);
    let before = fs::metadata(fixture.path().join("nested"))
        .unwrap()
        .mtime_nsec();
    fs::write(fixture.path().join("nested/artifact"), vec![1; 4096]).unwrap();
    assert_eq!(
        before,
        fs::metadata(fixture.path().join("nested"))
            .unwrap()
            .mtime_nsec()
    );
    let second = scan(&mut index, fixture.path());
    assert!(second.changed_entries.unwrap() >= 2);
    assert_eq!(
        index
            .entries(1000, 0)
            .unwrap()
            .iter()
            .find(|e| e.path == b"nested/artifact")
            .unwrap()
            .apparent,
        4096
    );
    fs::remove_file(fixture.path().join("shared")).unwrap();
    scan(&mut index, fixture.path());
    assert!(
        !index
            .entries(1000, 0)
            .unwrap()
            .iter()
            .any(|e| e.path == b"shared")
    );
}

#[test]
fn cancelled_refresh_retains_previous_generation() {
    let root = tempfile::tempdir().unwrap();
    let state = private_state();
    fs::write(root.path().join("keep"), b"fixture").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root.path());
    let previous = index.status().unwrap();
    let cancelled = scanner::scan(
        &mut index,
        &ScanOptions {
            root: root.path().into(),
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    let current = index.status().unwrap();
    assert_eq!(previous.unique_allocated, current.unique_allocated);
    assert_eq!(previous.last_successful, current.last_successful);
}

#[test]
fn denied_roots_and_private_state() {
    let root = tempfile::tempdir().unwrap();
    let state = private_state();
    symlink(root.path(), root.path().join("redirect")).unwrap();
    assert!(platform::linux::root(&root.path().join("redirect")).is_err());
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Index::open(state.path()).is_err());
    assert!(platform::absolute(Path::new("../unsafe")).is_err());
}

#[test]
fn database_corruption_and_future_schema_are_not_overwritten() {
    let state = private_state();
    fs::write(state.path().join("index.sqlite"), b"corrupt fixture").unwrap();
    assert!(Index::open(state.path()).is_err());
    assert!(omg_core::index::diagnose(state.path()).is_err());
    assert_eq!(
        fs::read(state.path().join("index.sqlite")).unwrap(),
        b"corrupt fixture"
    );
    let state = private_state();
    drop(Index::open(state.path()).unwrap());
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute_batch("PRAGMA user_version=999").unwrap();
    drop(db);
    assert!(Index::open(state.path()).is_err());
    let diagnosis = omg_core::index::diagnose(state.path()).unwrap();
    assert_eq!(diagnosis["schema"], 999);
    assert_eq!(diagnosis["schema_supported"], false);
}

#[test]
fn lock_contention_and_interrupted_run_recovery() {
    let root = tempfile::tempdir().unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    let owner = omg_core::index::lock(state.path(), "scan.lock").unwrap();
    assert!(
        scanner::scan(
            &mut index,
            &ScanOptions {
                root: root.path().into(),
                workers: 1,
                exclusions: vec![],
                full: true,
                publish_partial: false,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0))
        )
        .is_err()
    );
    drop(owner);
    scan(&mut index, root.path());
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute(
        "INSERT INTO scans(root,started,state,mode) VALUES(?,0,'running','full')",
        [b"fixture".as_slice()],
    )
    .unwrap();
    let previous = index.status().unwrap().last_successful;
    assert!(previous.is_some());
    scan(&mut index, root.path());
    let running: i64 = db
        .query_row(
            "SELECT count(*) FROM scans WHERE state='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(running, 0);
    let generations: i64 = db
        .query_row("SELECT count(*) FROM scans", [], |r| r.get(0))
        .unwrap();
    assert!(generations <= 2);
}

#[test]
fn partial_refresh_never_publishes_over_last_valid() {
    let root = tempfile::tempdir().unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    fs::write(root.path().join("keep"), b"fixture").unwrap();
    scan(&mut index, root.path());
    let previous = index.status().unwrap().last_successful;
    fs::create_dir(root.path().join("locked")).unwrap();
    fs::set_permissions(
        root.path().join("locked"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let report = scan(&mut index, root.path());
    assert_eq!(report.state, "partial");
    assert!(report.errors > 0);
    assert_eq!(index.status().unwrap().last_successful, previous);
    fs::set_permissions(
        root.path().join("locked"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
}

#[test]
fn explicit_partial_publication_is_labelled_and_opt_in() {
    let root = tempfile::tempdir().unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    fs::write(root.path().join("keep"), b"fixture").unwrap();
    fs::create_dir(root.path().join("locked")).unwrap();
    fs::write(root.path().join("locked/hidden"), b"x").unwrap();
    fs::set_permissions(
        root.path().join("locked"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let report = scanner::scan(
        &mut index,
        &ScanOptions {
            root: root.path().to_path_buf(),
            workers: 4,
            exclusions: vec![],
            full: true,
            publish_partial: true,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("locked"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert_eq!(report.state, "partial");
    let status = index.status().unwrap();
    assert_eq!(status.last_report.unwrap().state, "partial");
    assert!(status.entries >= 2, "partial view is published");
    assert!(index.rollups_available().unwrap());
    assert_eq!(report.changed_entries, Some(report.entries));
    assert!(report.estimated_total > 0);
}

#[test]
fn live_volume_sample_tracks_real_free_space() {
    let dir = tempfile::tempdir().unwrap();
    let (capacity, used_before) =
        platform::volume_sample(dir.path()).expect("supported filesystem");
    assert!(capacity > 0 && used_before <= capacity);
    fs::write(dir.path().join("big"), vec![7u8; 8 << 20]).unwrap();
    let (_, used_after) = platform::volume_sample(dir.path()).unwrap();
    // Other activity adds noise, but a deliberate 8 MiB write must not leave used space lower.
    assert!(
        used_after + (1 << 20) >= used_before,
        "used space shrank: {used_before} -> {used_after}"
    );
}
