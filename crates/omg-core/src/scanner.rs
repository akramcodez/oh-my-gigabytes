use crate::{Index, ScanOptions, ScanReport, index::lock};
use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};

pub fn scan(
    index: &mut Index,
    options: &ScanOptions,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
) -> Result<ScanReport> {
    if !(1..=16).contains(&options.workers) {
        bail!("workers must be between 1 and 16");
    }
    let _owner = lock(&index.state, "scan.lock")?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        walk::run(index, options, cancel, progress)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (options, cancel, progress);
        bail!("native scanner not certified on this platform")
    }
}

/// Set on the shared progress counter once traversal ends and the scan is finishing
/// (comparison, folder totals, publication). Readers mask it off to get the entry count.
pub const FINISHING: u64 = 1 << 63;

/// Fixed cost after folder totals: bounded pruning of the previous scan, checkpoint, trend sample.
const FINISH_FIXED: f64 = 4.0;

/// Entries per second the finishing phase processes (measured: ~1.5 M entries in about 6 s now
/// that folder totals are accumulated during the walk).
const FINISH_RATE: f64 = 250_000.0;

/// Estimated seconds left. `walking_secs` is time spent walking so far; `finishing_secs` is time
/// since finishing began, if it has. The inode total is an upper bound (it includes skipped
/// locations), so about 94% of it is expected to be visited.
pub fn eta(done: u64, total: u64, walking_secs: f64, finishing_secs: Option<f64>) -> f64 {
    let done = done as f64;
    if let Some(since) = finishing_secs {
        return (done / FINISH_RATE + FINISH_FIXED - since).max(1.0);
    }
    let rate = (done / walking_secs.max(0.001)).max(1.0);
    let expected = done.max(total as f64 * 0.94);
    (expected - done) / rate + expected / FINISH_RATE + FINISH_FIXED
}

/// Sensible default parallelism: filesystem metadata is I/O-latency bound, so use most cores
/// but stay bounded.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 8))
}

/// Approximate number of entries to expect: the filesystem's used-inode count. This is an upper
/// bound when the scan root is smaller than its volume, and it includes skipped locations.
pub fn estimate_entries(root: &std::path::Path) -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let dir =
            crate::platform::native::open_root(&crate::platform::absolute(root).ok()?).ok()?;
        crate::platform::native::inode_usage(&dir).ok().flatten()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = root;
        None
    }
}

