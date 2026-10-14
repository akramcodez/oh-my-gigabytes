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
    #[cfg(target_os = "linux")]
    {
        linux::run(index, options, cancel, progress)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (options, cancel, progress);
        bail!("native scanner not certified on this platform")
    }
}

/// Sensible default parallelism: filesystem metadata is I/O-latency bound, so use most cores
/// but stay bounded.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 8))
}

/// Approximate number of entries to expect: the filesystem's used-inode count. This is an upper
/// bound when the scan root is smaller than its volume, and it includes skipped locations.
pub fn estimate_entries(root: &std::path::Path) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let fd = crate::platform::linux::root(&crate::platform::absolute(root).ok()?).ok()?;
        let stat = rustix::fs::fstatvfs(&fd).ok()?;
        Some(stat.f_files.saturating_sub(stat.f_ffree))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::platform;
    use crate::{Entry, now};
    use crossbeam_channel::{Receiver, Sender, bounded};
    use rusqlite::params;
    use rustix::fs::{self, AtFlags, Dir};
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
        root: fs_root::Root,
        root_path: PathBuf,
        root_dev: u64,
        custom: Vec<PathBuf>,
        queue: Queue,
        cancel: Arc<AtomicBool>,
    }
    mod fs_root {
        pub type Root = rustix::fd::OwnedFd;
    }

    fn worker(shared: Arc<Shared>, events: Sender<Batch>) {
        while let Some(job) = shared.queue.pop(&shared.cancel) {
            let mut batch = Batch {
                entries: Vec::new(),
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
        let fd = platform::linux::acquire(&shared.root, &job.relative, true)?;
        let before = fs::fstat(&fd)?;
        if platform::linux::entry(&job.relative, &before).identity != job.identity {
            bail!("directory identity changed");
        }
        let mut directory = Dir::new(fd)?;
        while let Some(value) = directory.read() {
            if shared.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let value = value?;
            let name = platform::linux::name(value.file_name());
            if name.as_os_str() == "." || name.as_os_str() == ".." {
                continue;
            }
            let relative = job.relative.join(name);
            if platform::raw(&relative)?.len() > 4096 {
                bail!("path budget exceeded");
            }
            // One no-follow stat relative to the open directory; it describes the object that
            // currently has this name and never follows a link.
            let metadata = match fs::statat(
                directory.fd()?,
                value.file_name(),
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Ok(metadata) => metadata,
                Err(_) => {
                    batch
                        .issues
                        .push((platform::raw(&relative)?, "metadata unavailable".into()));
                    continue;
                }
            };
            let mut entry = platform::linux::entry(&relative, &metadata);
            entry.excluded = platform::excluded(&shared.root_path.join(&relative), &shared.custom);
            if entry.excluded.is_none() {
                if entry.kind == "symlink" || entry.kind == "special" {
                    entry.excluded = Some("symlink/special entry not followed".into());
                } else if entry.kind == "directory" && metadata.st_dev != shared.root_dev {
                    entry.excluded = Some("mount boundary not crossed".into());
                }
            }
            if entry.kind == "directory" && entry.excluded.is_none() {
                shared.queue.push(Job {
                    relative: relative.clone(),
                    identity: entry.identity.clone(),
                });
            }
            batch.entries.push(entry);
            if batch.entries.len() >= BATCH_ENTRIES {
                let full = Batch {
                    entries: std::mem::take(&mut batch.entries),
                    issues: std::mem::take(&mut batch.issues),
                };
                if !send(events, full, &shared.cancel) {
                    return Ok(());
                }
            }
        }
        let after = directory.stat()?;
        if before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            bail!("directory changed during enumeration; reconciliation needed");
        }
        Ok(())
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
        let root = platform::linux::root(&root_path)?;
        let root_meta = fs::fstat(&root)?;
        let root_entry = platform::linux::entry(std::path::Path::new(""), &root_meta);
        let estimated_total = {
            let stat = fs::fstatvfs(&root)?;
            stat.f_files.saturating_sub(stat.f_ffree)
        };
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
        let (capacity, available) = platform::linux::volume(&root)?;
        index.db.execute("INSERT INTO scans(root,started,state,mode,capacity,available,sampled) VALUES(?,?,'running',?,?,?,?)",params![platform::raw(&root_path)?,now(),if options.full {"full"} else {"reconcile"},i64::try_from(capacity)?,i64::try_from(available)?,now()])?;
        let id = index.db.last_insert_rowid();
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
            root_dev: root_meta.st_dev,
            custom: exclusions,
            queue: Queue {
                state: Mutex::new(QueueState {
                    jobs: vec![Job {
                        relative: PathBuf::new(),
                        identity: root_entry.identity.clone(),
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
                        for entry in batch.entries {
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
        report.elapsed_ms = started.elapsed().as_millis() as u64;
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
        // Changed entries: one set difference against the published scan, not a lookup per entry.
        if matches!(report.state.as_str(), "complete" | "partial") {
            report.changed_entries = match previous {
                Some(old) => index.db.query_row(
                    "SELECT count(*) FROM entries e LEFT JOIN entries p ON p.scan_id=?2 AND p.path=e.path WHERE e.scan_id=?1 AND (p.path IS NULL OR p.identity<>e.identity OR p.apparent<>e.apparent OR p.allocated<>e.allocated OR p.modified_ns<>e.modified_ns OR p.changed_ns<>e.changed_ns OR p.excluded IS NOT e.excluded)",
                    params![id, old],
                    |r| crate::index::unsigned(r, 0),
                )?,
                None => report.entries,
            };
        }
        index
            .db
            .execute_batch("PRAGMA synchronous=FULL; PRAGMA cache_size=-4096;")?;
        // Subtree totals are part of the published view: compute them for the staging scan before
        // the swap, so readers never see a complete scan without them.
        let publishable =
            |state: &str| state == "complete" || (state == "partial" && options.publish_partial);
        let mut rollup_error = None;
        if publishable(&report.state)
            && let Err(error) = crate::rollup::compute(index, id, &root_path)
        {
            report.state = "failed".into();
            rollup_error = Some(error);
        }
        let totals = if publishable(&report.state) {
            Some(index.db.query_row("SELECT coalesce(sum(apparent),0),coalesce(sum(allocated),0) FROM (SELECT max(apparent) apparent,max(allocated) allocated FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)", [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?)
        } else {
            None
        };
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
                params![i64::try_from(report.entries)?, apparent, allocated, id],
            )?;
        }
        if publishable(&report.state) {
            tx.execute("INSERT INTO active VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET scan_id=excluded.scan_id",[id])?;
        }
        tx.execute("DELETE FROM jobs", [])?;
        tx.commit()?;
        // Old scans are invisible once `active` moved, so removing their rows is housekeeping done
        // in bounded chunks instead of one multi-second transaction on the publish path.
        let _ = index.prune_stale_locked(Duration::from_secs(4));
        index.db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        // Trend history is advisory; failing to record it must never fail or alter the scan result.
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
