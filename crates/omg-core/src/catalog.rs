use crate::{Entry, Index, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod apps;
mod downloads;
mod projects;
mod stores;
mod toolchains;
pub use apps::*;
#[cfg(test)]
use downloads::fold_digits;
pub use projects::*;
pub use stores::*;
pub use toolchains::*;

pub const MAX_CATALOG: usize = 256 * 1024;
const MAX_RULES: usize = 128;
/// Builtin discovery kinds. A catalog can only refer to these. Only the store kinds take locations
/// from a rule, and only as a validated `Store` inside the tool areas of the home folder.
pub const DISCOVERIES: [&str; 49] = [
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
    "project_build_output",
    "app_caches",
    "ai_tool_old_versions",
    "tool_sessions",
    "tool_caches",
    "tool_logs",
    "unrecognized_store",
    "superseded_copies",
    "old_installers",
    "colima_vm",
    "docker_desktop_vm",
    "unused_clone",
    "unused_app_data",
    "unused_toolchain",
    "xcode_derived_data",
    "xcode_device_support",
    "simulator_caches",
    "bun_cache",
    "deno_cache",
    "maven_repository",
    "cocoapods_cache",
    "composer_cache",
    "playwright_browsers",
    "puppeteer_browsers",
    "jetbrains_caches",
    "huggingface_hub",
    "ollama_models",
    "lm_studio_models",
    "editor_old_extensions",
    "toolchain_old_versions",
    "home_logs_and_temp",
    "go_modules",
    "xcode_archives",
    "android_avd",
    "conda_envs",
    "jetbrains_old_versions",
    "app_leftovers",
    "fast_growing",
];

/// Logs and temporary files unchanged for this long are suggested.
pub const LOGS_MIN_AGE_DAYS: i64 = 7;

/// An application or toolchain unchanged for this long (about five months) counts as unused.
pub const UNUSED_DAYS: i64 = 150;

fn absolute_env(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// A `~/Library/Caches` folder the system manages, including one without Apple's usual prefix.
pub(crate) fn apple_cache(name: &str) -> bool {
    name.starts_with("com.apple.") || name == "GeoServices"
}

/// Smallest build output or application cache worth suggesting.
const SUGGEST_MIN_BYTES: i64 = 50 << 20;

/// Fixed default location of a directory-root discovery, honouring the tool's own absolute
/// environment override where it has one.
pub fn discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    shared_discovery_root(kind, home)
        .or_else(|| platform_discovery_root(kind, home))
        .filter(|root| tool_root_allowed(home, root))
}

/// Tools whose per-user location is the same on every platform: under the home folder or set by
/// their own absolute environment variable.
fn shared_discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(match kind {
        "bun_cache" => absolute_env("BUN_INSTALL_CACHE_DIR").unwrap_or_else(|| {
            absolute_env("BUN_INSTALL")
                .unwrap_or_else(|| home.join(".bun"))
                .join("install")
                .join("cache")
        }),
        "maven_repository" => home.join(".m2").join("repository"),
        "puppeteer_browsers" => absolute_env("PUPPETEER_CACHE_DIR")
            .unwrap_or_else(|| home.join(".cache").join("puppeteer")),
        "huggingface_hub" => absolute_env("HF_HUB_CACHE").unwrap_or_else(|| {
            absolute_env("HF_HOME")
                .unwrap_or_else(|| home.join(".cache").join("huggingface"))
                .join("hub")
        }),
        "ollama_models" => {
            absolute_env("OLLAMA_MODELS").unwrap_or_else(|| home.join(".ollama").join("models"))
        }
        "lm_studio_models" => home.join(".lmstudio").join("models"),
        "go_modules" => absolute_env("GOMODCACHE").unwrap_or_else(|| {
            std::env::var_os("GOPATH")
                .and_then(|p| std::env::split_paths(&p).next())
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join("go"))
                .join("pkg")
                .join("mod")
        }),
        "android_avd" => absolute_env("ANDROID_AVD_HOME").unwrap_or_else(|| {
            absolute_env("ANDROID_USER_HOME")
                .unwrap_or_else(|| home.join(".android"))
                .join("avd")
        }),
        "deno_cache" => absolute_env("DENO_DIR")?,
        "playwright_browsers" => absolute_env("PLAYWRIGHT_BROWSERS_PATH")?,
        "composer_cache" => absolute_env("COMPOSER_CACHE_DIR")?,
        _ => return None,
    })
}

