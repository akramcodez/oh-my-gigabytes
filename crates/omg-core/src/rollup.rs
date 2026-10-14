//! Per-directory subtree totals, computed once per complete scan and published with it.
//!
//! A hard-linked file counts once, under the directory of its lexicographically first path (the same
//! rule as `Index::status`). `protected` counts descendants (and the directory itself) that policy
//! would deny, so an apparently ordinary cache directory containing protected data is visible.
use crate::{Entry, Index, index::unsigned, platform, policy};
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

/// Aggregate and insert rollups for `scan_id`. Each entry is added once to its parent's bucket;
/// buckets are then folded upward deepest-first, so work is O(entries + directories·log) and memory
/// is proportional to the number of directories.
pub fn compute(index: &Index, scan_id: i64, root: &Path) -> Result<()> {
    let mut totals: HashMap<Vec<u8>, Totals> = HashMap::new();
    bucket(&mut totals, b"");
    {
        let mut statement = index.db.prepare(
            "SELECT min(path),max(apparent),max(allocated),kind FROM entries WHERE scan_id=? AND path<>x'' GROUP BY identity",
        )?;
        let mut rows = statement.query([scan_id])?;
        while let Some(row) = rows.next()? {
            let path: Vec<u8> = row.get(0)?;
            let (apparent, allocated) = (unsigned(row, 1)?, unsigned(row, 2)?);
            let directory = row.get::<_, String>(3)? == "directory";
            // Derived from the path: bare columns are unreliable with several min/max aggregates.
            let t = bucket(&mut totals, parent_of(&path));
            t.dirs += u64::from(directory);
            t.files += u64::from(!directory);
            // Bytes go to the entry's own bucket for directories (the fold below carries them up)
            // and to the parent for files, so nothing is counted twice.
            let owner = if directory {
                path.as_slice()
            } else {
                parent_of(&path)
            };
            let t = bucket(&mut totals, owner);
            t.allocated += allocated;
            t.apparent += apparent;
        }
    }
    {
        // Every path (not deduplicated by identity): any protected link name protects its ancestors.
        let mut statement = index.db.prepare(
            "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND path<>x''",
        )?;
        let mut rows = statement.query([scan_id])?;
        let mut memo = policy::RepositoryMemo::default();
        while let Some(row) = rows.next()? {
            let entry: Entry = crate::index::row_entry(row)?;
            if policy::protection_with(root, &entry, &mut memo).is_none() {
                continue;
            }
            bucket(&mut totals, &entry.parent).protected += 1;
            if entry.kind == "directory" {
                bucket(&mut totals, &entry.path).protected += 1;
            }
        }
    }
    // Fold each directory's subtree into its parent, deepest first.
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
            let mut insert = tx.prepare("INSERT INTO dir_totals VALUES(?,?,?,?,?,?,?,?)")?;
            for (path, t) in chunk {
                insert.execute(params![
                    scan_id,
                    path,
                    parent_of(path),
                    i64::try_from(t.allocated)?,
                    i64::try_from(t.apparent)?,
                    i64::try_from(t.files)?,
                    i64::try_from(t.dirs)?,
                    i64::try_from(t.protected)?
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
                "SELECT allocated,apparent,files,dirs,protected FROM dir_totals WHERE scan_id=? AND path=?",
                params![id, path],
                |r| {
                    Ok(Totals {
                        allocated: unsigned(r, 0)?,
                        apparent: unsigned(r, 1)?,
                        files: unsigned(r, 2)?,
                        dirs: unsigned(r, 3)?,
                        protected: unsigned(r, 4)?,
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
            "SELECT path,'directory',allocated,apparent,files,dirs,protected FROM dir_totals WHERE scan_id=?1 AND parent=?2 AND path<>x''
             UNION ALL
             SELECT path,kind,allocated,apparent,0,0,CASE WHEN excluded IS NULL THEN 0 ELSE 1 END FROM entries INDEXED BY entry_parent WHERE scan_id=?1 AND parent=?2 AND kind<>'directory'
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
