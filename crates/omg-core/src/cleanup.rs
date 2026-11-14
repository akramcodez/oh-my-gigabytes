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
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
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

/// What removing a subtree frees, grouped by file identity: the name itself, then every row whose
/// parent is the subtree's folder or lies below it. The key range also holds siblings such as
/// `a.b` next to `a`, which the last test removes. A plan of many small pieces calls this once per
/// piece, so both halves must stay key lookups.
const FREED_SQL: &str = "SELECT kind,count(*),max(links),max(apparent),max(allocated),coalesce(max(CASE WHEN excluded IS NULL THEN apparent END),0),coalesce(max(CASE WHEN excluded IS NULL THEN allocated END),0) FROM (SELECT * FROM entries WHERE scan_id=?1 AND parent=?2 AND name=?3 UNION ALL SELECT * FROM entries WHERE scan_id=?1 AND parent>=?4 AND parent<?5 AND (parent=?4 OR parent>?6)) GROUP BY identity";

/// Removing a subtree from the index: the name, then the range below it, in each table. Never one
/// statement joined by OR: SQLite cannot use an index for that in a DELETE.
/// Used by the Linux and Windows delete path (macOS has none) and by the query-plan test.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const FORGET_ENTRY: &str = "DELETE FROM entries WHERE scan_id=?1 AND parent=?2 AND name=?3";
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const FORGET_ENTRIES_BELOW: &str = "DELETE FROM entries WHERE scan_id=?1 AND parent>=?2 AND parent<?3 AND (parent=?2 OR parent>?4)";
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const FORGET_TOTAL: &str = "DELETE FROM dir_totals WHERE scan_id=?1 AND path=?2";
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const FORGET_TOTALS_BELOW: &str = "DELETE FROM dir_totals WHERE scan_id=?1 AND path>?2 AND path<?3";

/// A path or name for a message: lossy, with control characters (terminal escapes) replaced.
fn printable(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Protected items looked for inside a folder when it is planned. More than this and the folder is
/// refused rather than split around them.
const PROTECTED_LIMIT: usize = 2_000;
/// Most separate pieces a folder may be split into when it is deleted around protected items.
const MAX_PIECES: usize = 5_000;

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
    /// Ids of cache folders that cannot be deleted whole because protected items are inside, but
    /// can be deleted around them (`prepare_delete_keeping_protected`). They are listed in `denied`.
    #[serde(default)]
    pub keepable: Vec<i64>,
}

/// Finding categories whose owner can recreate them or does not need them: scans suggest them and
/// `omg clean` may select them. Large files and downloaded models are never among them.
pub const SUGGESTED_CATEGORIES: [&str; 9] = [
    "package_caches",
    "build_artifacts",
    "app_caches",
    "old_tool_versions",
    "session_history",
    "unused_clones",
    "unused_apps",
    "unused_toolchains",
    "logs_and_temp",
];

/// Findings chosen by category and age for a scripted cleanup. The fingerprint covers every
/// finding id (each hashes the entry, its subtree totals, the rule and the catalog), so it only
/// matches again while the index and the selection are unchanged.
#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    pub fingerprint: String,
    pub bytes: u64,
    pub items: Vec<Selected>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Selected {
    pub path: String,
    #[serde(skip)]
    pub raw: Vec<u8>,
    pub category: String,
    pub rule: String,
    pub estimate: u64,
    /// Newest modification time inside (Unix seconds); 0 when unknown.
    pub last_changed: i64,
    pub warning: Option<String>,
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
    if cfg!(target_os = "macos")
        && let Ok(below) = absolute.strip_prefix(home.join("Library/Caches"))
        && below
            .components()
            .next()
            .is_some_and(|first| !crate::catalog::apple_cache(&first.as_os_str().to_string_lossy()))
    {
        return true;
    }
    crate::catalog::DISCOVERIES
        .iter()
        .filter_map(|kind| crate::catalog::discovery_root(kind, home))
        .chain(
            crate::catalog::toolchains(home)
                .into_iter()
                .flat_map(|t| t.roots),
        )
        .any(|root| absolute.starts_with(&root))
}

/// An application's own folder directly in a per-user data area: not a cache, so it stays an
/// ordinary folder (anything protected inside stops the deletion) and carries a warning.
fn app_data_folder(absolute: &Path, home: &Path) -> bool {
    absolute.parent().is_some_and(|parent| {
        crate::catalog::app_data_parents(home)
            .iter()
            .any(|p| p == parent)
    })
}

const APP_DATA: &str = "an application's own data: its settings, sign-ins and offline files go with it, and it starts fresh next time";

