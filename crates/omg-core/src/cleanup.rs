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
#[cfg(any(target_os = "linux", windows))]
use std::sync::atomic::Ordering;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

/// Plans issued by this session, keyed by fingerprint, with the scan id they were prepared on.
pub(crate) type IssuedPlans = HashMap<String, (i64, DeletePlan)>;

/// Seconds a reviewed plan may wait for confirmation before it must be reviewed again.
const PLAN_TTL: i64 = 900;

/// Refusal for an item whose creation time is missing: identity alone cannot prove it is the saved object.
const UNRECORDED_BIRTH: &str =
    "creation time was not recorded when added; add it to OMG Trash again";

fn fingerprint_of(
    scan: i64,
    items: &[Marked],
    denied: &[(Vec<u8>, String)],
    created: i64,
) -> Result<String> {
    Ok(crate::catalog::digest(&serde_json::to_vec(&(
        scan, items, denied, created,
    ))?))
}

/// Consume an issued plan. It is accepted once, only as issued by this session, and only while
/// fresh. The stored copy is removed before any check, so a refused attempt also needs a new review.
/// Any difference in items, warnings, denials or times is refused, so the warnings the user saw are
/// part of what was confirmed.
fn take_issued(
    issued: &mut IssuedPlans,
    plan: &DeletePlan,
    confirmed: &str,
    current: i64,
) -> Result<(i64, DeletePlan)> {
    let Some((scan, stored)) = issued.remove(confirmed) else {
        bail!(
            "this plan was not issued by this session, was already used, or was discarded; review it again"
        );
    };
    if stored.created > current {
        bail!("plan is dated in the future; review it again");
    }
    if current - stored.created > PLAN_TTL {
        bail!("confirmation has expired; review it again");
    }
    if serde_json::to_vec(plan)? != serde_json::to_vec(&stored)? {
        bail!("the plan was changed after review; review it again");
    }
    Ok((scan, stored))
}

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
    /// ext4, so identity alone cannot tell a recreated file from the original; an item without one
    /// is never deleted.
    pub birth: Option<String>,
    /// "auto" when a scan suggested it, "user" when the user added it.
    pub source: String,
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
    /// Entries were removed from a target that stays listed, or hard-linked data outside a removed
    /// folder was left in place. Published totals may differ from a fresh scan until the next scan.
    pub needs_reconcile: bool,
    /// Cancellation stopped the run; the remaining targets are `unattempted`.
    pub cancelled: bool,
}

/// What removing a subtree frees, from the indexed names. A hard-linked file counts only when every
/// one of its names is inside the subtree; otherwise its bytes stay on disk, so they are left out and
/// `survivors` is set. Totals then need a scan, because the surviving data is still counted elsewhere.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Freed {
    pub allocated: u64,
    pub apparent: u64,
    /// Non-excluded bytes only, matching the scan-wide unique totals.
    pub unique_allocated: u64,
    pub unique_apparent: u64,
    pub files: u64,
    pub dirs: u64,
    pub survivors: bool,
}

/// Bounds for `path > prefix AND path < upper`, selecting the entries strictly below `path`.
/// '0' sorts right after '/', so `upper` closes the subtree range.
fn descendants(path: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut upper = path.to_vec();
    upper.push(b'0');
    let mut prefix = path.to_vec();
    prefix.push(b'/');
    (prefix, upper)
}

/// How a deletable target's contents are judged at deletion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Ordinary folder: every descendant must pass hard protection.
    Plain,
    /// Known tool cache or build/dependency folder: package contents may look like keys or
    /// databases, so only always-sensitive names and mount points block it.
    Cache,
    /// The root of a Git repository untouched for `policy::STALE_DAYS`: its `.git` goes with it.
    StaleRepo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub gate: Gate,
    pub kind: Kind,
}

fn is_cache_root(absolute: &Path, home: &Path) -> bool {
    crate::catalog::DISCOVERIES
        .iter()
        .filter_map(|kind| crate::catalog::discovery_root(kind, home))
        .any(|root| absolute.starts_with(&root))
}

/// Live (identity, creation time) of a root-relative path, read without following links.
#[cfg(target_os = "linux")]
fn live_identity(root: &Path, path: &[u8]) -> Result<(String, Option<String>)> {
    linux::live_identity(root, path)
}
#[cfg(windows)]
fn live_identity(root: &Path, path: &[u8]) -> Result<(String, Option<String>)> {
    platform::windows::live_identity(root, path)
}

/// Permanent-delete support for the filesystem holding the scan `root`. Checked in the backend
/// before a plan is built and again before any mutation, so a forged plan cannot bypass it.
fn delete_support(root: &Path) -> Result<crate::capability::Support> {
    #[cfg(target_os = "linux")]
    {
        let fd = platform::linux::root(root)?;
        Ok(crate::capability::permanent_delete(
            platform::linux::filesystem(&fd)?,
        ))
    }
    #[cfg(windows)]
    {
        let dir = platform::windows::open_root(root)?;
        Ok(crate::capability::permanent_delete(
            platform::windows::filesystem_of(&dir)?,
        ))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = root;
        Ok(crate::capability::Support::Unsupported)
    }
}

