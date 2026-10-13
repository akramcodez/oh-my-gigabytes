use crate::{
    model::{Entry, ScanReport, Status},
    now, platform,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Index {
    pub(crate) db: Connection,
    pub state: PathBuf,
}
pub fn sqlite_version() -> String {
    rusqlite::version().to_string()
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
    file.try_lock().map_err(|_| {
        anyhow::anyhow!("another omg process owns this operation; retry after it stops")
    })?;
    Ok(file)
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
        let _maintenance = lock(&state, "database.lock")?;
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
        db.busy_timeout(Duration::from_secs(2))?;
        if rusqlite::version_number() < 3_051_003 {
            bail!("SQLite 3.51.3+ required for WAL safety baseline");
        }
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            bail!("index schema is newer than this omg build");
        }
        let check: String = db
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .context("index unreadable; preserve DB/WAL and see docs/operations.md recovery")?;
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
        db.execute_batch(include_str!("../../../migrations/001.sql"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
                if state.join(name).exists() {
                    fs::set_permissions(state.join(name), fs::Permissions::from_mode(0o600))?;
                }
            }
        }
        Ok(Self { db, state })
    }
    pub fn status(&self) -> Result<Status> {
        let active: Option<i64> = self
            .db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let (entries, apparent, allocated) = if let Some(id) = active {
            let entries =
                self.db
                    .query_row("SELECT count(*) FROM entries WHERE scan_id=?", [id], |r| {
                        unsigned(r, 0)
                    })?;
            let (apparent, allocated) = self.db.query_row("SELECT coalesce(sum(apparent),0),coalesce(sum(allocated),0) FROM (SELECT max(apparent) apparent,max(allocated) allocated FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)", [id], |r| Ok((unsigned(r,0)?, unsigned(r,1)?)))?;
            (entries, apparent, allocated)
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
        let last_json: Option<String> = self
            .db
            .query_row(
                "SELECT report FROM scans ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let volume: Option<(u64,u64,i64)> = self.db.query_row("SELECT capacity,available,sampled FROM scans WHERE capacity IS NOT NULL ORDER BY id DESC LIMIT 1", [], |r| Ok((unsigned(r,0)?,unsigned(r,1)?,r.get(2)?))).optional()?;
        Ok(Status { root, last_successful, last_report: last_json.map(|json| serde_json::from_str::<ScanReport>(&json)).transpose()?, entries,
            unique_apparent: apparent, unique_allocated: allocated, capacity: volume.map(|v|v.0), available: volume.map(|v|v.1), volume_sample_at: volume.map(|v|v.2),
            scope_note: "Scoped metadata inventory, not whole-volume usage. Exclusions omitted; shared clones/snapshots unknown. Service stopped (not implemented).".into() })
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
    pub(crate) fn recover_interrupted(&self) -> Result<()> {
        self.db.execute(
            "UPDATE scans SET state='interrupted',ended=? WHERE state='running'",
            [now()],
        )?;
        self.db.execute("DELETE FROM jobs", [])?;
        Ok(())
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
