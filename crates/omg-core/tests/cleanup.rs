#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "permanent-delete"
))]
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
fn active_repos_lock_but_build_folders_and_stale_repos_are_deletable() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join("proj/.git")).unwrap();
    fs::write(h.join("proj/.git/HEAD"), b"ref").unwrap();
    fs::write(h.join("proj/main.rs"), b"fn main() {}").unwrap();
    fs::create_dir_all(h.join("proj/target/debug")).unwrap();
    fs::write(h.join("proj/target/debug/app"), vec![0u8; 50_000]).unwrap();
    fs::create_dir_all(h.join("old/.git")).unwrap();
    fs::write(h.join("old/.git/HEAD"), b"ref").unwrap();
    fs::write(h.join("old/notes.md"), b"old work").unwrap();
    // Make "old" untouched for 60 days.
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 86_400);
    for path in ["old/notes.md", "old/.git/HEAD", "old/.git", "old"] {
        let file = fs::File::open(h.join(path)).unwrap();
        file.set_modified(past).unwrap();
    }
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);

    // Active repository: its own files and the repository itself are locked...
    assert!(
        index
            .mark(b"proj/main.rs", 1, h)
            .unwrap_err()
            .to_string()
            .contains("active Git repository")
    );
    assert!(index.mark(b"proj", 1, h).is_err());
    // ...but its build output is deletable after a warning.
    assert!(
        matches!(index.mark(b"proj/target", 50_000, h).unwrap(), Gate::Warn(w) if w.contains("build/dependency"))
    );
    let plan = index
        .prepare_delete(&[id_of(&index, b"proj/target")], h)
        .unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert_eq!(report.deleted.len(), 1, "{:?}", report.skipped);
    assert!(!h.join("proj/target").exists() && h.join("proj/.git/HEAD").exists());

    // A repository untouched for 60 days can be deleted as a whole, .git included.
    assert!(
        matches!(index.mark(b"old", 1, h).unwrap(), Gate::Warn(w) if w.contains("untouched for"))
    );
    let plan = index.prepare_delete(&[id_of(&index, b"old")], h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert_eq!(report.deleted.len(), 1, "{:?}", report.skipped);
    assert!(!h.join("old").exists());
}

#[test]
fn caches_may_contain_key_like_files_but_never_sensitive_folders() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join(".cache/uv/pkg/tests")).unwrap();
    fs::write(h.join(".cache/uv/pkg/tests/fixture.pem"), b"test cert").unwrap();
    fs::write(h.join(".cache/uv/pkg/data.sqlite"), b"package data").unwrap();
    fs::create_dir_all(h.join("Documents/project")).unwrap();
    fs::write(h.join("Documents/project/app.db"), b"real data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b".cache/uv", 1, h).unwrap();
    index.mark(b"Documents/project", 1, h).unwrap();
    let ids = [
        id_of(&index, b".cache/uv"),
        id_of(&index, b"Documents/project"),
    ];
    let plan = index.prepare_delete(&ids, h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    // The cache goes despite package files that look like keys/databases...
    assert!(!h.join(".cache/uv").exists());
    // ...an ordinary folder with a real database inside does not.
    assert!(h.join("Documents/project/app.db").exists());
    assert!(
        report
            .skipped
            .iter()
            .any(|(p, why)| p == b"Documents/project" && why.contains("protected"))
    );
}

#[test]
fn scans_suggest_regenerable_caches_but_never_large_files_and_respect_dismissal() {
    let (home, state) = setup();
    let h = home.path();
    // Only the uv cache matches a cache rule here; the big file is a "review" finding.
    fs::create_dir_all(h.join(".cache/uv/archive")).unwrap();
    fs::write(h.join(".cache/uv/archive/wheel"), vec![1u8; 400_000]).unwrap();
    let big = fs::File::create(h.join("movie.mkv")).unwrap();
    big.set_len(150 << 20).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    let listed = index.suggest(h).unwrap();
    let marked = index.marked().unwrap();
    assert_eq!(listed, 1);
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].path, b".cache/uv");
    assert_eq!(marked[0].source, "auto");
    assert!(
        marked.iter().all(|m| m.path != b"movie.mkv"),
        "large files are never auto-added"
    );
    assert!(
        h.join(".cache/uv/archive/wheel").exists(),
        "suggesting never touches disk"
    );

    // Removing a suggestion keeps it dismissed across later scans.
    index.unmark(marked[0].id).unwrap();
    index.suggest(h).unwrap();
    let after = index.marked().unwrap();
    assert!(after.is_empty(), "a dismissed suggestion is not re-added");
    // The user can still add it explicitly.
    assert_eq!(index.mark(b".cache/uv", 1, h).unwrap(), Gate::Allowed);
    assert_eq!(index.marked().unwrap()[0].source, "user");
    assert!(index.auto_trash().unwrap(), "auto-fill is on by default");
    index.set_auto_trash(false).unwrap();
    assert!(!index.auto_trash().unwrap());
    index.set_auto_trash(true).unwrap();
    assert!(index.auto_trash().unwrap());
}

