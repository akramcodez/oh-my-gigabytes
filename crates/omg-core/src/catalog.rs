use crate::{Entry, Index, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_CATALOG: usize = 256 * 1024;
/// Builtin discovery kinds. A catalog can only refer to these; it can never name a path itself.
pub const DISCOVERIES: [&str; 11] = [
    "npm_content_cache",
    "pip_cache",
    "uv_cache",
    "pnpm_store",
    "yarn_cache",
    "cargo_registry",
    "go_build_cache",
    "gradle_caches",
    "homebrew_cache",
    "stale_dependencies",
    "large_file",
];

/// Fixed default location of a directory-root discovery, honouring the tool's own absolute
/// environment override where it has one.
pub fn discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        windows_discovery_root(kind, home)
    } else if cfg!(target_os = "macos") {
        macos_discovery_root(kind, home)
    } else {
        unix_discovery_root(kind, home)
    }
}

/// macOS defaults, per each tool's documented per-user location (pip, pnpm, Homebrew and Go are
/// checked against the tool's own documentation). Yarn Classic's macOS location is not stated in its
/// documentation, so it has no macOS root and the rule finds nothing there rather than guessing.
fn macos_discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let caches = home.join("Library/Caches");
    Some(match kind {
        "npm_content_cache" => home.join(".npm/_cacache"),
        "pip_cache" => caches.join("pip"),
        "uv_cache" => home.join(".cache/uv"),
        "pnpm_store" => env("PNPM_HOME")
            .map(|p| p.join("store"))
            .or_else(|| env("XDG_DATA_HOME").map(|p| p.join("pnpm/store")))
            .unwrap_or_else(|| home.join("Library/pnpm/store")),
        "cargo_registry" => env("CARGO_HOME")
            .unwrap_or_else(|| home.join(".cargo"))
            .join("registry"),
        "go_build_cache" => env("GOCACHE").unwrap_or_else(|| caches.join("go-build")),
        "gradle_caches" => env("GRADLE_USER_HOME")
            .unwrap_or_else(|| home.join(".gradle"))
            .join("caches"),
        "homebrew_cache" => env("HOMEBREW_CACHE").unwrap_or_else(|| caches.join("Homebrew")),
        _ => return None,
    })
}

/// Windows defaults of the same discoveries (npm, pip, uv, pnpm, Yarn, Go, Cargo, Gradle), per
/// each tool's documented per-user location.
fn windows_discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
    Some(match kind {
        "npm_content_cache" => local.join("npm-cache"),
        "pip_cache" => local.join("pip").join("Cache"),
        "uv_cache" => local.join("uv").join("cache"),
        "pnpm_store" => local.join("pnpm").join("store"),
        "yarn_cache" => local.join("Yarn").join("Cache"),
        "cargo_registry" => env("CARGO_HOME")
            .unwrap_or_else(|| home.join(".cargo"))
            .join("registry"),
        "go_build_cache" => env("GOCACHE").unwrap_or_else(|| local.join("go-build")),
        "gradle_caches" => env("GRADLE_USER_HOME")
            .unwrap_or_else(|| home.join(".gradle"))
            .join("caches"),
        _ => return None,
    })
}

