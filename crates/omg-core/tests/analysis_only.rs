//! Runs only in the analysis-only build (`cargo test -p omg-core --no-default-features`).
//! That build must not delete anything, and must say so.
#![cfg(not(feature = "permanent-delete"))]

use omg_core::{Index, ScanOptions, capability, scanner};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

/// Compile-time: this file only builds when the feature is off.
const _: () = assert!(!omg_core::PERMANENT_DELETE);

#[test]
fn analysis_only_build_reports_no_permanent_delete() {
    assert_eq!(
        capability::platform_permanent_delete(),
        capability::Support::Unsupported
    );
    assert_eq!(
        capability::permanent_delete(capability::Filesystem::Btrfs),
        capability::Support::Unsupported
    );
}

#[test]
fn analysis_only_build_refuses_delete_and_leaves_files_alone() {
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(home.path().join("scratch")).unwrap();
    fs::write(home.path().join("scratch/a.bin"), b"keep").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    let report = scanner::scan(
        &mut index,
        &ScanOptions {
            root: home.path().to_path_buf(),
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap();
    assert_eq!(report.state, "complete");
    index.mark(b"scratch", 4096, home.path()).unwrap();
    let id = index.marked().unwrap()[0].id;
    let error = index
        .prepare_delete(&[id], home.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("not available in this build"), "{error}");
    assert_eq!(
        fs::read(home.path().join("scratch/a.bin")).unwrap(),
        b"keep"
    );
}