#[test]
fn stale_project_dependencies_are_suggested_but_tool_installs_are_not() {
    let (home, state) = setup();
    let h = home.path();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(90 * 86_400);
    let make = |dir: &str| {
        fs::create_dir_all(h.join(dir).join("node_modules/pkg")).unwrap();
        fs::write(h.join(dir).join("package.json"), b"{}").unwrap();
        fs::write(
            h.join(dir).join("node_modules/pkg/index.js"),
            vec![0u8; 2 << 20],
        )
        .unwrap();
        for p in [
            "node_modules/pkg/index.js",
            "node_modules/pkg",
            "node_modules",
            "package.json",
            "",
        ] {
            fs::File::open(h.join(dir).join(p))
                .unwrap()
                .set_modified(past)
                .unwrap();
        }
    };
    make("projects/old-site");
    make(".nvm/versions/node/v22/lib/node_modules/some-cli");
    // A fresh project is not suggested.
    fs::create_dir_all(h.join("projects/fresh/node_modules/pkg")).unwrap();
    fs::write(h.join("projects/fresh/package.json"), b"{}").unwrap();
    fs::write(
        h.join("projects/fresh/node_modules/pkg/index.js"),
        vec![0u8; 2 << 20],
    )
    .unwrap();
    for p in [
        "projects/old-site",
        "projects",
        ".nvm/versions/node/v22/lib/node_modules",
        ".nvm/versions/node/v22/lib",
        ".nvm/versions/node/v22",
        ".nvm/versions/node",
        ".nvm/versions",
        ".nvm",
    ] {
        fs::File::open(h.join(p))
            .unwrap()
            .set_modified(past)
            .unwrap();
    }
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    let marked: Vec<Vec<u8>> = index
        .marked()
        .unwrap()
        .into_iter()
        .map(|m| m.path)
        .collect();
    assert_eq!(
        marked,
        vec![b"projects/old-site/node_modules".to_vec()],
        "{:?}",
        marked
            .iter()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect::<Vec<_>>()
    );
}

/// Capability refusal is enforced in the backend: a scan root whose filesystem is not an accepted
/// profile (here procfs) refuses both plan creation and permanent deletion, and neither writes an
/// operation record. The plan is built first, so only the backend check can stop the deletion.
#[cfg(target_os = "linux")]
#[test]
fn unsupported_root_refuses_plan_and_delete_with_zero_mutations() {
    let (home, state) = setup();
    let h = home.path();
    fs::write(h.join("cache.bin"), b"keep me").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"cache.bin", 7, h).unwrap();
    let id = id_of(&index, b"cache.bin");
    let plan = index.prepare_delete(&[id], h).unwrap();
    assert_eq!(plan.items.len(), 1);

    // Point the published scan at procfs, which is not an accepted filesystem profile.
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute(
        "UPDATE scans SET root=?1 WHERE id=(SELECT scan_id FROM active)",
        [b"/proc".as_slice()],
    )
    .unwrap();

    let error = index.prepare_delete(&[id], h).unwrap_err().to_string();
    assert!(error.contains("unsupported filesystem"), "{error}");
    let error = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported filesystem"), "{error}");

    let operations: i64 = db
        .query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(operations, 0, "a refused delete must not write intent");
    assert_eq!(fs::read(h.join("cache.bin")).unwrap(), b"keep me");
}

/// A reviewed plan is one-use and session-bound. Replaying it, or presenting it to a reopened
/// index (a new session), deletes nothing. A fresh review of the same item still works.
#[test]
fn reviewed_plan_is_one_use_and_does_not_survive_a_new_session() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");

    // Plan from the first session, then reopen: the plan must not work there.
    let stale = index.prepare_delete(&[id], h).unwrap();
    drop(index);
    let mut index = Index::open(state.path()).unwrap();
    let error = index
        .delete_permanently(&stale, &stale.fingerprint, h)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not issued by this session"), "{error}");
    assert!(h.join("scratch/a.bin").exists());

    // A fresh review in this session deletes once; replaying that plan does nothing.
    let plan = index.prepare_delete(&[id], h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(report.deleted, vec![b"scratch".to_vec()]);
    assert!(!h.join("scratch").exists());
    let error = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already used"), "{error}");
}

fn history_targets(index: &Index, kind_outcome: &str) -> Vec<(Vec<u8>, String)> {
    index
        .history()
        .unwrap()
        .into_iter()
        .find(|h| h.kind == "delete" && h.outcome == kind_outcome)
        .map(|h| h.targets.into_iter().map(|t| (t.path, t.outcome)).collect())
        .unwrap_or_default()
}

/// Intent write failure: the operation and its target rows are one transaction. When it fails,
/// nothing is deleted and no operation is recorded.
#[test]
fn failed_intent_write_means_zero_mutations_and_no_operation() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"keep").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();

    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_targets BEFORE INSERT ON delete_targets BEGIN SELECT RAISE(ABORT,'injected intent failure'); END;",
    )
    .unwrap();
    let error = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap_err()
        .to_string();
    assert!(error.contains("injected intent failure"), "{error}");
    assert_eq!(fs::read(h.join("scratch/a.bin")).unwrap(), b"keep");
    let operations: i64 = db
        .query_row("SELECT COUNT(*) FROM operations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(operations, 0);
}

/// Cancellation stops before the next target. Nothing new starts, and the remaining targets are
/// recorded as `unattempted` rather than silently dropped.
#[test]
fn cancellation_before_a_target_records_it_as_unattempted() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("one")).unwrap();
    fs::write(h.join("one/a.bin"), b"1").unwrap();
    fs::create_dir(h.join("two")).unwrap();
    fs::write(h.join("two/b.bin"), b"2").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"one", 4096, h).unwrap();
    index.mark(b"two", 4096, h).unwrap();
    let ids = [id_of(&index, b"one"), id_of(&index, b"two")];
    let plan = index.prepare_delete(&ids, h).unwrap();

    let cancel = AtomicBool::new(true);
    let report = index
        .delete_with_control(&plan, &plan.fingerprint, h, Some(&cancel), |_| {})
        .unwrap();
    assert!(report.cancelled);
    assert!(report.deleted.is_empty());
    assert!(h.join("one/a.bin").exists() && h.join("two/b.bin").exists());
    let targets = history_targets(&index, "cancelled");
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|(_, outcome)| outcome == "unattempted"));
}