fn unix_discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let cache = env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
    Some(match kind {
        "npm_content_cache" => home.join(".npm/_cacache"),
        "pip_cache" => home.join(".cache/pip"),
        "uv_cache" => home.join(".cache/uv"),
        "pnpm_store" => env("XDG_DATA_HOME")
            .unwrap_or_else(|| home.join(".local/share"))
            .join("pnpm/store"),
        "yarn_cache" => cache.join("yarn"),
        "cargo_registry" => env("CARGO_HOME")
            .unwrap_or_else(|| home.join(".cargo"))
            .join("registry"),
        "go_build_cache" => env("GOCACHE").unwrap_or_else(|| cache.join("go-build")),
        "gradle_caches" => env("GRADLE_USER_HOME")
            .unwrap_or_else(|| home.join(".gradle"))
            .join("caches"),
        _ => return None,
    })
}
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
            || !DISCOVERIES.contains(&rule.discovery.as_str())
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
        if !["package_caches", "build_artifacts", "large_files"].contains(&rule.category.as_str()) {
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
        self.findings_in(limit, offset, include_ignored, &platform::home()?)
    }
    /// Findings with rule roots resolved against an explicit home directory.
    pub fn findings_in(
        &self,
        limit: usize,
        offset: usize,
        include_ignored: bool,
        home: &std::path::Path,
    ) -> Result<Vec<Finding>> {
        let home = home.to_path_buf();
        let status = self.status()?;
        let Some(root_raw) = status.root else {
            return Ok(vec![]);
        };
        let root = platform::path(&root_raw)?;
        let (catalog, hash) = self.catalog()?;
        let ignores = self.ignores()?;
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
            // The same verdict that gates deletion decides the tier, so "protected" means the
            // item really cannot be deleted (not merely "somewhere under a Git repository").
            let lookup = |dir: &std::path::Path| crate::rollup::newest_at(self, &root, dir);
            let verdict = crate::cleanup::verdict(
                &root,
                &home,
                &entry,
                &mut policy::RepositoryMemo::with_ages(&lookup),
            );
            let estimate = subtree.as_ref().map_or(entry.allocated, |t| t.allocated);
            let (effective, reason) = match &verdict.gate {
                crate::cleanup::Gate::Denied(why) => ("protected", why.clone()),
                crate::cleanup::Gate::Warn(why) => {
                    (rule.tier.as_str(), format!("{} ⚠ {why}", rule.explanation))
                }
                crate::cleanup::Gate::Allowed => (rule.tier.as_str(), rule.explanation.clone()),
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
                reason,
                evidence,
            }))
        };
        for rule in catalog.rules.iter().filter(applicable) {
            let Some(relative) = discovery_root(&rule.discovery, &home) else {
                continue;
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
            .find(|r| r.discovery == "stale_dependencies")
        {
            for path in self.stale_dependencies(scan)? {
                if directory_roots
                    .iter()
                    .any(|d| policy::inside(&path, d, true))
                {
                    continue;
                }
                let Some(entry) = self
                    .db
                    .query_row(
                        "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND path=?",
                        params![scan, path],
                        crate::index::row_entry,
                    )
                    .optional()?
                else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                output.extend(assemble(rule, entry, subtree)?);
            }
        }
        if let Some(rule) = catalog
            .rules
            .iter()
            .filter(applicable)
            .find(|r| r.discovery == "large_file")
        {
            let mut statement = self.db.prepare(
                // Literal threshold so SQLite can use the partial `entry_large` index (migration 007).
                "SELECT path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND kind='file' AND apparent>=104857600 ORDER BY allocated DESC,path LIMIT 10000",
            )?;
            let rows = statement.query_map(params![scan], crate::index::row_entry)?;
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
    /// Top-level dependency folders (`node_modules` next to `package.json`, `target` next to
    /// `Cargo.toml`, Python virtual environments) of projects untouched for `policy::STALE_DAYS`,
    /// outside hidden (tool/application) folders.
    fn stale_dependencies(&self, scan: i64) -> Result<Vec<Vec<u8>>> {
        let mut statement = self.db.prepare(
            "SELECT path FROM dir_totals WHERE scan_id=? AND allocated>=1048576 AND path<>x''",
        )?;
        let mut candidates: Vec<Vec<u8>> = statement
            .query_map([scan], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        candidates.sort_by_key(|p| p.len());
        let exists = |path: Vec<u8>| -> Result<bool> {
            Ok(self
                .db
                .query_row(
                    "SELECT 1 FROM entries WHERE scan_id=? AND path=?",
                    params![scan, path],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some())
        };
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let mut chosen: Vec<Vec<u8>> = Vec::new();
        for path in candidates {
            let (parent, name) = match path.iter().rposition(|b| *b == b'/') {
                Some(n) => (path[..n].to_vec(), &path[n + 1..]),
                None => (Vec::new(), path.as_slice()),
            };
            let join = |base: &[u8], leaf: &str| {
                let mut out = base.to_vec();
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(leaf.as_bytes());
                out
            };
            let evidence = match name {
                b"node_modules" => exists(join(&parent, "package.json"))?,
                b"target" => exists(join(&parent, "Cargo.toml"))?,
                b".venv" | b"venv" => exists(join(&path, "pyvenv.cfg"))?,
                _ => false,
            };
            // Projects live in ordinary folders. Hidden folders hold installed tools (nvm global
            // packages, editor servers, pnpm's own versions), whose dependencies are not stale
            // project output: deleting them would break the tool.
            let hidden = path
                .split(|b| *b == b'/')
                .any(|part| part.first() == Some(&b'.'));
            if !evidence || hidden || chosen.iter().any(|c| policy::inside(&path, c, true)) {
                continue;
            }
            let newest = self.subtree(&parent)?.map_or(0, |t| t.newest);
            if newest > 0 && newest < cutoff {
                chosen.push(path);
            }
        }
        Ok(chosen)
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
    fn macos_cache_locations_follow_each_tools_documented_default() {
        let home = std::path::Path::new("/Users/dev");
        let root = |kind| macos_discovery_root(kind, home).unwrap();
        assert_eq!(root("pip_cache"), home.join("Library/Caches/pip"));
        assert_eq!(root("npm_content_cache"), home.join(".npm/_cacache"));
        assert!(macos_discovery_root("large_file", home).is_none());
        assert!(macos_discovery_root("yarn_cache", home).is_none());
        // These read the tool's own override variable, so their default is asserted only when unset.
        if std::env::var_os("PNPM_HOME").is_none() && std::env::var_os("XDG_DATA_HOME").is_none() {
            assert_eq!(root("pnpm_store"), home.join("Library/pnpm/store"));
        }
        if std::env::var_os("HOMEBREW_CACHE").is_none() {
            assert_eq!(root("homebrew_cache"), home.join("Library/Caches/Homebrew"));
        }
    }
    #[test]
    fn strict_catalog_and_tier_contract() {
        let payload = include_str!("../../../catalog/bundled.toml");
        assert_eq!(parse(payload).unwrap().rules.len(), 11);
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
                    "INSERT INTO entries VALUES(1,?,?,?,?,?,?,1,0,0,NULL)",
                    params![
                        path.as_bytes(),
                        path.rsplit_once('/')
                            .map_or("", |(parent, _)| parent)
                            .as_bytes(),
                        kind,
                        format!("fixture:{i}"),
                        size,
                        size
                    ],
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
        // Package contents that merely look like databases do not lock a known cache, and a Git
        // repository at the home folder (dotfiles) does not lock what lives under home.
        assert_eq!(tier(b".cache/uv"), "info");
        assert_eq!(tier(b".npm/_cacache"), "info");
        assert_eq!(tier(b".cache/pip"), "info");
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
