//! Per-directory subtree totals, computed once per complete scan and published with it.
//!
//! A hard-linked file counts once, under the directory of its lexicographically first path (the same
//! rule as `Index::status`). `protected` counts descendants (and the directory itself) that policy
//! would deny, so an apparently ordinary cache directory containing protected data is visible.
use crate::{Entry, Index, index::unsigned, platform, policy};

/// Newest modification time of a directory subtree, for an absolute path inside the scan root.
pub fn newest_at(index: &Index, root: &Path, absolute: &Path) -> Option<i64> {
    let relative = platform::raw(absolute.strip_prefix(root).ok()?).ok()?;
    index
        .subtree(&relative)
        .ok()
        .flatten()
        .map(|t| t.newest)
        .filter(|n| *n > 0)
}
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub allocated: u64,
    pub apparent: u64,
    pub files: u64,
    pub dirs: u64,
    pub protected: u64,
    /// Newest modification time (Unix seconds) anywhere in the subtree; 0 when unknown.
    #[serde(default)]
    pub newest: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Child {
    pub path: Vec<u8>,
    pub kind: String,
    pub allocated: u64,
    pub apparent: u64,
    pub files: u64,
    pub dirs: u64,
    /// Directories: protected/excluded entries inside. Files: 1 when excluded.
    pub protected: u64,
    /// Newest modification time (Unix seconds) in the subtree (or of the file).
    pub newest: i64,
}

fn parent_of(path: &[u8]) -> &[u8] {
    path.iter()
        .rposition(|b| *b == b'/')
        .map_or(&path[..0], |n| &path[..n])
}

fn bucket<'a>(map: &'a mut HashMap<Vec<u8>, Totals>, key: &[u8]) -> &'a mut Totals {
    // Look up by slice first so the common (existing) case allocates nothing.
    if !map.contains_key(key) {
        map.insert(key.to_vec(), Totals::default());
    }
    map.get_mut(key).expect("just inserted")
}

/// Totals identical to `compute`, accumulated entry by entry while a scan runs, so no later pass
/// over the stored rows is needed. A hard-linked file counts once, under its first path.
#[derive(Default)]
pub struct Aggregator {
    /// Whether entries carry real link counts. Windows enumeration does not read them (`links` is
    /// always 1), so hard-linked names cannot be recognised while walking.
    links_known: bool,
    totals: HashMap<Vec<u8>, Totals>,
    /// identity -> (first path, max apparent, max allocated, newest, bytes among non-excluded)
    links: HashMap<String, LinkedFile>,
    unique_apparent: u64,
    unique_allocated: u64,
}
struct LinkedFile {
    path: Vec<u8>,
    apparent: u64,
    allocated: u64,
    newest: i64,
    live: Option<(u64, u64)>,
}
impl Aggregator {
    pub fn new(links_known: bool) -> Self {
        let mut totals = HashMap::new();
        totals.insert(Vec::new(), Totals::default());
        Self {
            links_known,
            totals,
            ..Default::default()
        }
    }
    /// `protected` is `policy::hard_protection(..).is_some()`, evaluated by the caller (in parallel).
    pub fn add(&mut self, entry: &Entry, protected: bool) {
        let directory = entry.kind == "directory";
        let modified = entry.modified_ns / 1_000_000_000;
        if protected {
            bucket(&mut self.totals, &entry.parent).protected += 1;
            if directory {
                bucket(&mut self.totals, &entry.path).protected += 1;
            }
        }
        if !directory && entry.links > 1 {
            let live = entry
                .excluded
                .is_none()
                .then_some((entry.apparent, entry.allocated));
            self.links
                .entry(entry.identity.clone())
                .and_modify(|l| {
                    if entry.path < l.path {
                        l.path = entry.path.clone();
                    }
                    l.apparent = l.apparent.max(entry.apparent);
                    l.allocated = l.allocated.max(entry.allocated);
                    l.newest = l.newest.max(modified);
                    l.live = match (l.live, live) {
                        (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.max(b.1))),
                        (a, b) => a.or(b),
                    };
                })
                .or_insert(LinkedFile {
                    path: entry.path.clone(),
                    apparent: entry.apparent,
                    allocated: entry.allocated,
                    newest: modified,
                    live,
                });
            return;
        }
        self.count(
            &entry.path,
            directory,
            entry.apparent,
            entry.allocated,
            modified,
        );
        if entry.excluded.is_none() {
            self.unique_apparent += entry.apparent;
            self.unique_allocated += entry.allocated;
        }
    }
    fn count(
        &mut self,
        path: &[u8],
        directory: bool,
        apparent: u64,
        allocated: u64,
        modified: i64,
    ) {
        let t = bucket(&mut self.totals, parent_of(path));
        t.dirs += u64::from(directory);
        t.files += u64::from(!directory);
        let owner = if directory { path } else { parent_of(path) };
        let t = bucket(&mut self.totals, owner);
        t.allocated += allocated;
        t.apparent += apparent;
        t.newest = t.newest.max(modified);
    }
    /// Folded rollups plus the scan-wide unique (apparent, allocated) bytes of non-excluded entries.
    pub fn finish(mut self, index: &Index, scan_id: i64) -> Result<(u64, u64)> {
        let started = std::time::Instant::now();
        let shared = !self.links_known && shares_identity(index, scan_id)?;
        timing("shared-names check", started);
        if shared {
            // Several names of one file exist and the walk could not tell, so every name was counted.
            // Recompute sizes from the stored rows, which are grouped by identity. The protected
            // counts the walk already gathered do not depend on links and are kept, which avoids the
            // slow per-entry policy pass. Trees without hard links (the usual case) never reach this
            // and pay only the cheap query above.
            // The exact pass reads the whole entries table; a larger page cache keeps it from
            // re-reading pages. The usual small cache is restored afterwards, whatever happens.
            let normal: i64 = index
                .db
                .pragma_query_value(None, "cache_size", |r| r.get(0))?;
            index
                .db
                .pragma_update(None, "cache_size", EXACT_PASS_CACHE_KIB)?;
            let result = (|| -> Result<(u64, u64)> {
                let mut totals = sizes_by_identity(index, scan_id)?;
                timing("sizes by identity", started);
                for (path, counted) in &self.totals {
                    if counted.protected > 0 {
                        bucket(&mut totals, path).protected = counted.protected;
                    }
                }
                fold_and_store(index, scan_id, totals)?;
                timing("fold and store", started);
                let unique = unique_totals(index, scan_id)?;
                timing("unique totals", started);
                Ok(unique)
            })();
            let restored = index.db.pragma_update(None, "cache_size", normal);
            let unique = result?;
            restored?;
            return Ok(unique);
        }
        for (_, l) in std::mem::take(&mut self.links) {
            self.count(&l.path, false, l.apparent, l.allocated, l.newest);
            if let Some((a, b)) = l.live {
                self.unique_apparent += a;
                self.unique_allocated += b;
            }
        }
        let totals = std::mem::take(&mut self.totals);
        fold_and_store(index, scan_id, totals)?;
        Ok((self.unique_apparent, self.unique_allocated))
    }
}

