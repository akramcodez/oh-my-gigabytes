#![cfg(unix)]
//! Housekeeping (pruning the old generation of a scan) must not keep a scan or a delete waiting for
//! the whole cleanup: clearing a full scan of a large disk took about 50 seconds under `scan.lock`.
use omg_core::{
    Index,
    index::{Busy, LOCK_WAIT, lock, lock_held, lock_wait},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    thread,
    time::{Duration, Instant},
};

fn private_state() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[test]
fn lock_wait_gets_a_lock_that_is_let_go_and_reports_busy_for_one_that_is_not() {
    let state = private_state();
    let holder = lock(state.path(), "scan.lock").unwrap();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        drop(holder);
    });
    let started = Instant::now();
    let taken = lock_wait(state.path(), "scan.lock", Duration::from_secs(5)).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(200), "it waited");
    release.join().unwrap();

    // Held for good: the wait ends in the same typed error as an immediate attempt.
    let started = Instant::now();
    let error = lock_wait(state.path(), "scan.lock", Duration::from_millis(150)).unwrap_err();
    assert!(error.downcast_ref::<Busy>().is_some(), "{error}");
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert!(lock_held(state.path(), "scan.lock"));
    drop(taken);
    assert!(!lock_held(state.path(), "scan.lock"));
}

/// An old scan generation of `rows` entries, as left behind by a finished full scan.
fn stale_generation(state: &std::path::Path, rows: u32) -> rusqlite::Connection {
    drop(Index::open(state).unwrap());
    let db = rusqlite::Connection::open(state.join("index.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(20)).unwrap();
    db.execute_batch(&format!(
        "INSERT INTO scans(id,root,started,ended,state,mode) VALUES (1,x'2f',0,0,'complete','full'),(2,x'2f',1,1,'complete','full');
         INSERT INTO active(singleton,scan_id) VALUES (1,2);
         INSERT INTO entries(scan_id,path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded)
           WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<{rows})
           SELECT 1,CAST('p'||i AS BLOB),x'',  'file','1:'||i,1,1,1,0,0,NULL FROM n;"
    ))
    .unwrap();
    db
}

fn stale_rows(db: &rusqlite::Connection) -> i64 {
    db.query_row("SELECT count(*) FROM entries WHERE scan_id=1", [], |r| {
        r.get(0)
    })
    .unwrap()
}

#[test]
fn a_waiting_scan_or_delete_gets_in_long_before_housekeeping_is_finished() {
    let state = private_state();
    let db = stale_generation(state.path(), 400_000);
    assert_eq!(stale_rows(&db), 400_000);

    let path = state.path().to_path_buf();
    let housekeeping = thread::spawn(move || {
        let index = Index::open(&path).unwrap();
        index.prune_stale(Duration::from_secs(120)).unwrap()
    });
    // Wait until housekeeping has really started: the first rows it removes are the first inserted.
    let first: &str = "SELECT count(*) FROM entries WHERE scan_id=1 AND path=CAST('p1' AS BLOB)";
    let begun = Instant::now();
    while db.query_row(first, [], |r| r.get::<_, i64>(0)).unwrap() == 1 {
        assert!(
            begun.elapsed() < Duration::from_secs(20),
            "housekeeping never started"
        );
        thread::sleep(Duration::from_millis(5));
    }

    // What a delete or a scan does: wait for the lock. It must arrive within the bounded wait,
    // while most of the old generation is still there.
    let started = Instant::now();
    let owner = lock_wait(state.path(), "scan.lock", LOCK_WAIT)
        .expect("housekeeping let go of the lock within the wait");
    assert!(
        started.elapsed() < LOCK_WAIT,
        "waited {:?}",
        started.elapsed()
    );
    let remaining = stale_rows(&db);
    assert!(
        remaining > 0,
        "housekeeping was not finished when the lock was handed over"
    );

    // Holding the lock stops housekeeping at its next slice, without an error.
    assert!(
        !housekeeping.join().unwrap(),
        "it gives up while another process owns the lock"
    );
    let left = stale_rows(&db);
    assert!(left > 0, "the work stayed undone while the lock was held");
    drop(owner);

    // The next run resumes where it stopped and finishes.
    let index = Index::open(state.path()).unwrap();
    assert!(index.prune_stale(Duration::from_secs(120)).unwrap());
    assert_eq!(stale_rows(&db), 0);
    let scans: i64 = db
        .query_row("SELECT count(*) FROM scans", [], |r| r.get(0))
        .unwrap();
    assert_eq!(scans, 1, "only the published scan is left");
}
