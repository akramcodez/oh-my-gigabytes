use crate::{Entry, Index, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_CATALOG: usize = 256 * 1024;
/// Builtin discovery kinds. A catalog can only refer to these; it can never name a path itself.
pub const DISCOVERIES: [&str; 37] = [
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
    "ai_tool_sessions",
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
];

/// Logs and temporary files unchanged for this long are suggested.
pub const LOGS_MIN_AGE_DAYS: i64 = 7;

/// A version folder: parsed version, newest change time, path.
type VersionDir = (Vec<u64>, i64, Vec<u8>);

#[derive(Clone, Copy)]
enum VersionScheme {
    Node,
    Python,
    Rust,
}

impl VersionScheme {
    /// The line a version folder belongs to (only one per line is kept) and its comparable number.
    /// Anything that is not plainly a released version is not ours to judge.
    fn parse(self, name: &str) -> Option<(String, Vec<u64>)> {
        fn numbers(text: &str, count: usize) -> Option<Vec<u64>> {
            let parts: Vec<u64> = text
                .split('.')
                .map(|p| p.parse().ok().filter(|_| !p.is_empty()))
                .collect::<Option<_>>()?;
            (parts.len() == count).then_some(parts)
        }
        match self {
            Self::Node => {
                let version = numbers(name.strip_prefix('v')?, 3)?;
                Some((version[0].to_string(), version))
            }
            Self::Python => {
                let version = numbers(name, 3)?;
                Some((format!("{}.{}", version[0], version[1]), version))
            }
            Self::Rust => {
                let (head, rest) = name.split_once('-')?;
                if let Some(version) = numbers(head, 3) {
                    return Some((format!("release-{rest}"), version));
                }
                if head != "nightly" {
                    return None;
                }
                let mut fields = rest.splitn(4, '-');
                let (year, month, day, target) = (
                    fields.next()?,
                    fields.next()?,
                    fields.next()?,
                    fields.next()?,
                );
                if year.len() != 4 || month.len() != 2 || day.len() != 2 || target.is_empty() {
                    return None;
                }
                let date = vec![year.parse().ok()?, month.parse().ok()?, day.parse().ok()?];
                Some((format!("nightly-{target}"), date))
            }
        }
    }
}

/// An application or toolchain unchanged for this long (about five months) counts as unused.
pub const UNUSED_DAYS: i64 = 150;

fn absolute_env(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Folders whose direct subfolders each belong to one application: the per-user data, config and
/// cache areas of each platform. The defaults are always included; an absolute override is added.
pub fn app_data_parents(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = if cfg!(windows) {
        let mut out = vec![
            home.join("AppData").join("Local"),
            home.join("AppData").join("LocalLow"),
            home.join("AppData").join("Roaming"),
        ];
        out.extend(absolute_env("LOCALAPPDATA"));
        out.extend(absolute_env("APPDATA"));
        out
    } else if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support")]
    } else {
        let mut out = vec![
            home.join(".local/share"),
            home.join(".config"),
            home.join(".cache"),
        ];
        out.extend(absolute_env("XDG_DATA_HOME"));
        out.extend(absolute_env("XDG_CONFIG_HOME"));
        out.extend(absolute_env("XDG_CACHE_HOME"));
        out
    };
    out.retain(|parent| tool_root_allowed(home, parent));
    out.sort();
    out.dedup();
    out
}

/// Subfolders of the application areas that the system, the desktop or a VM manages, or that hold
/// installed programs, credentials or OMG's own state. Compared without regard to case.
const APP_DATA_SKIP: [&str; 34] = [
    // Windows
    "microsoft",
    "packages",
    "programs",
    "temp",
    "comms",
    "connecteddevicesplatform",
    "d3dscache",
    "crashdumps",
    "peerdistrepub",
    "publishers",
    "virtualstore",
    "history",
    "application data",
    "temporary internet files",
    "docker",
    "wsl",
    "lxss",
    // Linux desktop
    "trash",
    "keyrings",
    "containers",
    "flatpak",
    "systemd",
    "gnome-shell",
    "applications",
    "fonts",
    "icons",
    "dconf",
    "autostart",
    "evolution",
    // macOS
    "crashreporter",
    "knowledge",
    "fileprovider",
    "clouddocs",
    // OMG itself
    "omg",
];

/// A development toolchain: the folders it installs into (its default and any absolute override),
/// other folders it writes to while in use, and the names of project folders its builds write (a
/// recent one means it is still used).
pub struct Toolchain {
    pub name: &'static str,
    pub roots: Vec<std::path::PathBuf>,
    pub also: Vec<std::path::PathBuf>,
    pub outputs: &'static [&'static str],
}