/// An attempt that never finished (process killed after intent) is labelled honestly after restart.
/// Its targets are not replayed by the next delete.
#[test]
fn interrupted_attempt_is_labelled_after_restart_and_never_replayed() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    drop(index);

    // Simulate a killed run: an operation and target left at `intent`.
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute(
        "INSERT INTO operations(created,kind,fingerprint,outcome,detail) VALUES(1,'delete','x','intent','{}')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO delete_targets(operation,item,path,outcome,detail,updated) VALUES(last_insert_rowid(),?,?,'intent','',1)",
        rusqlite::params![id, b"scratch".to_vec()],
    )
    .unwrap();
    drop(db);

    let mut index = Index::open(state.path()).unwrap();
    let fresh = index.prepare_delete(&[id], h).unwrap();
    index
        .delete_permanently(&fresh, &fresh.fingerprint, h)
        .unwrap();
    let interrupted: Vec<String> = index
        .history()
        .unwrap()
        .into_iter()
        .filter(|h| h.kind == "delete")
        .flat_map(|h| h.targets.into_iter().map(|t| t.outcome))
        .collect();
    assert!(
        interrupted.contains(&"interrupted-outcome-unknown".to_string()),
        "{interrupted:?}"
    );
    assert!(
        !h.join("scratch").exists(),
        "the fresh review still deletes"
    );
}

/// A removal that fails partway is `partial`, not `skipped`, and marks the totals for a rescan.
#[test]
fn partial_removal_is_reported_as_partial_and_flags_reconcile() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join("scratch/locked")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"removable").unwrap();
    fs::write(h.join("scratch/locked/b.bin"), b"stuck").unwrap();
    fs::set_permissions(h.join("scratch/locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();

    let result = index.delete_permanently(&plan, &plan.fingerprint, h);
    // Restore before asserting so the temporary home can always be removed.
    fs::set_permissions(h.join("scratch/locked"), fs::Permissions::from_mode(0o755)).unwrap();
    let report = result.unwrap();
    assert!(report.deleted.is_empty());
    assert!(report.removed_entries >= 1, "{report:?}");
    assert!(report.needs_reconcile);
    assert!(
        !h.join("scratch/a.bin").exists(),
        "removed entries stay removed"
    );
    assert!(h.join("scratch/locked/b.bin").exists());
    let targets = history_targets(&index, "partial");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].1, "partial");
}

/// Race barriers. The target is changed after validation and before mutation. The mutation must
/// judge the object it finds, so a substituted object survives.
#[test]
fn directory_replaced_after_validation_survives() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"original").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();

    let report = index
        .delete_with_control(&plan, &plan.fingerprint, h, None, |path| {
            if path == b"scratch" {
                fs::rename(h.join("scratch"), h.join("moved")).unwrap();
                fs::create_dir(h.join("scratch")).unwrap();
                fs::write(h.join("scratch/new.txt"), b"new important data").unwrap();
            }
        })
        .unwrap();
    assert!(report.deleted.is_empty(), "{report:?}");
    assert_eq!(
        fs::read(h.join("scratch/new.txt")).unwrap(),
        b"new important data"
    );
}

#[test]
fn directory_swapped_for_symlink_never_leads_outside_home() {
    let (home, state) = setup();
    let h = home.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();

    index
        .delete_with_control(&plan, &plan.fingerprint, h, None, |path| {
            if path == b"scratch" {
                fs::remove_dir_all(h.join("scratch")).unwrap();
                symlink(outside.path(), h.join("scratch")).unwrap();
            }
        })
        .unwrap();
    assert_eq!(
        fs::read(outside.path().join("secret.txt")).unwrap(),
        b"secret"
    );
}

#[test]
fn protected_child_inserted_after_validation_stops_the_delete() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();

    let report = index
        .delete_with_control(&plan, &plan.fingerprint, h, None, |path| {
            if path == b"scratch" {
                fs::create_dir(h.join("scratch/.ssh")).unwrap();
                fs::write(h.join("scratch/.ssh/id_ed25519"), b"key").unwrap();
            }
        })
        .unwrap();
    assert!(report.deleted.is_empty(), "{report:?}");
    assert_eq!(fs::read(h.join("scratch/.ssh/id_ed25519")).unwrap(), b"key");
}

/// Hard links: deleting one name must not remove the other name's data, and the totals must match
/// an independent fresh scan afterwards (WP07 oracle).
#[test]
fn deleting_one_hard_link_name_keeps_data_and_totals_match_a_fresh_scan() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::create_dir(h.join("keep")).unwrap();
    fs::write(h.join("keep/data.bin"), vec![7u8; 8192]).unwrap();
    fs::hard_link(h.join("keep/data.bin"), h.join("scratch/alias.bin")).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    let id = id_of(&index, b"scratch");
    let plan = index.prepare_delete(&[id], h).unwrap();
    index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(fs::read(h.join("keep/data.bin")).unwrap().len(), 8192);

    let after_delete = index.subtree(b"").unwrap().unwrap();
    let fresh_state = tempfile::tempdir().unwrap();
    fs::set_permissions(fresh_state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut fresh = Index::open(fresh_state.path()).unwrap();
    scan(&mut fresh, h);
    let fresh_totals = fresh.subtree(b"").unwrap().unwrap();
    assert_eq!(after_delete.allocated, fresh_totals.allocated);
    assert_eq!(after_delete.apparent, fresh_totals.apparent);
    assert_eq!(after_delete.files, fresh_totals.files);
}

