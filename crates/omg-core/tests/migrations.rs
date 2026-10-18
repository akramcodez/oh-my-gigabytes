//! Schema upgrade and recovery tests (release plan R10): an index written by any earlier schema
//! version opens, keeps its data and reaches the current version; a newer or damaged index is
//! refused without being touched.
#![cfg(any(unix, windows))]
use omg_core::Index;
use rusqlite::Connection;
use std::fs;

const CURRENT: i64 = 11;
const MIGRATIONS: [&str; 11] = [
    include_str!("../../../migrations/001.sql"),
    include_str!("../../../migrations/002.sql"),
    include_str!("../../../migrations/003.sql"),
    include_str!("../../../migrations/004.sql"),
    include_str!("../../../migrations/005.sql"),
    include_str!("../../../migrations/006.sql"),
    include_str!("../../../migrations/007.sql"),
    include_str!("../../../migrations/008.sql"),
    include_str!("../../../migrations/009.sql"),
    include_str!("../../../migrations/010.sql"),
    include_str!("../../../migrations/011.sql"),
];

fn private_state() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}

/// An index as an old build left it: migrations 1..=`version` applied and one published scan.
fn index_at(state: &std::path::Path, version: usize) {
    let db = Connection::open(state.join("index.sqlite")).unwrap();
    for migration in &MIGRATIONS[..version] {
        db.execute_batch(migration).unwrap();
    }
    db.execute_batch(
        "INSERT INTO scans(id,root,started,ended,state,mode,capacity,available,sampled)
           VALUES(1,x'2f',1,2,'complete','full',1000000,500000,2);
         INSERT INTO active(singleton,scan_id) VALUES(1,1);
         INSERT INTO entries(scan_id,path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded)
           VALUES(1,x'61',x'','file','1:1',100,4096,1,1,1,NULL);
         INSERT INTO settings VALUES('root',x'2f');",
    )
    .unwrap();
    let stored: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(stored, version as i64, "fixture reached schema {version}");
}

#[test]
fn every_earlier_schema_upgrades_and_keeps_its_data() {
    for version in 1..=MIGRATIONS.len() {
        let state = private_state();
        index_at(state.path(), version);
        drop(Index::open(state.path()).unwrap_or_else(|e| panic!("schema {version}: {e:#}")));
        // A second open is a no-op.
        drop(Index::open(state.path()).unwrap());

        let db = Connection::open(state.path().join("index.sqlite")).unwrap();
        let after: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(
            after, CURRENT,
            "schema {version} reaches the current version"
        );
        let allocated: i64 = db
            .query_row("SELECT allocated FROM entries WHERE scan_id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(allocated, 4096, "schema {version} keeps its entries");
        let root: Vec<u8> = db
            .query_row("SELECT value FROM settings WHERE key='root'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(root, b"/", "schema {version} keeps its settings");
        let active: i64 = db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(active, 1, "schema {version} keeps the published scan");
    }
}

#[test]
fn a_newer_schema_is_refused_and_left_alone() {
    let state = private_state();
    index_at(state.path(), MIGRATIONS.len());
    {
        let db = Connection::open(state.path().join("index.sqlite")).unwrap();
        db.pragma_update(None, "user_version", CURRENT + 1).unwrap();
    }
    let error = Index::open(state.path())
        .err()
        .expect("newer schema refused");
    assert!(error.to_string().contains("newer"), "{error:#}");
    let db = Connection::open(state.path().join("index.sqlite")).unwrap();
    let after: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(after, CURRENT + 1, "the refused index is not downgraded");
}

#[test]
fn a_damaged_index_is_refused_and_not_repaired() {
    let state = private_state();
    let path = state.path().join("index.sqlite");
    let garbage = vec![0xA5u8; 8192];
    fs::write(&path, &garbage).unwrap();
    assert!(Index::open(state.path()).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        garbage,
        "a damaged index is preserved for recovery, never overwritten"
    );
}

#[test]
fn this_test_lists_every_migration_file() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let files = fs::read_dir(directory)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".sql")
        })
        .count();
    assert_eq!(
        files,
        MIGRATIONS.len(),
        "a new migration needs a MIGRATIONS entry and a new CURRENT here, plus the three edits in index.rs"
    );
}