fn platform_discovery_root(kind: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        windows_discovery_root(kind, home)
    } else if cfg!(target_os = "macos") {
        macos_discovery_root(kind, home)
    } else {
        unix_discovery_root(kind, home)
    }
}

/// Folders a user keeps their own files in, directly in the home folder.
const PERSONAL_FOLDERS: [&str; 13] = [
    "desktop",
    "documents",
    "downloads",
    "pictures",
    "music",
    "movies",
    "videos",
    "public",
    "applications",
    "library",
    "appdata",
    "projects",
    "src",
];

/// Whether a tool's folder (often taken from an environment variable) may be treated as that tool's
/// own. Folders inside one count as cache or app data and lose ordinary-folder checks, so the home
/// folder, anything above it and a personal folder directly in it never qualify.
pub(crate) fn tool_root_allowed(home: &std::path::Path, root: &std::path::Path) -> bool {
    if home.starts_with(root) {
        return false;
    }
    match root.strip_prefix(home) {
        Ok(below) => {
            let mut parts = below.components();
            !(parts.clone().count() == 1
                && parts.next().is_some_and(|first| {
                    PERSONAL_FOLDERS.contains(
                        &first
                            .as_os_str()
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .as_str(),
                    )
                }))
        }
        Err(_) => true,
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
        "colima_vm" => home.join(".colima"),
        "docker_desktop_vm" => home.join("Library/Containers/com.docker.docker"),
        "xcode_derived_data" => home.join("Library/Developer/Xcode/DerivedData"),
        "xcode_device_support" => home.join("Library/Developer/Xcode/iOS DeviceSupport"),
        "simulator_caches" => home.join("Library/Developer/CoreSimulator/Caches"),
        "xcode_archives" => home.join("Library/Developer/Xcode/Archives"),
        "deno_cache" => caches.join("deno"),
        "cocoapods_cache" => caches.join("CocoaPods"),
        "playwright_browsers" => caches.join("ms-playwright"),
        "jetbrains_caches" => caches.join("JetBrains"),
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
        "deno_cache" => local.join("deno"),
        "composer_cache" => local.join("Composer"),
        "playwright_browsers" => local.join("ms-playwright"),
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
        "colima_vm" => home.join(".colima"),
        "deno_cache" => cache.join("deno"),
        "composer_cache" => cache.join("composer"),
        "playwright_browsers" => cache.join("ms-playwright"),
        "jetbrains_caches" => cache.join("JetBrains"),
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
    /// Where a tool keeps its items, for the store kinds (`STORE_KINDS`) only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<Store>,
}

/// Discovery kinds whose locations come from the rule's `store` instead of code.
pub const STORE_KINDS: [&str; 3] = ["tool_sessions", "tool_caches", "tool_logs"];

/// A tool's own store, described with fixed parts only: root folders inside the tool areas of the
/// home folder and name patterns at an exact depth below them. It can only find items; deleting
/// one still goes through every check in `cleanup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    /// `~/...`, `{data}/...`, `{config}/...`, `{cache}/...` or `$NAME/...` (an absolute value of
    /// that environment variable). Every root that resolves is searched.
    pub roots: Vec<String>,
    /// Patterns relative to a root, one name pattern per level (`*` and `?` only). Empty means the
    /// root itself.
    pub patterns: Vec<String>,
    /// `file` or `directory`.
    pub target: String,
    /// Unchanged for at least this many days (for a directory, everything inside it).
    pub min_age_days: u32,
    pub min_bytes: u64,
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
    /// For build and dependency output: when its project was last worked on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
}
/// When a project was last worked on, from metadata only: its own files and folders, its build
/// output and its `.git` folder (commits, checkouts and fetches write there). Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub source: Option<i64>,
    pub build: Option<i64>,
    pub git: Option<i64>,
}
impl Activity {
    pub fn last(&self) -> Option<i64> {
        [self.source, self.build, self.git]
            .into_iter()
            .flatten()
            .max()
    }
    fn describe(&self) -> Option<String> {
        let days = |at: i64| (crate::now() - at).max(0) / 86_400;
        let last = self.last()?;
        let parts: Vec<String> = [
            ("files", self.source),
            ("last build", self.build),
            ("Git", self.git),
        ]
        .into_iter()
        .filter_map(|(what, at)| at.map(|at| format!("{what} {} days", days(at))))
        .collect();
        Some(format!(
            "project last worked on {} days ago: {}",
            days(last),
            parts.join(", ")
        ))
    }
}
/// Findings are reused for at most this long, so ages and the Git and application folders they
/// read stay current.
const FINDINGS_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// What a full list of findings was computed from: the published scan, the catalog, the ignores,
/// the arguments and SQLite's count of commits by other connections (a scan by the service).
#[derive(PartialEq, Eq)]
struct FindingsKey {
    scan: i64,
    catalog: String,
    ignores: Vec<u8>,
    include_ignored: bool,
    home: std::path::PathBuf,
    data_version: i64,
}