/// A hard link whose other name survives outside the deleted folder keeps its bytes on disk. The
/// estimate must not count them, and the totals must still match a fresh scan. "aaa" sorts before
/// "keep", so the removed name is the first path the linked file is attributed to.
#[test]
fn surviving_hard_link_is_not_estimated_as_freed_and_totals_match_a_fresh_scan() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("aaa")).unwrap();
    fs::create_dir(h.join("keep")).unwrap();
    fs::write(h.join("keep/data.bin"), vec![7u8; 8192]).unwrap();
    fs::hard_link(h.join("keep/data.bin"), h.join("aaa/alias.bin")).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"aaa", 8192, h).unwrap();
    let plan = index.prepare_delete(&[id_of(&index, b"aaa")], h).unwrap();
    assert!(
        plan.estimated < 8192,
        "estimate counts bytes that survive: {}",
        plan.estimated
    );
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(report.deleted, vec![b"aaa".to_vec()], "{report:?}");
    assert!(report.estimated_bytes < 8192, "{report:?}");
    assert!(report.needs_reconcile, "{report:?}");
    assert_eq!(fs::read(h.join("keep/data.bin")).unwrap().len(), 8192);

    let after = index.subtree(b"").unwrap().unwrap();
    let fresh_state = tempfile::tempdir().unwrap();
    fs::set_permissions(fresh_state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut fresh = Index::open(fresh_state.path()).unwrap();
    scan(&mut fresh, h);
    let expected = fresh.subtree(b"").unwrap().unwrap();
    assert_eq!(after.allocated, expected.allocated);
    assert_eq!(after.apparent, expected.apparent);
    assert_eq!(after.files, expected.files);
    assert_eq!(after.dirs, expected.dirs);
}

/// When every name of a hard-linked file is inside the removed folder, its bytes really are freed:
/// they are counted once, nothing is flagged, and the totals match a fresh scan.
#[test]
fn hard_link_wholly_inside_the_removed_folder_is_freed_once() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/one.bin"), vec![7u8; 8192]).unwrap();
    fs::hard_link(h.join("scratch/one.bin"), h.join("scratch/two.bin")).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 8192, h).unwrap();
    let plan = index
        .prepare_delete(&[id_of(&index, b"scratch")], h)
        .unwrap();
    assert!(plan.estimated >= 8192, "{}", plan.estimated);
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(report.deleted, vec![b"scratch".to_vec()], "{report:?}");
    assert!(report.estimated_bytes >= 8192, "{report:?}");
    assert!(!report.needs_reconcile, "{report:?}");

    let after = index.subtree(b"").unwrap().unwrap();
    let fresh_state = tempfile::tempdir().unwrap();
    fs::set_permissions(fresh_state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut fresh = Index::open(fresh_state.path()).unwrap();
    scan(&mut fresh, h);
    let expected = fresh.subtree(b"").unwrap().unwrap();
    assert_eq!(after.allocated, expected.allocated);
    assert_eq!(after.apparent, expected.apparent);
    assert_eq!(after.files, expected.files);
    assert_eq!(after.dirs, expected.dirs);
}

/// Without a recorded creation time, a saved item cannot be told apart from a replacement that
/// reused its inode number, so deletion refuses it instead of comparing identity alone.
#[test]
fn item_without_a_recorded_creation_time_is_never_deleted() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("scratch")).unwrap();
    fs::write(h.join("scratch/a.bin"), b"data").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"scratch", 4096, h).unwrap();
    // A row saved without a creation time: older rows, or a filesystem that cannot report one.
    rusqlite::Connection::open(state.path().join("index.sqlite"))
        .unwrap()
        .execute("UPDATE marked SET birth=NULL", [])
        .unwrap();
    let plan = index
        .prepare_delete(&[id_of(&index, b"scratch")], h)
        .unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert!(report.deleted.is_empty(), "{report:?}");
    assert!(report.skipped[0].1.contains("creation time"), "{report:?}");
    assert_eq!(fs::read(h.join("scratch/a.bin")).unwrap(), b"data");
}

#[test]
fn delete_waits_out_housekeeping_but_a_running_scan_still_stops_it() {
    use omg_core::index::{LOCK_WAIT, lock};
    let (home, state) = setup();
    let h = home.path();
    fs::write(h.join("big.iso"), vec![2u8; 200_000]).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    assert_eq!(index.mark(b"big.iso", 200_000, h).unwrap(), Gate::Allowed);

    // The scan lock is held for good (a scan is running): after the wait the delete is refused, says
    // why, and the file is untouched.
    let scanning = lock(state.path(), "scan.lock").unwrap();
    let plan = index
        .prepare_delete(&[id_of(&index, b"big.iso")], h)
        .unwrap();
    let started = std::time::Instant::now();
    let error = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap_err();
    assert!(
        error.to_string().contains("a scan is running"),
        "unexpected error: {error}"
    );
    assert!(started.elapsed() >= LOCK_WAIT, "it waited before giving up");
    assert_eq!(fs::read(h.join("big.iso")).unwrap().len(), 200_000);
    drop(scanning);

    // The lock is only held for a moment (housekeeping between two slices): the delete goes through.
    let busy = lock(state.path(), "scan.lock").unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(busy);
    });
    let plan = index
        .prepare_delete(&[id_of(&index, b"big.iso")], h)
        .unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    release.join().unwrap();
    assert_eq!(report.deleted, vec![b"big.iso".to_vec()]);
    assert!(!h.join("big.iso").exists());
}

/// A uv-like cache: Python packages ship library folders called `credentials`, which carry a
/// protected name, next to ordinary packages.
fn cache_with_credentials_folder(h: &Path) {
    let root = h.join(".cache/uv");
    fs::create_dir_all(root.join("archive-v0/aaa/lib")).unwrap();
    fs::write(root.join("archive-v0/aaa/lib/mod.py"), vec![1u8; 20_000]).unwrap();
    fs::create_dir_all(root.join("archive-v0/bbb/pkg/credentials")).unwrap();
    fs::write(
        root.join("archive-v0/bbb/pkg/credentials/auth.py"),
        b"library code",
    )
    .unwrap();
    fs::write(
        root.join("archive-v0/bbb/pkg/__init__.py"),
        vec![2u8; 20_000],
    )
    .unwrap();
    fs::create_dir_all(root.join("archive-v0/ccc")).unwrap();
    fs::write(root.join("archive-v0/ccc/data"), vec![3u8; 20_000]).unwrap();
    fs::create_dir_all(root.join("simple")).unwrap();
    fs::write(root.join("simple/index"), vec![4u8; 20_000]).unwrap();
}