/// Live (identity, creation time) of a root-relative path, read without following links.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn live_identity(root: &Path, path: &[u8]) -> Result<(String, Option<String>)> {
    unix::live_identity(root, path)
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
    #[cfg(target_os = "macos")]
    {
        let fd = platform::macos::root(root)?;
        Ok(crate::capability::permanent_delete(
            platform::macos::filesystem(&fd)?.0,
        ))
    }
    #[cfg(windows)]
    {
        let dir = platform::windows::open_root(root)?;
        Ok(crate::capability::permanent_delete(
            platform::windows::filesystem_of(&dir)?,
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = root;
        Ok(crate::capability::Support::Unsupported)
    }
}

/// Revalidate and remove one item: (entries removed, entries that could not be removed, whether a
/// removed file had other names). Windows reads link counts here because its scan does not.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn remove_target(
    root: &platform::native::Dir,
    root_path: &Path,
    home: &Path,
    item: &Marked,
    kind: Kind,
) -> std::result::Result<(u64, u64, bool), String> {
    unix::delete_one(root, root_path, home, item, kind)
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

const ONE_AT_A_TIME: &str = "a whole project, session history or application data is never deleted with other items; delete it on its own";

/// Owner decisions 2026-11-07 and 2026-11-08: whole repositories, session transcripts and an
/// application's own data cannot be recreated by a tool, so they never ride along with other items.
fn one_at_a_time(root: &Path, home: &Path, entry: &Entry, kind: Kind) -> bool {
    if kind == Kind::StaleRepo {
        return true;
    }
    let Ok(relative) = platform::path(&entry.path) else {
        return true;
    };
    let absolute = root.join(relative);
    (absolute.starts_with(home.join(".claude/projects"))
        && absolute.extension().is_some_and(|e| e == "jsonl"))
        || (app_data_folder(&absolute, home) && !is_cache_root(&absolute, home))
}

/// A folder whose name alone proves nothing, next to the file that shows a tool owns it.
fn proven_build_output(absolute: &Path) -> bool {
    let beside = |file: &str| absolute.parent().is_some_and(|p| p.join(file).is_file());
    match absolute.file_name().map(|n| n.to_string_lossy()).as_deref() {
        Some("target") => beside("Cargo.toml"),
        Some(".venv" | "venv") => absolute.join("pyvenv.cfg").is_file(),
        _ => false,
    }
}

/// Folders macOS and its apps expect to exist. What is inside them is judged on its own.
fn standard_mac_folder(home: &Path, absolute: &Path) -> bool {
    let Ok(below) = absolute.strip_prefix(home) else {
        return false;
    };
    let below = below.to_string_lossy().to_ascii_lowercase();
    [
        "applications",
        "desktop",
        "documents",
        "downloads",
        "library",
        "movies",
        "music",
        "pictures",
        "public",
        "library/application support",
        "library/caches",
        "library/preferences",
    ]
    .contains(&below.as_str())
}

/// Whether macOS may keep this path in iCloud. A synced Desktop or Documents folder looks like any
/// other local folder from here, and removing a file from one removes it from iCloud too.
fn may_sync_with_icloud(home: &Path, absolute: &Path) -> bool {
    cfg!(target_os = "macos")
        && absolute
            .strip_prefix(home)
            .ok()
            .and_then(|below| below.components().next())
            .is_some_and(|first| {
                let first = first.as_os_str().to_string_lossy().to_ascii_lowercase();
                first == "desktop" || first == "documents"
            })
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
    if cfg!(target_os = "macos") && standard_mac_folder(home, &absolute) {
        return Err("a standard macOS folder; choose what is inside it instead".into());
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
    let cache = is_cache_root(&absolute, home) || proven_build_output(&absolute);
    let verdict = match policy::repository_status(root, entry, memo) {
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
                gate: if !cache && app_data_folder(&absolute, home) {
                    Gate::Warn(APP_DATA.into())
                } else {
                    Gate::Allowed
                },
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
    };
    if !may_sync_with_icloud(home, &absolute) {
        return verdict;
    }
    const ICLOUD: &str = "if Desktop and Documents sync with iCloud, this also removes it from iCloud and your other devices";
    let gate = match verdict.gate {
        Gate::Allowed => Gate::Warn(ICLOUD.into()),
        Gate::Warn(text) => Gate::Warn(format!("{text}; {ICLOUD}")),
        denied => denied,
    };
    Verdict { gate, ..verdict }
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
                "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                params![scan, crate::index::split_path(path).0, crate::index::split_path(path).1],
                crate::index::row_entry,
            )
            .optional()?)
    }
    /// What removing the subtree at `path` frees (see [`Freed`]). Hard links are grouped by identity:
    /// a group whose names are all inside the subtree is freed, a larger group is a survivor.
    fn freed(&self, scan: i64, path: &[u8]) -> Result<Freed> {
        let (prefix, upper) = descendants(path);
        let mut statement = self.db.prepare(FREED_SQL)?;
        let mut freed = Freed::default();
        let (parent, name) = crate::index::split_path(path);
        let mut rows = statement.query(params![scan, parent, name, path, upper, prefix])?;
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
    /// Findings of the given categories, not protected, last changed more than `older_than_days`
    /// ago when given. Selecting changes nothing.
    pub fn select_for_clean(
        &self,
        home: &Path,
        categories: &[String],
        older_than_days: Option<u64>,
    ) -> Result<Selection> {
        if categories.is_empty() {
            bail!(
                "name at least one --category: {}",
                SUGGESTED_CATEGORIES.join(", ")
            );
        }
        if let Some(unknown) = categories
            .iter()
            .find(|c| !SUGGESTED_CATEGORIES.contains(&c.as_str()))
        {
            bail!(
                "category {unknown:?} cannot be cleaned by a script; choose from {}",
                SUGGESTED_CATEGORIES.join(", ")
            );
        }
        let cutoff = match older_than_days {
            Some(days) => Some(now() - i64::try_from(days.min(36_500))? * 86_400),
            None => None,
        };
        let mut items = Vec::new();
        let mut ids = Vec::new();
        for finding in self.findings_in(1000, 0, false, home)? {
            if finding.effective_tier == "protected" || !categories.contains(&finding.rule.category)
            {
                continue;
            }
            let last_changed = match &finding.subtree {
                Some(t) => t.newest,
                None => finding.entry.modified_ns / 1_000_000_000,
            };
            if cutoff.is_some_and(|c| last_changed <= 0 || last_changed >= c) {
                continue;
            }
            ids.push(finding.id.clone());
            items.push(Selected {
                path: crate::display_path(&finding.entry.path),
                raw: finding.entry.path.clone(),
                category: finding.rule.category.clone(),
                rule: finding.rule.id.clone(),
                estimate: finding.estimate,
                last_changed,
                warning: finding
                    .reason
                    .split_once(" ⚠ ")
                    .map(|(_, why)| why.to_string()),
            });
        }
        ids.sort();
        Ok(Selection {
            fingerprint: crate::catalog::digest(ids.join("\n").as_bytes())[..24].to_string(),
            bytes: items.iter().map(|i| i.estimate).sum(),
            items,
        })
    }
    /// Delete a selection the user confirmed by its fingerprint: the items go to OMG Trash and
    /// through the same plan and checks as a deletion from the interface. Whole repositories and
    /// session transcripts are left for one-at-a-time review there.
    pub fn clean_selection(
        &mut self,
        home: &Path,
        selection: &Selection,
        confirmed: &str,
    ) -> Result<(DeletePlan, DeleteReport)> {
        if confirmed != selection.fingerprint {
            bail!(
                "the index or the selection changed since that fingerprint was printed; run the command again without --confirm to review"
            );
        }
        if selection.items.is_empty() {
            bail!("nothing matches; nothing to delete");
        }
        for item in &selection.items {
            self.mark(&item.raw, item.estimate, home)?;
        }
        let wanted: Vec<&[u8]> = selection.items.iter().map(|i| i.raw.as_slice()).collect();
        let ids: Vec<i64> = self
            .marked()?
            .into_iter()
            .filter(|m| wanted.contains(&m.path.as_slice()))
            .map(|m| m.id)
            .collect();
        let plan = self.prepare_delete(&ids, home)?;
        if plan.items.is_empty() {
            return Ok((plan, DeleteReport::default()));
        }
        let report = self.delete_permanently(&plan, &plan.fingerprint.clone(), home)?;
        Ok((plan, report))
    }
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
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
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
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
        let saved: Vec<(i64, Vec<u8>)> = {
            let mut statement = self.db.prepare(
                "SELECT id,path FROM marked WHERE source='auto' AND dismissed=0 AND root=?",
            )?;
            let rows = statement.query_map([&root_raw], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, path) in saved {
            let (parent, name) = crate::index::split_path(&path);
            let listed = self
                .db
                .prepare_cached("SELECT 1 FROM entries WHERE scan_id=? AND parent=? AND name=?")?
                .exists(params![scan, parent, name])?;
            if !listed {
                self.db.execute("DELETE FROM marked WHERE id=?", [id])?;
            }
        }
        let mut listed = 0;
        let mut suggested: Vec<Vec<u8>> = Vec::new();
        for finding in self.findings_in(1000, 0, false, home)? {
            // What its owner can recreate or does not need; never large files. Listing is not
            // deleting: every item still goes through a confirmed plan.
            if !SUGGESTED_CATEGORIES.contains(&finding.rule.category.as_str())
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
        // On unless the user turned it off (owner decision 2026-11-07). The list authorizes nothing:
        // every deletion still needs its own reviewed, confirmed plan.
        Ok(value.as_deref() != Some(b"off".as_slice()))
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
        let mut keepable: Vec<i64> = Vec::new();
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
                Some(entry) => {
                    let verdict = self.verdict_for(&entry, home)?;
                    let warning = match verdict.gate {
                        Gate::Denied(reason) => {
                            denied.push((item.path.clone(), reason));
                            continue;
                        }
                        Gate::Warn(text) => Some(text),
                        Gate::Allowed => None,
                    };
                    if ids.len() > 1 && one_at_a_time(&root, home, &entry, verdict.kind) {
                        denied.push((item.path.clone(), ONE_AT_A_TIME.into()));
                        continue;
                    }
                    // Protected names inside make the deletion refuse the whole folder. Say so now,
                    // not after the user has confirmed.
                    let inside = if entry.kind == "directory" {
                        self.sensitive_inside(scan, &entry.path, PROTECTED_LIMIT)?
                    } else {
                        vec![]
                    };
                    if let Some(first) = inside.first() {
                        let shown = printable(first);
                        if verdict.kind == Kind::Cache {
                            keepable.push(item.id);
                            let count = if inside.len() >= PROTECTED_LIMIT {
                                format!("{PROTECTED_LIMIT}+")
                            } else {
                                inside.len().to_string()
                            };
                            denied.push((
                                item.path.clone(),
                                format!(
                                    "contains {count} protected item(s), for example {shown}; it cannot be deleted whole, but everything else in it can"
                                ),
                            ));
                        } else {
                            denied.push((
                                item.path.clone(),
                                format!("contains a sensitive item ({shown})"),
                            ));
                        }
                        continue;
                    }
                    items.push(Marked {
                        warning,
                        ..item.clone()
                    });
                }
                None => denied.push((item.path.clone(), "no longer in the index".into())),
            }
        }
        self.issue(scan, items, denied, keepable)
    }
    /// Freeze `items` and `denied` as a plan and issue it to this session. Only bytes the plan would
    /// free are estimated: a hard link whose other name survives is not counted.
    fn issue(
        &mut self,
        scan: i64,
        items: Vec<Marked>,
        denied: Vec<(Vec<u8>, String)>,
        keepable: Vec<i64>,
    ) -> Result<DeletePlan> {
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
            keepable,
        };
        self.issued
            .retain(|_, (_, held)| now() - held.created <= PLAN_TTL);
        self.issued.insert(fingerprint, (scan, plan.clone()));
        Ok(plan)
    }
    /// Entries strictly inside the directory `path` whose name is one of `policy::ALWAYS_SENSITIVE`
    /// (the names the deletion itself refuses to remove anywhere inside a folder), from the index.
    fn sensitive_inside(&self, scan: i64, path: &[u8], limit: usize) -> Result<Vec<Vec<u8>>> {
        use rusqlite::types::Value;
        let (prefix, upper) = descendants(path);
        let tests = (0..policy::ALWAYS_SENSITIVE.len())
            .map(|n| format!("CAST(parent||name AS TEXT) LIKE ?{}", n + 6))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut values = vec![
            Value::Integer(scan),
            Value::Blob(path.to_vec()),
            Value::Blob(upper),
            Value::Integer(i64::try_from(limit)?),
            Value::Blob(prefix),
        ];
        values.extend(
            policy::ALWAYS_SENSITIVE
                .iter()
                .map(|name| Value::Text(format!("%/{name}"))),
        );
        let mut statement = self.db.prepare(&format!(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent>=?2 AND parent<?3 AND (parent=?2 OR parent>?5) AND ({tests}) LIMIT ?4"
        ))?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    /// What the index lists directly inside the directory `parent`.
    fn children_entries(&self, scan: i64, parent: &[u8]) -> Result<Vec<Entry>> {
        let mut statement = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=?",
        )?;
        let rows = statement.query_map(params![scan, parent], crate::index::row_entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    /// Plan the deletion of cache folders around the protected items inside them: everything except
    /// those items and the folders on the way to them. A folder is split into ordinary pieces (files
    /// and folders with nothing protected inside) that are validated and removed exactly like any
    /// other item at deletion time; a protected item is never a piece, and one that appears after
    /// the review still stops the piece that contains it. Only cache and build folders qualify:
    /// splitting a user's ordinary folder would leave it half deleted.
    pub fn prepare_delete_keeping_protected(
        &mut self,
        ids: &[i64],
        home: &Path,
    ) -> Result<DeletePlan> {
        if ids.is_empty() || ids.len() > 100 {
            bail!("select 1–100 folders in OMG Trash");
        }
        let (scan, root, _) = self.scope()?;
        if delete_support(&root)? == crate::capability::Support::Unsupported {
            bail!("permanent deletion is not available in this build or on this filesystem");
        }
        let all = self.marked()?;
        let mut items: Vec<Marked> = Vec::new();
        let mut denied: Vec<(Vec<u8>, String)> = Vec::new();
        let mut pieces: Vec<Entry> = Vec::new();
        let mut kept: Vec<Vec<u8>> = Vec::new();
        for id in ids {
            let Some(item) = all.iter().find(|m| m.id == *id) else {
                bail!("item {id} is no longer in OMG Trash");
            };
            let Some(entry) = self.entry(&item.path)? else {
                denied.push((item.path.clone(), "no longer in the index".into()));
                continue;
            };
            let verdict = self.verdict_for(&entry, home)?;
            if let Gate::Denied(reason) = verdict.gate {
                denied.push((item.path.clone(), reason));
                continue;
            }
            if verdict.kind != Kind::Cache || entry.kind != "directory" {
                denied.push((
                    item.path.clone(),
                    "only cache and build folders can be deleted around protected items".into(),
                ));
                continue;
            }
            let hits = self.sensitive_inside(scan, &entry.path, PROTECTED_LIMIT)?;
            if hits.len() >= PROTECTED_LIMIT {
                denied.push((
                    item.path.clone(),
                    "too many protected items inside to keep them all safely".into(),
                ));
                continue;
            }
            if hits.is_empty() {
                // Nothing to keep: the folder goes as a whole, like any other item.
                if item.birth.is_none() {
                    denied.push((item.path.clone(), UNRECORDED_BIRTH.into()));
                } else {
                    items.retain(|i| !policy::inside(&i.path, &item.path, true));
                    items.push(item.clone());
                }
                continue;
            }
            // The folders on the way to a protected item stay; so does the item. Everything else
            // beside them is a piece.
            let mut ancestors: std::collections::HashSet<Vec<u8>> = Default::default();
            for hit in &hits {
                let mut above = hit.as_slice();
                while let Some(end) = above.iter().rposition(|b| *b == b'/') {
                    above = &above[..end];
                    if above.len() < entry.path.len() {
                        break;
                    }
                    ancestors.insert(above.to_vec());
                    if above.len() == entry.path.len() {
                        break;
                    }
                }
            }
            let protected: std::collections::HashSet<&[u8]> =
                hits.iter().map(Vec::as_slice).collect();
            let mut stack = vec![entry.path.clone()];
            while let Some(dir) = stack.pop() {
                for child in self.children_entries(scan, &dir)? {
                    if protected.contains(child.path.as_slice()) {
                        kept.push(child.path);
                    } else if ancestors.contains(&child.path) {
                        stack.push(child.path);
                    } else {
                        pieces.push(child);
                    }
                    if pieces.len() > MAX_PIECES {
                        bail!(
                            "{} would be split into more than {MAX_PIECES} pieces; delete smaller folders instead",
                            printable(&item.path)
                        );
                    }
                }
            }
        }
        for (position, piece) in pieces.into_iter().enumerate() {
            let warning = match self.verdict_for(&piece, home)?.gate {
                Gate::Denied(reason) => {
                    denied.push((piece.path.clone(), reason));
                    continue;
                }
                Gate::Warn(text) => Some(text),
                Gate::Allowed => None,
            };
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            let birth = match live_identity(&root, &piece.path) {
                Ok((identity, Some(created))) if identity == piece.identity => Some(created),
                Ok((identity, _)) if identity != piece.identity => {
                    denied.push((
                        piece.path.clone(),
                        "changed on disk since the last scan; rescan first".into(),
                    ));
                    continue;
                }
                _ => {
                    denied.push((piece.path.clone(), UNRECORDED_BIRTH.into()));
                    continue;
                }
            };
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
            let birth: Option<String> = {
                let _ = &root;
                None
            };
            let allocated = if piece.kind == "directory" {
                self.subtree(&piece.path)?
                    .map_or(piece.allocated, |t| t.allocated)
            } else {
                piece.allocated
            };
            items.push(Marked {
                // Pieces are not saved in OMG Trash; a negative id cannot be one that is.
                id: -(i64::try_from(position)?) - 1,
                path: piece.path,
                identity: piece.identity,
                kind: piece.kind,
                allocated,
                added: now(),
                warning,
                birth,
                source: "keep-protected".into(),
            });
        }
        for path in kept {
            let name = printable(path.rsplit(|b| *b == b'/').next().unwrap_or(&path));
            denied.push((path, format!("kept: protected name ({name})")));
        }
        self.issue(scan, items, denied, vec![])
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
        // Housekeeping lets go of this lock every few hundred milliseconds, so a short wait gets in;
        // a scan that is really running still stops the delete.
        let _scan = crate::index::lock_wait(&self.state, "scan.lock", crate::index::LOCK_WAIT)
            .map_err(|_| {
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
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
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
                            // The outcome, the index update and the OMG Trash entry change together,
                            // in one commit (one flush to disk per target, not three).
                            let tx = self.db.unchecked_transaction()?;
                            Self::set_target(&tx, operation, item.id, "deleted", "")?;
                            self.forget_in(&tx, scan, &item.path, &freed)?;
                            tx.execute("DELETE FROM marked WHERE id=?", [item.id])?;
                            tx.commit()?;
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
            let volume = platform::native::volume(&root_dir).ok();
            let after = volume.map(|v| v.1);
            report.observed_free_change = match (before, after) {
                (Some(b), Some(a)) => Some(a as i64 - b as i64),
                _ => None,
            };
            if let Some((capacity, available)) = volume
                && (!report.deleted.is_empty() || report.removed_entries > 0)
            {
                // Trend history is advisory: failing to record it never fails a finished delete.
                let _ = self.record_delete_sample(scan, capacity, available);
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
        Self::set_target(&self.db, operation, item, outcome, detail)
    }
    fn set_target(
        db: &rusqlite::Connection,
        operation: i64,
        item: i64,
        outcome: &str,
        detail: &str,
    ) -> Result<()> {
        db.execute(
            "UPDATE delete_targets SET outcome=?,detail=?,updated=? WHERE operation=? AND item=?",
            params![outcome, detail, now(), operation, item],
        )?;
        Ok(())
    }
    /// Record the state after a delete as a trend sample, so the growth numbers follow what was
    /// deleted without a rescan. The indexed total is the scan's own (kept current by `forget`), so
    /// this reads one row; the volume numbers are the live ones. An unknown total records nothing.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn record_delete_sample(&self, scan: i64, capacity: u64, available: u64) -> Result<()> {
        let row: Option<(Vec<u8>, String, Option<i64>)> = self
            .db
            .query_row(
                "SELECT root,state,unique_allocated FROM scans WHERE id=?",
                [scan],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((root, state, Some(indexed))) = row else {
            return Ok(());
        };
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO volume_samples(at,root,capacity,available,indexed_allocated,scan_state) VALUES(?,?,?,?,?,?)",
            params![
                now(),
                root,
                i64::try_from(capacity)?,
                i64::try_from(available)?,
                indexed,
                format!("{state}+delete")
            ],
        )?;
        let sample = tx.last_insert_rowid();
        if state == "complete" {
            for (path, bytes) in self.top_directories(scan)? {
                tx.execute(
                    "INSERT INTO growth_snapshots VALUES(?,?,?)",
                    params![sample, path, i64::try_from(bytes)?],
                )?;
            }
        }
        tx.commit()?;
        self.prune_samples(
            crate::service::trend::DEFAULT_SAMPLE_DAYS,
            crate::service::trend::DEFAULT_MAX_SAMPLES,
        )
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    /// Drop a deleted subtree from the published index and subtract what it freed from ancestor
    /// totals, so the views stay truthful without a full rescan. Hard-linked data that survives
    /// outside the subtree stays counted (see [`Freed`]).
    fn forget_in(
        &self,
        tx: &rusqlite::Connection,
        scan: i64,
        path: &[u8],
        freed: &Freed,
    ) -> Result<()> {
        let (prefix, upper) = descendants(path);
        // Protection counts are per name, and every name in the subtree is removed with it.
        let protected = self.subtree(path)?.map_or(0, |t| t.protected);
        // The name itself and the range below it are separate statements: joined by OR, SQLite
        // filters every row of the scan for each target (about 0.2 s each on a 1.3M-entry index).
        let (parent, name) = crate::index::split_path(path);
        let removed = tx.execute(FORGET_ENTRY, params![scan, parent, name])?
            + tx.execute(FORGET_ENTRIES_BELOW, params![scan, path, upper, prefix])?;
        tx.execute(FORGET_TOTAL, params![scan, path])?;
        tx.execute(FORGET_TOTALS_BELOW, params![scan, prefix, upper])?;
        // Items saved in OMG Trash inside what was just deleted are gone with it.
        tx.execute(
            "DELETE FROM marked WHERE root=(SELECT root FROM scans WHERE id=?1) AND path=?2",
            params![scan, path],
        )?;
        tx.execute(
            "DELETE FROM marked WHERE root=(SELECT root FROM scans WHERE id=?1) AND path>?2 AND path<?3",
            params![scan, prefix, upper],
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
            // A saved folder that lost only part of its contents shows its new size.
            tx.execute(
                "UPDATE marked SET allocated=coalesce((SELECT allocated FROM dir_totals WHERE scan_id=?1 AND path=?2),allocated) WHERE root=(SELECT root FROM scans WHERE id=?1) AND path=?2",
                params![scan, ancestor],
            )?;
            if ancestor.is_empty() {
                break;
            }
        }
        tx.execute(
            "UPDATE scans SET entry_count=max(0,entry_count-?2),unique_allocated=max(0,unique_allocated-?3),unique_apparent=max(0,unique_apparent-?4) WHERE id=?1",
            params![scan, removed as i64, i64::try_from(freed.unique_allocated)?, i64::try_from(freed.unique_apparent)?],
        )?;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    #[cfg(target_os = "linux")]
    use crate::platform::linux as os;
    #[cfg(target_os = "macos")]
    use crate::platform::macos as os;
    use rustix::fd::OwnedFd;
    use rustix::fs::{self, AtFlags, Dir, Mode, OFlags};
    use std::ffi::CString;

    const MAX_ENTRIES: u64 = 5_000_000;

    /// The device number as the scanner records it (`st_dev` is signed on macOS).
    #[cfg(target_os = "linux")]
    fn device(stat: &fs::Stat) -> u64 {
        stat.st_dev
    }
    #[cfg(target_os = "macos")]
    fn device(stat: &fs::Stat) -> u64 {
        u64::from(stat.st_dev as u32)
    }

    fn identity(stat: &fs::Stat) -> String {
        format!("{}:{}", device(stat), stat.st_ino)
    }

    #[cfg(target_os = "linux")]
    fn hazard(_: &fs::Stat) -> Option<&'static str> {
        None
    }
    #[cfg(target_os = "macos")]
    fn hazard(stat: &fs::Stat) -> Option<&'static str> {
        os::removal_hazard(stat.st_flags)
    }

    #[cfg(target_os = "macos")]
    fn birth_of(dir: &OwnedFd, name: &CString) -> Option<String> {
        let stat = fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).ok()?;
        Some(format!("{}.{}", stat.st_birthtime, stat.st_birthtime_nsec))
    }
    #[cfg(target_os = "linux")]
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
        let above = relative.parent().unwrap_or(Path::new(""));
        #[cfg(target_os = "linux")]
        let parent = os::acquire(root_fd, above, true);
        #[cfg(target_os = "macos")]
        let parent = os::acquire(root_fd, above);
        let parent = parent
            .map_err(|_| "parent folder moved, redirected or became inaccessible".to_string())?;
        let cname = CString::new(platform::raw(Path::new(&name)).map_err(|e| e.to_string())?)
            .map_err(|_| "invalid name".to_string())?;
        Ok((parent, cname))
    }

    /// Live (identity, birth time) of a root-relative path, without following links.
    pub fn live_identity(root: &Path, path: &[u8]) -> Result<(String, Option<String>)> {
        let root_fd = os::root(root)?;
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
        if let Some(reason) = hazard(&stat) {
            return Err(format!("this is {reason}"));
        }
        let fresh = os::entry(&relative, &stat);
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
        check_tree(&dir, root, &relative, device(&opened), kind, &mut seen)?;
        let (removed, failed) = remove_tree(&dir, device(&opened));
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
            let child = relative.join(os::name(&name));
            if let Some(reason) = hazard(&stat) {
                return Err(format!("contains {reason} ({})", child.display()));
            }
            let mut entry = os::entry(&child, &stat);
            // Links and special files are unlinked, never followed; check their names like files.
            if entry.kind == "symlink" || entry.kind == "special" {
                entry.kind = "file".into();
            }
            let name_text = os::name(&name).to_string_lossy().into_owned();
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
                if device(&stat) != dev {
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
            // Checked above, but something may have changed since; never remove one regardless.
            if hazard(&stat).is_some() {
                failed += 1;
                continue;
            }
            if fs::FileType::from_raw_mode(stat.st_mode) == fs::FileType::Directory {
                if device(&stat) != dev {
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
                            .is_ok_and(|s| s.st_ino == stat.st_ino && device(&s) == dev) =>
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
            TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
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

    /// Who "you" are for ownership: the token's user and its default owner, each held in an
    /// 8-byte-aligned buffer. An elevated administrator's new files are owned by the
    /// Administrators group (the token's default owner), not by the user, so both count. This is
    /// the Windows form of Linux's `st_uid == euid`.
    struct CurrentUser {
        user: Vec<u64>,
        owner: Vec<u64>,
    }

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
            Ok(Self {
                user: token_info(token.0, TokenUser)?,
                owner: token_info(token.0, TokenOwner)?,
            })
        }
        fn sids(&self) -> [PSID; 2] {
            // SAFETY: the buffers hold a `TOKEN_USER` and a `TOKEN_OWNER` written by
            // `GetTokenInformation`, and stay alive as long as `self`.
            unsafe {
                [
                    (*self.user.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                    (*self.owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
                ]
            }
        }
    }

    fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
        let mut needed = 0u32;
        // SAFETY: a size query with no buffer; it only writes `needed`.
        unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut needed) };
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        // SAFETY: the buffer is writable for its full length in bytes.
        let ok = unsafe {
            GetTokenInformation(
                token,
                class,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buffer)
    }

    /// Whether the object's owner is you (see [`CurrentUser`]). Needs `READ_CONTROL` on the handle.
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
        let same = user
            .sids()
            .into_iter()
            .any(|sid| unsafe { EqualSid(owner, sid) } != 0);
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
            keepable: vec![],
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

    #[cfg(unix)]
    #[test]
    fn freed_space_query_walks_only_the_subtree() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let index = Index::open(directory.path()).unwrap();
        let mut statement = index
            .db
            .prepare(&format!("EXPLAIN QUERY PLAN {FREED_SQL}"))
            .unwrap();
        let plan: Vec<String> = statement
            .query_map(
                params![
                    1i64,
                    b"".as_slice(),
                    b"a".as_slice(),
                    b"a".as_slice(),
                    b"a0".as_slice(),
                    b"a/".as_slice()
                ],
                |r| r.get(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" | ");
        assert!(plan.contains("scan_id=? AND parent=? AND name=?"), "{plan}");
        assert!(
            plan.contains("scan_id=? AND parent>? AND parent<?"),
            "{plan}"
        );
        assert!(!plan.contains("SCAN entries"), "{plan}");
        // Forgetting a deleted subtree must use the key too, not filter the whole scan.
        for (sql, arguments) in [
            (FORGET_ENTRY, 3),
            (FORGET_ENTRIES_BELOW, 4),
            (FORGET_TOTAL, 2),
            (FORGET_TOTALS_BELOW, 3),
        ] {
            let mut statement = index
                .db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let values: Vec<rusqlite::types::Value> =
                std::iter::once(rusqlite::types::Value::Integer(1))
                    .chain((1..arguments).map(|_| rusqlite::types::Value::Blob(b"a".to_vec())))
                    .collect();
            let plan: Vec<String> = statement
                .query_map(rusqlite::params_from_iter(values), |r| r.get(3))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let plan = plan.join(" | ");
            assert!(
                plan.contains("scan_id=? AND path") || plan.contains("scan_id=? AND parent"),
                "{sql}: {plan}"
            );
        }
    }

    #[test]
    fn unknown_fingerprint_is_refused() {
        let mut issued = IssuedPlans::new();
        let plan = issue(&mut issued, 1000, vec![item(None)]);
        assert!(take_issued(&mut issued, &plan, "not-a-real-fingerprint", 1001).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_subtree_means_its_name_and_what_is_below_it_never_a_sibling() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let index = Index::open(directory.path()).unwrap();
        index
            .db
            .execute_batch(
                "INSERT INTO scans(id,root,started,state,mode) VALUES(1,x'2f',0,'complete','full')",
            )
            .unwrap();
        // `a b`, `a-b` and `a.b` sort between `a` and `a/`, so they sit inside the key range.
        let rows: [(&[u8], &str, i64); 8] = [
            (b"a", "directory", 1),
            (b"a/file", "file", 10),
            (b"a/deep", "directory", 100),
            (b"a/deep/credentials", "file", 1000),
            (b"a b", "directory", 10_000),
            (b"a b/credentials", "file", 100_000),
            (b"a.b/credentials", "file", 1_000_000),
            (b"a0", "file", 10_000_000),
        ];
        for (n, (path, kind, size)) in rows.iter().enumerate() {
            let (parent, name) = crate::index::split_path(path);
            index
                .db
                .execute(
                    "INSERT INTO entries VALUES(1,?,?,?,?,?,?,1,0,0,NULL)",
                    params![parent, name, kind, format!("1:{n}"), size, size],
                )
                .unwrap();
        }
        let freed = index.freed(1, b"a").unwrap();
        assert_eq!(freed.allocated, 1111);
        assert_eq!(freed.files, 2);
        assert_eq!(freed.dirs, 2);
        assert_eq!(
            index.sensitive_inside(1, b"a", 10).unwrap(),
            vec![b"a/deep/credentials".to_vec()]
        );
        // Children of a folder come back as complete paths.
        let children: Vec<Vec<u8>> = index
            .children_entries(1, b"a")
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(children, vec![b"a/deep".to_vec(), b"a/file".to_vec()]);
    }
}