/// The walk itself is platform-neutral. Only the native operations (open a verified directory,
/// enumerate it, read a volume) come from `platform::native`.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod walk {
    use super::*;
    use crate::platform::{self, native};
    use crate::{Entry, now};
    use crossbeam_channel::{Receiver, Sender, bounded};
    use rusqlite::params;
    use std::ffi::OsString;
    use std::sync::{Condvar, Mutex, atomic::Ordering};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    const BATCH_ENTRIES: usize = 2000;
    const COMMIT_ENTRIES: usize = 5000;
    const EVENT_BOUND: usize = 64;

    struct Job {
        relative: PathBuf,
        identity: String,
    }
    struct Batch {
        entries: Vec<Entry>,
        /// `policy::hard_protection` verdict per entry, computed by the walking workers in parallel.
        protected: Vec<bool>,
        issues: Vec<(Vec<u8>, String)>,
    }
    struct QueueState {
        jobs: Vec<Job>,
        in_flight: usize,
    }
    /// Shared in-memory work queue. LIFO keeps memory proportional to tree depth × fan-out, and
    /// counters replace the per-iteration table scans of the earlier SQLite-backed queue.
    struct Queue {
        state: Mutex<QueueState>,
        wake: Condvar,
        peak: AtomicU64,
    }
    impl Queue {
        fn push(&self, job: Job) {
            let mut state = self.state.lock().unwrap();
            state.jobs.push(job);
            self.peak
                .fetch_max(state.jobs.len() as u64, Ordering::Relaxed);
            self.wake.notify_one();
        }
        fn pop(&self, cancel: &AtomicBool) -> Option<Job> {
            let mut state = self.state.lock().unwrap();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    self.wake.notify_all();
                    return None;
                }
                if let Some(job) = state.jobs.pop() {
                    state.in_flight += 1;
                    return Some(job);
                }
                if state.in_flight == 0 {
                    self.wake.notify_all();
                    return None;
                }
                state = self
                    .wake
                    .wait_timeout(state, Duration::from_millis(50))
                    .unwrap()
                    .0;
            }
        }
        fn finish(&self) {
            let mut state = self.state.lock().unwrap();
            state.in_flight -= 1;
            if state.in_flight == 0 && state.jobs.is_empty() {
                self.wake.notify_all();
            } else {
                self.wake.notify_one();
            }
        }
    }

    fn send(tx: &Sender<Batch>, mut batch: Batch, cancel: &AtomicBool) -> bool {
        loop {
            match tx.send_timeout(batch, Duration::from_millis(50)) {
                Ok(()) => return true,
                Err(crossbeam_channel::SendTimeoutError::Timeout(value)) => {
                    if cancel.load(Ordering::Relaxed) {
                        return false;
                    }
                    batch = value;
                }
                Err(_) => return false,
            }
        }
    }

    struct Shared {
        root: native::Dir,
        root_path: PathBuf,
        /// Volume of the scan root. An object on another volume is a mount boundary.
        root_dev: u64,
        custom: Vec<PathBuf>,
        queue: Queue,
        cancel: Arc<AtomicBool>,
    }

    fn worker(shared: Arc<Shared>, events: Sender<Batch>) {
        while let Some(job) = shared.queue.pop(&shared.cancel) {
            let mut batch = Batch {
                entries: Vec::new(),
                protected: Vec::new(),
                issues: Vec::new(),
            };
            let result = read_directory(&shared, &job, &events, &mut batch);
            if result.is_err() {
                batch.issues.push((
                    platform::raw(&job.relative).unwrap_or_default(),
                    "directory incomplete, changed or inaccessible".into(),
                ));
            }
            let alive = send(&events, batch, &shared.cancel);
            shared.queue.finish();
            if !alive {
                break;
            }
        }
    }

    fn read_directory(
        shared: &Shared,
        job: &Job,
        events: &Sender<Batch>,
        batch: &mut Batch,
    ) -> Result<()> {
        let fd = native::open_dir(&shared.root, &job.relative, &job.identity)?;
        let before = native::dir_meta(&fd)?;
        let complete = native::for_each_child(&fd, |name: OsString, metadata| {
            if shared.cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let relative = job.relative.join(&name);
            if platform::raw(&relative)?.len() > 4096 {
                bail!("path budget exceeded");
            }
            let metadata = match metadata {
                Ok(metadata) => metadata,
                Err(_) => {
                    batch
                        .issues
                        .push((platform::raw(&relative)?, "metadata unavailable".into()));
                    return Ok(true);
                }
            };
            let mut entry = platform::entry_from_meta(&relative, &metadata);
            entry.excluded = platform::excluded(&shared.root_path.join(&relative), &shared.custom);
            if entry.excluded.is_none() {
                if let Some(reason) = metadata.restricted {
                    // Cloud and offline placeholders are never read, so they are recorded only.
                    entry.excluded = Some(reason.into());
                } else if entry.kind == "symlink" || entry.kind == "special" {
                    entry.excluded = Some("symlink/special entry not followed".into());
                } else if entry.kind == "directory" && metadata.volume != shared.root_dev {
                    entry.excluded = Some("mount boundary not crossed".into());
                }
            }
            if entry.kind == "directory" && entry.excluded.is_none() {
                shared.queue.push(Job {
                    relative: relative.clone(),
                    identity: entry.identity.clone(),
                });
            }
            batch
                .protected
                .push(crate::policy::hard_protection(&shared.root_path, &entry).is_some());
            batch.entries.push(entry);
            if batch.entries.len() >= BATCH_ENTRIES {
                let full = Batch {
                    entries: std::mem::take(&mut batch.entries),
                    protected: std::mem::take(&mut batch.protected),
                    issues: std::mem::take(&mut batch.issues),
                };
                if !send(events, full, &shared.cancel) {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
        if !complete {
            return Ok(());
        }
        let after = native::dir_meta(&fd)?;
        if before.modified_ns != after.modified_ns || before.changed_ns != after.changed_ns {
            bail!("directory changed during enumeration; reconciliation needed");
        }
        Ok(())
    }

    /// `OMG_TIMINGS=1` prints cumulative phase times to stderr (diagnostics only).
    fn timing(phase: &str, started: Instant) {
        if std::env::var_os("OMG_TIMINGS").is_some() {
            eprintln!(
                "omg-timing {phase}: {:.1}s",
                started.elapsed().as_secs_f64()
            );
        }
    }

    pub fn run(
        index: &mut Index,
        options: &ScanOptions,
        cancel: Arc<AtomicBool>,
        progress: Arc<AtomicU64>,
    ) -> Result<ScanReport> {
        let started = Instant::now();
        let root_path = platform::absolute(&options.root)?;
        if platform::excluded(&root_path, &options.exclusions).is_some() {
            bail!("scan root excluded by mandatory or configured policy");
        }
        let root = native::open_root(&root_path)?;
        let root_meta = native::dir_meta(&root)?;
        let estimated_total = native::inode_usage(&root)?.unwrap_or(0);
        let mut exclusions = options
            .exclusions
            .iter()
            .map(|p| platform::absolute(p))
            .collect::<Result<Vec<_>>>()?;
        exclusions.push(index.state.clone());
        index.recover_interrupted()?;
        index.db.execute("INSERT INTO settings VALUES('root',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[platform::raw(&root_path)?])?;
        let configured = options
            .exclusions
            .iter()
            .map(|p| platform::absolute(p).and_then(|p| platform::raw(&p)))
            .collect::<Result<Vec<_>>>()?;
        index.db.execute("INSERT INTO settings VALUES('exclusions',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_vec(&configured)?])?;
        let (capacity, available) = native::volume(&root)?;
        index.db.execute("INSERT INTO scans(root,started,state,mode,capacity,available,sampled) VALUES(?,?,'running',?,?,?,?)",params![platform::raw(&root_path)?,now(),if options.full {"full"} else {"reconcile"},i64::try_from(capacity)?,i64::try_from(available)?,now()])?;
        let id = index.db.last_insert_rowid();
        index.db.execute(
            "UPDATE scans SET phase='walking',phase_at=?,progress=0,estimated_total=? WHERE id=?",
            params![now(), i64::try_from(estimated_total)?, id],
        )?;
        let previous: Option<i64> = {
            use rusqlite::OptionalExtension;
            index
                .db
                .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                    r.get(0)
                })
                .optional()?
        };
        // Staging scan only: a crash here yields an unpublished `interrupted` scan, so relaxed
        // durability is safe. Full durability is restored before the publishing transaction.
        index
            .db
            .execute_batch("PRAGMA synchronous=NORMAL; PRAGMA cache_size=-65536;")?;

        let shared = Arc::new(Shared {
            root,
            root_path: root_path.clone(),
            root_dev: root_meta.volume,
            custom: exclusions,
            queue: Queue {
                state: Mutex::new(QueueState {
                    jobs: vec![Job {
                        relative: PathBuf::new(),
                        identity: root_meta.identity.clone(),
                    }],
                    in_flight: 0,
                }),
                wake: Condvar::new(),
                peak: AtomicU64::new(1),
            },
            cancel: cancel.clone(),
        });
        let (tx, rx): (Sender<Batch>, Receiver<Batch>) = bounded(EVENT_BOUND);
        let handles: Vec<_> = (0..options.workers)
            .map(|_| {
                let (shared, tx) = (shared.clone(), tx.clone());
                std::thread::spawn(move || worker(shared, tx))
            })
            .collect();
        drop(tx);
        let mut report = ScanReport {
            id,
            state: "running".into(),
            workers: options.workers,
            event_queue_bound: EVENT_BOUND,
            estimated_total,
            ..Default::default()
        };
        let mut aggregate = crate::rollup::Aggregator::new();
        let result = (|| -> Result<()> {
            loop {
                let first = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(batch) => batch,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                };
                let tx = index
                    .db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let mut pending = vec![first];
                let mut rows = 0;
                while let Some(batch) = pending.pop() {
                    rows += batch.entries.len();
                    {
                        let mut insert =
                            tx.prepare_cached("INSERT INTO entries VALUES(?,?,?,?,?,?,?,?,?,?,?)")?;
                        for (entry, protected) in batch.entries.into_iter().zip(batch.protected) {
                            aggregate.add(&entry, protected);
                            if entry.excluded.is_some() {
                                report.excluded += 1;
                            }
                            insert.execute(params![
                                id,
                                entry.path,
                                entry.parent,
                                entry.kind,
                                entry.identity,
                                i64::try_from(entry.apparent)?,
                                i64::try_from(entry.allocated)?,
                                i64::try_from(entry.links)?,
                                entry.modified_ns,
                                entry.changed_ns,
                                entry.excluded
                            ])?;
                            report.entries += 1;
                        }
                    }
                    for (path, reason) in batch.issues {
                        report.errors += 1;
                        if report.errors <= 1000 {
                            tx.execute(
                                "INSERT INTO issues VALUES(?,?,?)",
                                params![id, path, reason],
                            )?;
                        }
                    }
                    progress.store(report.entries, Ordering::Relaxed);
                    if rows >= COMMIT_ENTRIES {
                        break;
                    }
                    if let Ok(next) = rx.try_recv() {
                        pending.push(next);
                    }
                }
                // Published for other processes (another OMG window shows this scan's progress).
                tx.execute(
                    "UPDATE scans SET progress=?,estimated_total=? WHERE id=?",
                    params![
                        i64::try_from(report.entries)?,
                        i64::try_from(estimated_total)?,
                        id
                    ],
                )?;
                tx.commit()?;
            }
            Ok(())
        })();
        if result.is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
        drop(rx);
        for handle in handles {
            if handle.join().is_err() {
                report.errors += 1;
            }
        }
        report.peak_pending_directories = shared.queue.peak.load(Ordering::Relaxed);
        progress.store(report.entries | super::FINISHING, Ordering::Relaxed);
        let _ = index.db.execute(
            "UPDATE scans SET phase='finishing',phase_at=?,progress=? WHERE id=?",
            params![now(), i64::try_from(report.entries)?, id],
        );
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        timing("walk", started);
        report.state = if result.is_err() {
            "failed"
        } else if cancel.load(Ordering::Relaxed) {
            "cancelled"
        } else if report.errors > 0 {
            "partial"
        } else {
            "complete"
        }
        .into();
        // Changed entries: one set difference against the published scan. Over very large indexes
        // the comparison costs more than it informs, so it is skipped and reported as "not compared".
        if matches!(report.state.as_str(), "complete" | "partial") {
            const COMPARE_LIMIT: i64 = 300_000;
            report.changed_entries = match previous {
                None => Some(report.entries),
                Some(old) => {
                    let before: Option<i64> = index
                        .db
                        .query_row("SELECT entry_count FROM scans WHERE id=?", [old], |r| {
                            r.get(0)
                        })
                        .unwrap_or(None);
                    if before.is_some_and(|n| n <= COMPARE_LIMIT)
                        && report.entries as i64 <= COMPARE_LIMIT * 2
                    {
                        Some(index.db.query_row(
                            "SELECT count(*) FROM entries e LEFT JOIN entries p ON p.scan_id=?2 AND p.path=e.path WHERE e.scan_id=?1 AND (p.path IS NULL OR p.identity<>e.identity OR p.apparent<>e.apparent OR p.allocated<>e.allocated OR p.modified_ns<>e.modified_ns OR p.changed_ns<>e.changed_ns OR p.excluded IS NOT e.excluded)",
                            params![id, old],
                            |r| crate::index::unsigned(r, 0),
                        )?)
                    } else {
                        None
                    }
                }
            };
        }
        timing("changed", started);
        let publishable =
            |state: &str| state == "complete" || (state == "partial" && options.publish_partial);
        let mut rollup_error = None;
        let mut totals = None;
        if publishable(&report.state) {
            // Folder totals were accumulated during the walk; only the fold and insert remain.
            match aggregate.finish(index, id) {
                Ok(unique) => totals = Some(unique),
                Err(error) => {
                    report.state = "failed".into();
                    rollup_error = Some(error);
                }
            }
        }
        timing("rollup", started);
        let tx = index
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE scans SET ended=?,state=?,report=? WHERE id=?",
            params![now(), report.state, serde_json::to_string(&report)?, id],
        )?;
        if let Some((apparent, allocated)) = totals {
            tx.execute(
                "UPDATE scans SET entry_count=?,unique_apparent=?,unique_allocated=? WHERE id=?",
                params![
                    i64::try_from(report.entries)?,
                    i64::try_from(apparent)?,
                    i64::try_from(allocated)?,
                    id
                ],
            )?;
        }
        if publishable(&report.state) {
            tx.execute("INSERT INTO active VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET scan_id=excluded.scan_id",[id])?;
            // A complete scan re-reads every folder, so earlier partial-deletion totals are current again.
            if report.state == "complete" {
                tx.execute("DELETE FROM settings WHERE key='reconcile_needed'", [])?;
            }
        }
        tx.execute("DELETE FROM jobs", [])?;
        tx.commit()?;
        // Old scans are invisible once `active` moved, so removing their rows is housekeeping done
        // in bounded chunks instead of one multi-second transaction on the publish path.
        timing("publish", started);
        let _ = index.prune_stale_locked(Duration::from_secs(4));
        index.db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        // Trend history is advisory; failing to record it must never fail or alter the scan result.
        timing("prune+checkpoint", started);
        let _ = index.record_sample(
            id,
            &report.state,
            crate::service::trend::DEFAULT_SAMPLE_DAYS,
            crate::service::trend::DEFAULT_MAX_SAMPLES,
        );
        result?;
        if let Some(error) = rollup_error {
            return Err(error.context("directory rollups failed; last valid index retained"));
        }
        Ok(report)
    }
}