/// The default location plus an absolute override, without duplicates.
fn located(default: std::path::PathBuf, overridden: &str) -> Vec<std::path::PathBuf> {
    let mut out = vec![default];
    out.extend(absolute_env(overridden).filter(|p| *p != out[0]));
    out
}

/// Per-user toolchain installs, at each tool's documented location or absolute override.
pub fn toolchains(home: &std::path::Path) -> Vec<Toolchain> {
    let mut out = declared_toolchains(home);
    for toolchain in &mut out {
        toolchain.roots.retain(|root| tool_root_allowed(home, root));
    }
    out
}

fn declared_toolchains(home: &std::path::Path) -> Vec<Toolchain> {
    let gradle = located(home.join(".gradle"), "GRADLE_USER_HOME");
    let android = if cfg!(windows) {
        home.join("AppData")
            .join("Local")
            .join("Android")
            .join("Sdk")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Android/sdk")
    } else {
        home.join("Android/Sdk")
    };
    let nvm = if cfg!(windows) {
        located(home.join("AppData").join("Roaming").join("nvm"), "NVM_HOME")
    } else {
        located(home.join(".nvm"), "NVM_DIR")
    };
    let mut out = vec![
        Toolchain {
            name: "Rust (rustup)",
            roots: located(home.join(".rustup"), "RUSTUP_HOME"),
            also: located(home.join(".cargo"), "CARGO_HOME"),
            outputs: &["target"],
        },
        Toolchain {
            name: "Node.js (nvm)",
            roots: nvm,
            also: vec![home.join(".npm")],
            outputs: &["node_modules"],
        },
        Toolchain {
            name: "Python (pyenv)",
            roots: located(home.join(".pyenv"), "PYENV_ROOT"),
            also: vec![],
            outputs: &["__pycache__", ".venv", "venv"],
        },
        Toolchain {
            name: "Android SDK",
            roots: located(android, "ANDROID_HOME"),
            also: [gradle.clone(), vec![home.join(".android")]].concat(),
            outputs: &["build", ".gradle"],
        },
    ];
    if !cfg!(windows) {
        out.push(Toolchain {
            name: "Java (SDKMAN!)",
            roots: located(home.join(".sdkman"), "SDKMAN_DIR"),
            also: [gradle, vec![home.join(".m2")]].concat(),
            outputs: &["build", ".gradle", "target"],
        });
    }
    out
}

/// (version, modified time in ns, path) of one installed extension folder.
type ExtensionVersion = (Vec<u64>, i64, Vec<u8>);

/// Splits an extension folder name into (`publisher.name`, version, platform suffix). The version is
/// the first `-` followed by three dot-separated numbers; anything else is not judged.
fn extension_version(name: &str) -> Option<(String, Vec<u64>, String)> {
    for (at, _) in name.match_indices('-') {
        let rest = &name[at + 1..];
        let (version, suffix) = rest.split_once('-').unwrap_or((rest, ""));
        let parts: Option<Vec<u64>> = version.split('.').map(|p| p.parse().ok()).collect();
        if let Some(parts) = parts.filter(|p| p.len() == 3)
            && at > 0
            && name[..at].contains('.')
        {
            return Some((name[..at].to_ascii_lowercase(), parts, suffix.to_string()));
        }
    }
    None
}

/// A `~/Library/Caches` folder the system manages, including one without Apple's usual prefix.
pub(crate) fn apple_cache(name: &str) -> bool {
    name.starts_with("com.apple.") || name == "GeoServices"
}