/// Revalidate and remove one item: (entries removed, entries that could not be removed, whether a
/// removed file had other names). Windows reads link counts here because its scan does not.
#[cfg(target_os = "linux")]
fn remove_target(
    root: &platform::native::Dir,
    root_path: &Path,
    home: &Path,
    item: &Marked,
    kind: Kind,
) -> std::result::Result<(u64, u64, bool), String> {
    linux::delete_one(root, root_path, home, item, kind)
        .map(|(removed, failed)| (removed, failed, false))
}
#[cfg(windows)]
fn remove_target(
    root: &platform::native::Dir,
    root_path: &Path,
    home: &Path,
    item: &Marked,
    kind: Kind,
) -> std::result::Result<(u64, u64, bool), String> {
    windows::delete_one(root, root_path, home, item, kind)
}

/// Path-level checks shared by the gate and the final revalidation.
fn target_checks(root: &Path, home: &Path, entry: &Entry) -> std::result::Result<PathBuf, String> {
    let relative =
        platform::path(&entry.path).map_err(|_| "unsupported native path".to_string())?;
    if !policy::safe_relative(&relative) {
        return Err("the scan root itself or an unsafe path".into());
    }
    let absolute = root.join(&relative);
    if !absolute.starts_with(home) || absolute == home {
        return Err("outside your home folder; OMG never deletes system data".into());
    }
    if !matches!(entry.kind.as_str(), "file" | "directory") {
        return Err("links and special files are not deletable here".into());
    }
    if let Some(reason) = policy::hard_protection(root, entry) {
        return Err(reason);
    }
    Ok(absolute)
}

/// Whether `entry` (relative to the scan `root`) may be deleted, with what warning, and how its
/// contents will be checked. `memo.newest` supplies repository ages; unknown age means active.
pub fn verdict(
    root: &Path,
    home: &Path,
    entry: &Entry,
    memo: &mut policy::RepositoryMemo,
) -> Verdict {
    let absolute = match target_checks(root, home, entry) {
        Ok(path) => path,
        Err(reason) => {
            return Verdict {
                gate: Gate::Denied(reason),
                kind: Kind::Plain,
            };
        }
    };
    let cache = is_cache_root(&absolute, home);
    match policy::repository_status(root, entry, memo) {
        policy::RepoStatus::Outside => {
            let regenerable = absolute.file_name().is_some_and(|n| {
                [
                    "node_modules",
                    "__pycache__",
                    ".pytest_cache",
                    ".mypy_cache",
                    ".ruff_cache",
                ]
                .contains(&n.to_string_lossy().as_ref())
            });
            Verdict {
                gate: Gate::Allowed,
                kind: if cache || regenerable {
                    Kind::Cache
                } else {
                    Kind::Plain
                },
            }
        }
        policy::RepoStatus::Regenerable(repo) => Verdict {
            gate: Gate::Warn(format!(
                "build/dependency folder of the Git repository at {}; its tool recreates it",
                repo.display()
            )),
            kind: Kind::Cache,
        },
        policy::RepoStatus::Stale { repo, days } => Verdict {
            gate: Gate::Warn(format!(
                "Git repository at {} untouched for {days} days; uncommitted or unpushed work would be lost",
                repo.display()
            )),
            kind: if absolute == repo {
                Kind::StaleRepo
            } else {
                Kind::Plain
            },
        },
        policy::RepoStatus::Active { .. } if cache => Verdict {
            gate: Gate::Allowed,
            kind: Kind::Cache,
        },
        policy::RepoStatus::Active { repo, days } => Verdict {
            gate: Gate::Denied(format!(
                "active Git repository at {}{}; only build/dependency folders inside are deletable",
                repo.display(),
                days.map(|d| format!(" (changed {d} day(s) ago)"))
                    .unwrap_or_default()
            )),
            kind: Kind::Plain,
        },
    }
}

/// `verdict` without repository age information (unknown age counts as active).
pub fn gate(root: &Path, home: &Path, entry: &Entry) -> Gate {
    verdict(root, home, entry, &mut policy::RepositoryMemo::default()).gate
}

