//! Owner-approved user cleanup (decision D15). Two steps, both explicit:
//!
//! * **OMG Trash** is a list of saved paths. Adding an item changes nothing on disk.
//! * **Delete permanently** removes listed items after a confirmed plan, revalidating each target's
//!   identity, and removes directory contents through directory descriptors (never following links
//!   or crossing mounts), so a path swapped mid-operation cannot redirect it outside the target.
//!
//! Scope is the user's home directory only, current-user owned entries only. Hard protections
//! (exclusions, `.git`, credentials, databases, disk images, system paths) always deny. Being inside
//! a Git repository is a *warning* that the interface must surface before confirmation.
use crate::{Entry, Index, index::unsigned, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    Allowed,
    /// Allowed only after the user has seen this warning (e.g. inside a Git repository).
    Warn(String),
    Denied(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marked {
    pub id: i64,
    pub path: Vec<u8>,
    pub identity: String,
    pub kind: String,
    pub allocated: u64,
    pub added: i64,
    pub warning: Option<String>,
    /// Creation time captured when the item was added. Inode numbers are reused immediately on
    /// ext4, so identity alone cannot tell a recreated file from the original.
    pub birth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePlan {
    pub fingerprint: String,
    pub items: Vec<Marked>,
    pub denied: Vec<(Vec<u8>, String)>,
    pub estimated: u64,
    pub created: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteReport {
    pub deleted: Vec<Vec<u8>>,
    pub skipped: Vec<(Vec<u8>, String)>,
    pub removed_entries: u64,
    pub failed_entries: u64,
    pub estimated_bytes: u64,
    /// Observed change in available bytes on the volume (other activity can affect it).
    pub observed_free_change: Option<i64>,
}

/// Whether `entry` (relative to the scan `root`) may be deleted, and with what warning.
pub fn gate(root: &Path, home: &Path, entry: &Entry) -> Gate {
    let Ok(relative) = platform::path(&entry.path) else {
        return Gate::Denied("unsupported native path".into());
    };
    if !policy::safe_relative(&relative) {
        return Gate::Denied("the scan root itself or an unsafe path".into());
    }
    let absolute = root.join(&relative);
    if !absolute.starts_with(home) || absolute == home {
        return Gate::Denied("outside your home folder; OMG never deletes system data".into());
    }
    if !matches!(entry.kind.as_str(), "file" | "directory") {
        return Gate::Denied("links and special files are not deletable here".into());
    }
    if let Some(reason) = policy::hard_protection(root, entry) {
        return Gate::Denied(reason);
    }
    match policy::repository_of(root, entry, &mut policy::RepositoryMemo::default()) {
        Some(repo) => Gate::Warn(format!(
            "inside the Git repository at {}; uncommitted work there would be lost",
            repo.display()
        )),
        None => Gate::Allowed,
    }
}

impl Index {
    fn scope(&self) -> Result<(i64, PathBuf, Vec<u8>)> {
        let (scan, root): (i64, Vec<u8>) = self
            .db
            .query_row(
                "SELECT s.id,s.root FROM scans s JOIN active a ON a.scan_id=s.id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("no scan yet"))?;
        Ok((scan, platform::path(&root)?, root))
    }
    /// One indexed entry of the published scan, by root-relative path.
    pub fn entry(&self, path: &[u8]) -> Result<Option<Entry>> {
        let (scan, _, _) = self.scope()?;
        Ok(self
            .db
            .query_row(
                "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND path=?",
                params![scan, path],
                crate::index::row_entry,
            )
            .optional()?)
    }
    /// Save a path in OMG Trash. Nothing on disk changes. Denied items are refused.
    pub fn mark(&self, path: &[u8], allocated: u64, home: &Path) -> Result<Gate> {
        let (_, root, root_raw) = self.scope()?;
        let entry = self
            .entry(path)?
            .ok_or_else(|| anyhow::anyhow!("not in the current index; rescan first"))?;
        let verdict = gate(&root, home, &entry);
        #[cfg(target_os = "linux")]
        let birth = linux::live_identity(&root, &entry.path)
            .ok()
            .filter(|(identity, _)| *identity == entry.identity)
            .ok_or_else(|| anyhow::anyhow!("changed on disk since the last scan; rescan first"))?
            .1;
        #[cfg(not(target_os = "linux"))]
        let birth: Option<String> = None;
        let warning = match &verdict {
            Gate::Denied(reason) => bail!("cannot add to OMG Trash: {reason}"),
            Gate::Warn(text) => Some(text.clone()),
            Gate::Allowed => None,
        };
        self.db.execute(
            "INSERT INTO marked(root,path,identity,kind,allocated,added,warning,birth) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(root,path) DO UPDATE SET identity=excluded.identity,kind=excluded.kind,allocated=excluded.allocated,warning=excluded.warning,birth=excluded.birth",
            params![root_raw, entry.path, entry.identity, entry.kind, i64::try_from(allocated)?, now(), warning, birth],
        )?;
        Ok(verdict)
    }
    pub fn marked(&self) -> Result<Vec<Marked>> {
        let Ok((_, _, root_raw)) = self.scope() else {
            return Ok(vec![]);
        };
        let mut statement = self.db.prepare(
            "SELECT id,path,identity,kind,allocated,added,warning,birth FROM marked WHERE root=? ORDER BY allocated DESC,id",
        )?;
        Ok(statement
            .query_map([root_raw], |r| {
                Ok(Marked {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    identity: r.get(2)?,
                    kind: r.get(3)?,
                    allocated: unsigned(r, 4)?,
                    added: r.get(5)?,
                    warning: r.get(6)?,
                    birth: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn unmark(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM marked WHERE id=?", [id])?;
        Ok(())
    }
    /// Freeze the exact set to delete. The interface shows it and must pass the same plan back.
    pub fn prepare_delete(&self, ids: &[i64], home: &Path) -> Result<DeletePlan> {
        if ids.is_empty() || ids.len() > 10_000 {
            bail!("select 1–10000 items in OMG Trash to delete");
        }
        let (_, root, _) = self.scope()?;
        let all = self.marked()?;
        let mut items: Vec<Marked> = Vec::new();
        let mut denied = Vec::new();
        for id in ids {
            let Some(item) = all.iter().find(|m| m.id == *id) else {
                bail!("item {id} is no longer in OMG Trash");
            };
            // Nested selections collapse into the enclosing directory.
            if items
                .iter()
                .any(|i| policy::inside(&item.path, &i.path, true))
            {
                continue;
            }
            items.retain(|i| !policy::inside(&i.path, &item.path, true));
            match self.entry(&item.path)? {
                Some(entry) => match gate(&root, home, &entry) {
                    Gate::Denied(reason) => denied.push((item.path.clone(), reason)),
                    Gate::Warn(text) => items.push(Marked {
                        warning: Some(text),
                        ..item.clone()
                    }),
                    Gate::Allowed => items.push(item.clone()),
                },
                None => denied.push((item.path.clone(), "no longer in the index".into())),
            }
        }
        let estimated = items.iter().map(|i| i.allocated).sum();
        let fingerprint = crate::catalog::digest(&serde_json::to_vec(&(&items, &denied))?);
        Ok(DeletePlan {
            fingerprint,
            items,
            denied,
            estimated,
            created: now(),
        })
    }
    /// Permanently delete the items of a confirmed plan. `confirmed` must be the fingerprint the
    /// user saw; every target is revalidated again here and skipped if anything changed.
    pub fn delete_permanently(
        &mut self,
        plan: &DeletePlan,
        confirmed: &str,
        home: &Path,
    ) -> Result<DeleteReport> {
        if confirmed != plan.fingerprint || now() - plan.created > 900 {
            bail!("confirmation does not match this plan or has expired; review it again");
        }
        let _cleanup = crate::index::lock(&self.state, "cleanup.lock")?;
        let _scan = crate::index::lock(&self.state, "scan.lock").map_err(|_| {
            anyhow::anyhow!("a scan is running; wait for it to finish, then delete")
        })?;
        let (_, root, _) = self.scope()?;
        // Unresolved older attempts are never replayed; they are labelled honestly.
        self.db.execute(
            "UPDATE operations SET outcome='interrupted-outcome-unknown' WHERE kind='delete' AND outcome='intent'",
            [],
        )?;
        self.db.execute(
            "INSERT INTO operations(created,kind,fingerprint,outcome,detail) VALUES(?,'delete',?,'intent',?)",
            params![now(), plan.fingerprint, serde_json::to_string(plan)?],
        )?;
        let operation = self.db.last_insert_rowid();
        let mut report = DeleteReport {
            skipped: plan.denied.clone(),
            ..Default::default()
        };
        #[cfg(target_os = "linux")]
        {
            let root_fd = platform::linux::root(&root)?;
            let before = platform::linux::volume(&root_fd).ok().map(|v| v.1);
            for item in &plan.items {
                match linux::delete_one(&root_fd, &root, home, item) {
                    Ok((removed, failed)) => {
                        report.removed_entries += removed;
                        report.failed_entries += failed;
                        if failed == 0 {
                            report.deleted.push(item.path.clone());
                            report.estimated_bytes += item.allocated;
                            self.forget(&item.path)?;
                            self.db
                                .execute("DELETE FROM marked WHERE id=?", [item.id])?;
                        } else {
                            report.skipped.push((
                                item.path.clone(),
                                format!("{failed} entries could not be removed; partially deleted"),
                            ));
                        }
                    }
                    Err(reason) => report.skipped.push((item.path.clone(), reason)),
                }
            }
            let after = platform::linux::volume(&root_fd).ok().map(|v| v.1);
            report.observed_free_change = match (before, after) {
                (Some(b), Some(a)) => Some(a as i64 - b as i64),
                _ => None,
            };
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (home, &root);
            report.skipped.extend(plan.items.iter().map(|i| {
                (
                    i.path.clone(),
                    "deletion is not supported on this platform".into(),
                )
            }));
        }
        let outcome = if report.skipped.is_empty() {
            "deleted"
        } else if report.deleted.is_empty() {
            "skipped"
        } else {
            "partial"
        };
        self.db.execute(
            "UPDATE operations SET outcome=?,detail=? WHERE id=?",
            params![outcome, serde_json::to_string(&report)?, operation],
        )?;
        Ok(report)
    }
    /// Drop a deleted subtree from the published index and subtract it from ancestor totals, so the
    /// views stay truthful without a full rescan.
    fn forget(&self, path: &[u8]) -> Result<()> {
        let (scan, _, _) = self.scope()?;
        let mut upper = path.to_vec();
        upper.extend_from_slice(b"0"); // '0' sorts right after '/', bounding the subtree range
        let mut prefix = path.to_vec();
        prefix.push(b'/');
        let entry = self.entry(path)?;
        let totals = self.subtree(path)?.unwrap_or(crate::rollup::Totals {
            allocated: entry.as_ref().map_or(0, |e| e.allocated),
            apparent: entry.as_ref().map_or(0, |e| e.apparent),
            ..Default::default()
        });
        let is_dir = entry.as_ref().is_some_and(|e| e.kind == "directory");
        let tx = self.db.unchecked_transaction()?;
        let removed = tx.execute(
            "DELETE FROM entries WHERE scan_id=?1 AND (path=?2 OR (path>?3 AND path<?4))",
            params![scan, path, prefix, upper],
        )?;
        tx.execute(
            "DELETE FROM dir_totals WHERE scan_id=?1 AND (path=?2 OR (path>?3 AND path<?4))",
            params![scan, path, prefix, upper],
        )?;
        let mut ancestor = path;
        loop {
            ancestor = ancestor
                .iter()
                .rposition(|b| *b == b'/')
                .map_or(&ancestor[..0], |n| &ancestor[..n]);
            tx.execute(
                "UPDATE dir_totals SET allocated=max(0,allocated-?2),apparent=max(0,apparent-?3),files=max(0,files-?4),dirs=max(0,dirs-?5),protected=max(0,protected-?6) WHERE scan_id=?1 AND path=?7",
                params![
                    scan,
                    i64::try_from(totals.allocated)?,
                    i64::try_from(totals.apparent)?,
                    i64::try_from(totals.files + u64::from(!is_dir))?,
                    i64::try_from(totals.dirs + u64::from(is_dir))?,
                    i64::try_from(totals.protected)?,
                    ancestor
                ],
            )?;
            if ancestor.is_empty() {
                break;
            }
        }
        tx.execute(
            "UPDATE scans SET entry_count=max(0,entry_count-?2),unique_allocated=max(0,unique_allocated-?3),unique_apparent=max(0,unique_apparent-?4) WHERE id=?1",
            params![scan, removed as i64, i64::try_from(totals.allocated)?, i64::try_from(totals.apparent)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use rustix::fd::OwnedFd;
    use rustix::fs::{self, AtFlags, Dir, Mode, OFlags};
    use std::ffi::CString;

    const MAX_ENTRIES: u64 = 5_000_000;

    fn identity(stat: &fs::Stat) -> String {
        format!("{}:{}", stat.st_dev, stat.st_ino)
    }

    fn birth_of(dir: &OwnedFd, name: &CString) -> Option<String> {
        let x = fs::statx(dir, name, AtFlags::SYMLINK_NOFOLLOW, fs::StatxFlags::BTIME).ok()?;
        (x.stx_mask & fs::StatxFlags::BTIME.bits() != 0)
            .then(|| format!("{}.{}", x.stx_btime.tv_sec, x.stx_btime.tv_nsec))
    }

    fn open_parent(
        root_fd: &OwnedFd,
        relative: &Path,
    ) -> std::result::Result<(OwnedFd, CString), String> {
        let name = relative.file_name().ok_or("no file name")?.to_os_string();
        let parent =
            platform::linux::acquire(root_fd, relative.parent().unwrap_or(Path::new("")), true)
                .map_err(|_| {
                    "parent folder moved, redirected or became inaccessible".to_string()
                })?;
        let cname = CString::new(platform::raw(Path::new(&name)).map_err(|e| e.to_string())?)
            .map_err(|_| "invalid name".to_string())?;
        Ok((parent, cname))
    }

    /// Live (identity, birth time) of a root-relative path, without following links.
    pub fn live_identity(root: &Path, path: &[u8]) -> Result<(String, Option<String>)> {
        let root_fd = platform::linux::root(root)?;
        let relative = platform::path(path)?;
        let (parent, name) = open_parent(&root_fd, &relative).map_err(anyhow::Error::msg)?;
        let stat = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        Ok((identity(&stat), birth_of(&parent, &name)))
    }

    /// Revalidate and delete one item. Ok((removed, failed)) or Err(reason it was skipped).
    pub fn delete_one(
        root_fd: &OwnedFd,
        root: &Path,
        home: &Path,
        item: &Marked,
    ) -> std::result::Result<(u64, u64), String> {
        let relative = platform::path(&item.path).map_err(|e| e.to_string())?;
        let (parent, cname) = open_parent(root_fd, &relative)?;
        let stat = fs::statat(&parent, &cname, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| "no longer exists".to_string())?;
        if identity(&stat) != item.identity
            || (item.birth.is_some() && birth_of(&parent, &cname) != item.birth)
        {
            return Err("replaced by a different file since it was added; skipped".into());
        }
        if stat.st_uid != rustix::process::geteuid().as_raw() {
            return Err("not owned by you".into());
        }
        let fresh = platform::linux::entry(&relative, &stat);
        if fresh.kind != item.kind {
            return Err("type changed since it was added; skipped".into());
        }
        if let Gate::Denied(reason) = gate(root, home, &fresh) {
            return Err(reason);
        }
        if fresh.kind == "file" {
            fs::unlinkat(&parent, &cname, AtFlags::empty()).map_err(|e| e.to_string())?;
            return Ok((1, 0));
        }
        let dir = fs::openat(
            &parent,
            &cname,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "folder could not be opened safely".to_string())?;
        let opened = fs::fstat(&dir).map_err(|e| e.to_string())?;
        if identity(&opened) != item.identity {
            return Err("folder was swapped during the check; skipped".into());
        }
        // Fresh descendant check right before deletion: no protected names, no mounts.
        let mut seen = 0u64;
        check_tree(&dir, root, &relative, opened.st_dev, &mut seen)?;
        let (removed, failed) = remove_tree(&dir, opened.st_dev);
        if failed == 0 {
            // rmdir only removes an empty directory, so a late swap cannot take anything with it.
            fs::unlinkat(&parent, &cname, AtFlags::REMOVEDIR).map_err(|e| e.to_string())?;
            return Ok((removed + 1, 0));
        }
        Ok((removed, failed))
    }

    fn entries(dir: &OwnedFd) -> std::result::Result<Vec<(CString, fs::Stat)>, String> {
        let mut stream = Dir::read_from(dir).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(item) = stream.read() {
            let item = item.map_err(|e| e.to_string())?;
            let name = item.file_name();
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            let stat =
                fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| e.to_string())?;
            out.push((name.to_owned(), stat));
        }
        Ok(out)
    }

    fn check_tree(
        dir: &OwnedFd,
        root: &Path,
        relative: &Path,
        dev: u64,
        seen: &mut u64,
    ) -> std::result::Result<(), String> {
        for (name, stat) in
            entries(dir).map_err(|_| "a folder inside could not be read".to_string())?
        {
            *seen += 1;
            if *seen > MAX_ENTRIES {
                return Err("too many entries to verify safely".into());
            }
            let child = relative.join(platform::linux::name(&name));
            let mut entry = platform::linux::entry(&child, &stat);
            // Links and special files are unlinked, never followed; check their names like files.
            if entry.kind == "symlink" || entry.kind == "special" {
                entry.kind = "file".into();
            }
            if let Some(reason) = policy::hard_protection(root, &entry) {
                return Err(format!(
                    "contains a protected item ({}): {reason}",
                    child.display()
                ));
            }
            if entry.kind == "directory" {
                if stat.st_dev != dev {
                    return Err("contains a mount point; skipped".into());
                }
                let sub = fs::openat(
                    dir,
                    &name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| "a folder inside could not be opened".to_string())?;
                check_tree(&sub, root, &child, dev, seen)?;
            }
        }
        Ok(())
    }

    /// Remove everything below `dir` through descriptors. Returns (removed, failed).
    fn remove_tree(dir: &OwnedFd, dev: u64) -> (u64, u64) {
        let Ok(list) = entries(dir) else {
            return (0, 1);
        };
        let (mut removed, mut failed) = (0, 0);
        for (name, stat) in list {
            if fs::FileType::from_raw_mode(stat.st_mode) == fs::FileType::Directory {
                if stat.st_dev != dev {
                    failed += 1;
                    continue;
                }
                match fs::openat(
                    dir,
                    &name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(sub)
                        if fs::fstat(&sub)
                            .is_ok_and(|s| s.st_ino == stat.st_ino && s.st_dev == dev) =>
                    {
                        let (r, f) = remove_tree(&sub, dev);
                        removed += r;
                        failed += f;
                        if f == 0 && fs::unlinkat(dir, &name, AtFlags::REMOVEDIR).is_ok() {
                            removed += 1;
                        } else if f == 0 {
                            failed += 1;
                        }
                    }
                    _ => failed += 1,
                }
            } else if fs::unlinkat(dir, &name, AtFlags::empty()).is_ok() {
                removed += 1;
            } else {
                failed += 1;
            }
        }
        (removed, failed)
    }
}
