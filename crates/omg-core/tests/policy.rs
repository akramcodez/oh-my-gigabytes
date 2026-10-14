#![cfg(target_os = "linux")]
use omg_core::{Entry, policy};
use std::{fs, path::Path};

fn entry(path: &str) -> Entry {
    Entry {
        path: path.as_bytes().to_vec(),
        parent: vec![],
        kind: "file".into(),
        identity: "fixture:1".into(),
        apparent: 1,
        allocated: 1,
        links: 1,
        modified_ns: 0,
        changed_ns: 0,
        excluded: None,
    }
}
#[test]
fn catalog_labels_cannot_override_protected_paths_or_unknown_handler() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        ".ssh/private",
        ".git/config",
        "credentials/token",
        "data.sqlite",
        "machine.vhdx",
        "download.part",
        "../escape",
        "OneDrive/file",
        "secret.pem",
    ] {
        assert!(
            policy::protection(root.path(), &entry(path)).is_some(),
            "{path}"
        );
    }
    fs::create_dir(root.path().join(".git")).unwrap();
    // Inside an active repository ordinary files are protected; build/dependency folders are not.
    assert!(policy::protection(root.path(), &entry("src/main.rs")).is_some());
    assert!(policy::protection(root.path(), &entry("node_modules/file")).is_none());
    assert!(policy::protection(root.path(), &entry("target/debug/app")).is_none());
    let plain = tempfile::tempdir().unwrap();
    assert!(
        policy::cleanup_denial(plain.path(), &entry("ordinary"))
            .contains("OS Trash is not implemented")
    );
    assert!(!policy::safe_relative(Path::new("../escape")));
    assert!(!policy::safe_relative(Path::new("/")));
}
#[test]
fn scoped_ignore_boundaries_do_not_match_siblings() {
    assert!(policy::inside(b"cache/item", b"cache", true));
    assert!(!policy::inside(b"cache-other/item", b"cache", true));
    assert!(!policy::inside(b"cache/item", b"cache", false));
    assert!(policy::inside(b"cache", b"cache", false));
}

#[test]
fn stale_repositories_and_unknown_age() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("old/.git")).unwrap();
    let old = |_: &Path| Some(omg_core::now() - 45 * 86_400);
    let fresh = |_: &Path| Some(omg_core::now() - 86_400);
    let mut memo = policy::RepositoryMemo::with_ages(&old);
    assert!(matches!(
        policy::repository_status(root.path(), &entry("old/notes.txt"), &mut memo),
        policy::RepoStatus::Stale { days: 45, .. }
    ));
    assert!(policy::protection_with(root.path(), &entry("old/notes.txt"), &mut memo).is_none());
    let mut memo = policy::RepositoryMemo::with_ages(&fresh);
    assert!(policy::protection_with(root.path(), &entry("old/notes.txt"), &mut memo).is_some());
    // Without age information a repository is treated as active.
    assert!(policy::protection(root.path(), &entry("old/notes.txt")).is_some());
    // .git itself stays hard-protected either way.
    let mut memo = policy::RepositoryMemo::with_ages(&old);
    assert!(policy::protection_with(root.path(), &entry("old/.git/config"), &mut memo).is_some());
}
