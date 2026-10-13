use crate::{Entry, Index, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_CATALOG: usize = 256 * 1024;
const DOMAIN: &[u8] = b"OMG-CATALOG-V1\0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub version: u64,
    pub engine_major: u32,
    pub rules: Vec<Rule>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub revision: u32,
    pub platforms: Vec<String>,
    pub tool_versions: Vec<String>,
    pub category: String,
    pub tier: String,
    pub action: String,
    pub discovery: String,
    pub explanation: String,
    pub side_effects: String,
    pub regeneration: String,
    pub warnings: Vec<String>,
    pub preconditions: Vec<String>,
    pub exclusions: Vec<String>,
    pub privileges: String,
    pub failure_behavior: String,
    pub compatibility: String,
    pub deprecated: bool,
    pub fixtures: Vec<String>,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub rule: Rule,
    pub catalog_digest: String,
    pub entry: Entry,
    /// Subtree totals when the target is a directory with published rollups.
    pub subtree: Option<crate::rollup::Totals>,
    /// Estimated allocated bytes: the deduplicated subtree total for directories, else the file's.
    pub estimate: u64,
    pub effective_tier: String,
    pub action: String,
    pub reason: String,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ignore {
    pub id: i64,
    pub rule: String,
    pub root: Vec<u8>,
    pub path: Vec<u8>,
    pub subtree: bool,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn signed_message(payload: &str) -> Vec<u8> {
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(payload.as_bytes());
    message
}
pub fn parse(payload: &str) -> Result<Catalog> {
    if payload.len() > MAX_CATALOG {
        bail!("catalog exceeds byte budget");
    }
    let catalog: Catalog = toml::from_str(payload)?;
    if catalog.schema_version != 1
        || catalog.engine_major != 0
        || catalog.version == 0
        || catalog.version > i64::MAX as u64
        || catalog.rules.len() > 64
    {
        bail!("unsupported catalog version/schema/engine or rule budget");
    }
    let mut ids = BTreeSet::new();
    for rule in &catalog.rules {
        if !ids.insert(&rule.id)
            || rule.id.len() > 100
            || rule.id.is_empty()
            || !rule
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            bail!("invalid/duplicate rule ID");
        }
        if rule.revision == 0
            || !["info", "review"].contains(&rule.tier.as_str())
            || rule.action != "info"
            || !["npm_content_cache", "pip_cache", "uv_cache", "large_file"]
                .contains(&rule.discovery.as_str())
        {
            bail!("unqualified tier/action/discovery; catalog cannot enable deletion or commands");
        }
        if rule.platforms.is_empty()
            || rule
                .platforms
                .iter()
                .any(|p| !["linux", "macos", "windows"].contains(&p.as_str()))
            || rule.tool_versions.is_empty()
        {
            bail!("platform/tool compatibility missing");
        }
        if rule.privileges != "current-user"
            || rule.compatibility != "inventory-only"
            || rule.failure_behavior != "informational"
            || rule.fixtures.len() < 2
        {
            bail!("invalid privilege/failure/fixture contract");
        }
        if rule
            .preconditions
            .iter()
            .any(|p| !["known_root", "metadata_only"].contains(&p.as_str()))
            || !rule.preconditions.iter().any(|p| p == "metadata_only")
            || rule
                .exclusions
                .iter()
                .any(|p| !["protected", "cloud", "active_work"].contains(&p.as_str()))
        {
            bail!("unsupported evidence/policy predicate");
        }
        if !["package_caches", "large_files"].contains(&rule.category.as_str()) {
            bail!("unsupported category");
        }
        for text in [
            &rule.name,
            &rule.explanation,
            &rule.side_effects,
            &rule.regeneration,
            &rule.source,
        ] {
            if text.is_empty() || text.len() > 4096 || text.chars().any(|c| c.is_control()) {
                bail!("invalid rule text");
            }
        }
        if !rule.source.starts_with("https://")
            || rule.warnings.is_empty()
            || rule.warnings.len() > 16
            || rule
                .warnings
                .iter()
                .any(|w| w.len() > 4096 || w.chars().any(|c| c.is_control()))
        {
            bail!("invalid sources/warnings");
        }
    }
    Ok(catalog)
}

impl Index {
    pub fn catalog(&self) -> Result<(Catalog, String)> {
        let payload:Option<String>=self.db.query_row("SELECT payload FROM catalog_releases WHERE digest=(SELECT active FROM catalog_state WHERE singleton=1)",[],|r|r.get(0)).optional()?;
        let payload =
            payload.unwrap_or_else(|| include_str!("../../../catalog/bundled.toml").into());
        Ok((parse(&payload)?, digest(payload.as_bytes())))
    }
    pub fn import_catalog(&mut self, payload: &str, signature: &[u8]) -> Result<String> {
        // Explicitly configured trust root belongs to the binary/state owner, never the bundle.
        let key: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='catalog-key'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(key) = key else {
            bail!("No release verification key provisioned; signed catalog import disabled");
        };
        self.import_verified(payload, signature, &key)
    }
    fn import_verified(&mut self, payload: &str, signature: &[u8], key: &[u8]) -> Result<String> {
        if payload.len() > MAX_CATALOG {
            bail!("catalog byte budget exceeded");
        }
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid trust root"))?;
        let key = ed25519_dalek::VerifyingKey::from_bytes(&key)?;
        let signature = ed25519_dalek::Signature::try_from(signature)?;
        key.verify_strict(&signed_message(payload), &signature)?;
        let catalog = parse(payload)?;
        let highest: i64 = self
            .db
            .query_row(
                "SELECT highest FROM catalog_state WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(1);
        if catalog.version <= highest as u64 {
            bail!("replayed/downgraded catalog rejected; use explicit rollback");
        }
        let hash = digest(payload.as_bytes());
        let (_, previous) = self.catalog()?;
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO catalog_releases VALUES(?,?,?,?)",
            params![
                previous,
                1,
                include_str!("../../../catalog/bundled.toml"),
                now()
            ],
        )?;
        tx.execute(
            "INSERT INTO catalog_releases VALUES(?,?,?,?)",
            params![hash, catalog.version as i64, payload, now()],
        )?;
        tx.execute("INSERT INTO catalog_state VALUES(1,?,?,?) ON CONFLICT(singleton) DO UPDATE SET previous=catalog_state.active,active=excluded.active,highest=excluded.highest",params![hash,previous,catalog.version as i64])?;
        tx.execute("DELETE FROM catalog_releases WHERE digest NOT IN (SELECT active FROM catalog_state UNION SELECT previous FROM catalog_state)",[])?;
        tx.commit()?;
        Ok(hash)
    }
    pub fn rollback_catalog(&mut self) -> Result<()> {
        let tx = self.db.transaction()?;
        let (active, previous): (String, Option<String>) = tx.query_row(
            "SELECT active,previous FROM catalog_state WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Some(previous) = previous else {
            bail!("no previous verified catalog");
        };
        tx.execute(
            "UPDATE catalog_state SET active=?,previous=? WHERE singleton=1",
            params![previous, active],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Directory-root rules resolve their root directly (no dependence on size paging) and carry
    /// subtree totals; large files come from an indexed query and never overlap a directory finding.
    pub fn findings(
        &self,
        limit: usize,
        offset: usize,
        include_ignored: bool,
    ) -> Result<Vec<Finding>> {
        let status = self.status()?;
        let Some(root_raw) = status.root else {
            return Ok(vec![]);
        };
        let root = platform::path(&root_raw)?;
        let (catalog, hash) = self.catalog()?;
        let ignores = self.ignores()?;
        let home = platform::home()?;
        let scan: i64 =
            self.db
                .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                    r.get(0)
                })?;
        let rollups = self.rollups_available()?;
        let mut output: Vec<Finding> = Vec::new();
        let mut directory_roots: Vec<Vec<u8>> = Vec::new();
        let applicable = |rule: &&Rule| {
            !rule.deprecated && rule.platforms.iter().any(|p| p == std::env::consts::OS)
        };
        let assemble = |rule: &Rule,
                        entry: Entry,
                        subtree: Option<crate::rollup::Totals>|
         -> Result<Option<Finding>> {
            if !include_ignored
                && ignores.iter().any(|i| {
                    i.rule == rule.id
                        && i.root == root_raw
                        && policy::inside(&entry.path, &i.path, i.subtree)
                })
            {
                return Ok(None);
            }
            let mut protected = policy::protection(&root, &entry);
            if protected.is_none() && subtree.as_ref().is_some_and(|t| t.protected > 0) {
                protected = Some(
                    "contains protected, excluded or uncertain descendants; no cleanup action"
                        .into(),
                );
            }
            let estimate = subtree.as_ref().map_or(entry.allocated, |t| t.allocated);
            let effective = if protected.is_some() {
                "protected"
            } else {
                rule.tier.as_str()
            };
            let totals = match (&subtree, rollups) {
                (Some(t), _) => format!(
                    "subtree {} allocated over {} files and {} directories (hard links counted once); {} protected/excluded entries inside",
                    t.allocated, t.files, t.dirs, t.protected
                ),
                (None, true) => "single entry".into(),
                (None, false) => "subtree total unavailable (rescan to compute rollups); estimate is the entry itself".into(),
            };
            let evidence = format!(
                "metadata identity {}; observed scan {:?}; known default-root match {}; tool version unknown; allocation {} logical {}; {}; no cleanup qualification",
                entry.identity,
                status.last_successful,
                rule.discovery,
                entry.allocated,
                entry.apparent,
                totals
            );
            let mut fingerprint = serde_json::to_vec(&entry)?;
            fingerprint.extend_from_slice(hash.as_bytes());
            fingerprint.extend_from_slice(rule.id.as_bytes());
            fingerprint.extend_from_slice(&root_raw);
            fingerprint.extend_from_slice(&serde_json::to_vec(&subtree)?);
            Ok(Some(Finding {
                id: digest(&fingerprint),
                rule: rule.clone(),
                catalog_digest: hash.clone(),
                entry,
                subtree,
                estimate,
                effective_tier: effective.into(),
                action: "info".into(),
                reason: protected.unwrap_or_else(|| rule.explanation.clone()),
                evidence,
            }))
        };
        for rule in catalog.rules.iter().filter(applicable) {
            let relative = match rule.discovery.as_str() {
                "npm_content_cache" => home.join(".npm/_cacache"),
                "pip_cache" => home.join(".cache/pip"),
                "uv_cache" => home.join(".cache/uv"),
                _ => continue,
            };
            let Some(relative) = crate::rollup::relative_raw(&root, &relative) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            let entry = self
                .db
                .query_row(
                    "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND path=?",
                    params![scan, relative],
                    crate::index::row_entry,
                )
                .optional()?;
            let Some(entry) = entry.filter(|e| e.kind == "directory") else {
                continue;
            };
            directory_roots.push(entry.path.clone());
            let subtree = self.subtree(&entry.path)?;
            output.extend(assemble(rule, entry, subtree)?);
        }
        if let Some(rule) = catalog
            .rules
            .iter()
            .filter(applicable)
            .find(|r| r.discovery == "large_file")
        {
            let mut statement = self.db.prepare(
                "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND kind='file' AND apparent>=? ORDER BY allocated DESC,path LIMIT 10000",
            )?;
            let rows = statement
                .query_map(params![scan, 100i64 * 1024 * 1024], crate::index::row_entry)?;
            for entry in rows {
                let entry = entry?;
                if directory_roots
                    .iter()
                    .any(|d| policy::inside(&entry.path, d, true))
                {
                    continue; // already inside a directory finding's estimate
                }
                output.extend(assemble(rule, entry, None)?);
            }
        }
        output.sort_by(|a, b| {
            b.estimate
                .cmp(&a.estimate)
                .then(a.entry.path.cmp(&b.entry.path))
        });
        Ok(output
            .into_iter()
            .skip(offset)
            .take(limit.min(1000))
            .collect())
    }
    pub fn ignores(&self) -> Result<Vec<Ignore>> {
        let mut statement = self
            .db
            .prepare("SELECT id,rule,root,path,subtree FROM ignores ORDER BY id LIMIT 10000")?;
        Ok(statement
            .query_map([], |r| {
                Ok(Ignore {
                    id: r.get(0)?,
                    rule: r.get(1)?,
                    root: r.get(2)?,
                    path: r.get(3)?,
                    subtree: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn ignore_finding(&self, finding: &Finding, subtree: bool) -> Result<()> {
        let count: i64 = self
            .db
            .query_row("SELECT count(*) FROM ignores", [], |r| r.get(0))?;
        if count >= 10000 {
            bail!("ignore budget exceeded");
        }
        let root = self
            .status()?
            .root
            .ok_or_else(|| anyhow::anyhow!("no active scope"))?;
        self.db.execute(
            "INSERT OR IGNORE INTO ignores(rule,root,path,subtree,created) VALUES(?,?,?,?,?)",
            params![finding.rule.id, root, finding.entry.path, subtree, now()],
        )?;
        Ok(())
    }
    pub fn remove_ignore(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM ignores WHERE id=?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use ed25519_dalek::Signer;
    #[test]
    fn strict_catalog_and_tier_contract() {
        let payload = include_str!("../../../catalog/bundled.toml");
        assert_eq!(parse(payload).unwrap().rules.len(), 4);
        assert!(parse(&payload.replace("action = \"info\"", "action = \"shell\"")).is_err());
        assert!(parse(&payload.replace("tier = \"info\"", "tier = \"safe\"")).is_err());
        assert!(parse(&format!("{payload}\ncommand = 'rm -rf /'")).is_err());
        assert!(parse(&"x".repeat(MAX_CATALOG + 1)).is_err());
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn rule_fixtures_evidence_ignores_and_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let index = Index::open(state.path()).unwrap();
        let home = platform::home().unwrap();
        index
            .db
            .execute(
                "INSERT INTO scans(root,started,ended,state,mode) VALUES(?,0,1,'complete','full')",
                [platform::raw(&home).unwrap()],
            )
            .unwrap();
        index
            .db
            .execute("INSERT INTO active VALUES(1,1)", [])
            .unwrap();
        // (path, kind, bytes). Cache roots are directories; near-miss names must not match.
        const BIG: i64 = 200 * 1024 * 1024;
        let paths: [(&str, &str, i64); 16] = [
            (".npm", "directory", 1),
            (".npm/_cacache", "directory", 1),
            (".npm/_cacache/item", "file", 1000),
            (".cache", "directory", 1),
            (".cache/pip", "directory", 1),
            (".cache/pip/wheel", "file", 2000),
            // Large file inside a cache root: counted by the directory, never a separate finding.
            (".cache/pip/huge", "file", BIG),
            (".cache/uv", "directory", 1),
            (".cache/uv/item", "file", 3000),
            // A protected-looking descendant makes the whole cache directory protected.
            (".cache/uv/store.sqlite", "file", 10),
            (".cache/pip-near", "directory", 1),
            (".cache/pip-near/wheel", "file", 1),
            (".cache/uv-near", "directory", 1),
            (".cache/uv-near/item", "file", 1),
            ("ordinary", "file", BIG),
            ("private.sqlite", "file", BIG),
        ];
        for (i, (path, kind, size)) in paths.iter().enumerate() {
            index
                .db
                .execute(
                    "INSERT INTO entries VALUES(1,?,x'',?,?,?,?,1,0,0,NULL)",
                    params![path.as_bytes(), kind, format!("fixture:{i}"), size, size],
                )
                .unwrap();
        }
        assert!(!index.rollups_available().unwrap());
        crate::rollup::compute(&index, 1, &home).unwrap();
        assert!(index.rollups_available().unwrap());
        let findings = index.findings(1000, 0, false).unwrap();
        assert_eq!(findings.len(), 5);
        assert!(
            findings.iter().all(|f| !f.evidence.is_empty()
                && f.action == "info"
                && f.effective_tier != "safe")
        );
        assert_eq!(
            findings
                .iter()
                .find(|f| f.entry.path == b"private.sqlite")
                .unwrap()
                .effective_tier,
            "protected"
        );
        let pip = findings
            .iter()
            .find(|f| f.entry.path == b".cache/pip")
            .unwrap();
        assert_eq!(pip.estimate, 1 + 2000 + BIG as u64);
        assert_eq!(pip.subtree.as_ref().unwrap().files, 2);
        assert!(findings.iter().all(|f| f.entry.path != b".cache/pip/huge"));
        assert!(findings.iter().all(|f| !f.entry.path.ends_with(b"-near")));
        assert!(
            findings[0].estimate >= findings[1].estimate,
            "ordered by estimate"
        );
        let tier = |path: &[u8]| {
            findings
                .iter()
                .find(|f| f.entry.path == path)
                .unwrap()
                .effective_tier
                .as_str()
        };
        assert_eq!(tier(b".cache/uv"), "protected");
        // The fixture scope is the real home directory, which may itself be a Git repository
        // (then everything under it is rightly protected), so only assert the info tier otherwise.
        if home.join(".git").symlink_metadata().is_err() {
            assert_eq!(tier(b".npm/_cacache"), "info");
            assert_eq!(tier(b".cache/pip"), "info");
        }
        let selected = findings
            .iter()
            .find(|f| f.entry.path == b"ordinary")
            .unwrap();
        index.ignore_finding(selected, false).unwrap();
        assert_eq!(index.findings(1000, 0, false).unwrap().len(), 4);
        assert_eq!(index.findings(1000, 0, true).unwrap().len(), 5);
        index.remove_ignore(index.ignores().unwrap()[0].id).unwrap();
        index
            .db
            .execute(
                "UPDATE entries SET identity='replacement' WHERE path=?",
                [b"ordinary".as_slice()],
            )
            .unwrap();
        assert_ne!(
            index
                .findings(1000, 0, false)
                .unwrap()
                .iter()
                .find(|f| f.entry.path == b"ordinary")
                .unwrap()
                .id,
            selected.id
        );
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn signed_activation_replay_rollback_and_last_good() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut index = Index::open(state.path()).unwrap();
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        index
            .db
            .execute(
                "INSERT INTO settings VALUES('catalog-key',?)",
                [key.verifying_key().to_bytes().to_vec()],
            )
            .unwrap();
        let payload = include_str!("../../../catalog/bundled.toml").replacen(
            "\nversion = 1\n",
            "\nversion = 2\n",
            1,
        );
        let signature = key.sign(&signed_message(&payload)).to_bytes();
        let previous = index.catalog().unwrap().1;
        index.import_catalog(&payload, &signature).unwrap();
        let active = index.catalog().unwrap().1;
        assert_ne!(active, previous);
        assert!(index.import_catalog(&payload, &signature).is_err());
        assert!(
            index
                .import_catalog(&(payload.clone() + "\n# tamper"), &signature)
                .is_err()
        );
        assert_eq!(index.catalog().unwrap().1, active);
        let invalid = payload.replace("action = \"info\"", "action = \"trash\"");
        assert!(
            index
                .import_catalog(&invalid, &key.sign(&signed_message(&invalid)).to_bytes())
                .is_err()
        );
        index.rollback_catalog().unwrap();
        assert_eq!(index.catalog().unwrap().1, previous);
        assert!(index.import_catalog(&payload, &signature).is_err());
    }
}
