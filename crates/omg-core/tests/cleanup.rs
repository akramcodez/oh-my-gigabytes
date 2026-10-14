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
    assert!(!index.auto_trash().unwrap(), "auto-fill is off by default");
    index.set_auto_trash(true).unwrap();
    assert!(index.auto_trash().unwrap());
    index.set_auto_trash(false).unwrap();
    assert!(!index.auto_trash().unwrap());
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
