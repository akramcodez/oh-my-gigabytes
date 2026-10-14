use crate::{
    model::{Entry, ScanReport, Status},
    platform,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

/// Progress of a scan, as recorded by whichever process is running it.
#[derive(Debug, Clone)]
pub struct RunningScan {
    pub started: i64,
    pub progress: u64,
    pub estimated_total: u64,
    pub finishing: bool,
    pub phase_at: Option<i64>,
}

pub struct Index {
    pub(crate) db: Connection,
    pub state: PathBuf,
    /// Delete plans issued by this session and not yet used. Memory only: a reopened index cannot
    /// delete a plan that a previous session showed.
    pub(crate) issued: crate::cleanup::IssuedPlans,
}
pub fn sqlite_version() -> String {
    rusqlite::version().to_string()
}

/// Diagnosis does not create state, migrate schemas or repair an index.
pub fn diagnose(state: &Path) -> Result<serde_json::Value> {
    let absolute = platform::absolute(state)?;
    let mut current = PathBuf::new();
    for part in absolute.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!("diagnosis: unsafe state ancestor")
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(
                    serde_json::json!({"platform":std::env::consts::OS,"sqlite":sqlite_version(),"state":"not initialized","cleanup":crate::capability::platform_permanent_delete().as_str(),"os_trash":crate::capability::os_trash().as_str(),"service":"see omg service status"}),
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(&absolute)?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            bail!("diagnosis: state directory is not private/current-user owned");
        }
        platform::linux::root(&absolute)?;
    }
    let database = absolute.join("index.sqlite");
    if !database.exists() {
        return Ok(
            serde_json::json!({"state":"no index","sqlite":sqlite_version(),"cleanup":crate::capability::platform_permanent_delete().as_str(),"os_trash":crate::capability::os_trash().as_str(),"service":"see omg service status"}),
        );
    }
    for name in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
        if fs::symlink_metadata(absolute.join(name))
            .is_ok_and(|m| m.file_type().is_symlink() || !m.is_file())
        {
            bail!("diagnosis: unsafe database/sidecar");
        }
    }
    // Read-only queries may participate in existing WAL shared memory; no migrations or repairs.
    let db = Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(Duration::from_secs(2))?;
    let schema: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    Ok(
        serde_json::json!({"state":"existing index","schema":schema,"schema_supported":schema<=11,"integrity":integrity,"sqlite":sqlite_version(),"cleanup":crate::capability::platform_permanent_delete().as_str(),"os_trash":crate::capability::os_trash().as_str(),"service":"see omg service status","native_analysis":"development Linux only"}),
    )
}

pub fn lock(state: &Path, name: &str) -> Result<File> {
    let path = state.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        bail!("unsafe lock file");
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "linux")]
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options.open(path)?;
    file.try_lock().map_err(|_| anyhow::Error::new(Busy))?;
    Ok(file)
}

/// Another process holds the kernel advisory lock for this operation.
#[derive(Debug)]
pub struct Busy;
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("another omg process owns this operation; retry after it stops")
    }
}
impl std::error::Error for Busy {}

/// Whether some process currently holds a state-directory lock. Never creates the lock file.
pub fn lock_held(state: &Path, name: &str) -> bool {
    let path = state.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return false;
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    match options.open(path) {
        Ok(file) => file.try_lock_shared().is_err(),
        Err(_) => false,
    }
}