impl Index {
    /// Delete verdict using repository ages from the published rollups.
    pub fn verdict_for(&self, entry: &Entry, home: &Path) -> Result<Verdict> {
        let (_, root, _) = self.scope()?;
        let lookup = |dir: &Path| crate::rollup::newest_at(self, &root, dir);
        let mut memo = policy::RepositoryMemo::with_ages(&lookup);
        Ok(verdict(&root, home, entry, &mut memo))
    }
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
    /// What removing the subtree at `path` frees (see [`Freed`]). Hard links are grouped by identity:
    /// a group whose names are all inside the subtree is freed, a larger group is a survivor.
    fn freed(&self, scan: i64, path: &[u8]) -> Result<Freed> {
        let (prefix, upper) = descendants(path);
        let mut statement = self.db.prepare(
            "SELECT kind,count(*),max(links),max(apparent),max(allocated),coalesce(max(CASE WHEN excluded IS NULL THEN apparent END),0),coalesce(max(CASE WHEN excluded IS NULL THEN allocated END),0) FROM entries WHERE scan_id=?1 AND (path=?2 OR (path>?3 AND path<?4)) GROUP BY identity",
        )?;
        let mut freed = Freed::default();
        let mut rows = statement.query(params![scan, path, prefix, upper])?;
        while let Some(row) = rows.next()? {
            let directory = row.get::<_, String>(0)? == "directory";
            let (names, links): (i64, i64) = (row.get(1)?, row.get(2)?);
            if !directory && names < links {
                freed.survivors = true;
                continue;
            }
            freed.allocated += unsigned(row, 4)?;
            freed.apparent += unsigned(row, 3)?;
            freed.unique_allocated += unsigned(row, 6)?;
            freed.unique_apparent += unsigned(row, 5)?;
            if directory {
                freed.dirs += 1;
            } else {
                freed.files += 1;
            }
        }
        Ok(freed)
    }
    /// Save a path in OMG Trash. Nothing on disk changes. Denied items are refused.
    pub fn mark(&self, path: &[u8], allocated: u64, home: &Path) -> Result<Gate> {
        self.upsert_marked(path, allocated, home, "user")
    }
    fn upsert_marked(
        &self,
        path: &[u8],
        allocated: u64,
        home: &Path,
        source: &str,
    ) -> Result<Gate> {
        let (_, root, root_raw) = self.scope()?;
        let entry = self
            .entry(path)?
            .ok_or_else(|| anyhow::anyhow!("not in the current index; rescan first"))?;
        let verdict = self.verdict_for(&entry, home)?.gate;
        let warning = match &verdict {
            Gate::Denied(reason) => bail!("cannot add to OMG Trash: {reason}"),
            Gate::Warn(text) => Some(text.clone()),
            Gate::Allowed => None,
        };
        #[cfg(any(target_os = "linux", windows))]
        let birth = {
            let (_, created) = live_identity(&root, &entry.path)
                .ok()
                .filter(|(identity, _)| *identity == entry.identity)
                .ok_or_else(|| {
                    anyhow::anyhow!("changed on disk since the last scan; rescan first")
                })?;
            // Refused rather than saved without a creation time: deletion could never verify it.
            created.ok_or_else(|| {
                anyhow::anyhow!(
                    "this filesystem reports no creation time, so the item cannot be verified before deletion"
                )
            })?
        };
        #[cfg(not(any(target_os = "linux", windows)))]
        let birth: Option<String> = {
            let _ = &root;
            None
        };
        if source == "user" {
            // A user's choice always wins, including over an earlier dismissed suggestion.
            self.db.execute(
                "INSERT INTO marked(root,path,identity,kind,allocated,added,warning,birth,source,dismissed) VALUES(?,?,?,?,?,?,?,?,'user',0) ON CONFLICT(root,path) DO UPDATE SET identity=excluded.identity,kind=excluded.kind,allocated=excluded.allocated,warning=excluded.warning,birth=excluded.birth,source='user',dismissed=0",
                params![root_raw, entry.path, entry.identity, entry.kind, i64::try_from(allocated)?, now(), warning, birth],
            )?;
        } else {
            // Suggestions refresh their own facts but never resurrect something the user removed.
            self.db.execute(
                "INSERT INTO marked(root,path,identity,kind,allocated,added,warning,birth,source,dismissed) VALUES(?,?,?,?,?,?,?,?,'auto',0) ON CONFLICT(root,path) DO UPDATE SET identity=excluded.identity,kind=excluded.kind,allocated=excluded.allocated,warning=excluded.warning,birth=excluded.birth WHERE marked.dismissed=0",
                params![root_raw, entry.path, entry.identity, entry.kind, i64::try_from(allocated)?, now(), warning, birth],
            )?;
        }
        Ok(verdict)
    }
    /// Add regenerable findings (tool caches, dependency folders of stale projects) to OMG Trash
    /// as suggestions (never large files, never anything a gate denies). Returns how many are listed after the refresh.
    pub fn suggest(&self, home: &Path) -> Result<usize> {
        let (scan, _, root_raw) = self.scope()?;
        // Suggestions whose folder vanished from the index are stale.
        self.db.execute(
            "DELETE FROM marked WHERE source='auto' AND dismissed=0 AND root=?1 AND NOT EXISTS (SELECT 1 FROM entries e WHERE e.scan_id=?2 AND e.path=marked.path)",
            params![root_raw, scan],
        )?;
        let mut listed = 0;
        let mut suggested: Vec<Vec<u8>> = Vec::new();
        for finding in self.findings_in(1000, 0, false, home)? {
            // Regenerable things only: tool caches and dependency folders of stale projects.
            if !["package_caches", "build_artifacts"].contains(&finding.rule.category.as_str())
                || finding.effective_tier == "protected"
            {
                continue;
            }
            if self
                .upsert_marked(&finding.entry.path, finding.estimate, home, "auto")
                .is_ok()
            {
                listed += 1;
                suggested.push(finding.entry.path.clone());
            }
        }
        // Suggestions reflect the current rules: drop earlier ones no longer suggested.
        let mut statement = self
            .db
            .prepare("SELECT id,path FROM marked WHERE source='auto' AND dismissed=0 AND root=?")?;
        let stale: Vec<i64> = statement
            .query_map([&root_raw], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .filter_map(|r| r.ok())
            .filter(|(_, path)| !suggested.contains(path))
            .map(|(id, _)| id)
            .collect();
        for id in stale {
            self.db.execute("DELETE FROM marked WHERE id=?", [id])?;
        }
        Ok(listed)
    }
    /// Whether scans add cache suggestions to OMG Trash (default: on).
    pub fn auto_trash(&self) -> Result<bool> {
        let value: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='auto_trash'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        // Off unless the user turned it on: suggestions must never be the only thing between a
        // scan and a deletion list the user did not review.
        Ok(value.as_deref() == Some(b"on".as_slice()))
    }
    pub fn set_auto_trash(&self, on: bool) -> Result<()> {
        self.db.execute(
            "INSERT INTO settings VALUES('auto_trash',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if on { b"on".to_vec() } else { b"off".to_vec() }],
        )?;
        Ok(())
    }
    pub fn marked(&self) -> Result<Vec<Marked>> {
        let Ok((_, _, root_raw)) = self.scope() else {
            return Ok(vec![]);
        };
        let mut statement = self.db.prepare(
            "SELECT id,path,identity,kind,allocated,added,warning,birth,source FROM marked WHERE root=? AND dismissed=0 ORDER BY allocated DESC,id",
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
                    source: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Take an item off the list. A removed suggestion stays dismissed so scans don't re-add it.
    pub fn unmark(&self, id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE marked SET dismissed=1 WHERE id=? AND source='auto'",
            [id],
        )?;
        self.db
            .execute("DELETE FROM marked WHERE id=? AND source='user'", [id])?;
        Ok(())
    }
    /// Forget an issued plan that the user cancelled. Unknown fingerprints are ignored.
    pub fn discard_plan(&mut self, fingerprint: &str) {
        self.issued.remove(fingerprint);
    }
    /// Freeze the exact set to delete and issue it to this session. The interface shows it and must
    /// pass the same plan back. The issued copy is the only one `delete_permanently` will accept.
    pub fn prepare_delete(&mut self, ids: &[i64], home: &Path) -> Result<DeletePlan> {
        if ids.is_empty() || ids.len() > 10_000 {
            bail!("select 1–10000 items in OMG Trash to delete");
        }
        let (scan, root, _) = self.scope()?;
        if delete_support(&root)? == crate::capability::Support::Unsupported {
            bail!("permanent deletion is not available in this build or on this filesystem");
        }
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
            if item.birth.is_none() {
                denied.push((item.path.clone(), UNRECORDED_BIRTH.into()));
                continue;
            }
            items.retain(|i| !policy::inside(&i.path, &item.path, true));
            match self.entry(&item.path)? {
                Some(entry) => match self.verdict_for(&entry, home)?.gate {
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
        // Only bytes this plan would free: a hard link whose other name survives is not counted.
        let mut estimated = 0;
        for item in &items {
            estimated += self.freed(scan, &item.path)?.allocated;
        }
        let created = now();
        let fingerprint = fingerprint_of(scan, &items, &denied, created)?;
        let plan = DeletePlan {
            fingerprint: fingerprint.clone(),
            items,
            denied,
            estimated,
            created,
        };
        self.issued
            .retain(|_, (_, held)| now() - held.created <= PLAN_TTL);
        self.issued.insert(fingerprint, (scan, plan.clone()));
        Ok(plan)
    }
    /// Permanently delete the items of a confirmed plan. `confirmed` must be the fingerprint the
    /// user saw; every target is revalidated again here and skipped if anything changed.
    pub fn delete_permanently(
        &mut self,
        plan: &DeletePlan,
        confirmed: &str,
        home: &Path,
    ) -> Result<DeleteReport> {
        self.delete_with_control(plan, confirmed, home, None, |_| {})
    }
    /// `delete_permanently` with two test and runtime controls. `cancel` is checked before each
    /// target: once set, the remaining targets are recorded as `unattempted` and nothing new starts.
    /// `before_mutate` runs after a target has been validated and just before it is changed.
    pub fn delete_with_control<F: FnMut(&[u8])>(
        &mut self,
        plan: &DeletePlan,
        confirmed: &str,
        home: &Path,
        cancel: Option<&AtomicBool>,
        mut before_mutate: F,
    ) -> Result<DeleteReport> {
        let (scan, plan) = take_issued(&mut self.issued, plan, confirmed, now())?;
        let _cleanup = crate::index::lock(&self.state, "cleanup.lock")?;
        let _scan = crate::index::lock(&self.state, "scan.lock").map_err(|_| {
            anyhow::anyhow!("a scan is running; wait for it to finish, then delete")
        })?;
        let (current, root, _) = self.scope()?;
        if current != scan {
            bail!("the scan changed after review; review it again");
        }
        // Refused before the intent record: an unsupported filesystem changes nothing and logs nothing.
        if delete_support(&root)? == crate::capability::Support::Unsupported {
            bail!("permanent deletion is not available on this filesystem");
        }
        // Intent and every target row are written in one transaction before anything is touched.
        // If that write fails, no file has changed and no operation exists.
        let operation = self.record_intent(&plan)?;
        let mut report = DeleteReport {
            skipped: plan.denied.clone(),
            ..Default::default()
        };
        #[cfg(any(target_os = "linux", windows))]
        {
            let root_dir = platform::native::open_root(&root)?;
            let before = platform::native::volume(&root_dir).ok().map(|v| v.1);
            for (position, item) in plan.items.iter().enumerate() {
                if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                    report.cancelled = true;
                    for rest in &plan.items[position..] {
                        self.record_target(
                            operation,
                            rest.id,
                            "unattempted",
                            "cancelled before this target",
                        )?;
                    }
                    break;
                }
                let kind = match self.entry(&item.path)? {
                    Some(entry) => match self.verdict_for(&entry, home)? {
                        Verdict {
                            gate: Gate::Denied(reason),
                            ..
                        } => {
                            self.record_target(operation, item.id, "skipped", &reason)?;
                            report.skipped.push((item.path.clone(), reason));
                            continue;
                        }
                        v => v.kind,
                    },
                    None => {
                        let reason = "no longer in the index".to_string();
                        self.record_target(operation, item.id, "skipped", &reason)?;
                        report.skipped.push((item.path.clone(), reason));
                        continue;
                    }
                };
                // Read before anything changes: what the index attributes to this subtree.
                let freed = self.freed(scan, &item.path)?;
                before_mutate(&item.path);
                match remove_target(&root_dir, &root, home, item, kind) {
                    Ok((removed, failed, shared)) => {
                        report.removed_entries += removed;
                        report.failed_entries += failed;
                        if failed == 0 {
                            report.deleted.push(item.path.clone());
                            report.estimated_bytes += freed.allocated;
                            // A removed name whose data has other names leaves totals to reconcile.
                            report.needs_reconcile |= freed.survivors || shared;
                            self.record_target(operation, item.id, "deleted", "")?;
                            self.forget(scan, &item.path, &freed)?;
                            self.db
                                .execute("DELETE FROM marked WHERE id=?", [item.id])?;
                        } else {
                            // Some entries are gone but the target is not. The published totals
                            // still include them, so the index must be reconciled by a scan.
                            report.needs_reconcile |= removed > 0;
                            let detail = format!(
                                "{failed} entries could not be removed; {removed} removed before the failure"
                            );
                            self.record_target(operation, item.id, "partial", &detail)?;
                            report.skipped.push((item.path.clone(), detail));
                        }
                    }
                    Err(reason) => {
                        self.record_target(operation, item.id, "skipped", &reason)?;
                        report.skipped.push((item.path.clone(), reason));
                    }
                }
            }
            let after = platform::native::volume(&root_dir).ok().map(|v| v.1);
            report.observed_free_change = match (before, after) {
                (Some(b), Some(a)) => Some(a as i64 - b as i64),
                _ => None,
            };
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (home, &root, &mut before_mutate, cancel);
            for item in &plan.items {
                let reason = "deletion is not supported on this platform".to_string();
                self.record_target(operation, item.id, "skipped", &reason)?;
                report.skipped.push((item.path.clone(), reason));
            }
        }
        if report.needs_reconcile {
            self.db.execute(
                "INSERT INTO settings VALUES('reconcile_needed','1') ON CONFLICT(key) DO UPDATE SET value='1'",
                [],
            )?;
        }
        let outcome = if report.cancelled {
            "cancelled"
        } else if report.skipped.is_empty() {
            "deleted"
        } else if report.deleted.is_empty() && report.removed_entries == 0 {
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
    /// Write the operation, its targets, and the labelled outcome of any older unresolved attempt.
    /// Unresolved attempts are never replayed.
    fn record_intent(&self, plan: &DeletePlan) -> Result<i64> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "UPDATE operations SET outcome='interrupted-outcome-unknown' WHERE kind='delete' AND outcome='intent'",
            [],
        )?;
        tx.execute(
            "UPDATE delete_targets SET outcome='interrupted-outcome-unknown',updated=?1 WHERE outcome='intent'",
            [now()],
        )?;
        tx.execute(
            "INSERT INTO operations(created,kind,fingerprint,outcome,detail) VALUES(?,'delete',?,'intent',?)",
            params![now(), plan.fingerprint, serde_json::to_string(plan)?],
        )?;
        let operation = tx.last_insert_rowid();
        for item in &plan.items {
            tx.execute(
                "INSERT INTO delete_targets(operation,item,path,outcome,detail,updated) VALUES(?,?,?,'intent','',?)",
                params![operation, item.id, item.path, now()],
            )?;
        }
        for (path, reason) in &plan.denied {
            // Denied at review: no marked item to refer to, so the reference is 0.
            tx.execute(
                "INSERT INTO delete_targets(operation,item,path,outcome,detail,updated) VALUES(?,0,?,'skipped',?,?)",
                params![operation, path, reason, now()],
            )?;
        }
        tx.commit()?;
        Ok(operation)
    }
    fn record_target(&self, operation: i64, item: i64, outcome: &str, detail: &str) -> Result<()> {
        self.db.execute(
            "UPDATE delete_targets SET outcome=?,detail=?,updated=? WHERE operation=? AND item=?",
            params![outcome, detail, now(), operation, item],
        )?;
        Ok(())
    }
    #[cfg(any(target_os = "linux", windows))]
    /// Drop a deleted subtree from the published index and subtract what it freed from ancestor
    /// totals, so the views stay truthful without a full rescan. Hard-linked data that survives
    /// outside the subtree stays counted (see [`Freed`]).
    fn forget(&self, scan: i64, path: &[u8], freed: &Freed) -> Result<()> {
        let (prefix, upper) = descendants(path);
        // Protection counts are per name, and every name in the subtree is removed with it.
        let protected = self.subtree(path)?.map_or(0, |t| t.protected);
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
                    i64::try_from(freed.allocated)?,
                    i64::try_from(freed.apparent)?,
                    i64::try_from(freed.files)?,
                    i64::try_from(freed.dirs)?,
                    i64::try_from(protected)?,
                    ancestor
                ],
            )?;
            if ancestor.is_empty() {
                break;
            }
        }
        tx.execute(
            "UPDATE scans SET entry_count=max(0,entry_count-?2),unique_allocated=max(0,unique_allocated-?3),unique_apparent=max(0,unique_apparent-?4) WHERE id=?1",
            params![scan, removed as i64, i64::try_from(freed.unique_allocated)?, i64::try_from(freed.unique_apparent)?],
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
        kind: Kind,
    ) -> std::result::Result<(u64, u64), String> {
        let relative = platform::path(&item.path).map_err(|e| e.to_string())?;
        let (parent, cname) = open_parent(root_fd, &relative)?;
        // Both must match: inode numbers are reused, so identity alone cannot prove the same object.
        let recorded = item.birth.as_deref().ok_or(UNRECORDED_BIRTH)?;
        let stat = fs::statat(&parent, &cname, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| "no longer exists".to_string())?;
        if identity(&stat) != item.identity
            || birth_of(&parent, &cname).as_deref() != Some(recorded)
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
        target_checks(root, home, &fresh)?;
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
        check_tree(&dir, root, &relative, opened.st_dev, kind, &mut seen)?;
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
        kind: Kind,
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
            let name_text = platform::linux::name(&name).to_string_lossy().into_owned();
            if policy::always_sensitive(&name_text) {
                return Err(format!("contains a sensitive item ({})", child.display()));
            }
            // Caches and stale repositories may legitimately contain `.git`, keys or databases
            // (package contents, the repository itself); ordinary folders may not.
            if kind == Kind::Plain
                && let Some(reason) = policy::hard_protection(root, &entry)
            {
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
                check_tree(&sub, root, &child, dev, kind, seen)?;
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

#[cfg(windows)]
mod windows {
    //! Windows removal. Each object is opened without delete sharing, so no other program can
    //! rename, replace or delete it while it is held. It is revalidated through that handle and
    //! deleted through the same handle (`FileDispositionInfoEx`). The object checked is therefore
    //! the object removed, even if its name is reused meanwhile.
    use super::*;
    use crate::platform::windows::{
        BACKSLASH, Child, Dir, Handle, SHARE_ALL, children, final_path, join, object, open_shared,
        same_path,
    };
    use std::{ffi::OsString, io, mem, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND,
            ERROR_SHARING_VIOLATION, HANDLE, LocalFree,
        },
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
            TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{
            DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfoEx,
            GetVolumeNameForVolumeMountPointW, READ_CONTROL, SetFileInformationByHandle,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    const MAX_ENTRIES: u64 = 5_000_000;
    /// Others may keep reading and writing, but not rename or delete, while OMG holds the object.
    const SHARE_NO_DELETE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
    const TAG_SYMLINK: u32 = 0xA000_000C;
    const TAG_MOUNT_POINT: u32 = 0xA000_0003;

    fn reason(error: &io::Error) -> String {
        match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_SHARING_VIOLATION) => "in use by another program; skipped".into(),
            Some(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) => "no longer exists".into(),
            Some(ERROR_ACCESS_DENIED) => "access denied".into(),
            _ => error.to_string(),
        }
    }

    /// Delete the object behind `handle`. POSIX semantics remove the name at once, even if another
    /// program still reads the file. A folder must already be empty. The read-only attribute does
    /// not block removal, as on Linux, where a file's mode does not govern unlinking it.
    fn dispose(handle: &Handle) -> io::Result<()> {
        let info = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        // SAFETY: `info` is a plain structure of the size passed, and the handle is live.
        let ok = unsafe {
            SetFileInformationByHandle(
                handle.raw(),
                FileDispositionInfoEx,
                (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
                mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// The current process's user, as a `TOKEN_USER` held in an 8-byte-aligned buffer.
    struct CurrentUser(Vec<u64>);

    impl CurrentUser {
        fn load() -> io::Result<Self> {
            struct Token(HANDLE);
            impl Drop for Token {
                fn drop(&mut self) {
                    // SAFETY: the token handle came from `OpenProcessToken` and is closed only here.
                    unsafe { CloseHandle(self.0) };
                }
            }
            let mut raw: HANDLE = ptr::null_mut();
            // SAFETY: the process pseudo-handle needs no closing; `raw` receives an owned token.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Token(raw);
            let mut needed = 0u32;
            // SAFETY: a size query with no buffer; it only writes `needed`.
            unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed) };
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            // SAFETY: the buffer is writable for its full length in bytes.
            let ok = unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                    &mut needed,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(buffer))
        }
        fn sid(&self) -> PSID {
            // SAFETY: the buffer holds a `TOKEN_USER` written by `GetTokenInformation`.
            unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }
    }

    /// Whether the object's owner is the current user. Needs `READ_CONTROL` on the handle.
    fn owned_by(handle: &Handle, user: &CurrentUser) -> io::Result<bool> {
        let mut owner: PSID = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: the out-pointers are valid; only the owner and its descriptor are requested.
        let status = unsafe {
            GetSecurityInfo(
                handle.raw(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        // SAFETY: `owner` points into `descriptor`, which stays allocated until `LocalFree`.
        let same = unsafe { EqualSid(owner, user.sid()) } != 0;
        // SAFETY: `descriptor` was allocated by `GetSecurityInfo` and is freed once.
        unsafe { LocalFree(descriptor) };
        Ok(same)
    }

    /// A junction to another volume's root is a volume mount point; removing it unmounts a volume.
    fn is_volume_mount(path: &[u16]) -> bool {
        if path.len() > 240 {
            // Too long for this query: treat as a mount point and refuse.
            return true;
        }
        let mut wide = path.to_vec();
        if wide.last() != Some(&BACKSLASH) {
            wide.push(BACKSLASH);
        }
        wide.push(0);
        let mut name = [0u16; 64];
        // SAFETY: `wide` is NUL-terminated; `name` is writable for its length.
        let volume = unsafe {
            GetVolumeNameForVolumeMountPointW(wide.as_ptr(), name.as_mut_ptr(), name.len() as u32)
        };
        volume != 0
    }

    fn with_name(path: &[u16], name: &[u16]) -> Vec<u16> {
        let mut out = path.to_vec();
        if out.last() != Some(&BACKSLASH) {
            out.push(BACKSLASH);
        }
        out.extend_from_slice(name);
        out
    }

    /// The opened object is still the listed child, at the listed path.
    fn still_listed(handle: &Handle, child: &Child, path: &[u16]) -> bool {
        object(handle).is_ok_and(|found| found.meta.identity == child.meta.identity)
            && final_path(handle).is_ok_and(|actual| same_path(&actual, path))
    }

    /// Revalidate and delete one item. Ok((removed, failed, shared links)) or Err(reason skipped).
    pub fn delete_one(
        root: &Dir,
        root_path: &Path,
        home: &Path,
        item: &Marked,
        kind: Kind,
    ) -> std::result::Result<(u64, u64, bool), String> {
        let recorded = item.birth.as_deref().ok_or(UNRECORDED_BIRTH)?;
        let relative = platform::path(&item.path).map_err(|e| e.to_string())?;
        let wanted = join(root.path(), &relative).map_err(|e| e.to_string())?;
        let directory = item.kind == "directory";
        let access = DELETE
            | READ_CONTROL
            | FILE_READ_ATTRIBUTES
            | if directory { FILE_LIST_DIRECTORY } else { 0 };
        let handle = open_shared(&wanted, access, SHARE_NO_DELETE).map_err(|e| reason(&e))?;
        let found = object(&handle).map_err(|e| e.to_string())?;
        // Both must match: identity proves the object, creation time proves it was not recreated.
        if found.meta.identity != item.identity || found.created.to_string() != recorded {
            return Err("replaced by a different file since it was added; skipped".into());
        }
        if !final_path(&handle).is_ok_and(|actual| same_path(&actual, &wanted)) {
            return Err("moved or redirected since it was checked; skipped".into());
        }
        if let Some(why) = found.meta.restricted {
            return Err(format!(
                "{why}; deleting it would also delete the cloud copy"
            ));
        }
        let user = CurrentUser::load().map_err(|e| e.to_string())?;
        if !owned_by(&handle, &user).map_err(|e| e.to_string())? {
            return Err("not owned by you".into());
        }
        let fresh = platform::entry_from_meta(&relative, &found.meta);
        if fresh.kind != item.kind {
            return Err("type changed since it was added; skipped".into());
        }
        target_checks(root_path, home, &fresh)?;
        if !directory {
            dispose(&handle).map_err(|e| reason(&e))?;
            return Ok((1, 0, found.links > 1));
        }
        let volume = found.meta.volume;
        let mut seen = 0u64;
        check_tree(
            &handle, &wanted, root_path, &relative, volume, kind, &mut seen,
        )?;
        let (removed, failed, shared) = remove_tree(&handle, &wanted, volume);
        if failed == 0 {
            // The folder is empty now; its own handle removes it.
            dispose(&handle).map_err(|e| reason(&e))?;
            return Ok((removed + 1, 0, shared));
        }
        Ok((removed, failed, shared))
    }

    /// Fresh descendant check right before deletion: no sensitive or protected names, no cloud
    /// placeholders, no mount points or unknown links.
    fn check_tree(
        handle: &Handle,
        path: &[u16],
        root: &Path,
        relative: &Path,
        volume: u64,
        kind: Kind,
        seen: &mut u64,
    ) -> std::result::Result<(), String> {
        let list = children(handle, volume)
            .map_err(|_| "a folder inside could not be read".to_string())?;
        for child in list {
            *seen += 1;
            if *seen > MAX_ENTRIES {
                return Err("too many entries to verify safely".into());
            }
            let name = OsString::from_wide(&child.name);
            let child_relative = relative.join(&name);
            let child_path = with_name(path, &child.name);
            let shown = child_relative.display();
            if policy::always_sensitive(&name.to_string_lossy()) {
                return Err(format!("contains a sensitive item ({shown})"));
            }
            if let Some(why) = child.meta.restricted {
                return Err(format!("contains a {why} ({shown}); skipped"));
            }
            if child.meta.kind == "symlink" {
                // Links are removed as themselves, never followed. Only plain links qualify.
                match child.tag {
                    TAG_SYMLINK => {}
                    TAG_MOUNT_POINT if !is_volume_mount(&child_path) => {}
                    TAG_MOUNT_POINT => {
                        return Err(format!("contains a mount point ({shown}); skipped"));
                    }
                    _ => return Err(format!("contains an unsupported link ({shown}); skipped")),
                }
            }
            let mut entry = platform::entry_from_meta(&child_relative, &child.meta);
            if entry.kind == "symlink" {
                entry.kind = "file".into();
            }
            // Caches and stale repositories may legitimately contain `.git`, keys or databases
            // (package contents, the repository itself); ordinary folders may not.
            if kind == Kind::Plain
                && let Some(why) = policy::hard_protection(root, &entry)
            {
                return Err(format!("contains a protected item ({shown}): {why}"));
            }
            if child.meta.kind == "directory" {
                let sub = open_shared(
                    &child_path,
                    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
                    SHARE_ALL,
                )
                .map_err(|_| "a folder inside could not be opened".to_string())?;
                if !still_listed(&sub, &child, &child_path) {
                    return Err("a folder inside changed during the check; skipped".into());
                }
                check_tree(&sub, &child_path, root, &child_relative, volume, kind, seen)?;
            }
        }
        Ok(())
    }

    /// Remove everything below `handle`, bottom-up, each object through its own verified handle.
    /// Returns (removed, failed, whether a removed file had other names).
    fn remove_tree(handle: &Handle, path: &[u16], volume: u64) -> (u64, u64, bool) {
        let Ok(list) = children(handle, volume) else {
            return (0, 1, false);
        };
        let (mut removed, mut failed, mut shared) = (0, 0, false);
        for child in list {
            let child_path = with_name(path, &child.name);
            let directory = child.meta.kind == "directory";
            let access =
                DELETE | FILE_READ_ATTRIBUTES | if directory { FILE_LIST_DIRECTORY } else { 0 };
            let Ok(sub) = open_shared(&child_path, access, SHARE_NO_DELETE) else {
                failed += 1;
                continue;
            };
            let Ok(found) = object(&sub) else {
                failed += 1;
                continue;
            };
            if found.meta.identity != child.meta.identity
                || !final_path(&sub).is_ok_and(|actual| same_path(&actual, &child_path))
            {
                failed += 1;
                continue;
            }
            if directory {
                let (r, f, s) = remove_tree(&sub, &child_path, volume);
                removed += r;
                failed += f;
                shared |= s;
                if f == 0 && dispose(&sub).is_ok() {
                    removed += 1;
                } else if f == 0 {
                    failed += 1;
                }
            } else {
                shared |= found.links > 1;
                if dispose(&sub).is_ok() {
                    removed += 1;
                } else {
                    failed += 1;
                }
            }
        }
        (removed, failed, shared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(warning: Option<&str>) -> Marked {
        Marked {
            id: 1,
            path: b"projects/build".to_vec(),
            identity: "1:2".into(),
            kind: "directory".into(),
            allocated: 4096,
            added: 0,
            warning: warning.map(str::to_string),
            birth: None,
            source: "user".into(),
        }
    }

    fn issue(issued: &mut IssuedPlans, created: i64, items: Vec<Marked>) -> DeletePlan {
        let denied = Vec::new();
        let fingerprint = fingerprint_of(7, &items, &denied, created).unwrap();
        let plan = DeletePlan {
            fingerprint: fingerprint.clone(),
            items,
            denied,
            estimated: 4096,
            created,
        };
        issued.insert(fingerprint, (7, plan.clone()));
        plan
    }

    #[test]
    fn issued_plan_is_accepted_once_then_refused_as_replay() {
        let mut issued = IssuedPlans::new();
        let plan = issue(&mut issued, 1000, vec![item(None)]);
        let fingerprint = plan.fingerprint.clone();
        let (scan, _) = take_issued(&mut issued, &plan, &fingerprint, 1001).unwrap();
        assert_eq!(scan, 7);
        let error = take_issued(&mut issued, &plan, &fingerprint, 1002).unwrap_err();
        assert!(error.to_string().contains("not issued by this session"));
    }

    #[test]
    fn modified_items_or_warnings_are_refused_and_the_plan_is_consumed() {
        let mut issued = IssuedPlans::new();
        let plan = issue(
            &mut issued,
            1000,
            vec![item(Some("inside a Git repository"))],
        );
        let fingerprint = plan.fingerprint.clone();

        // A caller that drops the warning the user saw must not get the plan through.
        let mut softened = plan.clone();
        softened.items[0].warning = None;
        assert!(take_issued(&mut issued, &softened, &fingerprint, 1001).is_err());
        // The attempt consumed the stored plan, so the original cannot be used afterwards either.
        assert!(take_issued(&mut issued, &plan, &fingerprint, 1001).is_err());
    }

    #[test]
    fn expired_and_future_dated_plans_are_refused() {
        let mut issued = IssuedPlans::new();
        let plan = issue(&mut issued, 1000, vec![item(None)]);
        let fingerprint = plan.fingerprint.clone();
        let error = take_issued(&mut issued, &plan, &fingerprint, 1000 + PLAN_TTL + 1).unwrap_err();
        assert!(error.to_string().contains("expired"));

        let plan = issue(&mut issued, 2000, vec![item(None)]);
        let fingerprint = plan.fingerprint.clone();
        let error = take_issued(&mut issued, &plan, &fingerprint, 1999).unwrap_err();
        assert!(error.to_string().contains("future"));
    }

    #[test]
    fn unknown_fingerprint_is_refused() {
        let mut issued = IssuedPlans::new();
        let plan = issue(&mut issued, 1000, vec![item(None)]);
        assert!(take_issued(&mut issued, &plan, "not-a-real-fingerprint", 1001).is_err());
    }
}
