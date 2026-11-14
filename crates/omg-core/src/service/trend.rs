//! Bounded local volume samples and explainable growth between scans. Observed changes are
//! labelled as observations: volume movement includes activity outside the scanned root.
use crate::{Index, index::unsigned, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;

pub const DEFAULT_SAMPLE_DAYS: u64 = 365;
pub const DEFAULT_MAX_SAMPLES: u64 = 4000;
const SNAPSHOT_PATHS: usize = 50;
/// Folders kept per size snapshot (the largest first) and the smallest one kept.
const SIZE_PATHS: i64 = 2000;
const SIZE_MIN_BYTES: i64 = 1 << 20;
/// Growth smaller than this is not reported.
const GREW_MIN_BYTES: i64 = 1 << 20;
const PRUNE_BATCH: i64 = 500;

#[derive(Debug, Serialize)]
pub struct Grower {
    pub path: Vec<u8>,
    pub allocated: u64,
    /// `None` when the path was not in the earlier top-N list, so its earlier size is unknown.
    pub change: Option<i64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Grown {
    pub path: Vec<u8>,
    pub allocated: u64,
    pub change: i64,
    /// Not among the earlier scan's folders of 1 MB or more: the change counts it from zero.
    pub new: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct GrewReport {
    /// Times of the two complete scans compared; `None` until there are two.
    pub since: Option<i64>,
    pub at: Option<i64>,
    pub total_change: Option<i64>,
    pub items: Vec<Grown>,
}
#[derive(Debug, Serialize)]
pub struct TrendReport {
    pub samples: u64,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
    pub available_change: Option<i64>,
    pub indexed_allocated_change: Option<i64>,
    pub growers: Vec<Grower>,
    pub note: &'static str,
}

impl Index {
    /// Record the scan's volume sample (complete and partial scans only) and prune old samples.
    pub fn record_sample(
        &self,
        scan_id: i64,
        scan_state: &str,
        max_age_days: u64,
        max_samples: u64,
    ) -> Result<Option<i64>> {
        if !matches!(scan_state, "complete" | "partial") {
            return Ok(None);
        }
        let row: Option<(Vec<u8>, Option<i64>, Option<i64>)> = self
            .db
            .query_row(
                "SELECT root,capacity,available FROM scans WHERE id=?",
                [scan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((root, Some(capacity), Some(available))) = row else {
            return Ok(None);
        };
        let allocated: i64 = self.db.query_row(
            "SELECT coalesce(sum(a),0) FROM (SELECT max(allocated) a FROM entries WHERE scan_id=? AND excluded IS NULL GROUP BY identity)",
            [scan_id],
            |r| r.get(0),
        )?;
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO volume_samples(at,root,capacity,available,indexed_allocated,scan_state) VALUES(?,?,?,?,?,?)",
            params![now(), root, capacity, available, allocated, scan_state],
        )?;
        let sample = tx.last_insert_rowid();
        if scan_state == "complete" {
            for (path, bytes) in self.top_directories(scan_id)? {
                tx.execute(
                    "INSERT INTO growth_snapshots VALUES(?,?,?)",
                    params![sample, path, i64::try_from(bytes)?],
                )?;
            }
            tx.execute(
                "INSERT INTO size_snapshots(at,root,indexed_allocated) VALUES(?,?,?)",
                params![now(), root, allocated],
            )?;
            let snapshot = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO size_snapshot_paths SELECT ?1,path,allocated FROM dir_totals WHERE scan_id=?2 AND path<>x'' AND allocated>=?3 ORDER BY allocated DESC LIMIT ?4",
                params![snapshot, scan_id, SIZE_MIN_BYTES, SIZE_PATHS],
            )?;
            tx.execute(
                "DELETE FROM size_snapshots WHERE root=?1 AND id NOT IN (SELECT id FROM size_snapshots WHERE root=?1 ORDER BY id DESC LIMIT 2)",
                [&root],
            )?;
        }
        tx.commit()?;
        self.prune_samples(max_age_days, max_samples)?;
        Ok(Some(sample))
    }

    /// Largest depth-1 and depth-2 directories under the scan root, from the published rollups.
    pub(crate) fn top_directories(&self, scan_id: i64) -> Result<Vec<(Vec<u8>, u64)>> {
        let mut statement = self.db.prepare(
            "SELECT path,allocated FROM dir_totals WHERE scan_id=? AND path<>x'' ORDER BY allocated DESC,path",
        )?;
        let mut rows = statement.query([scan_id])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            let path: Vec<u8> = row.get(0)?;
            if path.iter().filter(|b| **b == b'/').count() < 2 {
                list.push((path, unsigned(row, 1)?));
                if list.len() == SNAPSHOT_PATHS {
                    break;
                }
            }
        }
        Ok(list)
    }

    /// What grew between the last two complete scans of the published root. A folder is left out
    /// when one inside it accounts for most of its growth, so the list names where it happened.
    pub fn grew(&self, limit: usize) -> Result<GrewReport> {
        let mut statement = self.db.prepare(
            "SELECT id,at FROM size_snapshots WHERE root=(SELECT root FROM scans WHERE id=(SELECT scan_id FROM active WHERE singleton=1)) ORDER BY id DESC LIMIT 2",
        )?;
        let snapshots: Vec<(i64, i64)> = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let indexed = |id: i64| -> Result<Option<i64>> {
            Ok(self.db.query_row(
                "SELECT indexed_allocated FROM size_snapshots WHERE id=?",
                [id],
                |r| r.get(0),
            )?)
        };
        let [(newer, at), (older, since)] = snapshots[..] else {
            return Ok(GrewReport {
                since: None,
                at: None,
                total_change: None,
                items: vec![],
            });
        };
        let sizes = |id: i64| -> Result<HashMap<Vec<u8>, u64>> {
            let mut statement = self
                .db
                .prepare("SELECT path,allocated FROM size_snapshot_paths WHERE snapshot=?")?;
            Ok(statement
                .query_map([id], |r| Ok((r.get::<_, Vec<u8>>(0)?, unsigned(r, 1)?)))?
                .collect::<rusqlite::Result<_>>()?)
        };
        let (before, after) = (sizes(older)?, sizes(newer)?);
        let total = match (indexed(older)?, indexed(newer)?) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        };
        let mut grown: Vec<Grown> = Vec::new();
        for (path, allocated) in after {
            let old = before.get(&path).copied();
            let change = i64::try_from(allocated)? - i64::try_from(old.unwrap_or(0))?;
            if change >= GREW_MIN_BYTES {
                grown.push(Grown {
                    path,
                    allocated,
                    change,
                    new: old.is_none(),
                });
            }
        }
        let explained: Vec<bool> = grown
            .iter()
            .map(|outer| {
                grown.iter().any(|inner| {
                    inner.path.len() > outer.path.len()
                        && inner.path.starts_with(&outer.path)
                        && inner.path[outer.path.len()] == b'/'
                        && inner.change * 5 >= outer.change * 4
                })
            })
            .collect();
        let mut items: Vec<Grown> = grown
            .into_iter()
            .zip(explained)
            .filter(|(_, explained)| !explained)
            .map(|(g, _)| g)
            .collect();
        items.sort_by(|a, b| b.change.cmp(&a.change).then(a.path.cmp(&b.path)));
        items.truncate(limit.clamp(1, 200));
        Ok(GrewReport {
            since: Some(since),
            at: Some(at),
            total_change: total,
            items,
        })
    }

    pub fn prune_samples(&self, max_age_days: u64, max_samples: u64) -> Result<()> {
        let cutoff = now().saturating_sub(i64::try_from(max_age_days)?.saturating_mul(86_400));
        self.db.execute(
            "DELETE FROM volume_samples WHERE id IN (SELECT id FROM volume_samples WHERE at<? ORDER BY at LIMIT ?)",
            params![cutoff, PRUNE_BATCH],
        )?;
        self.db.execute(
            "DELETE FROM volume_samples WHERE id IN (SELECT id FROM volume_samples ORDER BY at DESC,id DESC LIMIT ? OFFSET ?)",
            params![PRUNE_BATCH, i64::try_from(max_samples)?],
        )?;
        Ok(())
    }

    pub fn trend(&self, window_days: u64) -> Result<TrendReport> {
        if window_days == 0 || window_days > 3_650 {
            bail!("window must be 1..=3650 days");
        }
        let cutoff = now().saturating_sub(i64::try_from(window_days)? * 86_400);
        let mut statement = self.db.prepare(
            // Only samples of the currently published scope are comparable.
            "SELECT id,at,available,indexed_allocated FROM volume_samples WHERE at>=? AND root=coalesce((SELECT root FROM scans WHERE id=(SELECT scan_id FROM active WHERE singleton=1)),root) ORDER BY at,id",
        )?;
        let samples: Vec<(i64, i64, i64, Option<i64>)> = statement
            .query_map([cutoff], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let note = "Observed changes; volume movement includes activity outside the scanned root and attribution is uncertain.";
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            return Ok(TrendReport {
                samples: 0,
                first_at: None,
                last_at: None,
                available_change: None,
                indexed_allocated_change: None,
                growers: vec![],
                note,
            });
        };
        let mut growers = vec![];
        if first.0 != last.0 {
            let snapshot = |id: i64| -> Result<HashMap<Vec<u8>, u64>> {
                let mut statement = self
                    .db
                    .prepare("SELECT path,allocated FROM growth_snapshots WHERE sample_id=?")?;
                Ok(statement
                    .query_map([id], |r| Ok((r.get::<_, Vec<u8>>(0)?, unsigned(r, 1)?)))?
                    .collect::<rusqlite::Result<_>>()?)
            };
            let (before, after) = (snapshot(first.0)?, snapshot(last.0)?);
            let have_before = !before.is_empty();
            for (path, allocated) in after {
                let change = match before.get(&path) {
                    Some(old) => Some(i64::try_from(allocated)? - i64::try_from(*old)?),
                    None if have_before => None,
                    None => continue,
                };
                if change.is_none_or(|c| c > 0) {
                    growers.push(Grower {
                        path,
                        allocated,
                        change,
                    });
                }
            }
            growers.sort_by(|a, b| {
                b.change
                    .unwrap_or(i64::MAX)
                    .cmp(&a.change.unwrap_or(i64::MAX))
                    .then(b.allocated.cmp(&a.allocated))
            });
            growers.truncate(10);
        }
        Ok(TrendReport {
            samples: samples.len() as u64,
            first_at: Some(first.1),
            last_at: Some(last.1),
            available_change: (first.0 != last.0).then(|| last.2 - first.2),
            indexed_allocated_change: match (first.3, last.3) {
                (Some(a), Some(b)) if first.0 != last.0 => Some(b - a),
                _ => None,
            },
            growers,
            note,
        })
    }
}
