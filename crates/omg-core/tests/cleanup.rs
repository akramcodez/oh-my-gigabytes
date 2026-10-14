#![cfg(target_os = "linux")]
use omg_core::{Index, ScanOptions, cleanup::Gate, scanner};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    (home, state)
}
fn scan(index: &mut Index, root: &Path) {
    let report = scanner::scan(
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
    .unwrap();
    assert_eq!(report.state, "complete");
}
fn id_of(index: &Index, path: &[u8]) -> i64 {
    index
        .marked()
        .unwrap()
        .into_iter()
        .find(|m| m.path == path)
        .unwrap()
        .id
}

#[test]
fn trash_list_only_saves_paths_and_permanent_delete_removes_after_confirmation() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join("cache/deep")).unwrap();
    fs::write(h.join("cache/deep/blob"), vec![1u8; 300_000]).unwrap();
    fs::write(h.join("outside-target"), b"keep me").unwrap();
    symlink(h.join("outside-target"), h.join("cache/link")).unwrap();
    fs::write(h.join("big.iso"), vec![2u8; 200_000]).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    let before = index.subtree(b"").unwrap().unwrap().allocated;

    assert_eq!(index.mark(b"cache", 300_000, h).unwrap(), Gate::Allowed);
    assert_eq!(index.mark(b"big.iso", 200_000, h).unwrap(), Gate::Allowed);
    assert_eq!(index.marked().unwrap().len(), 2);
    assert!(
        h.join("cache/deep/blob").exists(),
        "marking never touches disk"
    );
    index.unmark(id_of(&index, b"big.iso")).unwrap();
    assert_eq!(index.marked().unwrap().len(), 1);

    let plan = index.prepare_delete(&[id_of(&index, b"cache")], h).unwrap();
    assert!(
        index.delete_permanently(&plan, "wrong", h).is_err(),
        "needs the exact plan"
    );
    assert!(h.join("cache").exists());
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert_eq!(report.deleted, vec![b"cache".to_vec()]);
    assert!(report.skipped.is_empty());
    assert!(!h.join("cache").exists());
    assert_eq!(
        fs::read(h.join("outside-target")).unwrap(),
        b"keep me",
        "links are never followed"
    );
    assert!(h.join("big.iso").exists());
    // The index is adjusted without a rescan and the item leaves OMG Trash.
    assert!(index.subtree(b"cache").unwrap().is_none());
    assert!(index.subtree(b"").unwrap().unwrap().allocated < before);
    assert!(index.marked().unwrap().is_empty());
    let history = index.history().unwrap();
    assert!(
        history
            .iter()
            .any(|h| h.kind == "delete" && h.outcome == "deleted")
    );
}

#[test]
fn protected_outside_home_and_replaced_targets_are_never_deleted() {
    let (base, state) = setup();
    let root = base.path();
    let home = root.join("home");
    fs::create_dir_all(home.join(".ssh")).unwrap();
    fs::write(home.join(".ssh/id"), b"secret").unwrap();
    fs::write(home.join("app.sqlite"), b"db").unwrap();
    fs::write(root.join("system.bin"), b"sys").unwrap();
    fs::write(home.join("swap-me"), b"original").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root);
    for (path, why) in [
        (b"home/.ssh".as_slice(), "sensitive"),
        (b"home/app.sqlite".as_slice(), "database"),
        (b"system.bin".as_slice(), "outside your home"),
        (b"home".as_slice(), "outside your home"),
    ] {
        let error = index.mark(path, 1, &home).unwrap_err().to_string();
        assert!(error.contains("cannot add"), "{why}: {error}");
    }
    assert_eq!(
        index.mark(b"home/swap-me", 8, &home).unwrap(),
        Gate::Allowed
    );
    // Replace the file after it was saved: same name, different object.
    fs::remove_file(home.join("swap-me")).unwrap();
    fs::write(home.join("swap-me"), b"new important data").unwrap();
    let plan = index
        .prepare_delete(&[id_of(&index, b"home/swap-me")], &home)
        .unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), &home)
        .unwrap();
    assert!(report.deleted.is_empty());
    assert!(report.skipped[0].1.contains("replaced"));
    assert_eq!(
        fs::read(home.join("swap-me")).unwrap(),
        b"new important data"
    );
}

#[test]
fn git_repositories_warn_and_git_metadata_is_never_deleted() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join("proj/.git")).unwrap();
    fs::write(h.join("proj/.git/HEAD"), b"ref").unwrap();
    fs::create_dir_all(h.join("proj/target/debug")).unwrap();
    fs::write(h.join("proj/target/debug/app"), vec![0u8; 50_000]).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    // Inside a repository: allowed only with a warning.
    assert!(
        matches!(index.mark(b"proj/target", 50_000, h).unwrap(), Gate::Warn(w) if w.contains("Git repository"))
    );
    // The whole repository (contains .git) can be listed but deletion refuses at the fresh check.
    assert!(matches!(
        index.mark(b"proj", 50_000, h).unwrap(),
        Gate::Warn(_)
    ));
    let plan = index.prepare_delete(&[id_of(&index, b"proj")], h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert!(report.deleted.is_empty());
    assert!(
        report.skipped[0].1.contains("protected"),
        "{:?}",
        report.skipped
    );
    assert!(h.join("proj/.git/HEAD").exists());
    let plan = index
        .prepare_delete(&[id_of(&index, b"proj/target")], h)
        .unwrap();
    assert!(
        plan.items[0].warning.is_some(),
        "the plan carries the warning to show"
    );
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert_eq!(report.deleted.len(), 1);
    assert!(!h.join("proj/target").exists() && h.join("proj/.git").exists());
}
