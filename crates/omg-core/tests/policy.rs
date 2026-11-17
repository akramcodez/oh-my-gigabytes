#![cfg(unix)]
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
        "Library/Messages/Archive/2024",
        "Library/Safari/History",
        "Library/Mail/V10/INBOX",
        "Library/Application Support/AddressBook/Sources/one",
        "Library/Application Support/MobileSync/Backup/abc/Info",
        "Library/Group Containers/group.example/data",
        "Pictures/Photos Library.photoslibrary/originals/a",
        "Backups/disk.sparsebundle/bands/0",
        "installer.dmg",
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

#[test]
fn macos_personal_data_is_protected_but_ordinary_folders_are_not() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "src/mail/send.rs",
        "app/messages/inbox.rs",
        "Library/Caches/pip/wheel",
        "Library/Application Support/Code/User/settings.json",
        "docs/safari-notes.md",
    ] {
        assert!(
            policy::hard_protection(root.path(), &entry(path)).is_none(),
            "{path} is ordinary"
        );
    }
}

#[test]
fn names_the_filesystem_folds_to_a_protected_name_are_protected() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        ".\u{17f}sh/id_ed25519",
        ".SSH/id_ed25519",
        "Library/\u{212a}eychains/login.keychain-db",
        "Library/MAIL/V10/INBOX",
        "data.\u{17f}qlite",
        "\u{17f}ecret.PEM",
    ] {
        assert!(
            policy::hard_protection(root.path(), &entry(path)).is_some(),
            "{path}"
        );
    }
    assert!(policy::always_sensitive(".\u{17f}sh"));
    assert_eq!(policy::fold("Node_Modules"), "node_modules");
    assert!(policy::hard_protection(root.path(), &entry("cache/\u{e9}t\u{e9}.bin")).is_none());
}

#[test]
fn credentials_is_package_code_only_inside_third_party_trees() {
    let parts = |p: &str| -> Vec<String> { p.split('/').map(String::from).collect() };
    for inside in [
        "proj/node_modules/google-auth-library/build",
        "venv/lib/python3.12/site-packages/google/auth",
        ".cache/uv/archive-v0/abc/google/auth",
        "zed/target/release/build/webrtc-sys-1/out/include/grpc",
        "zed/target/aarch64-apple-darwin/release/build/webrtc-sys-1/out",
    ] {
        assert!(
            !policy::sensitive_name("credentials", &parts(inside)),
            "{inside}"
        );
        assert!(policy::sensitive_name(".ssh", &parts(inside)), "{inside}");
        assert!(policy::sensitive_name(".aws", &parts(inside)), "{inside}");
    }
    for outside in [
        "",
        ".config/gcloud",
        "proj",
        "zed/target/debug",
        "zed/target/release/build/x",
    ] {
        assert!(
            policy::sensitive_name("credentials", &parts(outside)),
            "{outside}"
        );
        assert!(
            policy::sensitive_name("Credentials", &parts(outside)),
            "{outside}"
        );
    }
    assert!(!policy::sensitive_path(b"a/node_modules/x/credentials"));
    assert!(policy::sensitive_path(b"a/credentials"));
}
