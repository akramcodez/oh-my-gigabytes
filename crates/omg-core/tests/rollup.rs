#![cfg(target_os = "linux")]
use omg_core::{Index, ScanOptions, scanner};
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

fn private_state() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
fn scan_with(index: &mut Index, root: &Path, cancelled: bool) -> omg_core::ScanReport {
    scanner::scan(
        index,
        &ScanOptions {
            root: root.to_path_buf(),
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(cancelled)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap()
}
/// Independent oracle: unique inodes under `dir` (the directory itself included when `own`).
fn oracle(dir: &Path, own: bool, seen: &mut HashSet<(u64, u64)>) -> (u64, u64, u64) {
    let (mut bytes, mut files, mut dirs) = (0, 0, 0);
    let meta = fs::symlink_metadata(dir).unwrap();
    if own && seen.insert((meta.dev(), meta.ino())) {
        bytes += meta.blocks() * 512;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let meta = fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            let (b, f, d) = oracle(&path, true, seen);
            bytes += b;
            files += f;
            dirs += d + 1;
        } else if seen.insert((meta.dev(), meta.ino())) {
            bytes += meta.blocks() * 512;
            files += 1;
        }
    }
    (bytes, files, dirs)
}

#[test]
fn subtree_totals_match_an_independent_oracle_and_count_hard_links_once() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("a/b")).unwrap();
    fs::create_dir_all(root.path().join("c/empty")).unwrap();
    fs::write(root.path().join("a/b/f1"), vec![1u8; 1 << 20]).unwrap();
    fs::write(root.path().join("a/f2"), vec![2u8; 300_000]).unwrap();
    fs::hard_link(root.path().join("a/f2"), root.path().join("c/f2-link")).unwrap();
    fs::write(root.path().join("top.bin"), vec![3u8; 70_000]).unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    assert!(!index.rollups_available().unwrap());
    assert_eq!(scan_with(&mut index, root.path(), false).state, "complete");
    assert!(index.rollups_available().unwrap());

    for (relative, own) in [("", false), ("a", true), ("a/b", true), ("c", true)] {
        let mut seen = HashSet::new();
        if relative == "c" {
            // The shared inode is attributed to its first path ("a/f2"), not to "c".
            let first = fs::symlink_metadata(root.path().join("a/f2")).unwrap();
            seen.insert((first.dev(), first.ino()));
        }
        let (bytes, files, dirs) = oracle(&root.path().join(relative), own, &mut seen);
        let t = index.subtree(relative.as_bytes()).unwrap().unwrap();
        assert_eq!(t.allocated, bytes, "allocated of {relative:?}");
        assert_eq!((t.files, t.dirs), (files, dirs), "counts of {relative:?}");
        assert_eq!(t.protected, 0);
    }
    assert!(index.subtree(b"missing").unwrap().is_none());

    // Children are ordered by size and mix directories with files.
    let children = index.children(b"", 10, 0).unwrap();
    let names: Vec<&[u8]> = children.iter().map(|c| c.path.as_slice()).collect();
    assert_eq!(names[0], b"a");
    assert!(names.contains(&b"top.bin".as_slice()) && names.contains(&b"c".as_slice()));
    assert!(
        children
            .windows(2)
            .all(|w| w[0].allocated >= w[1].allocated)
    );
    let paged = index.children(b"", 1, 1).unwrap();
    assert_eq!(paged[0].path, children[1].path);
    assert_eq!(index.children(b"a/b", 10, 0).unwrap()[0].path, b"a/b/f1");
}