impl Index {
    pub fn open(state: &Path) -> Result<Self> {
        if !platform::supported() {
            bail!(
                "native scanning/state support currently requires Linux; other platforms unverified"
            );
        }
        let state = platform::absolute(state)?;
        let mut current = PathBuf::new();
        for component in state.components() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                    bail!("state ancestor must be a real directory")
                }
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt;
                        fs::DirBuilder::new().mode(0o700).create(&current)?;
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::symlink_metadata(&state)?;
            if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0
            {
                bail!(
                    "state directory must be owned by current user with private (0700) permissions"
                );
            }
            platform::linux::root(&state)?;
        }
        // The maintenance lock is held only while another process opens/migrates; wait briefly
        // instead of failing a concurrent CLI or service scan.
        let mut waited = 0;
        let _maintenance = loop {
            match lock(&state, "database.lock") {
                Err(error) if error.downcast_ref::<Busy>().is_some() && waited < 250 => {
                    waited += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => break other?,
            }
        };
        for name in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
            if fs::symlink_metadata(state.join(name))
                .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
            {
                bail!("unsafe database sidecar");
            }
        }
        let db = Connection::open_with_flags(
            state.join("index.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        // Several local writers (scan, interface worker, housekeeping) share the database; wait
        // for a short write rather than failing a scan after two seconds.
        db.busy_timeout(Duration::from_secs(20))?;
        if rusqlite::version_number() < 3_051_003 {
            bail!("SQLite 3.51.3+ required for WAL safety baseline");
        }
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 11 {
            bail!("index schema is newer than this omg build");
        }
        // quick_check reads the whole file (seconds on a multi-GB index), so very large indexes
        // are verified by `omg doctor` instead of on every command; SQLite still reports
        // corruption it hits while reading.
        let large = fs::metadata(state.join("index.sqlite")).is_ok_and(|m| m.len() > 256 << 20);
        let check: String = if large {
            db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
                .context("index unreadable; preserve DB/WAL and see docs/operations.md recovery")?;
            "ok".into()
        } else {
            db.query_row("PRAGMA quick_check", [], |r| r.get(0))
                .context("index unreadable; preserve DB/WAL and see docs/operations.md recovery")?
        };
        if check != "ok" {
            bail!("index corruption detected; preserve state and rebuild inventory explicitly");
        }
        let mode: String = db.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            bail!("WAL unavailable on state filesystem");
        }
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA cache_size=-4096;",
        )?;
        if version < 1 {
            db.execute_batch(include_str!("../../../migrations/001.sql"))?;
        }
        if version < 2 {
            db.execute_batch(include_str!("../../../migrations/002.sql"))?;
        }
        if version < 3 {
            db.execute_batch(include_str!("../../../migrations/003.sql"))?;
        }
        if version < 4 {
            db.execute_batch(include_str!("../../../migrations/004.sql"))?;
        }
        if version < 5 {
            db.execute_batch(include_str!("../../../migrations/005.sql"))?;
        }
        if version < 6 {
            db.execute_batch(include_str!("../../../migrations/006.sql"))?;
        }
        if version < 7 {
            db.execute_batch(include_str!("../../../migrations/007.sql"))?;
        }
        if version < 8 {
            db.execute_batch(include_str!("../../../migrations/008.sql"))?;
        }
        if version < 9 {
            db.execute_batch(include_str!("../../../migrations/009.sql"))?;
        }
        if version < 10 {
            db.execute_batch(include_str!("../../../migrations/010.sql"))?;
        }
        if version < 11 {
            db.execute_batch(include_str!("../../../migrations/011.sql"))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
                if state.join(name).exists() {
                    fs::set_permissions(state.join(name), fs::Permissions::from_mode(0o600))?;
                }
            }
        }
        Ok(Self {
            db,
            state,
            issued: Default::default(),
        })
    }
    pub fn status(&self) -> Result<Status> {
        let active: Option<i64> = self
            .db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let (entries, apparent, allocated) = if let Some(id) = active {
            // Totals are stored when a scan is published; recompute only for older indexes.
            let stored: (Option<i64>, Option<i64>, Option<i64>) = self.db.query_row(
                "SELECT entry_count,unique_apparent,unique_allocated FROM scans WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            if let (Some(n), Some(a), Some(b)) = stored {
                (n.max(0) as u64, a.max(0) as u64, b.max(0) as u64)
            } else {
                let entries = self.db.query_row(
                    "SELECT count(*) FROM entries WHERE scan_id=?",
                    [id],
                    |r| unsigned(r, 0),
                )?;
                let (apparent, allocated) = self.db.query_row("SELECT coalesce(sum(apparent),0),coalesce(sum(allocated),0) FROM (SELECT max(apparent) apparent,max(allocated) allocated FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)", [id], |r| Ok((unsigned(r,0)?, unsigned(r,1)?)))?;
                (entries, apparent, allocated)
            }
        } else {
            (0, 0, 0)
        };
        let root = active
            .map(|id| {
                self.db
                    .query_row("SELECT root FROM scans WHERE id=?", [id], |r| r.get(0))
            })
            .transpose()?;
        let last_successful = active
            .map(|id| {
                self.db
                    .query_row("SELECT ended FROM scans WHERE id=?", [id], |r| r.get(0))
            })
            .transpose()?;
        let last_record: Option<(i64, String, Option<String>)> = self
            .db
            .query_row(
                "SELECT id,state,report FROM scans ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let last_report = last_record
            .map(|(id, state, json)| -> Result<ScanReport> {
                let mut report = match json {
                    Some(json) => serde_json::from_str(&json)?,
                    None => ScanReport {
                        id,
                        ..Default::default()
                    },
                };
                report.state = state;
                Ok(report)
            })
            .transpose()?;
        let volume: Option<(u64,u64,i64)> = self.db.query_row("SELECT capacity,available,sampled FROM scans WHERE capacity IS NOT NULL ORDER BY id DESC LIMIT 1", [], |r| Ok((unsigned(r,0)?,unsigned(r,1)?,r.get(2)?))).optional()?;
        Ok(Status { root, last_successful, last_report, entries,
            unique_apparent: apparent, unique_allocated: allocated, capacity: volume.map(|v|v.0), available: volume.map(|v|v.1), volume_sample_at: volume.map(|v|v.2),
            scope_note: "Scoped metadata inventory, not whole-volume usage. Exclusions omitted; shared clones/snapshots unknown, so indexed bytes are not reclaimable-space estimates. Background service state is reported separately.".into() })
    }
    pub fn entries(&self, limit: usize, offset: usize) -> Result<Vec<Entry>> {
        let mut statement = self.db.prepare("SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=(SELECT scan_id FROM active WHERE singleton=1) ORDER BY allocated DESC,path LIMIT ? OFFSET ?")?;
        Ok(statement
            .query_map(
                params![limit.min(1000) as i64, i64::try_from(offset)?],
                row_entry,
            )?
            .collect::<rusqlite::Result<_>>()?)
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn recover_interrupted(&self) -> Result<()> {
        use crate::now;
        self.db.execute(
            "UPDATE scans SET state='interrupted',ended=? WHERE state='running'",
            [now()],
        )?;
        self.db.execute("DELETE FROM jobs", [])?;
        Ok(())
    }
    /// The scan currently running in any process, if one holds the scan lock.
    pub fn running_scan(&self) -> Result<Option<RunningScan>> {
        if !lock_held(&self.state, "scan.lock") {
            return Ok(None);
        }
        Ok(self
            .db
            .query_row(
                "SELECT started,coalesce(progress,0),coalesce(estimated_total,0),coalesce(phase,'walking'),phase_at FROM scans WHERE state='running' ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok(RunningScan {
                        started: r.get(0)?,
                        progress: unsigned(r, 1)?,
                        estimated_total: unsigned(r, 2)?,
                        finishing: r.get::<_, String>(3)? == "finishing",
                        phase_at: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }
    /// Delete rows of scans that are neither published nor the latest record, in chunks, stopping
    /// after `budget`. Returns true when nothing stale remains. Safe to call repeatedly.
    pub fn prune_stale(&self, budget: Duration) -> Result<bool> {
        // Never compete with a running scan for the write lock; it prunes when it finishes.
        let Ok(_owner) = lock(&self.state, "scan.lock") else {
            return Ok(false);
        };
        self.prune_stale_locked(budget)
    }
    /// `prune_stale` for a caller that already owns the scan lock (the scanner itself).
    pub(crate) fn prune_stale_locked(&self, budget: Duration) -> Result<bool> {
        let started = std::time::Instant::now();
        let stale: Vec<i64> = {
            let mut statement = self.db.prepare(
                "SELECT id FROM scans WHERE state<>'running' AND id<>coalesce((SELECT scan_id FROM active WHERE singleton=1),-1) AND id<>(SELECT max(id) FROM scans) ORDER BY id",
            )?;
            statement
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in stale {
            loop {
                let removed = self.db.execute(
                    "DELETE FROM entries WHERE rowid IN (SELECT rowid FROM entries WHERE scan_id=? LIMIT 4000)",
                    [id],
                )?;
                if removed == 0 {
                    break;
                }
                if started.elapsed() >= budget {
                    return Ok(false);
                }
            }
            self.db.execute("DELETE FROM scans WHERE id=?", [id])?;
        }
        Ok(true)
    }
    pub fn issues(&self) -> Result<Vec<(Vec<u8>, String)>> {
        let mut stmt = self.db.prepare(
            "SELECT path,reason FROM issues WHERE scan_id=(SELECT max(id) FROM scans) LIMIT 100",
        )?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn options(&self) -> Result<Option<crate::ScanOptions>> {
        let root: Option<Vec<u8>> = self
            .db
            .query_row("SELECT value FROM settings WHERE key='root'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(root) = root else { return Ok(None) };
        let exclusions: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='exclusions'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let paths: Vec<Vec<u8>> = exclusions
            .map(|v| serde_json::from_slice(&v))
            .transpose()?
            .unwrap_or_default();
        Ok(Some(crate::ScanOptions {
            root: platform::path(&root)?,
            workers: 2,
            exclusions: paths
                .iter()
                .map(|p| platform::path(p))
                .collect::<Result<_>>()?,
            full: true,
            publish_partial: false,
        }))
    }
}
pub(crate) fn row_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        path: row.get(0)?,
        parent: row.get(1)?,
        kind: row.get(2)?,
        identity: row.get(3)?,
        apparent: unsigned(row, 4)?,
        allocated: unsigned(row, 5)?,
        links: unsigned(row, 6)?,
        modified_ns: row.get(7)?,
        changed_ns: row.get(8)?,
        excluded: row.get(9)?,
    })
}

pub(crate) fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