/// What tools write into a clone without it being anyone's work. `build`, `dist` and `coverage` are
/// left out on purpose: those names can also be tracked folders that someone edited.
const CLONE_NOISE: [&str; 8] = [
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".git",
    ".DS_Store",
    "Thumbs.db",
];
/// Timestamps of a checkout and its index can differ by this much without anyone having edited.
const CLONE_SLACK_NS: i128 = 2_000_000_000;

/// Smallest build output or application cache worth suggesting.
const SUGGEST_MIN_BYTES: i64 = 50 << 20;
/// A session transcript this recent may still be open or about to be resumed.
const SESSION_MIN_AGE_DAYS: i64 = 7;
/// Smallest transcript worth suggesting: they cannot be recreated, so tiny ones are not worth it.
const SESSION_MIN_BYTES: i64 = 1 << 20;

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
        if ![
            "package_caches",
            "build_artifacts",
            "large_files",
            "app_caches",
            "old_tool_versions",
            "session_history",
            "container_storage",
            "unused_clones",
            "unused_apps",
            "unused_toolchains",
            "ml_models",
            "logs_and_temp",
        ]
        .contains(&rule.category.as_str())
        {
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
                    "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                    params![scan, crate::index::split_path(&relative).0, crate::index::split_path(&relative).1],
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
        // First, so that folders inside a clone are counted once, in the clone's own estimate.
        if let Some(rule) = catalog
            .rules
            .iter()
            .filter(applicable)
            .find(|r| r.discovery == "unused_clone")
        {
            for path in self.unused_clones(scan, &root)? {
                let Some(entry) = self
                    .db
                    .query_row(
                        "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                        params![scan, crate::index::split_path(&path).0, crate::index::split_path(&path).1],
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
                        "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                        params![scan, crate::index::split_path(&path).0, crate::index::split_path(&path).1],
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
        let mut found: Vec<(&str, Vec<Vec<u8>>)> = vec![
            (
                "project_build_output",
                self.project_outputs(scan, SUGGEST_MIN_BYTES, None)?,
            ),
            ("ai_tool_old_versions", {
                self.ai_tool_old_versions(scan, &root, &home)?
            }),
            ("ai_tool_sessions", {
                self.ai_tool_sessions(scan, &root, &home)?
            }),
            (
                "editor_old_extensions",
                self.editor_old_extensions(scan, &root, &home)?,
            ),
        ];
        if cfg!(target_os = "macos") {
            found.push(("app_caches", self.app_caches(scan, &root, &home)?));
        }
        // Before app data, so a toolchain inside an application area is judged as a toolchain.
        found.push((
            "unused_toolchain",
            self.unused_toolchains(scan, &root, &home)?,
        ));
        found.push((
            "toolchain_old_versions",
            self.toolchain_old_versions(scan, &root, &home)?,
        ));
        found.push((
            "home_logs_and_temp",
            self.home_logs_and_temp(scan, &root, &home)?,
        ));
        found.push(("unused_app_data", self.unused_app_data(scan, &root, &home)?));
        for (discovery, paths) in found {
            let Some(rule) = catalog
                .rules
                .iter()
                .filter(applicable)
                .find(|r| r.discovery == discovery)
            else {
                continue;
            };
            for path in paths {
                if directory_roots
                    .iter()
                    .any(|d| policy::inside(&path, d, true))
                {
                    continue;
                }
                let Some(entry) = self
                    .db
                    .query_row(
                        "SELECT CAST(parent||name AS BLOB) AS path,parent,kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=? AND parent=? AND name=?",
                        params![scan, crate::index::split_path(&path).0, crate::index::split_path(&path).1],
                        crate::index::row_entry,
                    )
                    .optional()?
                else {
                    continue;
                };
                directory_roots.push(entry.path.clone());
                let subtree = if entry.kind == "directory" {
                    self.subtree(&entry.path)?
                } else {
                    None
                };
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
    /// Git clones untouched for `policy::STALE_DAYS` with nothing local to lose: see
    /// `policy::pristine_clone`, plus nothing in the working tree changed after the checkout. The
    /// change time is used because moving or copying a file in keeps its old modification time.
    fn unused_clones(&self, scan: i64, root: &std::path::Path) -> Result<Vec<Vec<u8>>> {
        let mut statement = self.db.prepare(
            "SELECT parent FROM entries WHERE scan_id=? AND kind='directory' AND name=CAST('/.git' AS BLOB) UNION SELECT parent FROM entries WHERE scan_id=? AND kind='directory' AND parent=x'' AND name=CAST('.git' AS BLOB)",
        )?;
        let mut repositories: Vec<Vec<u8>> = statement
            .query_map(params![scan, scan], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        repositories.sort_by_key(|p| p.len());
        let gaps = self.gaps(scan)?;
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let mut changed = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent>=?2 AND parent<?3 AND (parent=?2 OR parent>?4) AND changed_ns>?5",
        )?;
        let mut chosen: Vec<Vec<u8>> = Vec::new();
        for repository in repositories {
            let hidden = repository
                .split(|b| *b == b'/')
                .any(|part| part.first() == Some(&b'.'));
            if repository.is_empty()
                || hidden
                || gaps.covers(&repository)
                || chosen.iter().any(|c| policy::inside(&repository, c, true))
            {
                continue;
            }
            let newest = self.subtree(&repository)?.map_or(0, |t| t.newest);
            if newest <= 0 || newest >= cutoff {
                continue;
            }
            let Some(written) = policy::pristine_clone(&root.join(platform::path(&repository)?))
            else {
                continue;
            };
            let limit = i64::try_from(written.saturating_add(CLONE_SLACK_NS)).unwrap_or(i64::MAX);
            let mut upper = repository.clone();
            upper.push(b'0');
            let mut prefix = repository.clone();
            prefix.push(b'/');
            let mut rows = changed.query(params![scan, repository, upper, prefix, limit])?;
            let mut edited = false;
            while let Some(row) = rows.next()? {
                let path: Vec<u8> = row.get(0)?;
                let below = &path[repository.len()..];
                let noise = below
                    .split(|b| *b == b'/')
                    .any(|part| CLONE_NOISE.iter().any(|name| name.as_bytes() == part));
                if !noise {
                    edited = true;
                    break;
                }
            }
            if !edited {
                chosen.push(repository);
            }
        }
        Ok(chosen)
    }
    /// Applications' folders directly in `~/Library/Caches`, which apps must be able to recreate.
    /// Apple's own are left out: the system manages them and they come straight back.
    fn app_caches(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let Some(caches) = crate::rollup::relative_raw(root, &home.join("Library/Caches")) else {
            return Ok(vec![]);
        };
        let mut statement = self.db.prepare(
            "SELECT path FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let rows = statement.query_map(params![scan, caches, SUGGEST_MIN_BYTES], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        let mut out = Vec::new();
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            if !apple_cache(&String::from_utf8_lossy(
                name.strip_prefix(b"/").unwrap_or(name),
            )) {
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Toolchains not changed for `UNUSED_DAYS`, whose other folders did not change either, and with
    /// no project build folder of theirs written since. Unreadable parts mean unknown, not unused.
    fn unused_toolchains(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - UNUSED_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut out = Vec::new();
        for toolchain in toolchains(home) {
            for install in &toolchain.roots {
                let Some(path) = crate::rollup::relative_raw(root, install) else {
                    continue;
                };
                let Some(totals) = self.subtree(&path)? else {
                    continue;
                };
                if path.is_empty()
                    || totals.allocated < SUGGEST_MIN_BYTES as u64
                    || totals.newest <= 0
                    || totals.newest >= cutoff
                    || gaps.covers(&path)
                {
                    continue;
                }
                let mut recent = false;
                for other in &toolchain.also {
                    let Some(other) = crate::rollup::relative_raw(root, other) else {
                        continue;
                    };
                    if let Some(t) = self.subtree(&other)? {
                        recent |= t.newest >= cutoff || gaps.covers(&other);
                    }
                }
                for name in toolchain.outputs {
                    recent = recent || self.recent_folder_named(scan, name, cutoff)?;
                }
                if !recent && !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Whether any folder called `name` anywhere in the scan changed at or after `since`.
    fn recent_folder_named(&self, scan: i64, name: &str, since: i64) -> Result<bool> {
        let mut nested = b"/".to_vec();
        nested.extend_from_slice(name.as_bytes());
        Ok(self
            .db
            .prepare_cached(
                "SELECT 1 FROM dir_totals WHERE scan_id=?1 AND newest>=?2 AND (path=?3 OR substr(path,-?4)=?5) LIMIT 1",
            )?
            .exists(params![
                scan,
                since,
                name.as_bytes(),
                i64::try_from(nested.len())?,
                nested
            ])?)
    }
    /// Applications' own folders in the per-user data areas, 50 MB or larger, not changed for
    /// `UNUSED_DAYS`, with nothing protected inside (a database or credential inside means the
    /// folder is not suggested at all). Folders that hold or sit in a known cache or toolchain are
    /// left to those rules.
    fn unused_app_data(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - UNUSED_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut known: Vec<Vec<u8>> = DISCOVERIES
            .iter()
            .filter_map(|kind| discovery_root(kind, home))
            .chain(toolchains(home).into_iter().flat_map(|t| t.roots))
            .chain([home.join(".local/share/claude"), home.join(".claude")])
            .filter_map(|p| crate::rollup::relative_raw(root, &p))
            .collect();
        known.retain(|p| !p.is_empty());
        let mut statement = self.db.prepare(
            "SELECT path,newest,protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out: Vec<Vec<u8>> = Vec::new();
        for parent in app_data_parents(home) {
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows = statement.query_map(params![scan, parent, SUGGEST_MIN_BYTES], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for row in rows {
                let (path, newest, protected) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                let newest = newest.unwrap_or(0);
                if name.starts_with('.')
                    || name.starts_with("com.apple.")
                    || APP_DATA_SKIP.contains(&name.as_str())
                    || protected != 0
                    || newest <= 0
                    || newest >= cutoff
                    || gaps.covers(&path)
                    || known
                        .iter()
                        .any(|k| policy::inside(&path, k, true) || policy::inside(k, &path, true))
                    || out.contains(&path)
                {
                    continue;
                }
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Installed versions of the AI coding tool other than the newest and the one its launcher
    /// points at. An unreadable launcher only means the newest is the one kept.
    fn ai_tool_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let directory = home.join(".local/share/claude/versions");
        let Some(parent) = crate::rollup::relative_raw(root, &directory) else {
            return Ok(vec![]);
        };
        let in_use = std::fs::read_link(home.join(".local/bin/claude")).ok();
        let mut statement = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=? AND parent=?",
        )?;
        let rows = statement.query_map(params![scan, parent], |r| r.get::<_, Vec<u8>>(0))?;
        let mut versions: Vec<(Vec<u64>, bool, Vec<u8>)> = Vec::new();
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            let name =
                String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name)).into_owned();
            // Only names that are plainly a version: anything else is not ours to judge.
            let parts: Option<Vec<u64>> = name.split('.').map(|p| p.parse().ok()).collect();
            let Some(parts) = parts else { continue };
            let used = in_use.as_ref().is_some_and(|target| {
                target
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy() == name)
            });
            versions.push((parts, used, path));
        }
        versions.sort();
        versions.pop();
        Ok(versions
            .into_iter()
            .filter(|(_, used, _)| !used)
            .map(|(_, _, path)| path)
            .collect())
    }
    /// Extension folders (`publisher.name-1.2.3`, maybe with a platform suffix) that a newer version
    /// of the same extension replaced in VS Code or Cursor. Only names; the editors' own lists are
    /// file contents and are not read. The newer version must be a day old, so the editor has had
    /// the chance to restart onto it.
    fn editor_old_extensions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let settled = (crate::now() - 86_400).saturating_mul(1_000_000_000);
        let mut statement = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB),kind,modified_ns FROM entries WHERE scan_id=? AND parent=?",
        )?;
        let mut out = Vec::new();
        for editor in [".vscode", ".cursor"] {
            let Some(parent) =
                crate::rollup::relative_raw(root, &home.join(editor).join("extensions"))
            else {
                continue;
            };
            let rows = statement.query_map(params![scan, parent], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(i64::MAX),
                ))
            })?;
            let mut groups: std::collections::BTreeMap<(String, String), Vec<ExtensionVersion>> =
                Default::default();
            for row in rows {
                let (path, kind, modified) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                if kind != "directory" {
                    continue;
                }
                let Some((id, version, suffix)) = extension_version(&name) else {
                    continue;
                };
                groups
                    .entry((id, suffix))
                    .or_default()
                    .push((version, modified, path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                let Some((_, newest_time, _)) = versions.pop() else {
                    continue;
                };
                if newest_time < settled {
                    out.extend(versions.into_iter().map(|(_, _, path)| path));
                }
            }
        }
        Ok(out)
    }
    /// Installed versions that a newer one of the same line replaced: Node.js (nvm) within a major
    /// version, Python (pyenv) within a minor version, and Rust (rustup) numbered releases and dated
    /// nightlies per target, once the newest is a day old. Channel toolchains (`stable`, `beta`,
    /// `nightly`) are never suggested.
    /// Version pin files are contents and are not read, so an old version must also be unchanged
    /// for `policy::STALE_DAYS`; each tool installs a pinned version again on demand.
    fn toolchain_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let settled = crate::now() - 86_400;
        let gaps = self.gaps(scan)?;
        let mut folders: Vec<(std::path::PathBuf, VersionScheme)> = Vec::new();
        for toolchain in toolchains(home) {
            for install in toolchain.roots {
                match toolchain.name {
                    "Rust (rustup)" => {
                        folders.push((install.join("toolchains"), VersionScheme::Rust))
                    }
                    "Node.js (nvm)" if cfg!(windows) => {
                        folders.push((install, VersionScheme::Node))
                    }
                    "Node.js (nvm)" => {
                        folders.push((install.join("versions").join("node"), VersionScheme::Node))
                    }
                    "Python (pyenv)" => {
                        folders.push((install.join("versions"), VersionScheme::Python))
                    }
                    _ => {}
                }
            }
        }
        let mut statement = self.db.prepare(
            "SELECT path,newest FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for (folder, scheme) in folders {
            let Some(parent) = crate::rollup::relative_raw(root, &folder) else {
                continue;
            };
            if parent.is_empty() || gaps.covers(&parent) {
                continue;
            }
            let rows = statement.query_map(params![scan, parent], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<i64>>(1)?))
            })?;
            let mut groups: std::collections::BTreeMap<String, Vec<VersionDir>> =
                Default::default();
            for row in rows {
                let (path, newest) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                let Some((line, version)) = scheme.parse(&name) else {
                    continue;
                };
                groups
                    .entry(line)
                    .or_default()
                    .push((version, newest.unwrap_or(0), path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                // A newest version installed within a day may still be on trial.
                if versions
                    .pop()
                    .is_none_or(|(_, newest, _)| newest <= 0 || newest >= settled)
                {
                    continue;
                }
                for (_, newest, path) in versions {
                    // pyenv-virtualenv keeps environments inside the version they were made with.
                    let mut envs = path.clone();
                    envs.extend_from_slice(b"/envs");
                    if newest > 0
                        && newest < cutoff
                        && !gaps.covers(&path)
                        && self.subtree(&envs)?.is_none()
                    {
                        out.push(path);
                    }
                }
            }
        }
        Ok(out)
    }
    /// Logs, crash reports and temporary files the current user's programs left in the home folder:
    /// each direct item of `~/Library/Logs` (macOS) or of `%LOCALAPPDATA%\Temp` and
    /// `%LOCALAPPDATA%\CrashDumps` (Windows), 1 MB or larger and unchanged for
    /// `LOGS_MIN_AGE_DAYS`. The folders themselves are never suggested.
    fn home_logs_and_temp(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let folders: Vec<std::path::PathBuf> = if cfg!(target_os = "macos") {
            vec![home.join("Library/Logs")]
        } else if cfg!(windows) {
            let local = home.join("AppData").join("Local");
            vec![local.join("Temp"), local.join("CrashDumps")]
        } else {
            vec![]
        };
        let cutoff = crate::now() - LOGS_MIN_AGE_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut items = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB),kind,allocated,modified_ns FROM entries WHERE scan_id=? AND parent=? AND excluded IS NULL",
        )?;
        let mut out = Vec::new();
        for folder in folders {
            let Some(parent) = crate::rollup::relative_raw(root, &folder) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows = items.query_map(params![scan, parent], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?;
            for row in rows {
                let (path, kind, allocated, modified) = row?;
                let (size, newest) = match kind.as_str() {
                    "directory" => match self.subtree(&path)? {
                        Some(t) => (Some(t.allocated), t.newest),
                        None => continue,
                    },
                    "file" => (
                        allocated.and_then(|a| u64::try_from(a).ok()),
                        modified.unwrap_or(0) / 1_000_000_000,
                    ),
                    _ => continue,
                };
                if size.is_some_and(|s| s >= 1 << 20)
                    && newest > 0
                    && newest < cutoff
                    && !gaps.covers(&path)
                {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Old session transcripts of the AI coding tool: `projects/<project>/<session>.jsonl`, exactly
    /// that depth, so memory and settings beside or below them are never included.
    fn ai_tool_sessions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let Some(projects) = crate::rollup::relative_raw(root, &home.join(".claude/projects"))
        else {
            return Ok(vec![]);
        };
        let mut low = projects.clone();
        low.push(b'/');
        let mut high = projects.clone();
        high.push(b'0');
        let cutoff = (crate::now() - SESSION_MIN_AGE_DAYS * 86_400).saturating_mul(1_000_000_000);
        let mut statement = self.db.prepare(
            "SELECT parent,name FROM entries WHERE scan_id=? AND parent>? AND parent<? AND kind='file' AND excluded IS NULL AND allocated>=? AND modified_ns<?",
        )?;
        let rows = statement
            .query_map(params![scan, low, high, SESSION_MIN_BYTES, cutoff], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?;
        let mut out = Vec::new();
        for row in rows {
            let (parent, name) = row?;
            let direct = !parent[projects.len() + 1..].contains(&b'/');
            if direct && name.ends_with(b".jsonl") {
                let mut path = parent;
                path.extend_from_slice(&name);
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Top-level dependency folders (`node_modules` next to `package.json`, `target` next to
    /// `Cargo.toml`, Python virtual environments) outside hidden (tool/application) folders.
    fn stale_dependencies(&self, scan: i64) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        self.project_outputs(scan, 1 << 20, Some(cutoff))
    }
    /// The same folders at least `min_bytes` large, and untouched since `untouched_since` if given.
    fn project_outputs(
        &self,
        scan: i64,
        min_bytes: i64,
        untouched_since: Option<i64>,
    ) -> Result<Vec<Vec<u8>>> {
        let mut statement = self.db.prepare(
            "SELECT path FROM dir_totals WHERE scan_id=? AND allocated>=? AND path<>x''",
        )?;
        let mut candidates: Vec<Vec<u8>> = statement
            .query_map(params![scan, min_bytes], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        candidates.sort_by_key(|p| p.len());
        let exists = |path: Vec<u8>| -> Result<bool> {
            Ok(self
                .db
                .query_row(
                    "SELECT 1 FROM entries WHERE scan_id=? AND parent=? AND name=?",
                    params![
                        scan,
                        crate::index::split_path(&path).0,
                        crate::index::split_path(&path).1
                    ],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some())
        };
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
            let Some(cutoff) = untouched_since else {
                chosen.push(path);
                continue;
            };
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
    fn strict_catalog_and_tier_contract() {
        let payload = include_str!("../../../catalog/bundled.toml");
        assert_eq!(parse(payload).unwrap().rules.len(), 37);
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