#[test]
fn a_cache_with_protected_names_inside_can_be_deleted_around_them() {
    let (home, state) = setup();
    let h = home.path();
    cache_with_credentials_folder(h);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b".cache/uv", 1, h).unwrap();
    let id = id_of(&index, b".cache/uv");

    // Whole: refused when planned, with the reason and the alternative, not after confirming.
    let whole = index.prepare_delete(&[id], h).unwrap();
    assert!(whole.items.is_empty());
    assert_eq!(whole.keepable, vec![id]);
    assert!(
        whole.denied[0].1.contains("1 protected item"),
        "{}",
        whole.denied[0].1
    );
    assert!(h.join(".cache/uv").exists());

    // Around the protected folder: ordinary pieces, never the protected folder or what leads to it.
    let plan = index.prepare_delete_keeping_protected(&[id], h).unwrap();
    let pieces: Vec<String> = plan
        .items
        .iter()
        .map(|i| String::from_utf8(i.path.clone()).unwrap())
        .collect();
    for expected in [
        ".cache/uv/archive-v0/aaa",
        ".cache/uv/archive-v0/ccc",
        ".cache/uv/simple",
        ".cache/uv/archive-v0/bbb/pkg/__init__.py",
    ] {
        assert!(
            pieces.iter().any(|p| p == expected),
            "{expected}: {pieces:?}"
        );
    }
    assert!(
        !pieces
            .iter()
            .any(|p| p.contains("credentials") || p.ends_with("bbb") || p.ends_with("/pkg")),
        "{pieces:?}"
    );
    assert!(plan.items.iter().all(|i| i.id < 0 && i.birth.is_some()));
    assert!(plan.keepable.is_empty());
    assert!(
        plan.denied
            .iter()
            .any(|(p, why)| p.ends_with(b"bbb/pkg/credentials") && why.starts_with("kept"))
    );

    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert_eq!(
        report.deleted.len(),
        plan.items.len(),
        "{:?}",
        report.skipped
    );
    let root = h.join(".cache/uv");
    assert!(!root.join("archive-v0/aaa").exists());
    assert!(!root.join("archive-v0/ccc").exists());
    assert!(!root.join("simple").exists());
    assert!(!root.join("archive-v0/bbb/pkg/__init__.py").exists());
    assert_eq!(
        fs::read(root.join("archive-v0/bbb/pkg/credentials/auth.py")).unwrap(),
        b"library code",
        "the protected folder is untouched"
    );
    // The index follows without a rescan, and the saved folder is still listed: it still exists.
    assert!(index.entry(b".cache/uv/archive-v0/aaa").unwrap().is_none());
    assert!(
        index
            .entry(b".cache/uv/archive-v0/bbb/pkg/credentials")
            .unwrap()
            .is_some()
    );
    assert!(index.marked().unwrap().iter().any(|m| m.id == id));
    assert!(
        index
            .history()
            .unwrap()
            .iter()
            .any(|h| h.kind == "delete" && h.outcome == "partial")
    );
}

#[test]
fn only_cache_folders_can_be_deleted_around_protected_items() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join("Documents/project/credentials")).unwrap();
    fs::write(h.join("Documents/project/credentials/token"), b"secret").unwrap();
    fs::write(h.join("Documents/project/notes.txt"), b"mine").unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"Documents/project", 1, h).unwrap();
    let id = id_of(&index, b"Documents/project");
    // An ordinary folder is refused when planned (it would be skipped when deleted), with no
    // alternative offered.
    let whole = index.prepare_delete(&[id], h).unwrap();
    assert!(whole.items.is_empty() && whole.keepable.is_empty());
    assert!(whole.denied[0].1.contains("sensitive item"));
    let plan = index.prepare_delete_keeping_protected(&[id], h).unwrap();
    assert!(plan.items.is_empty());
    assert!(plan.denied[0].1.contains("only cache and build folders"));
    assert!(h.join("Documents/project/notes.txt").exists());
    assert!(h.join("Documents/project/credentials/token").exists());
}

#[test]
fn a_protected_item_that_appears_after_review_still_stops_its_piece() {
    let (home, state) = setup();
    let h = home.path();
    cache_with_credentials_folder(h);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b".cache/uv", 1, h).unwrap();
    let id = id_of(&index, b".cache/uv");
    let plan = index.prepare_delete_keeping_protected(&[id], h).unwrap();
    // After the review, a protected folder shows up inside one of the pieces.
    let late = h.join(".cache/uv/archive-v0/ccc/.ssh");
    fs::create_dir_all(&late).unwrap();
    fs::write(late.join("id"), b"private key").unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint.clone(), h)
        .unwrap();
    assert!(late.join("id").exists(), "nothing protected is removed");
    assert!(h.join(".cache/uv/archive-v0/ccc/data").exists());
    assert!(
        report
            .skipped
            .iter()
            .any(|(p, why)| p == b".cache/uv/archive-v0/ccc" && why.contains("sensitive item"))
    );
    // The other pieces were not affected by it.
    assert!(!h.join(".cache/uv/archive-v0/aaa").exists());
    assert!(!h.join(".cache/uv/simple").exists());
}