/// Page cache (negative means KiB) used while the exact pass reads every stored entry: 128 MiB.
const EXACT_PASS_CACHE_KIB: i64 = -131_072;

/// `OMG_TIMINGS=1` prints cumulative rollup times to stderr (diagnostics only).
fn timing(phase: &str, started: std::time::Instant) {
    if std::env::var_os("OMG_TIMINGS").is_some() {
        eprintln!(
            "omg-timing rollup/{phase}: {:.1}s",
            started.elapsed().as_secs_f64()
        );
    }
}

/// Whether any non-directory identity is stored under more than one name.
fn shares_identity(index: &Index, scan_id: i64) -> Result<bool> {
    Ok(index
        .db
        .query_row(
            "SELECT 1 FROM entries WHERE scan_id=? AND kind<>'directory' GROUP BY identity HAVING count(*)>1 LIMIT 1",
            [scan_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

/// Scan-wide unique (apparent, allocated) bytes of non-excluded entries, from the stored rows.
fn unique_totals(index: &Index, scan_id: i64) -> Result<(u64, u64)> {
    Ok(index.db.query_row(
        "SELECT coalesce(sum(apparent),0),coalesce(sum(allocated),0) FROM (SELECT max(apparent) apparent,max(allocated) allocated FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)",
        [scan_id],
        |r| Ok((unsigned(r, 0)?, unsigned(r, 1)?)),
    )?)
}

/// Per-directory sizes, file and folder counts and newest times, with each identity counted once
/// under its lexicographically first path. Protected counts are not included.
fn sizes_by_identity(index: &Index, scan_id: i64) -> Result<HashMap<Vec<u8>, Totals>> {
    let mut totals: HashMap<Vec<u8>, Totals> = HashMap::new();
    bucket(&mut totals, b"");
    let mut statement = index.db.prepare(
        "SELECT min(path),max(apparent),max(allocated),kind,max(modified_ns) FROM entries WHERE scan_id=? AND path<>x'' GROUP BY identity",
    )?;
    let mut rows = statement.query([scan_id])?;
    while let Some(row) = rows.next()? {
        let path: Vec<u8> = row.get(0)?;
        let (apparent, allocated) = (unsigned(row, 1)?, unsigned(row, 2)?);
        let directory = row.get::<_, String>(3)? == "directory";
        let modified = row.get::<_, i64>(4)? / 1_000_000_000;
        // Derived from the path: bare columns are unreliable with several min/max aggregates.
        let t = bucket(&mut totals, parent_of(&path));
        t.dirs += u64::from(directory);
        t.files += u64::from(!directory);
        // Bytes go to the entry's own bucket for directories (the fold carries them up) and to
        // the parent for files, so nothing is counted twice.
        let owner = if directory {
            path.as_slice()
        } else {
            parent_of(&path)
        };
        let t = bucket(&mut totals, owner);
        t.allocated += allocated;
        t.apparent += apparent;
        t.newest = t.newest.max(modified);
    }
    Ok(totals)
}

/// Aggregate and insert rollups for `scan_id` from the stored rows (reference implementation;
/// scans use `Aggregator`). Each entry is added once to its parent's bucket;
/// buckets are then folded upward deepest-first, so work is O(entries + directories·log) and memory
/// is proportional to the number of directories.
pub fn compute(index: &Index, scan_id: i64, root: &Path) -> Result<()> {
    let mut totals = sizes_by_identity(index, scan_id)?;
    {
        // Every path (not deduplicated by identity): any protected link name protects its ancestors.
        let mut statement = index.db.prepare(
            "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND path<>x''",
        )?;
        let mut rows = statement.query([scan_id])?;
        // Hard protections only: repository age and build folders are judged at decision time.
        while let Some(row) = rows.next()? {
            let entry: Entry = crate::index::row_entry(row)?;
            if policy::hard_protection(root, &entry).is_none() {
                continue;
            }
            bucket(&mut totals, &entry.parent).protected += 1;
            if entry.kind == "directory" {
                bucket(&mut totals, &entry.path).protected += 1;
            }
        }
    }
    fold_and_store(index, scan_id, totals)
}

/// Fold each directory's subtree into its parent (deepest first) and insert `dir_totals` rows.
pub fn fold_and_store(
    index: &Index,
    scan_id: i64,
    mut totals: HashMap<Vec<u8>, Totals>,
) -> Result<()> {
    let mut order: Vec<Vec<u8>> = totals.keys().filter(|k| !k.is_empty()).cloned().collect();
    order.sort_by_key(|k| std::cmp::Reverse(k.iter().filter(|b| **b == b'/').count()));
    for key in &order {
        let child = totals.get(key).cloned().unwrap_or_default();
        let parent = bucket(&mut totals, parent_of(key));
        parent.allocated += child.allocated;
        parent.apparent += child.apparent;
        parent.files += child.files;
        parent.dirs += child.dirs;
        parent.protected += child.protected;
        parent.newest = parent.newest.max(child.newest);
    }
    let mut rows: Vec<_> = totals.into_iter().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for chunk in rows.chunks(5000) {
        // IMMEDIATE takes the write lock up front (and waits for it) instead of failing on upgrade.
        let tx = rusqlite::Transaction::new_unchecked(
            &index.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO dir_totals(scan_id,path,parent,allocated,apparent,files,dirs,protected,newest) VALUES(?,?,?,?,?,?,?,?,?)",
            )?;
            for (path, t) in chunk {
                insert.execute(params![
                    scan_id,
                    path,
                    parent_of(path),
                    i64::try_from(t.allocated)?,
                    i64::try_from(t.apparent)?,
                    i64::try_from(t.files)?,
                    i64::try_from(t.dirs)?,
                    i64::try_from(t.protected)?,
                    t.newest
                ])?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

impl Index {
    fn active_scan(&self) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()?)
    }
    /// True when the published scan has rollups (older indexes need one more `omg scan`).
    pub fn rollups_available(&self) -> Result<bool> {
        let Some(id) = self.active_scan()? else {
            return Ok(false);
        };
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM dir_totals WHERE scan_id=? AND path=x''",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some())
    }
    /// Totals for one directory (`""` is the scan root); `None` when unknown or rollups are unavailable.
    pub fn subtree(&self, path: &[u8]) -> Result<Option<Totals>> {
        let Some(id) = self.active_scan()? else {
            return Ok(None);
        };
        Ok(self
            .db
            .query_row(
                "SELECT allocated,apparent,files,dirs,protected,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND path=?",
                params![id, path],
                |r| {
                    Ok(Totals {
                        allocated: unsigned(r, 0)?,
                        apparent: unsigned(r, 1)?,
                        files: unsigned(r, 2)?,
                        dirs: unsigned(r, 3)?,
                        protected: unsigned(r, 4)?,
                        newest: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }
    /// Largest direct children (subdirectory subtrees and files) of a directory. The planner would
    /// otherwise pick the size index and scan every entry, so the parent index is forced.
    pub fn children(&self, path: &[u8], limit: usize, offset: usize) -> Result<Vec<Child>> {
        let Some(id) = self.active_scan()? else {
            return Ok(vec![]);
        };
        let mut statement = self.db.prepare(
            "SELECT path,'directory',allocated,apparent,files,dirs,protected,coalesce(newest,0) FROM dir_totals WHERE scan_id=?1 AND parent=?2 AND path<>x''
             UNION ALL
             SELECT path,kind,allocated,apparent,0,0,CASE WHEN excluded IS NULL THEN 0 ELSE 1 END,modified_ns/1000000000 FROM entries INDEXED BY entry_parent WHERE scan_id=?1 AND parent=?2 AND kind<>'directory'
             ORDER BY 3 DESC,1 LIMIT ?3 OFFSET ?4",
        )?;
        let rows = statement.query_map(
            params![id, path, limit.min(1000) as i64, i64::try_from(offset)?],
            |r| {
                Ok(Child {
                    path: r.get(0)?,
                    kind: r.get(1)?,
                    allocated: unsigned(r, 2)?,
                    apparent: unsigned(r, 3)?,
                    files: unsigned(r, 4)?,
                    dirs: unsigned(r, 5)?,
                    protected: unsigned(r, 6)?,
                    newest: r.get(7)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Relative scan-root path for an absolute path inside the root.
pub fn relative_raw(root: &Path, absolute: &Path) -> Option<Vec<u8>> {
    absolute
        .strip_prefix(root)
        .ok()
        .and_then(|p| platform::raw(p).ok())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{ScanOptions, scanner};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    };

    /// Windows cannot read link counts while walking. Feeding a real scan's entries with their
    /// counts hidden must give the same folder and scan totals as the Linux (counts known) path.
    #[test]
    fn totals_without_link_counts_match_totals_with_them() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir_all(root.path().join("a/deep")).unwrap();
        std::fs::create_dir(root.path().join("b")).unwrap();
        std::fs::write(root.path().join("a/deep/data.bin"), vec![3u8; 40_000]).unwrap();
        std::fs::hard_link(
            root.path().join("a/deep/data.bin"),
            root.path().join("b/alias.bin"),
        )
        .unwrap();
        std::fs::write(root.path().join("a/solo.bin"), vec![4u8; 5_000]).unwrap();
        // A protected name inside a folder with a shared file: protected counts must survive.
        std::fs::create_dir(root.path().join("b/.ssh")).unwrap();
        std::fs::write(root.path().join("b/.ssh/id"), b"key").unwrap();

        let mut index = Index::open(state.path()).unwrap();
        scanner::scan(
            &mut index,
            &ScanOptions {
                root: root.path().to_path_buf(),
                workers: 2,
                exclusions: vec![],
                full: true,
                publish_partial: false,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        )
        .unwrap();
        let scan_id: i64 = index
            .db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let stored = |index: &Index| -> Vec<(Vec<u8>, Totals)> {
            ["", "a", "a/deep", "b"]
                .iter()
                .map(|p| {
                    (
                        p.as_bytes().to_vec(),
                        index.subtree(p.as_bytes()).unwrap().unwrap(),
                    )
                })
                .collect()
        };
        let expected = stored(&index);
        let expected_unique = unique_totals(&index, scan_id).unwrap();

        // Rebuild the rollups as a platform without link counts would: every entry says links = 1.
        index
            .db
            .execute("DELETE FROM dir_totals WHERE scan_id=?", [scan_id])
            .unwrap();
        let mut aggregate = Aggregator::new(false);
        for mut entry in index.entries(1000, 0).unwrap() {
            entry.links = 1;
            let protected = policy::hard_protection(root.path(), &entry).is_some();
            aggregate.add(&entry, protected);
        }
        let unique = aggregate.finish(&index, scan_id).unwrap();
        assert_eq!(stored(&index), expected);
        assert_eq!(unique, expected_unique);
        // `.ssh` is excluded, so its contents are not indexed: two files remain, not three names.
        assert_eq!(expected[0].1.files, 2);
        assert!(
            expected[3].1.protected > 0,
            "the protected folder is still counted"
        );
    }

    #[test]
    fn a_tree_without_shared_names_takes_the_fast_path() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(root.path().join("one.bin"), b"1").unwrap();
        std::fs::write(root.path().join("two.bin"), b"22").unwrap();
        let mut index = Index::open(state.path()).unwrap();
        scanner::scan(
            &mut index,
            &ScanOptions {
                root: root.path().to_path_buf(),
                workers: 2,
                exclusions: vec![],
                full: true,
                publish_partial: false,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        )
        .unwrap();
        assert!(!shares_identity(&index, 1).unwrap());
    }
}