#[test]
fn protected_descendants_are_flagged_up_the_tree() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("proj/.git")).unwrap();
    fs::create_dir_all(root.path().join("proj/src")).unwrap();
    fs::write(root.path().join("proj/.git/config"), "x").unwrap();
    fs::write(root.path().join("proj/src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.path().join("proj/data.sqlite"), vec![0u8; 5000]).unwrap();
    fs::create_dir_all(root.path().join("other/src")).unwrap();
    fs::write(root.path().join("other/src/lib.rs"), "ordinary").unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    scan_with(&mut index, root.path(), false);
    let proj = index.subtree(b"proj").unwrap().unwrap();
    assert!(
        proj.protected >= 2,
        "excluded .git and sqlite name: {proj:?}"
    );
    assert!(index.subtree(b"").unwrap().unwrap().protected >= proj.protected);
    // Counts are hard protections only (.git, database names); repository age is judged when a
    // decision is made, so ordinary source folders are not flagged.
    assert_eq!(index.subtree(b"proj/src").unwrap().unwrap().protected, 0);
    assert_eq!(index.subtree(b"other").unwrap().unwrap().protected, 0);
    // Newest change time is tracked per subtree (used to tell stale repositories apart).
    let newest = index.subtree(b"proj").unwrap().unwrap().newest;
    assert!(
        (omg_core::now() - newest).abs() < 600,
        "freshly written fixture: {newest}"
    );
    let git = index
        .children(b"proj", 10, 0)
        .unwrap()
        .into_iter()
        .find(|c| c.path == b"proj/.git")
        .unwrap();
    assert!(git.protected >= 1, "excluded directory is marked");
}

#[test]
fn rollups_are_published_with_the_scan_and_pruned_with_it() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    fs::write(root.path().join("d/one"), vec![1u8; 100_000]).unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    scan_with(&mut index, root.path(), false);
    let before = index.subtree(b"d").unwrap().unwrap();

    // A cancelled scan publishes nothing and keeps the previous rollups readable.
    fs::write(root.path().join("d/two"), vec![2u8; 400_000]).unwrap();
    assert_eq!(scan_with(&mut index, root.path(), true).state, "cancelled");
    assert_eq!(index.subtree(b"d").unwrap().unwrap(), before);

    // A completed rescan replaces them, and only the active scan's rows remain.
    assert_eq!(scan_with(&mut index, root.path(), false).state, "complete");
    let after = index.subtree(b"d").unwrap().unwrap();
    assert!(after.allocated >= before.allocated + 400_000);
    assert_eq!(after.files, 2);
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    let scans: i64 = db
        .query_row("SELECT count(DISTINCT scan_id) FROM dir_totals", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(scans, 1);
}

#[test]
fn indexes_without_rollups_report_unavailable_instead_of_zero() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("f"), b"x").unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    scan_with(&mut index, root.path(), false);
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    db.execute("DELETE FROM dir_totals", []).unwrap();
    assert!(!index.rollups_available().unwrap());
    assert!(index.subtree(b"").unwrap().is_none());
    assert!(
        index.children(b"", 10, 0).unwrap().is_empty()
            || index
                .children(b"", 10, 0)
                .unwrap()
                .iter()
                .all(|c| c.kind != "directory")
    );
}

#[test]
fn in_walk_rollups_equal_the_reference_computation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("a/b/.git")).unwrap();
    fs::create_dir_all(root.path().join("c/d")).unwrap();
    fs::write(root.path().join("a/b/.git/config"), "x").unwrap();
    fs::write(root.path().join("a/b/f1"), vec![1u8; 200_000]).unwrap();
    fs::write(root.path().join("a/data.sqlite"), vec![2u8; 9_000]).unwrap();
    fs::write(root.path().join("c/d/f2"), vec![3u8; 70_000]).unwrap();
    fs::hard_link(
        root.path().join("c/d/f2"),
        root.path().join("a/b/link-of-f2"),
    )
    .unwrap();
    fs::hard_link(root.path().join("a/b/f1"), root.path().join("c/f1-link")).unwrap();
    std::os::unix::fs::symlink("a", root.path().join("c/sym")).unwrap();
    let state = private_state();
    let mut index = Index::open(state.path()).unwrap();
    scan_with(&mut index, root.path(), false);
    let db = rusqlite::Connection::open(state.path().join("index.sqlite")).unwrap();
    type Row = (Vec<u8>, i64, i64, i64, i64, i64, i64);
    let read = |db: &rusqlite::Connection| -> Vec<Row> {
        let mut s = db
            .prepare("SELECT path,allocated,apparent,files,dirs,protected,coalesce(newest,0) FROM dir_totals ORDER BY path")
            .unwrap();
        s.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
    };
    let in_walk = read(&db);
    let (scan_id, stored): (i64, (i64, i64, i64)) = db
        .query_row(
            "SELECT scan_id,(SELECT unique_allocated FROM scans WHERE id=scan_id),0 FROM active",
            [],
            |r| Ok((r.get(0)?, (r.get(1)?, 0, 0))),
        )
        .unwrap();
    db.execute("DELETE FROM dir_totals", []).unwrap();
    omg_core::rollup::compute(&index, scan_id, root.path()).unwrap();
    assert_eq!(
        in_walk,
        read(&db),
        "in-walk aggregation must equal the stored-row computation"
    );
    // Scan-wide unique bytes match the old GROUP BY identity query.
    let expected: i64 = db
        .query_row(
            "SELECT coalesce(sum(a),0) FROM (SELECT max(allocated) a FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)",
            [scan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored.0, expected);
}