#[test]
fn a_cache_without_protected_items_is_still_one_whole_item() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir_all(h.join(".cache/uv/archive-v0/aaa")).unwrap();
    fs::write(h.join(".cache/uv/archive-v0/aaa/data"), vec![1u8; 20_000]).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b".cache/uv", 1, h).unwrap();
    let id = id_of(&index, b".cache/uv");
    let plan = index.prepare_delete(&[id], h).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert!(plan.keepable.is_empty() && plan.denied.is_empty());
    // Asking to keep protected items when there are none is the same single item.
    let around = index.prepare_delete_keeping_protected(&[id], h).unwrap();
    assert_eq!(around.items.len(), 1);
    assert_eq!(around.items[0].id, id);
}

#[cfg(target_os = "macos")]
fn set_flags(path: &Path, flags: &str) {
    let done = std::process::Command::new("chflags")
        .arg(flags)
        .arg(path)
        .status()
        .unwrap();
    assert!(done.success());
}

#[cfg(target_os = "macos")]
#[test]
fn a_locked_item_stops_the_whole_folder_before_anything_is_removed() {
    let (home, state) = setup();
    let h = home.path();
    fs::create_dir(h.join("stuff")).unwrap();
    fs::write(h.join("stuff/first.txt"), b"keep").unwrap();
    fs::write(h.join("stuff/locked.txt"), b"keep").unwrap();
    fs::write(h.join("alone.txt"), b"keep").unwrap();
    set_flags(&h.join("stuff/locked.txt"), "uchg");
    set_flags(&h.join("alone.txt"), "uchg");
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.mark(b"stuff", 7, h).unwrap();
    index.mark(b"alone.txt", 7, h).unwrap();
    let ids = [id_of(&index, b"stuff"), id_of(&index, b"alone.txt")];
    let plan = index.prepare_delete(&ids, h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    set_flags(&h.join("stuff/locked.txt"), "nouchg");
    set_flags(&h.join("alone.txt"), "nouchg");
    assert!(report.deleted.is_empty(), "{report:?}");
    assert_eq!(report.removed_entries, 0, "{report:?}");
    assert_eq!(report.skipped.len(), 2, "{report:?}");
    assert!(report.skipped.iter().all(|(_, why)| why.contains("locked")));
    assert!(h.join("stuff/first.txt").exists());
    assert!(h.join("stuff/locked.txt").exists());
    assert!(h.join("alone.txt").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_standard_folders_and_app_bundles_are_refused_and_icloud_folders_warn() {
    let (home, state) = setup();
    let h = home.path();
    for folder in [
        "Desktop",
        "Documents",
        "Library/Caches/tool",
        "Library/Preferences",
        "Library/Application Support/Tool",
        "Tools/Thing.app/Contents",
        "projects",
    ] {
        fs::create_dir_all(h.join(folder)).unwrap();
    }
    for file in [
        "Desktop/notes.txt",
        "Documents/report.txt",
        "Tools/Thing.app/Contents/Info.plist",
        "projects/notes.txt",
    ] {
        fs::write(h.join(file), b"x").unwrap();
    }
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    let entries = index.entries(1000, 0).unwrap();
    let gate_of = |path: &[u8]| {
        let entry = entries.iter().find(|e| e.path == path).unwrap();
        omg_core::cleanup::gate(h, h, entry)
    };
    for path in [
        b"Desktop".as_slice(),
        b"Documents",
        b"Library",
        b"Library/Caches",
        b"Library/Preferences",
        b"Library/Application Support",
        b"Tools/Thing.app",
        b"Tools/Thing.app/Contents/Info.plist",
    ] {
        assert!(
            matches!(gate_of(path), Gate::Denied(_)),
            "{}",
            String::from_utf8_lossy(path)
        );
    }
    for path in [b"Desktop/notes.txt".as_slice(), b"Documents/report.txt"] {
        let Gate::Warn(text) = gate_of(path) else {
            panic!("{}", String::from_utf8_lossy(path));
        };
        assert!(text.contains("iCloud"), "{text}");
    }
    assert!(matches!(gate_of(b"projects/notes.txt"), Gate::Allowed));
    assert!(matches!(gate_of(b"Library/Caches/tool"), Gate::Allowed));
}

fn write_bytes(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7u8; bytes]).unwrap();
}

fn suggested(index: &Index) -> Vec<Vec<u8>> {
    index
        .marked()
        .unwrap()
        .into_iter()
        .map(|m| m.path)
        .collect()
}

#[test]
fn big_project_output_is_suggested_with_proof_of_its_owner_and_a_size_floor() {
    let (home, state) = setup();
    let h = home.path();
    write_bytes(&h.join("rusty/Cargo.toml"), 10);
    write_bytes(&h.join("rusty/target/debug/app"), 60 << 20);
    // Inside build output, names that would stop an ordinary folder do not stop this one.
    write_bytes(&h.join("rusty/target/debug/incremental/cache.sqlite"), 10);
    write_bytes(&h.join("small/Cargo.toml"), 10);
    write_bytes(&h.join("small/target/debug/app"), 1 << 20);
    write_bytes(&h.join("photos/target/holiday.jpg"), 60 << 20);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    assert_eq!(suggested(&index), vec![b"rusty/target".to_vec()]);

    let id = id_of(&index, b"rusty/target");
    let plan = index.prepare_delete(&[id], h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(report.deleted.len(), 1, "{report:?}");
    assert!(!h.join("rusty/target").exists());
    assert!(h.join("rusty/Cargo.toml").exists());
    assert!(h.join("photos/target/holiday.jpg").exists());
}

#[test]
fn container_disks_are_shown_never_suggested_and_cannot_be_deleted() {
    let (home, state) = setup();
    let h = home.path();
    write_bytes(&h.join(".colima/_lima/_disks/colima/datadisk"), 120 << 20);
    write_bytes(&h.join(".colima/default/colima.yaml"), 10);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    assert!(suggested(&index).is_empty());
    let findings = index.findings_in(100, 0, false, h).unwrap();
    let vm: Vec<_> = findings
        .iter()
        .filter(|f| f.rule.discovery == "colima_vm")
        .collect();
    let [vm] = vm.as_slice() else {
        panic!("{findings:?}");
    };
    assert_eq!(vm.effective_tier, "protected");
    assert!(vm.rule.explanation.contains("docker system prune"));
    assert!(
        findings.iter().all(|f| f.rule.discovery != "large_file"),
        "the disk is explained by the folder's finding"
    );
    for path in [
        b".colima".as_slice(),
        b".colima/_lima/_disks/colima/datadisk",
    ] {
        assert!(index.mark(path, 1, h).is_err());
    }
    assert!(h.join(".colima/_lima/_disks/colima/datadisk").exists());
}

#[test]
fn old_tool_versions_are_suggested_but_never_the_newest_or_the_one_in_use() {
    let (home, state) = setup();
    let h = home.path();
    let versions = h.join(".local/share/claude/versions");
    for version in ["1.2.3", "1.2.4", "1.2.10", "1.10.0", "notes"] {
        write_bytes(&versions.join(version), 100);
    }
    fs::create_dir_all(h.join(".local/bin")).unwrap();
    symlink(versions.join("1.2.3"), h.join(".local/bin/claude")).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    let mut listed = suggested(&index);
    listed.sort();
    assert_eq!(
        listed,
        vec![
            b".local/share/claude/versions/1.2.10".to_vec(),
            b".local/share/claude/versions/1.2.4".to_vec(),
        ],
        "numbers compare as numbers, so 1.10.0 is the newest"
    );
}

#[test]
fn when_the_newest_tool_version_is_the_one_in_use_only_it_is_kept() {
    let (home, state) = setup();
    let h = home.path();
    let versions = h.join(".local/share/claude/versions");
    for version in ["2.0.1", "2.0.2", "2.0.3"] {
        write_bytes(&versions.join(version), 100);
    }
    fs::create_dir_all(h.join(".local/bin")).unwrap();
    symlink(versions.join("2.0.3"), h.join(".local/bin/claude")).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    let mut listed = suggested(&index);
    listed.sort();
    assert_eq!(
        listed,
        vec![
            b".local/share/claude/versions/2.0.1".to_vec(),
            b".local/share/claude/versions/2.0.2".to_vec(),
        ]
    );
}

#[test]
fn only_old_session_transcripts_are_suggested_and_memory_never() {
    let (home, state) = setup();
    let h = home.path();
    let project = h.join(".claude/projects/some-project");
    for file in [
        "old.jsonl",
        "new.jsonl",
        "memory/MEMORY.md",
        "memory/old.jsonl",
        "notes.md",
    ] {
        write_bytes(&project.join(file), 2 << 20);
    }
    write_bytes(&project.join("tiny.jsonl"), 100);
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400);
    for file in [
        "old.jsonl",
        "memory/MEMORY.md",
        "memory/old.jsonl",
        "notes.md",
        "tiny.jsonl",
    ] {
        fs::File::options()
            .write(true)
            .open(project.join(file))
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
    }
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    assert_eq!(
        suggested(&index),
        vec![b".claude/projects/some-project/old.jsonl".to_vec()]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn big_app_caches_are_suggested_and_apple_ones_are_not() {
    let (home, state) = setup();
    let h = home.path();
    let caches = h.join("Library/Caches");
    write_bytes(&caches.join("SomeBrowser/data.bin"), 60 << 20);
    write_bytes(&caches.join("com.apple.something/data.bin"), 60 << 20);
    write_bytes(&caches.join("GeoServices/data.bin"), 60 << 20);
    write_bytes(&caches.join("TinyApp/data.bin"), 1 << 20);
    write_bytes(&caches.join("Updater/New.app/Contents/x"), 60 << 20);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    let mut listed = suggested(&index);
    listed.sort();
    assert_eq!(
        listed,
        vec![
            b"Library/Caches/SomeBrowser".to_vec(),
            b"Library/Caches/Updater".to_vec(),
        ]
    );
    let id = id_of(&index, b"Library/Caches/Updater");
    let plan = index.prepare_delete(&[id], h).unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(
        report.deleted.len(),
        1,
        "an update waiting in a cache: {report:?}"
    );
    assert!(caches.join("SomeBrowser/data.bin").exists());
    assert!(caches.join("com.apple.something/data.bin").exists());
}

fn git(directory: &Path, arguments: &[&str]) {
    let done = std::process::Command::new("git")
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(arguments)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(done.status.success(), "{arguments:?}: {done:?}");
}

fn age(path: &Path, past: std::time::SystemTime) {
    let metadata = path.symlink_metadata().unwrap();
    if metadata.is_dir() && path.file_name().is_some_and(|n| n == ".git") {
        fs::File::open(path).unwrap().set_modified(past).unwrap();
        return;
    }
    if metadata.is_dir() {
        for item in fs::read_dir(path).unwrap() {
            age(&item.unwrap().path(), past);
        }
    }
    if !metadata.is_symlink() {
        fs::File::open(path).unwrap().set_modified(past).unwrap();
    }
}

/// A real clone of a throwaway repository, its files 60 days old (Git's own index stays newer than
/// the change times that ageing itself causes, as it is after a real checkout).
fn old_clone(home: &Path, source: &Path, name: &str) -> std::path::PathBuf {
    let url = format!("file://{}", source.display());
    git(home, &["clone", "--quiet", &url, name]);
    let clone = home.join(name);
    age(
        &clone,
        std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 86_400),
    );
    clone
}

fn clone_source() -> tempfile::TempDir {
    let source = tempfile::tempdir().unwrap();
    git(source.path(), &["init", "--quiet", "-b", "main"]);
    write_bytes(&source.path().join("src/main.rs"), 2000);
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "--quiet", "-m", "one"]);
    source
}

