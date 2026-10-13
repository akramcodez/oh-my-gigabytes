use crate::{Index, ScanOptions, ScanReport, index::lock, platform};
use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub fn scan(
    index: &mut Index,
    options: &ScanOptions,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
) -> Result<ScanReport> {
    if !(1..=8).contains(&options.workers) {
        bail!("workers must be between 1 and 8");
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

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::{Entry, now};
    use crossbeam_channel::{Receiver, Sender, bounded};
    use rusqlite::params;
    use rustix::fs::{self, AtFlags, Dir};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    struct Job {
        id: i64,
        relative: PathBuf,
        identity: String,
    }
    enum Event {
        Entry(Entry),
        Issue(Vec<u8>, String),
        Done(i64),
    }

    fn send(tx: &Sender<Event>, mut event: Event, cancel: &AtomicBool) -> bool {
        loop {
            match tx.send_timeout(event, Duration::from_millis(50)) {
                Ok(()) => return true,
                Err(crossbeam_channel::SendTimeoutError::Timeout(value)) => {
                    if cancel.load(Ordering::Relaxed) {
                        return false;
                    }
                    event = value;
                }
                Err(_) => return false,
            }
        }
    }
    fn worker(
        root: Arc<rustix::fd::OwnedFd>,
        root_path: PathBuf,
        custom: Vec<PathBuf>,
        jobs: Receiver<Job>,
        events: Sender<Event>,
        cancel: Arc<AtomicBool>,
    ) {
        for job in jobs {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let result = (|| -> Result<()> {
                let fd = platform::linux::acquire(&root, &job.relative, true)?;
                let before = fs::fstat(&fd)?;
                if platform::linux::entry(&job.relative, &before).identity != job.identity {
                    bail!("directory identity changed");
                }
                let mut directory = Dir::new(fd)?;
                while let Some(value) = directory.read() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
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
                    match fs::statat(
                        directory.fd()?,
                        value.file_name(),
                        AtFlags::SYMLINK_NOFOLLOW,
                    ) {
                        Ok(metadata) => {
                            let mut entry = platform::linux::entry(&relative, &metadata);
                            entry.excluded =
                                platform::excluded(&root_path.join(&relative), &custom);
                            if entry.excluded.is_none()
                                && matches!(entry.kind.as_str(), "file" | "directory")
                            {
                                match platform::linux::acquire(&root, &relative, false)
                                    .and_then(|fd| Ok(fs::fstat(fd)?))
                                {
                                    Ok(fresh) => {
                                        let observed = platform::linux::entry(&relative, &fresh);
                                        if observed.identity != entry.identity {
                                            bail!("entry changed during enumeration");
                                        }
                                        entry = observed;
                                    }
                                    Err(_) => {
                                        entry.excluded = Some(
                                            "boundary, redirection or inaccessible metadata".into(),
                                        );
                                        send(
                                            &events,
                                            Event::Issue(
                                                entry.path.clone(),
                                                "boundary or inaccessible entry".into(),
                                            ),
                                            &cancel,
                                        );
                                    }
                                }
                            }
                            if entry.kind == "symlink" || entry.kind == "special" {
                                entry.excluded = Some("symlink/special entry not followed".into());
                            }
                            if !send(&events, Event::Entry(entry), &cancel) {
                                return Ok(());
                            }
                        }
                        Err(_) => {
                            if !send(
                                &events,
                                Event::Issue(
                                    platform::raw(&relative)?,
                                    "metadata unavailable".into(),
                                ),
                                &cancel,
                            ) {
                                return Ok(());
                            }
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
            })();
            if result.is_err() {
                send(
                    &events,
                    Event::Issue(
                        platform::raw(&job.relative).unwrap_or_default(),
                        "directory incomplete, changed or inaccessible".into(),
                    ),
                    &cancel,
                );
            }
            if !send(&events, Event::Done(job.id), &cancel) {
                break;
            }
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
        let root = Arc::new(platform::linux::root(&root_path)?);
        let root_meta = fs::fstat(&root)?;
        let root_entry = platform::linux::entry(std::path::Path::new(""), &root_meta);
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
        index.db.execute(
            "INSERT INTO jobs(path,identity) VALUES(?,?)",
            params![root_entry.path, root_entry.identity],
        )?;
        let bound = 64;
        let (job_tx, job_rx) = bounded(options.workers);
        let (event_tx, event_rx) = bounded(bound);
        let mut handles = Vec::new();
        for _ in 0..options.workers {
            let (root, path, custom, jobs, events, cancel) = (
                root.clone(),
                root_path.clone(),
                exclusions.clone(),
                job_rx.clone(),
                event_tx.clone(),
                cancel.clone(),
            );
            handles.push(std::thread::spawn(move || {
                worker(root, path, custom, jobs, events, cancel)
            }));
        }
        drop(job_rx);
        drop(event_tx);
        let mut report = ScanReport {
            id,
            state: "running".into(),
            workers: options.workers,
            event_queue_bound: bound,
            ..Default::default()
        };
        let result = (|| -> Result<()> {
            while !cancel.load(Ordering::Relaxed) {
                let mut in_flight = index.db.query_row(
                    "SELECT count(*) FROM jobs WHERE state='running'",
                    [],
                    |r| crate::index::unsigned(r, 0),
                )?;
                while in_flight < (options.workers as u64) {
                    use rusqlite::OptionalExtension;
                    let pending: Option<(i64, Vec<u8>, String)> = index
                        .db
                        .query_row(
                            "SELECT id,path,identity FROM jobs WHERE state='pending' LIMIT 1",
                            [],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                        )
                        .optional()?;
                    let Some((job_id, path, identity)) = pending else {
                        break;
                    };
                    index
                        .db
                        .execute("UPDATE jobs SET state='running' WHERE id=?", [job_id])?;
                    job_tx.send(Job {
                        id: job_id,
                        relative: platform::path(&path)?,
                        identity,
                    })?;
                    in_flight += 1;
                }
                let pending = index.db.query_row(
                    "SELECT count(*) FROM jobs WHERE state='pending'",
                    [],
                    |r| crate::index::unsigned(r, 0),
                )?;
                report.peak_pending_directories = report.peak_pending_directories.max(pending);
                if in_flight == 0 {
                    break;
                }
                let first = match event_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => event,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => bail!("scanner workers stopped unexpectedly"),
                };
                let mut events = vec![first];
                events.extend(event_rx.try_iter().take(127));
                let tx = index.db.transaction()?;
                for event in events {
                    match event {
                        Event::Entry(entry) => {
                            let changed:bool=tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM entries WHERE scan_id=(SELECT scan_id FROM active WHERE singleton=1) AND path=? AND identity=? AND apparent=? AND allocated=? AND modified_ns=? AND changed_ns=? AND excluded IS ?)",params![entry.path,entry.identity,i64::try_from(entry.apparent)?,i64::try_from(entry.allocated)?,entry.modified_ns,entry.changed_ns,entry.excluded],|r|r.get(0))?;
                            report.changed_entries += u64::from(changed);
                            if entry.excluded.is_some() {
                                report.excluded += 1;
                            }
                            if entry.kind == "directory" && entry.excluded.is_none() {
                                tx.execute(
                                    "INSERT OR IGNORE INTO jobs(path,identity) VALUES(?,?)",
                                    params![entry.path, entry.identity],
                                )?;
                            }
                            tx.execute(
                                "INSERT INTO entries VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                                params![
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
                                ],
                            )?;
                            report.entries += 1;
                            progress.store(report.entries, Ordering::Relaxed);
                        }
                        Event::Issue(path, reason) => {
                            report.errors += 1;
                            if report.errors <= 1000 {
                                tx.execute(
                                    "INSERT INTO issues VALUES(?,?,?)",
                                    params![id, path, reason],
                                )?;
                            }
                        }
                        Event::Done(job) => {
                            tx.execute("UPDATE jobs SET state='done' WHERE id=?", [job])?;
                        }
                    }
                }
                tx.commit()?;
            }
            Ok(())
        })();
        if result.is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
        drop(job_tx);
        drop(event_rx);
        for handle in handles {
            if handle.join().is_err() {
                report.errors += 1;
            }
        }
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
        let tx = index.db.transaction()?;
        tx.execute(
            "UPDATE scans SET ended=?,state=?,report=? WHERE id=?",
            params![now(), report.state, serde_json::to_string(&report)?, id],
        )?;
        if report.state == "complete" {
            tx.execute("INSERT INTO active VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET scan_id=excluded.scan_id",[id])?;
        }
        tx.execute("DELETE FROM jobs", [])?;
        tx.execute("DELETE FROM scans WHERE id<>? AND id<>coalesce((SELECT scan_id FROM active WHERE singleton=1),?)",params![id,id])?;
        tx.commit()?;
        index.db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        result?;
        Ok(report)
    }
}