pub(crate) struct FindingsMemo {
    key: FindingsKey,
    at: std::time::Instant,
    findings: Vec<Finding>,
}

/// A folder that no finding explains, from `Index::unexplained`.
#[derive(Debug, Clone, Serialize)]
pub struct Unexplained {
    pub path: Vec<u8>,
    pub allocated: u64,
    /// What is left of `allocated` once the findings inside are taken away.
    pub unexplained: u64,
}
const UNEXPLAINED_MIN_BYTES: u64 = 256 << 20;
/// Growth between two scans this large is listed as a finding.
const GROWTH_MIN_BYTES: i64 = 256 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ignore {
    pub id: i64,
    pub rule: String,
    pub root: Vec<u8>,
    pub path: Vec<u8>,
    pub subtree: bool,
}

pub fn digest(bytes: &[u8]) -> String {
    // Lowercase hex, byte by byte: sha2 0.11 digests no longer implement `LowerHex`.
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn signed_message(payload: &str) -> Vec<u8> {
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(payload.as_bytes());
    message
}
/// The one category each discovery kind belongs to. Categories decide what scans suggest and what
/// `omg clean --category` selects, so a catalog must not move a kind into another category.
pub fn category_of(discovery: &str) -> Option<&'static str> {
    Some(match discovery {
        "npm_content_cache"
        | "pip_cache"
        | "uv_cache"
        | "pnpm_store"
        | "yarn_cache"
        | "cargo_registry"
        | "go_build_cache"
        | "gradle_caches"
        | "homebrew_cache"
        | "bun_cache"
        | "deno_cache"
        | "maven_repository"
        | "cocoapods_cache"
        | "composer_cache"
        | "playwright_browsers"
        | "puppeteer_browsers"
        | "go_modules" => "package_caches",
        "stale_dependencies" | "project_build_output" | "xcode_derived_data" => "build_artifacts",
        "large_file" => "large_files",
        "app_caches" | "xcode_device_support" | "simulator_caches" | "jetbrains_caches" => {
            "app_caches"
        }
        "ai_tool_old_versions" | "editor_old_extensions" | "toolchain_old_versions" => {
            "old_tool_versions"
        }
        "tool_sessions" => "session_history",
        "tool_caches" => "app_caches",
        "tool_logs" => "logs_and_temp",
        "unrecognized_store" => "unrecognized",
        "superseded_copies" => "duplicates",
        "old_installers" => "old_downloads",
        "colima_vm" | "docker_desktop_vm" => "container_storage",
        "unused_clone" => "unused_clones",
        "unused_app_data" => "unused_apps",
        "unused_toolchain" => "unused_toolchains",
        "huggingface_hub" | "ollama_models" | "lm_studio_models" => "ml_models",
        "home_logs_and_temp" => "logs_and_temp",
        "jetbrains_old_versions" => "old_tool_versions",
        "xcode_archives" | "android_avd" | "conda_envs" => "developer_data",
        "app_leftovers" => "app_leftovers",
        "fast_growing" => "growth",
        _ => return None,
    })
}