#[test]
fn only_a_clone_with_nothing_local_to_lose_is_suggested_and_it_deletes_whole() {
    let (home, state) = setup();
    let h = home.path();
    let source = clone_source();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 86_400);

    old_clone(h, source.path(), "untouched");
    write_bytes(&h.join("untouched/node_modules/pkg/index.js"), 100);
    age(&h.join("untouched"), old);

    // Every near miss below is a real Git state, and each must stay out of the list.
    let committed = old_clone(h, source.path(), "committed");
    write_bytes(&committed.join("new.txt"), 10);
    git(&committed, &["add", "."]);
    git(&committed, &["commit", "--quiet", "-m", "local"]);
    age(&committed, old);

    let staged = old_clone(h, source.path(), "staged");
    write_bytes(&staged.join("new.txt"), 10);
    git(&staged, &["add", "."]);
    age(&staged, old);

    let branch = old_clone(h, source.path(), "branch");
    git(&branch, &["branch", "local-only"]);
    age(&branch, old);

    let tagged = old_clone(h, source.path(), "tagged");
    git(&tagged, &["tag", "local-tag"]);
    age(&tagged, old);

    let orphan = old_clone(h, source.path(), "orphan");
    git(&orphan, &["remote", "remove", "origin"]);
    age(&orphan, old);

    git(
        h,
        &[
            "clone",
            "--quiet",
            &format!("file://{}", source.path().display()),
            "recent",
        ],
    );

    fs::create_dir_all(h.join("plain/.git")).unwrap();
    write_bytes(&h.join("plain/notes.md"), 10);
    age(&h.join("plain"), old);

    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    assert_eq!(suggested(&index), vec![b"untouched".to_vec()]);

    let finding = index
        .findings_in(100, 0, false, h)
        .unwrap()
        .into_iter()
        .find(|f| f.rule.discovery == "unused_clone")
        .unwrap();
    assert_eq!(finding.effective_tier, "review");
    assert!(
        finding.reason.contains("uncommitted or unpushed"),
        "{}",
        finding.reason
    );

    let plan = index
        .prepare_delete(&[id_of(&index, b"untouched")], h)
        .unwrap();
    let report = index
        .delete_permanently(&plan, &plan.fingerprint, h)
        .unwrap();
    assert_eq!(report.deleted.len(), 1, "{report:?}");
    assert!(!h.join("untouched").exists());
    for kept in [
        "committed",
        "staged",
        "branch",
        "tagged",
        "orphan",
        "recent",
        "plain",
    ] {
        assert!(h.join(kept).exists(), "{kept}");
    }
}

