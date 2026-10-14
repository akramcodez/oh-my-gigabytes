//! Immutable read-only plans; no destructive adapter exists in this build.
use crate::{
    Entry, Index,
    catalog::{Finding, digest},
    index::lock,
    now, platform, policy,
};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub candidate: String,
    pub path: Vec<u8>,
    pub identity: String,
    pub rule: String,
    pub estimated: u64,
    pub action: String,
    pub decision: String,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub fingerprint: String,
    pub created: i64,
    pub catalog_digest: String,
    pub items: Vec<PlanItem>,
    pub estimated: u64,
    pub actionable: u64,
    pub dry_run: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct History {
    pub id: i64,
    pub created: i64,
    pub kind: String,
    pub fingerprint: String,
    pub outcome: String,
    pub detail: String,
    /// Per-target outcomes of a permanent-delete operation; empty for other kinds.
    pub targets: Vec<Target>,
}

/// Outcome of one target of a permanent-delete operation, as last recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub path: Vec<u8>,
    pub outcome: String,
    pub detail: String,
}

impl Index {
    pub fn finding(&self, id: &str) -> Result<Finding> {
        self.findings(1000, 0, true)?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "candidate absent or stale; refresh and inspect before selecting again"
                )
            })
    }
    pub fn preview(&self, ids: &[String]) -> Result<Plan> {
        if ids.is_empty() || ids.len() > 1000 {
            bail!("dry-run requires 1–1000 explicit candidate IDs; no implicit selection");
        }
        let _owner = lock(&self.state, "cleanup.lock")?;
        let status = self.status()?;
        let root = platform::path(
            &status
                .root
                .ok_or_else(|| anyhow::anyhow!("no active scope"))?,
        )?;
        let (_, catalog_digest) = self.catalog()?;
        let mut seen = BTreeSet::new();
        let mut estimated = 0u64;
        let mut items = vec![];
        for id in ids {
            let finding = self.finding(id)?;
            if !seen.insert(finding.entry.identity.clone()) {
                continue;
            }
            // Overlap: a target inside an included one adds nothing; an enclosing target replaces
            // the included descendants so the estimate never counts a byte twice.
            if items
                .iter()
                .any(|i: &PlanItem| policy::inside(&finding.entry.path, &i.path, true))
            {
                continue;
            }
            items.retain(|i: &PlanItem| {
                let covered = policy::inside(&i.path, &finding.entry.path, true);
                if covered {
                    estimated = estimated.saturating_sub(i.estimated);
                }
                !covered
            });
            estimated = estimated.saturating_add(finding.estimate);
            let decision = match revalidate(&root, &finding.entry) {
                Ok(()) => policy::cleanup_denial(&root, &finding.entry),
                Err(_) => "Stale, inaccessible or redirected target; skipped".into(),
            };
            items.push(PlanItem {
                candidate: id.clone(),
                path: finding.entry.path,
                identity: finding.entry.identity,
                rule: finding.rule.id,
                estimated: finding.estimate,
                action: "informational; no filesystem mutation".into(),
                decision,
                warnings: finding.rule.warnings,
            });
        }
        let fingerprint = digest(&serde_json::to_vec(&(catalog_digest.clone(), &items))?);
        Ok(Plan {
            fingerprint,
            created: now(),
            catalog_digest,
            items,
            estimated,
            actionable: 0,
            dry_run: true,
        })
    }
    pub fn record_preview(&self, plan: &Plan) -> Result<()> {
        self.db.execute("INSERT INTO operations(created,kind,fingerprint,outcome,detail) VALUES(?,'dry-run',?,'evaluated-no-mutation',?)",params![now(),plan.fingerprint,serde_json::to_string(plan)?])?;
        self.db.execute("DELETE FROM operations WHERE id NOT IN (SELECT id FROM operations ORDER BY id DESC LIMIT 10000)",[])?;
        Ok(())
    }
    pub fn history(&self) -> Result<Vec<History>> {
        let mut statement=self.db.prepare("SELECT id,created,kind,fingerprint,outcome,detail FROM operations ORDER BY id DESC LIMIT 100")?;
        let mut rows: Vec<History> = statement
            .query_map([], |r| {
                Ok(History {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    kind: r.get(2)?,
                    fingerprint: r.get(3)?,
                    outcome: r.get(4)?,
                    detail: r.get(5)?,
                    targets: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut targets = self.db.prepare(
            "SELECT path,outcome,detail FROM delete_targets WHERE operation=? ORDER BY id",
        )?;
        for row in rows.iter_mut().filter(|r| r.kind == "delete") {
            row.targets = targets
                .query_map([row.id], |r| {
                    Ok(Target {
                        path: r.get(0)?,
                        outcome: r.get(1)?,
                        detail: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(rows)
    }
    /// Fail closed even if a caller supplies a forged/confirmed plan.
    pub fn execute(&self, _plan: &Plan) -> Result<()> {
        bail!(
            "OS Trash is not implemented: no certified identity-safe Trash handler; selection/confirmation cannot bypass policy"
        )
    }
}

pub fn revalidate(root: &std::path::Path, entry: &Entry) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let root = platform::linux::root(root)?;
        let path = platform::path(&entry.path)?;
        if !policy::safe_relative(&path) {
            bail!("target path outside explicit scope");
        }
        let fd = platform::linux::acquire(&root, &path, false)?;
        let fresh = platform::linux::entry(&path, &rustix::fs::fstat(fd)?);
        if fresh.identity != entry.identity
            || fresh.kind != entry.kind
            || fresh.apparent != entry.apparent
            || fresh.modified_ns != entry.modified_ns
            || fresh.changed_ns != entry.changed_ns
            || fresh.links != entry.links
        {
            bail!("target changed since observation");
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, entry);
        bail!("native target acquisition not certified")
    }
}

pub fn open_location(root: &std::path::Path, entry: &Entry) -> Result<()> {
    if policy::protection(root, entry).is_some() {
        bail!("protected/uncertain locations cannot be opened by this integration");
    }
    revalidate(root, entry)?;
    #[cfg(target_os = "linux")]
    {
        let executable = std::path::Path::new("/usr/bin/xdg-open");
        if !executable.is_file() {
            bail!("file manager integration unavailable");
        }
        let relative = platform::path(&entry.path)?;
        let directory = if entry.kind == "directory" {
            root.join(relative)
        } else {
            root.join(relative.parent().unwrap_or(std::path::Path::new("")))
        };
        std::process::Command::new(executable)
            .arg(directory)
            .spawn()?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        bail!("native file manager integration unavailable")
    }
}