fn plain_name(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_STORE_PART
        && part != "."
        && part != ".."
        && !part
            .chars()
            .any(|c| c.is_control() || "/\\:*?[]{}$~".contains(c))
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
        || catalog.rules.len() > MAX_RULES
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
        if category_of(&rule.discovery) != Some(rule.category.as_str()) {
            bail!("category does not match the discovery kind");
        }
        match (&rule.store, STORE_KINDS.contains(&rule.discovery.as_str())) {
            (Some(store), true) => check_store(store)?,
            (None, false) => {}
            _ => bail!("a store is required by the store kinds and refused by every other kind"),
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
        let key = FindingsKey {
            scan,
            catalog: hash.clone(),
            ignores: serde_json::to_vec(&ignores)?,
            include_ignored,
            home: home.clone(),
            data_version: self.db.query_row("PRAGMA data_version", [], |r| r.get(0))?,
        };
        let page = |all: &[Finding]| -> Vec<Finding> {
            all.iter()
                .skip(offset)
                .take(limit.min(1000))
                .cloned()
                .collect()
        };
        if let Some(memo) = self.findings_memo.borrow().as_ref()
            && memo.key == key
            && memo.at.elapsed() < FINDINGS_TTL
        {
            return Ok(page(&memo.findings));
        }
        let rollups = self.rollups_available()?;
        let mut output: Vec<Finding> = Vec::new();
        let mut directory_roots: Vec<Vec<u8>> = Vec::new();
        let applicable = |rule: &&Rule| {
            !rule.deprecated && rule.platforms.iter().any(|p| p == std::env::consts::OS)
        };
        let assemble = |rule: &Rule,
                        entry: Entry,
                        subtree: Option<crate::rollup::Totals>,
                        note: Option<String>,
                        activity: Option<Activity>|
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
            let explanation = match note
                .into_iter()
                .chain(activity.as_ref().and_then(Activity::describe))
                .reduce(|a, b| format!("{a}; {b}"))
            {
                Some(note) => format!("{} ({note})", rule.explanation),
                None => rule.explanation.clone(),
            };
            let (effective, reason) = match &verdict.gate {
                crate::cleanup::Gate::Denied(why) => ("protected", why.clone()),
                crate::cleanup::Gate::Warn(why) => {
                    (rule.tier.as_str(), format!("{explanation} ⚠ {why}"))
                }
                crate::cleanup::Gate::Allowed => (rule.tier.as_str(), explanation),
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
                activity,
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
            let entry = self.entry_at(scan, &relative)?;
            let Some(entry) = entry.filter(|e| e.kind == "directory") else {
                continue;
            };
            directory_roots.push(entry.path.clone());
            let subtree = self.subtree(&entry.path)?;
            output.extend(assemble(rule, entry, subtree, None, None)?);
        }
        // First, so that folders inside a clone are counted once, in the clone's own estimate.
        if let Some(rule) = catalog
            .rules
            .iter()
            .filter(applicable)
            .find(|r| r.discovery == "unused_clone")
        {
            for path in self.unused_clones(scan, &root)? {
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                output.extend(assemble(rule, entry, subtree, None, None)?);
            }
        }
        if let Some(rule) = catalog
            .rules
            .iter()
            .filter(applicable)
            .find(|r| r.discovery == "stale_dependencies")
        {
            for path in self.stale_dependencies(scan, &root)? {
                if directory_roots
                    .iter()
                    .any(|d| policy::inside(&path, d, true))
                {
                    continue;
                }
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                let activity = self.output_activity(scan, &root, &path)?;
                output.extend(assemble(rule, entry, subtree, None, Some(activity))?);
            }
        }
        let rule_of = |kind: &str| {
            catalog
                .rules
                .iter()
                .filter(applicable)
                .find(|r| r.discovery == kind)
        };
        let mut found: Vec<(&Rule, Vec<Vec<u8>>)> = Vec::new();
        if let Some(rule) = rule_of("project_build_output") {
            found.push((
                rule,
                self.project_outputs(scan, &root, SUGGEST_MIN_BYTES, None)?,
            ));
        }
        if let Some(rule) = rule_of("ai_tool_old_versions") {
            found.push((rule, self.ai_tool_old_versions(scan, &root, &home)?));
        }
        // Before the broader kinds below, so a known tool's store keeps its own explanation.
        for rule in catalog.rules.iter().filter(applicable) {
            if let Some(store) = &rule.store {
                found.push((rule, self.store_items(scan, &root, &home, store)?));
            }
        }
        if let Some(rule) = rule_of("editor_old_extensions") {
            found.push((rule, self.editor_old_extensions(scan, &root, &home)?));
        }
        if cfg!(target_os = "macos")
            && let Some(rule) = rule_of("app_caches")
        {
            found.push((rule, self.app_caches(scan, &root, &home)?));
        }
        // Before app data, so a toolchain inside an application area is judged as a toolchain.
        if let Some(rule) = rule_of("unused_toolchain") {
            found.push((rule, self.unused_toolchains(scan, &root, &home)?));
        }
        if let Some(rule) = rule_of("toolchain_old_versions") {
            found.push((rule, self.toolchain_old_versions(scan, &root, &home)?));
        }
        if let Some(rule) = rule_of("jetbrains_old_versions") {
            found.push((rule, self.jetbrains_old_versions(scan, &root, &home)?));
        }
        if let Some(rule) = rule_of("conda_envs") {
            found.push((rule, self.conda_envs(scan, &root, &home)?));
        }
        if let Some(rule) = rule_of("home_logs_and_temp") {
            found.push((rule, self.home_logs_and_temp(scan, &root, &home)?));
        }
        if let Some(rule) = rule_of("unused_app_data") {
            found.push((rule, self.unused_app_data(scan, &root, &home)?));
        }
        // After app data: a folder old enough for both keeps that kind's suggestion.
        if cfg!(target_os = "macos")
            && let Some(rule) = rule_of("app_leftovers")
        {
            let apps = installed_apps(&application_folders(&home));
            found.push((rule, self.app_leftovers(scan, &root, &home, &apps)?));
        }
        if let Some(rule) = rule_of("old_installers") {
            found.push((rule, self.old_installers(scan, &root, &home)?));
        }
        let unrecognized = rule_of("unrecognized_store");
        for (rule, paths) in found {
            for path in paths {
                if directory_roots
                    .iter()
                    .any(|d| policy::inside(&path, d, true))
                {
                    continue;
                }
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = if entry.kind == "directory" {
                    self.subtree(&entry.path)?
                } else {
                    None
                };
                let activity = (rule.discovery == "project_build_output")
                    .then(|| self.output_activity(scan, &root, &path))
                    .transpose()?;
                output.extend(assemble(rule, entry, subtree, None, activity)?);
            }
        }
        if let Some(rule) = rule_of("superseded_copies") {
            for path in self.superseded_copies(scan, &root, &home, &directory_roots)? {
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                output.extend(assemble(rule, entry, subtree, None, None)?);
            }
        }
        // Last of the folder kinds: only what no rule above explains.
        if let Some(rule) = unrecognized {
            for path in self.unrecognized_stores(scan, &root, &home, &catalog, &directory_roots)? {
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                output.extend(assemble(rule, entry, subtree, None, None)?);
            }
        }
        if let Some(rule) = rule_of("fast_growing") {
            let report = self.grew(200)?;
            let since = report.since.map(|at| (crate::now() - at).max(0) / 86_400);
            for grown in report.items {
                if grown.change < GROWTH_MIN_BYTES
                    || directory_roots.iter().any(|d| {
                        policy::inside(&grown.path, d, true) || policy::inside(d, &grown.path, true)
                    })
                {
                    continue;
                }
                let Some(entry) = self.entry_at(scan, &grown.path)? else {
                    continue;
                };
                let note = format!(
                    "grew by {} MB since the scan {} days earlier",
                    grown.change >> 20,
                    since.unwrap_or(0)
                );
                directory_roots.push(entry.path.clone());
                let subtree = self.subtree(&entry.path)?;
                output.extend(assemble(rule, entry, subtree, Some(note), None)?);
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
                "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND kind='file' AND apparent>=104857600 ORDER BY allocated DESC,path LIMIT 10000",
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
                output.extend(assemble(rule, entry, None, None, None)?);
            }
        }
        output.sort_by(|a, b| {
            b.estimate
                .cmp(&a.estimate)
                .then(a.entry.path.cmp(&b.entry.path))
        });
        let found = page(&output);
        self.findings_memo.replace(Some(FindingsMemo {
            key,
            at: std::time::Instant::now(),
            findings: output,
        }));
        Ok(found)
    }

    fn entry_at(&self, scan: i64, path: &[u8]) -> Result<Option<Entry>> {
        Ok(self
            .db
            .query_row(
                "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                params![scan, crate::index::split_path(path).0, crate::index::split_path(path).1],
                crate::index::row_entry,
            )
            .optional()?)
    }

    /// The biggest folders that no finding explains: each one's size minus the findings inside it,
    /// followed down while one subfolder holds most of what is left.
    pub fn unexplained(&self, limit: usize) -> Result<Vec<Unexplained>> {
        self.unexplained_in(limit, &platform::home()?)
    }
    pub fn unexplained_in(&self, limit: usize, home: &std::path::Path) -> Result<Vec<Unexplained>> {
        let Some(scan) = self
            .db
            .query_row("SELECT scan_id FROM active WHERE singleton=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        else {
            return Ok(vec![]);
        };
        let findings = self.findings_in(1000, 0, true, home)?;
        let explained = |path: &[u8]| -> (u64, bool) {
            let mut bytes = 0;
            for f in &findings {
                if policy::inside(path, &f.entry.path, true) {
                    return (0, true);
                }
                if policy::inside(&f.entry.path, path, true) {
                    bytes += f.estimate;
                }
            }
            (bytes, false)
        };
        let mut statement = self.db.prepare(
            "SELECT path,allocated FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x'' ORDER BY allocated DESC LIMIT 50",
        )?;
        let mut children = |parent: &[u8]| -> Result<Vec<(Vec<u8>, u64)>> {
            let mut list = Vec::new();
            for row in statement.query_map(params![scan, parent], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, crate::index::unsigned(r, 1)?))
            })? {
                let (path, allocated) = row?;
                let (inside, whole) = explained(&path);
                if !whole {
                    list.push((path, allocated.saturating_sub(inside)));
                }
            }
            Ok(list)
        };
        let mut out: Vec<Unexplained> = Vec::new();
        for (mut path, mut left) in children(b"")? {
            if left < UNEXPLAINED_MIN_BYTES {
                continue;
            }
            for _ in 0..32 {
                let Some((child, child_left)) = children(&path)?.into_iter().max_by_key(|c| c.1)
                else {
                    break;
                };
                if child_left * 5 < left * 4 {
                    break;
                }
                (path, left) = (child, child_left);
            }
            let allocated = self.subtree(&path)?.map_or(left, |t| t.allocated);
            out.push(Unexplained {
                path,
                allocated,
                unexplained: left,
            });
        }
        out.sort_by(|a, b| b.unexplained.cmp(&a.unexplained).then(a.path.cmp(&b.path)));
        out.truncate(limit.clamp(1, 200));
        Ok(out)
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
    use std::path::Path;

    #[test]
    fn a_tool_folder_from_the_environment_never_takes_in_the_home_folder_or_a_personal_folder() {
        let home = Path::new("/home/me");
        for root in [
            "/home/me",
            "/home",
            "/",
            "/home/me/Documents",
            "/home/me/projects",
        ] {
            assert!(!tool_root_allowed(home, Path::new(root)), "{root}");
        }
        for root in [
            "/home/me/.rustup",
            "/home/me/Documents/sdk",
            "/home/me/.cache",
            "/opt/rust",
        ] {
            assert!(tool_root_allowed(home, Path::new(root)), "{root}");
        }
    }

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
    fn toolchain_version_folders_group_by_line_and_skip_channels() {
        let rust = VersionScheme::Rust;
        assert_eq!(
            rust.parse("1.75.0-aarch64-apple-darwin"),
            Some(("release-aarch64-apple-darwin".into(), vec![1, 75, 0]))
        );
        assert_eq!(
            rust.parse("nightly-2024-01-31-x86_64-unknown-linux-gnu"),
            Some(("nightly-x86_64-unknown-linux-gnu".into(), vec![2024, 1, 31]))
        );
        for channel in [
            "stable-aarch64-apple-darwin",
            "beta-aarch64-apple-darwin",
            "nightly-aarch64-apple-darwin",
            "1.75-aarch64-apple-darwin",
        ] {
            assert_eq!(rust.parse(channel), None, "{channel}");
        }
        assert_eq!(
            VersionScheme::Node.parse("v20.11.1"),
            Some(("20".into(), vec![20, 11, 1]))
        );
        assert_eq!(VersionScheme::Node.parse("20.11.1"), None);
        assert_eq!(
            VersionScheme::Python.parse("3.11.4"),
            Some(("3.11".into(), vec![3, 11, 4]))
        );
        for odd in ["3.12.0b1", "miniconda3-latest", "3.11", "my-env"] {
            assert_eq!(VersionScheme::Python.parse(odd), None, "{odd}");
        }
    }
    #[test]
    fn extension_folder_names_split_into_id_version_and_platform() {
        let v = |n| extension_version(n);
        assert_eq!(
            v("ms-python.python-2024.1.0"),
            Some(("ms-python.python".into(), vec![2024, 1, 0], String::new()))
        );
        assert_eq!(
            v("rust-lang.rust-analyzer-0.3.2-darwin-arm64"),
            Some((
                "rust-lang.rust-analyzer".into(),
                vec![0, 3, 2],
                "darwin-arm64".into()
            ))
        );
        assert_eq!(
            v("Esbenp.Prettier-Vscode-10.1.0").unwrap().0,
            "esbenp.prettier-vscode"
        );
        assert!(v("extensions.json").is_none());
        assert!(v("ms-python.python-latest").is_none());
        assert!(v("nopublisher-1.0.0").is_none());
        assert!(v("a.b-1.0").is_none());
    }
    #[test]
    fn new_tool_folders_use_each_tools_documented_default() {
        let home = std::path::Path::new("/Users/dev");
        let root = |kind| discovery_root(kind, home);
        if std::env::var_os("BUN_INSTALL_CACHE_DIR").is_none()
            && std::env::var_os("BUN_INSTALL").is_none()
        {
            assert_eq!(root("bun_cache").unwrap(), home.join(".bun/install/cache"));
        }
        assert_eq!(
            root("maven_repository").unwrap(),
            home.join(".m2/repository")
        );
        if std::env::var_os("HF_HUB_CACHE").is_none() && std::env::var_os("HF_HOME").is_none() {
            assert_eq!(
                root("huggingface_hub").unwrap(),
                home.join(".cache/huggingface/hub")
            );
        }
        if std::env::var_os("OLLAMA_MODELS").is_none() {
            assert_eq!(root("ollama_models").unwrap(), home.join(".ollama/models"));
        }
        assert_eq!(
            root("lm_studio_models").unwrap(),
            home.join(".lmstudio/models")
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                root("xcode_derived_data").unwrap(),
                home.join("Library/Developer/Xcode/DerivedData")
            );
            assert_eq!(
                root("xcode_device_support").unwrap(),
                home.join("Library/Developer/Xcode/iOS DeviceSupport")
            );
            assert_eq!(
                root("simulator_caches").unwrap(),
                home.join("Library/Developer/CoreSimulator/Caches")
            );
            assert_eq!(
                root("cocoapods_cache").unwrap(),
                home.join("Library/Caches/CocoaPods")
            );
            assert_eq!(
                root("jetbrains_caches").unwrap(),
                home.join("Library/Caches/JetBrains")
            );
        } else {
            assert!(root("xcode_derived_data").is_none());
            assert!(root("cocoapods_cache").is_none());
        }
        for kind in ["huggingface_hub", "ollama_models", "lm_studio_models"] {
            let rule = parse(include_str!("../../../catalog/bundled.toml"))
                .unwrap()
                .rules
                .into_iter()
                .find(|rule| rule.discovery == kind)
                .unwrap();
            assert_eq!(rule.category, "ml_models", "{kind}");
        }
    }
    #[test]
    fn versions_fold_to_one_name() {
        for (a, b) in [
            ("/app-1.2.0-beta.1.tgz", "/app-1.3.0.tgz"),
            ("/app-0.1.0-rc2.zip", "/app-0.1.0.zip"),
            ("/v12/lib.so", "/v13/lib.so"),
            ("/omg-0.1.0-r3/omg", "/omg-0.1.0/omg"),
        ] {
            assert_eq!(fold_digits(a.as_bytes()), fold_digits(b.as_bytes()), "{a}");
        }
        assert_ne!(fold_digits(b"/app-1.tgz"), fold_digits(b"/app-x.tgz"));
        assert_eq!(fold_digits(b"/a1.2.tgz"), b"/a#.tgz");
    }
    #[test]
    fn strict_catalog_and_tier_contract() {
        let payload = include_str!("../../../catalog/bundled.toml");
        assert_eq!(parse(payload).unwrap().rules.len(), 72);
        let claude = "roots = [\"~/.claude/projects\"]";
        assert!(payload.contains(claude));
        for bad in [
            "roots = [\"/etc\"]",
            "roots = [\"~/Documents\"]",
            "roots = [\"~/.claude/../Documents\"]",
            "roots = [\"$home/.x\"]",
            "roots = [\"~\"]",
            "roots = []",
        ] {
            assert!(parse(&payload.replace(claude, bad)).is_err(), "{bad}");
        }
        let patterns = "patterns = [\"*\", \"*.jsonl\"]";
        assert!(payload.contains(patterns));
        for bad in [
            "patterns = [\"..\"]",
            "patterns = [\"a/b\"]",
            "patterns = [\"[ab]\"]",
            "patterns = []",
        ] {
            assert!(parse(&payload.replacen(patterns, bad, 1)).is_err(), "{bad}");
        }
        assert!(
            parse(&payload.replacen("min_age_days = 7", "min_age_days = 7\nshell = \"x\"", 1))
                .is_err()
        );
        assert!(
            parse(&payload.replacen(
                "discovery = \"tool_sessions\"",
                "discovery = \"large_file\"",
                1
            ))
            .is_err()
        );
        assert!(glob(b"rollout-*.jsonl", b"rollout-2026-01-02-abc.jsonl"));
        assert!(glob(b"*.json*", b"a.json") && glob(b"*.json*", b"a.jsonl"));
        assert!(!glob(b"*.jsonl", b"a.json") && glob(b"?b", b"ab") && !glob(b"?b", b"b"));
        let home = std::path::Path::new("/home/me");
        assert_eq!(
            store_root("~/.claude/projects", home),
            Some(home.join(".claude/projects"))
        );
        assert_eq!(store_root("$OMG_TEST_UNSET_VARIABLE/x", home), None);
        assert!(parse(&payload.replace("action = \"info\"", "action = \"shell\"")).is_err());
        assert!(parse(&payload.replace("tier = \"info\"", "tier = \"safe\"")).is_err());
        assert!(parse(&format!("{payload}\ncommand = 'rm -rf /'")).is_err());
        assert!(parse(&"x".repeat(MAX_CATALOG + 1)).is_err());
        assert_eq!(payload.matches("category = \"large_files\"").count(), 1);
        for category in ["package_caches", "build_artifacts", "logs_and_temp"] {
            let moved = payload.replace(
                "category = \"large_files\"",
                &format!("category = \"{category}\""),
            );
            assert!(parse(&moved).is_err(), "large files moved to {category}");
        }
        assert!(DISCOVERIES.iter().all(|d| category_of(d).is_some()));
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
                        crate::index::split_path(path.as_bytes()).0,
                        crate::index::split_path(path.as_bytes()).1,
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
                "UPDATE entries SET identity='replacement' WHERE parent=x'' AND name=?",
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