#[test]
fn a_file_changed_after_the_checkout_keeps_a_clone_out_even_with_an_old_modified_time() {
    let (home, state) = setup();
    let h = home.path();
    let source = clone_source();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 86_400);
    let clone = old_clone(h, source.path(), "edited");
    std::thread::sleep(std::time::Duration::from_millis(2300));
    // As after `cp -p` or `mv`: a new file that claims to be old. Only its change time gives it away.
    write_bytes(&clone.join(".env.backup"), 10);
    age(&clone.join(".env.backup"), old);
    fs::File::open(&clone).unwrap().set_modified(old).unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    assert!(suggested(&index).is_empty(), "{:?}", suggested(&index));
}

#[test]
fn projects_and_session_history_are_only_deleted_on_their_own() {
    let (home, state) = setup();
    let h = home.path();
    let source = clone_source();
    old_clone(h, source.path(), "clone");
    write_bytes(&h.join(".claude/projects/p/old.jsonl"), 2 << 20);
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 86_400);
    fs::File::options()
        .write(true)
        .open(h.join(".claude/projects/p/old.jsonl"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    write_bytes(&h.join("rusty/Cargo.toml"), 10);
    write_bytes(&h.join("rusty/target/debug/app"), 60 << 20);
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, h);
    index.suggest(h).unwrap();
    let ids: Vec<i64> = index.marked().unwrap().iter().map(|m| m.id).collect();
    assert_eq!(ids.len(), 3, "{:?}", suggested(&index));

    // "Delete everything listed" leaves both out, with the reason, and keeps the rest.
    let plan = index.prepare_delete(&ids, h).unwrap();
    let planned: Vec<_> = plan.items.iter().map(|i| i.path.clone()).collect();
    assert_eq!(planned, vec![b"rusty/target".to_vec()]);
    let mut held: Vec<_> = plan.denied.iter().map(|(p, _)| p.clone()).collect();
    held.sort();
    assert_eq!(
        held,
        vec![b".claude/projects/p/old.jsonl".to_vec(), b"clone".to_vec()]
    );
    assert!(
        plan.denied
            .iter()
            .all(|(_, why)| why.contains("on its own"))
    );
    index.discard_plan(&plan.fingerprint);

    // On its own, each can still be deleted.
    for path in [b"clone".as_slice(), b".claude/projects/p/old.jsonl"] {
        let plan = index.prepare_delete(&[id_of(&index, path)], h).unwrap();
        assert_eq!(plan.items.len(), 1, "{:?}", plan.denied);
        let report = index
            .delete_permanently(&plan, &plan.fingerprint, h)
            .unwrap();
        assert_eq!(report.deleted.len(), 1, "{report:?}");
    }
    assert!(!h.join("clone").exists());
    assert!(!h.join(".claude/projects/p/old.jsonl").exists());
    assert!(h.join("rusty/target/debug/app").exists());
}
