use crate::{Entry, Index, now, platform, policy};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

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

/// A folder compared for copies: path, newest change, folded file names, bytes.
type FolderCopy = (Vec<u8>, i64, BTreeSet<Vec<u8>>, u64);
/// Old installers: unchanged this long, at least this big.
const INSTALLER_DAYS: i64 = 30;
const INSTALLER_MIN_BYTES: i64 = 10 << 20;
const INSTALLER_EXTENSIONS: [&str; 22] = [
    "dmg", "pkg", "mpkg", "xip", "iso", "msi", "msix", "exe", "appimage", "deb", "rpm", "apk",
    "ipa", "zip", "tar", "tgz", "tar.gz", "tar.xz", "tar.bz2", "7z", "rar", "vsix",
];
/// Folder copies: the size of a folder worth comparing, how many groups and files are read.
const COPY_MIN_BYTES: i64 = 10 << 20;
const COPY_PARENTS: i64 = 300;
const COPY_MAX_FILES: usize = 5_000;
const COPY_ENTRY_BUDGET: usize = 300_000;
const COPY_SIMILARITY: usize = 60;
const ARTIFACT_EXTENSIONS: [&str; 34] = [
    "tgz", "gz", "zip", "tar", "xz", "bz2", "zst", "7z", "whl", "jar", "war", "aar", "apk", "aab",
    "ipa", "dmg", "pkg", "deb", "rpm", "msi", "exe", "dll", "so", "dylib", "a", "o", "lib", "wasm",
    "node", "bin", "img", "iso", "sig", "map",
];
/// A store search gives up on a root after listing this many entries, so a wrong pattern stays cheap.
const MAX_STORE_VISITS: usize = 200_000;
/// Unrecognized stores: unchanged this long, at least this big, at most this deep below an area.
const UNRECOGNIZED_DAYS: i64 = 30;
const UNRECOGNIZED_MIN_BYTES: i64 = 100 << 20;
const UNRECOGNIZED_MIN_FILES: u64 = 10;
const UNRECOGNIZED_DEPTH: usize = 4;
const UNRECOGNIZED_LIMIT: usize = 50;
const UNRECOGNIZED_NAMES: [&str; 8] = [
    "cache",
    "caches",
    "tmp",
    "temp",
    "logs",
    "log",
    "code cache",
    "gpucache",
];
const UNRECOGNIZED_FORMATS: [&str; 4] = ["jsonl", "ndjson", "json", "log"];

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
const CONDA_ENV_DAYS: i64 = 90;
const LEFTOVER_DAYS: i64 = 90;

/// The product and version of a JetBrains IDE folder (`IntelliJIdea2024.1`, `PyCharmCE2023.3`).
fn ide_version(name: &str) -> Option<(String, Vec<u64>)> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let (product, version) = name.split_at(start);
    if product.len() < 2 || !product.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    (parts.len() >= 2 && parts[0] >= 2000 && parts[0] < 3000).then(|| (product.to_string(), parts))
}

/// Where applications are installed on macOS, plus one level of folders inside them.
fn application_folders(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/System/Applications"),
        home.join("Applications"),
    ];
    let mut nested = Vec::new();
    for folder in &out {
        for entry in std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .take(2000)
        {
            let path = entry.path();
            if path.extension().is_none() && entry.file_type().is_ok_and(|t| t.is_dir()) {
                nested.push(path);
            }
        }
    }
    out.extend(nested);
    out
}

/// Names of the application bundles in `folders`, lower case with only letters and digits. Only
/// the folder listings are read, never a bundle's contents.
fn installed_apps(folders: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for folder in folders {
        for entry in std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .take(5000)
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(app) = name.strip_suffix(".app") {
                out.push(squash(app));
            }
        }
    }
    out.retain(|a| a.len() >= 3);
    out.sort();
    out.dedup();
    out
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Whether a Library folder named `name` (lower case) may belong to an installed application. A
/// bundle identifier is matched part by part, so a vendor's other applications also count; when
/// in doubt it counts as installed.
fn app_installed(name: &str, apps: &[String]) -> bool {
    let name = name.strip_suffix(".savedstate").unwrap_or(name);
    let parts: Vec<String> = if name.contains('.') {
        let parts: Vec<&str> = name.split('.').collect();
        // A bundle identifier starts with a domain (`com`, `org`, `io`) or a team id or `group`.
        parts.iter().skip(1).map(|p| squash(p)).collect()
    } else {
        vec![squash(name)]
    };
    let parts: Vec<String> = parts.into_iter().filter(|p| p.len() >= 3).collect();
    if parts.is_empty() {
        return true;
    }
    parts.iter().any(|part| {
        apps.iter().any(|app| {
            app == part
                || (part.len() >= 4 && app.contains(part.as_str()))
                || (app.len() >= 4 && part.contains(app.as_str()))
        })
    })
}

/// Projects worked on within this many days keep their build output out of suggestions.
pub const PROJECT_ACTIVE_DAYS: i64 = 7;
/// Folder names a build or package tool writes inside a project.
const PROJECT_OUTPUT_NAMES: [&str; 10] = [
    "bin",
    "obj",
    "vendor",
    "deps",
    "out",
    "Library",
    "Temp",
    "Intermediate",
    "Binaries",
    "public",
];
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
const MAX_STORE_ROOTS: usize = 8;
const MAX_STORE_PATTERNS: usize = 6;
const MAX_STORE_PART: usize = 128;

fn plain_name(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_STORE_PART
        && part != "."
        && part != ".."
        && !part
            .chars()
            .any(|c| c.is_control() || "/\\:*?[]{}$~".contains(c))
}

fn check_store(store: &Store) -> Result<()> {
    if store.roots.is_empty() || store.roots.len() > MAX_STORE_ROOTS {
        bail!("a store needs 1 to {MAX_STORE_ROOTS} roots");
    }
    for root in &store.roots {
        let Some((base, rest)) = root.split_once('/') else {
            bail!("store root {root:?} needs a folder below its base");
        };
        let base_ok = matches!(base, "~" | "{data}" | "{config}" | "{cache}")
            || base.strip_prefix('$').is_some_and(|name| {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            });
        if !base_ok || rest.split('/').any(|p| !plain_name(p)) || rest.split('/').count() > 8 {
            bail!("invalid store root {root:?}");
        }
        // Under the home folder itself only a tool's own hidden folder can hold a store.
        if base == "~" && !rest.starts_with('.') {
            bail!("store root {root:?} is not a tool folder");
        }
    }
    if store.patterns.len() > MAX_STORE_PATTERNS
        || store.patterns.iter().any(|p| {
            p.is_empty()
                || p.len() > MAX_STORE_PART
                || p == "."
                || p == ".."
                || p.chars()
                    .any(|c| c.is_control() || "/\\:[]{}$~".contains(c))
        })
    {
        bail!("invalid store patterns");
    }
    if !["file", "directory"].contains(&store.target.as_str())
        || (store.patterns.is_empty() && store.target != "directory")
        || store.min_age_days > 3650
    {
        bail!("invalid store target or age");
    }
    Ok(())
}

/// Shell-style match of one name: `*` is any run of bytes, `?` one byte.
fn glob(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|b| *b == b'*')
}

/// The per-user data, config and cache areas of this platform.
fn tool_area(base: &str, home: &std::path::Path) -> std::path::PathBuf {
    let env = |name: &str| absolute_env(name);
    if cfg!(windows) {
        let roaming = env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
        return match base {
            "{cache}" => local,
            _ => roaming,
        };
    }
    if cfg!(target_os = "macos") {
        return match base {
            "{cache}" => home.join("Library/Caches"),
            _ => home.join("Library/Application Support"),
        };
    }
    match base {
        "{cache}" => env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
        "{config}" => env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
        _ => env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
    }
}

/// A store root resolved on this machine, or `None` when it does not apply here. Even a root taken
/// from an environment variable must be inside the home folder and a tool's own folder.
pub fn store_root(template: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let (base, rest) = template.split_once('/')?;
    let start = match base {
        "~" => home.to_path_buf(),
        "{data}" | "{config}" | "{cache}" => tool_area(base, home),
        _ => absolute_env(base.strip_prefix('$')?)?,
    };
    let root = rest.split('/').fold(start, |path, part| path.join(part));
    (root.starts_with(home) && tool_root_allowed(home, &root) && root != home).then_some(root)
}

/// A file that build or release tooling produces: archives, packages, binaries, libraries and
/// checksums. A copy made of these is cheap to make again; a copy of photos or documents is not.
fn build_artifact(name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).to_ascii_lowercase();
    match name.rsplit_once('.') {
        None => true,
        Some((_, ext)) => ARTIFACT_EXTENSIONS.contains(&ext),
    }
}

/// A relative path with every version replaced by `#`: a run of digits and dots, with any
/// prerelease suffix such as `-beta.2` or `-rc1`, so `app-1.2.0-beta.1.tgz` and `app-1.3.0.tgz`
/// name the same file.
fn fold_digits(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if !path[i].is_ascii_digit() {
            out.push(path[i]);
            i += 1;
            continue;
        }
        let digits = |mut j: usize| {
            while j < path.len() && (path[j].is_ascii_digit() || path[j] == b'.') {
                j += 1;
            }
            j
        };
        i = digits(i);
        while path[..i].ends_with(b".") {
            i -= 1;
        }
        if i < path.len() && matches!(path[i], b'-' | b'.') {
            let rest = &path[i + 1..];
            if let Some(tag) = ["alpha", "beta", "rc", "pre", "dev", "next", "r"]
                .iter()
                .find(|t| {
                    rest.len() >= t.len() && rest[..t.len()].eq_ignore_ascii_case(t.as_bytes())
                })
            {
                let mut j = i + 1 + tag.len();
                if j < path.len() && path[j] == b'.' {
                    j += 1;
                }
                if j < path.len() && path[j].is_ascii_digit() {
                    i = digits(j);
                    while path[..i].ends_with(b".") {
                        i -= 1;
                    }
                }
            }
        }
        out.push(b'#');
    }
    out
}

/// At least `COPY_SIMILARITY` percent of the two folders' files are the same.
fn similar(a: &BTreeSet<Vec<u8>>, b: &BTreeSet<Vec<u8>>) -> bool {
    let shared = a.intersection(b).count();
    let all = a.len() + b.len() - shared;
    all > 0 && shared * 100 >= all * COPY_SIMILARITY
}

/// The root template that names `path` on any machine: relative to the closest tool area, or to
/// the home folder when the first folder below it is a hidden tool folder.
fn store_template(path: &std::path::Path, home: &std::path::Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for base in ["{cache}", "{config}", "{data}"] {
        let area = tool_area(base, home);
        if let Ok(rest) = path.strip_prefix(&area)
            && rest.components().count() >= 1
            && best
                .as_ref()
                .is_none_or(|(len, _)| area.as_os_str().len() > *len)
        {
            let rest = rest.to_str()?.replace('\\', "/");
            best = Some((area.as_os_str().len(), format!("{base}/{rest}")));
        }
    }
    if let Some((_, template)) = best {
        return Some(template);
    }
    let rest = path.strip_prefix(home).ok()?.to_str()?.replace('\\', "/");
    rest.starts_with('.').then(|| format!("~/{rest}"))
}

const DRAFT_LISTING: usize = 2_000;
const DRAFT_FOLDERS: usize = 20;

/// Children of a folder, links skipped: (name, is_directory). Metadata only, bounded.
fn draft_children(path: &std::path::Path) -> Result<Vec<(String, bool)>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(path)?.take(DRAFT_LISTING) {
        let item = item?;
        let kind = item.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if let Some(name) = item.file_name().to_str() {
            out.push((name.to_string(), kind.is_dir()));
        }
    }
    Ok(out)
}

/// The extension most of these files share, when one does.
fn common_extension(names: &[&str]) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        if let Some((stem, ext)) = name.rsplit_once('.')
            && !stem.is_empty()
            && plain_name(ext)
        {
            *counts.entry(ext).or_default() += 1;
        }
    }
    let (ext, count) = counts.into_iter().max_by_key(|(_, c)| *c)?;
    (count * 2 > names.len()).then(|| ext.to_string())
}

/// A draft rule for a store folder, read from metadata and folder listings only (never a file's
/// contents). The result is checked with `parse` and is a starting point to review, not a rule
/// to trust: it still needs a source, fixtures and a signature before omg can load it.
pub fn draft_rule(path: &std::path::Path, home: &std::path::Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || std::fs::canonicalize(path)? != path {
        bail!("give the real path of a folder, without links or relative parts");
    }
    let Some(template) = store_template(path, home) else {
        bail!("the folder is not in a tool's own area of the home folder");
    };
    let tool = template
        .trim_start_matches("~/")
        .split('/')
        .find(|p| !p.starts_with('{'))
        .unwrap_or("tool")
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let children = draft_children(path)?;
    let files: Vec<&str> = children
        .iter()
        .filter(|c| !c.1)
        .map(|c| c.0.as_str())
        .collect();
    let folders: Vec<&str> = children
        .iter()
        .filter(|c| c.1)
        .map(|c| c.0.as_str())
        .collect();
    let (patterns, target): (Vec<String>, &str) = if let Some(ext) = (files.len() >= folders.len())
        .then(|| common_extension(&files))
        .flatten()
    {
        (vec![format!("*.{ext}")], "file")
    } else if !folders.is_empty() {
        let mut inner = Vec::new();
        for folder in folders.iter().take(DRAFT_FOLDERS) {
            inner.extend(
                draft_children(&path.join(folder))?
                    .into_iter()
                    .filter(|c| !c.1)
                    .map(|c| c.0),
            );
        }
        let inner: Vec<&str> = inner.iter().map(String::as_str).collect();
        match common_extension(&inner) {
            Some(ext) => (vec!["*".into(), format!("*.{ext}")], "file"),
            None => (vec!["*".into()], "directory"),
        }
    } else {
        bail!("the folder holds no items that share a format");
    };
    let name = template
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let (discovery, age) = if name.contains("cache") {
        ("tool_caches", 30)
    } else if name.contains("log") {
        ("tool_logs", 7)
    } else {
        ("tool_sessions", 7)
    };
    let category = category_of(discovery).unwrap_or_default();
    let quote = |items: &[String]| {
        items
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let rule = format!(
        r#"[[rules]]
id = "local.{tool}.store"
name = "Old {tool} items"
revision = 1
platforms = [{os:?}]
tool_versions = ["unknown-inventory-only"]
category = "{category}"
tier = "review"
action = "info"
discovery = "{discovery}"
explanation = "Items in {template} unchanged for {age} or more days."
side_effects = "Check what {tool} loses when one is deleted."
regeneration = "Unknown until checked against the tool's documentation."
warnings = ["Drafted from folder names only; review before use."]
preconditions = ["known_root", "metadata_only"]
exclusions = ["protected", "cloud", "active_work"]
privileges = "current-user"
failure_behavior = "informational"
compatibility = "inventory-only"
deprecated = false
fixtures = ["old-item-positive", "recent-item-near-miss"]
source = "https://example.invalid/replace-with-the-tool-documentation"

[rules.store]
roots = [{root:?}]
patterns = [{patterns}]
target = "{target}"
min_age_days = {age}
min_bytes = 1048576
"#,
        os = std::env::consts::OS,
        root = template,
        patterns = quote(&patterns),
    );
    parse(&format!(
        "schema_version = 1\nversion = 1\nengine_major = 0\n\n{rule}"
    ))
    .map_err(|e| anyhow::anyhow!("the draft did not pass the catalog checks: {e}"))?;
    Ok(rule)
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
                output.extend(assemble(rule, entry, subtree, None, None)?);
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
                let activity = self.project_activity(scan, crate::index::split_path(&path).0)?;
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
            found.push((rule, self.project_outputs(scan, SUGGEST_MIN_BYTES, None)?));
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
                let activity = (rule.discovery == "project_build_output")
                    .then(|| self.project_activity(scan, crate::index::split_path(&path).0))
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
    /// Installers, disk images and archives in the Downloads folder, untouched for a month: what
    /// they installed or unpacked lives elsewhere, and they can be downloaded again.
    fn old_installers(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let Some(downloads) =
            crate::rollup::relative_raw(root, &home.join("Downloads")).filter(|d| !d.is_empty())
        else {
            return Ok(vec![]);
        };
        let cutoff = (now() - INSTALLER_DAYS * 86_400) as i128 * 1_000_000_000;
        let mut upper = downloads.clone();
        upper.push(b'0');
        let mut below = downloads.clone();
        below.push(b'/');
        let mut statement = self.db.prepare_cached(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent=?4 AND kind='file' AND excluded IS NULL AND allocated>=?2 AND modified_ns<?3 UNION ALL SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent>=?5 AND parent<?6 AND kind='file' AND excluded IS NULL AND allocated>=?2 AND modified_ns<?3",
        )?;
        let rows = statement.query_map(
            params![
                scan,
                INSTALLER_MIN_BYTES,
                cutoff as i64,
                downloads,
                below,
                upper
            ],
            |r| r.get::<_, Vec<u8>>(0),
        )?;
        let mut out = Vec::new();
        for path in rows {
            let path = path?;
            let name =
                String::from_utf8_lossy(crate::index::split_path(&path).1).to_ascii_lowercase();
            if INSTALLER_EXTENSIONS.iter().any(|ext| {
                name.strip_suffix(ext)
                    .is_some_and(|stem| stem.ends_with('.') && stem.len() > 2)
            }) {
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Older copies of a folder: sibling folders holding the same files (names compared with their
    /// digits folded, so `app-1.2.tgz` and `app-1.3.tgz` count as the same file), mostly build or
    /// release output, where a newer sibling exists. Every copy but the newest is listed for
    /// review; nothing is suggested. Metadata only, and bounded: a few hundred parents, a few
    /// thousand files per folder.
    fn superseded_copies(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        claimed: &[Vec<u8>],
    ) -> Result<Vec<Vec<u8>>> {
        let Some(home_raw) = crate::rollup::relative_raw(root, home) else {
            return Ok(vec![]);
        };
        let mut groups = self.db.prepare_cached(
            "SELECT parent FROM dir_totals WHERE scan_id=? AND allocated>=? AND parent IS NOT NULL GROUP BY parent HAVING count(*)>=2 ORDER BY sum(allocated) DESC LIMIT ?",
        )?;
        let parents: Vec<Vec<u8>> = groups
            .query_map(params![scan, COPY_MIN_BYTES, COPY_PARENTS], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut members = self.db.prepare_cached(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? ORDER BY allocated DESC LIMIT 12",
        )?;
        let mut files = self.db.prepare_cached(
            "SELECT CAST(parent||name AS BLOB),apparent FROM entries WHERE scan_id=?1 AND parent=?2 AND kind='file' AND excluded IS NULL UNION ALL SELECT CAST(parent||name AS BLOB),apparent FROM entries WHERE scan_id=?1 AND parent>=?3 AND parent<?4 AND kind='file' AND excluded IS NULL LIMIT ?5",
        )?;
        let mut budget = COPY_ENTRY_BUDGET;
        let mut out = Vec::new();
        for parent in parents {
            let below_home = policy::inside(&parent, &home_raw, true);
            let relative = parent.strip_prefix(home_raw.as_slice()).unwrap_or(&parent);
            // Only the user's own folders: tool areas, application data and dependency trees keep
            // versions side by side on purpose, and their own kinds judge them.
            let tool_area = relative.split(|b| *b == b'/').any(|part| {
                part.first() == Some(&b'.')
                    || part.eq_ignore_ascii_case(b"Library")
                    || part.eq_ignore_ascii_case(b"AppData")
                    || policy::REGENERABLE.iter().any(|r| r.as_bytes() == part)
            });
            if !below_home || tool_area || claimed.iter().any(|c| policy::inside(&parent, c, true))
            {
                continue;
            }
            let mut copies: Vec<FolderCopy> = Vec::new();
            let rows: Vec<(Vec<u8>, i64)> = members
                .query_map(params![scan, parent, COPY_MIN_BYTES], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (dir, newest) in rows {
                if budget == 0 {
                    return Ok(out);
                }
                let mut git = dir.clone();
                git.extend_from_slice(b"/.git");
                if self.entry_at(scan, &git)?.is_some()
                    || claimed.iter().any(|c| policy::inside(c, &dir, true))
                {
                    continue;
                }
                let mut below = dir.clone();
                below.push(b'/');
                let mut upper = dir.clone();
                upper.push(b'0');
                let listed: Vec<(Vec<u8>, i64)> = files
                    .query_map(
                        params![scan, dir, below, upper, COPY_MAX_FILES as i64 + 1],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?
                    .collect::<rusqlite::Result<_>>()?;
                budget = budget.saturating_sub(listed.len());
                if listed.len() > COPY_MAX_FILES || listed.len() < 3 {
                    continue;
                }
                let mut names = BTreeSet::new();
                let (mut bytes, mut artifact_bytes) = (0u64, 0u64);
                for (path, apparent) in listed {
                    let rest = &path[dir.len()..];
                    if rest
                        .split(|b| *b == b'/')
                        .any(|part| policy::REGENERABLE.iter().any(|r| r.as_bytes() == part))
                    {
                        continue;
                    }
                    let size = apparent.max(0) as u64;
                    bytes += size;
                    let name = crate::index::split_path(&path).1;
                    if build_artifact(name.strip_prefix(b"/").unwrap_or(name)) {
                        artifact_bytes += size;
                    }
                    names.insert(fold_digits(rest));
                }
                if bytes >= COPY_MIN_BYTES as u64 && artifact_bytes * 2 >= bytes {
                    copies.push((dir, newest, names, bytes));
                }
            }
            copies.sort_by_key(|c| std::cmp::Reverse(c.1));
            for (i, candidate) in copies.iter().enumerate().skip(1) {
                let superseded = copies[..i].iter().any(|keeper| {
                    keeper.1 > candidate.1
                        && similar(&keeper.2, &candidate.2)
                        && keeper.3 * 2 >= candidate.3
                        && candidate.3 * 2 >= keeper.3
                });
                if superseded {
                    out.push(candidate.0.clone());
                }
            }
        }
        Ok(out)
    }
    /// Items of a rule's store: each root resolved, then one level per pattern, listing only the
    /// children of folders already matched, so the cost follows the store and not the index.
    fn store_items(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        store: &Store,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = now() - i64::from(store.min_age_days) * 86_400;
        let mut children = self.db.prepare_cached(
            "SELECT name,kind FROM entries WHERE scan_id=? AND parent=? AND excluded IS NULL",
        )?;
        let mut out = Vec::new();
        let mut bases = BTreeSet::new();
        for template in &store.roots {
            let Some(base) = store_root(template, home)
                .and_then(|absolute| crate::rollup::relative_raw(root, &absolute))
                .filter(|b| !b.is_empty())
            else {
                continue;
            };
            if !bases.insert(base.clone()) {
                continue;
            }
            let mut frontier = vec![base];
            let mut visited = 0usize;
            for (level, pattern) in store.patterns.iter().enumerate() {
                let last = level + 1 == store.patterns.len();
                let mut next = Vec::new();
                for dir in &frontier {
                    let rows = children.query_map(params![scan, dir], |r| {
                        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
                    })?;
                    for row in rows {
                        let (name, kind) = row?;
                        visited += 1;
                        let bare = name.strip_prefix(b"/").unwrap_or(&name);
                        if (last || kind == "directory") && glob(pattern.as_bytes(), bare) {
                            let mut path = dir.clone();
                            path.extend_from_slice(&name);
                            next.push(path);
                        }
                    }
                }
                frontier = next;
                if visited > MAX_STORE_VISITS {
                    frontier.clear();
                }
            }
            for path in frontier {
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                if entry.kind != store.target || entry.excluded.is_some() {
                    continue;
                }
                let (bytes, newest) = if entry.kind == "directory" {
                    match self.subtree(&path)? {
                        Some(t) if !t.incomplete && t.newest > 0 => (t.allocated, t.newest),
                        _ => continue,
                    }
                } else {
                    (entry.allocated, entry.modified_ns.div_euclid(1_000_000_000))
                };
                if bytes >= store.min_bytes && newest < cutoff {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Folders in the tool areas of the home folder that look like a store no rule knows yet:
    /// named like a cache, log or temporary folder, or holding many files of one log or history
    /// format. Shown for review only; never suggested. Found by walking `dir_totals` from each
    /// area through big folders only, so small trees are never read.
    fn unrecognized_stores(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        catalog: &Catalog,
        claimed: &[Vec<u8>],
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = now() - UNRECOGNIZED_DAYS * 86_400;
        let mut known: Vec<Vec<u8>> = claimed.to_vec();
        for kind in DISCOVERIES {
            if let Some(path) = discovery_root(kind, home) {
                known.extend(crate::rollup::relative_raw(root, &path));
            }
        }
        for rule in &catalog.rules {
            for template in rule.store.iter().flat_map(|s| &s.roots) {
                if let Some(path) = store_root(template, home) {
                    known.extend(crate::rollup::relative_raw(root, &path));
                }
            }
        }
        for toolchain in toolchains(home) {
            for path in toolchain.roots.iter().chain(&toolchain.also) {
                known.extend(crate::rollup::relative_raw(root, path));
            }
        }
        known.retain(|k| !k.is_empty());
        let Some(home_raw) = crate::rollup::relative_raw(root, home) else {
            return Ok(vec![]);
        };
        let mut areas: Vec<Vec<u8>> = Vec::new();
        let mut big = self.db.prepare_cached(
            "SELECT path,allocated,files,protected,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=?",
        )?;
        // The hidden folders directly in the home folder, then the per-user application areas.
        let rows = big.query_map(params![scan, home_raw, UNRECOGNIZED_MIN_BYTES], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            if name.strip_prefix(b"/").unwrap_or(name).starts_with(b".") {
                areas.push(path);
            }
        }
        let mut parents: Vec<std::path::PathBuf> = app_data_parents(home);
        parents.push(tool_area("{cache}", home));
        for parent in parents {
            let Some(parent) = crate::rollup::relative_raw(root, &parent).filter(|p| !p.is_empty())
            else {
                continue;
            };
            if areas.iter().any(|a| policy::inside(&parent, a, true)) {
                continue;
            }
            let rows = big.query_map(params![scan, parent, UNRECOGNIZED_MIN_BYTES], |r| {
                r.get::<_, Vec<u8>>(0)
            })?;
            for path in rows {
                areas.push(path?);
            }
        }
        let mut files = self.db.prepare_cached(
            "SELECT name,allocated,modified_ns FROM entries WHERE scan_id=? AND parent=? AND kind='file' AND excluded IS NULL",
        )?;
        let mut out: Vec<Vec<u8>> = Vec::new();
        // Breadth first through big folders only, a few levels deep.
        let mut frontier: Vec<Vec<u8>> = areas;
        for _ in 0..UNRECOGNIZED_DEPTH {
            let mut next = Vec::new();
            for dir in frontier {
                if known
                    .iter()
                    .any(|k| policy::inside(&dir, k, true) || policy::inside(k, &dir, true))
                {
                    // A known root at or inside this folder: look below, never at the whole.
                    if !known.iter().any(|k| policy::inside(&dir, k, true)) {
                        let rows = big
                            .query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                                r.get::<_, Vec<u8>>(0)
                            })?;
                        for row in rows {
                            next.push(row?);
                        }
                    }
                    continue;
                }
                let Some(totals) = self.subtree(&dir)? else {
                    continue;
                };
                if totals.protected > 0 || totals.incomplete || totals.newest == 0 {
                    let rows = big.query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                        r.get::<_, Vec<u8>>(0)
                    })?;
                    for row in rows {
                        next.push(row?);
                    }
                    continue;
                }
                let name = crate::index::split_path(&dir).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                let named = UNRECOGNIZED_NAMES.contains(&name.as_str()) && totals.newest < cutoff;
                let mut by_format: std::collections::HashMap<&str, (u64, u64)> =
                    std::collections::HashMap::new();
                if !named {
                    let rows = files.query_map(params![scan, dir], |r| {
                        Ok((
                            r.get::<_, Vec<u8>>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (file, allocated, modified) = row?;
                        if modified.div_euclid(1_000_000_000) >= cutoff {
                            continue;
                        }
                        let Some(format) = UNRECOGNIZED_FORMATS
                            .iter()
                            .find(|f| file.ends_with(format!(".{f}").as_bytes()))
                        else {
                            continue;
                        };
                        let slot = by_format.entry(format).or_default();
                        slot.0 += 1;
                        slot.1 += allocated.max(0) as u64;
                    }
                }
                let history = by_format.values().any(|(count, bytes)| {
                    *count >= UNRECOGNIZED_MIN_FILES && *bytes >= UNRECOGNIZED_MIN_BYTES as u64
                });
                if named || history {
                    out.push(dir);
                    if out.len() >= UNRECOGNIZED_LIMIT {
                        return Ok(out);
                    }
                    continue;
                }
                let rows = big.query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                    r.get::<_, Vec<u8>>(0)
                })?;
                for row in rows {
                    next.push(row?);
                }
            }
            frontier = next;
        }
        Ok(out)
    }
    /// Older settings and data folders of JetBrains IDEs, one per product and version
    /// (`IntelliJIdea2024.1`, `PyCharm2023.3`). The newest of each product is kept; an older one is
    /// listed once unchanged for `policy::STALE_DAYS` and the newest is a day old.
    fn jetbrains_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let parents: Vec<std::path::PathBuf> = if cfg!(windows) {
            vec![
                tool_area("{data}", home).join("JetBrains"),
                tool_area("{cache}", home).join("JetBrains"),
                tool_area("{data}", home).join("Google"),
                tool_area("{cache}", home).join("Google"),
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                home.join("Library/Application Support/JetBrains"),
                home.join("Library/Logs/JetBrains"),
                home.join("Library/Application Support/Google"),
                home.join("Library/Logs/Google"),
            ]
        } else {
            vec![
                tool_area("{config}", home).join("JetBrains"),
                tool_area("{data}", home).join("JetBrains"),
                tool_area("{config}", home).join("Google"),
                tool_area("{data}", home).join("Google"),
            ]
        };
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let settled = crate::now() - 86_400;
        let gaps = self.gaps(scan)?;
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for parent in parents {
            let google = parent.ends_with("Google");
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() || gaps.covers(&parent) {
                continue;
            }
            let mut groups: std::collections::BTreeMap<String, Vec<VersionDir>> =
                Default::default();
            for row in statement.query_map(params![scan, parent], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
            })? {
                let (path, newest) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                let Some((product, version)) = ide_version(&name) else {
                    continue;
                };
                if google && product != "AndroidStudio" {
                    continue;
                }
                groups
                    .entry(product)
                    .or_default()
                    .push((version, newest, path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                if versions
                    .pop()
                    .is_none_or(|(_, newest, _)| newest <= 0 || newest >= settled)
                {
                    continue;
                }
                out.extend(
                    versions
                        .into_iter()
                        .filter(|(_, newest, path)| {
                            *newest > 0 && *newest < cutoff && !gaps.covers(path)
                        })
                        .map(|(_, _, path)| path),
                );
            }
        }
        Ok(out)
    }
    /// conda and mamba environments (folders holding `conda-meta`) of 50 MB or more, unchanged for
    /// `CONDA_ENV_DAYS`. An environment is made again from its list of packages, if one was kept.
    fn conda_envs(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - CONDA_ENV_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut parents: Vec<std::path::PathBuf> = [
            "miniconda3",
            "anaconda3",
            "miniforge3",
            "mambaforge",
            "micromamba",
            ".conda",
            ".mamba",
        ]
        .iter()
        .map(|base| home.join(base).join("envs"))
        .collect();
        parents.extend(absolute_env("MAMBA_ROOT_PREFIX").map(|p| p.join("envs")));
        parents.extend(absolute_env("CONDA_ENVS_PATH"));
        parents.sort();
        parents.dedup();
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0),protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for parent in parents {
            if !tool_root_allowed(home, &parent) {
                continue;
            }
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows: Vec<(Vec<u8>, i64, i64)> = statement
                .query_map(params![scan, parent, SUGGEST_MIN_BYTES], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (path, newest, protected) in rows {
                let mut meta = path.clone();
                meta.extend_from_slice(b"/conda-meta");
                if newest > 0
                    && newest < cutoff
                    && protected == 0
                    && !gaps.covers(&path)
                    && self.entry_at(scan, &meta)?.is_some()
                    && !out.contains(&path)
                {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Data an application left in the per-user Library after it was removed: folders named after
    /// an application (by bundle identifier or name) that no installed application matches,
    /// unchanged for `LEFTOVER_DAYS`. Applications are known by their bundle names only.
    fn app_leftovers(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        apps: &[String],
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - LEFTOVER_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut known: Vec<Vec<u8>> = DISCOVERIES
            .iter()
            .filter_map(|kind| discovery_root(kind, home))
            .chain(toolchains(home).into_iter().flat_map(|t| t.roots))
            .filter_map(|p| crate::rollup::relative_raw(root, &p))
            .collect();
        known.retain(|p| !p.is_empty());
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0),protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for area in [
            "Library/Application Support",
            "Library/Saved Application State",
            "Library/HTTPStorages",
            "Library/WebKit",
        ] {
            let Some(parent) = crate::rollup::relative_raw(root, &home.join(area)) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows: Vec<(Vec<u8>, i64, i64)> = statement
                .query_map(params![scan, parent, 1i64 << 20], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (path, newest, protected) in rows {
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                if name.starts_with('.')
                    || name.starts_with("com.apple.")
                    || name.starts_with("group.com.apple.")
                    || APP_DATA_SKIP.contains(&name.as_str())
                    || protected != 0
                    || newest <= 0
                    || newest >= cutoff
                    || gaps.covers(&path)
                    || known
                        .iter()
                        .any(|k| policy::inside(&path, k, true) || policy::inside(k, &path, true))
                    || app_installed(&name, apps)
                {
                    continue;
                }
                out.push(path);
            }
        }
        Ok(out)
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
    /// How recently the project folder `project` was worked on, split by where the change was.
    pub fn project_activity(&self, scan: i64, project: &[u8]) -> Result<Activity> {
        let mut activity = Activity::default();
        let bump = |slot: &mut Option<i64>, at: i64| {
            if at > 0 {
                *slot = Some(slot.map_or(at, |s| s.max(at)));
            }
        };
        let mut folders = self.db.prepare_cached(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let rows = folders.query_map(params![scan, project], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows.take(5000) {
            let (path, newest) = row?;
            let name = crate::index::split_path(&path).1;
            let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
            if name == ".git" {
                bump(&mut activity.git, newest);
            } else if policy::REGENERABLE.contains(&name.as_ref())
                || PROJECT_OUTPUT_NAMES.contains(&name.as_ref())
            {
                bump(&mut activity.build, newest);
            } else {
                bump(&mut activity.source, newest);
            }
        }
        let mut files = self.db.prepare_cached(
            "SELECT modified_ns FROM entries WHERE scan_id=? AND parent=? AND kind<>'directory' LIMIT 5000",
        )?;
        for at in files.query_map(params![scan, project], |r| r.get::<_, i64>(0))? {
            bump(&mut activity.source, at? / 1_000_000_000);
        }
        Ok(activity)
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
            let beside = |files: &[&str]| -> Result<bool> {
                for file in files {
                    if exists(join(&parent, file))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            let beside_kind = |extensions: &[&str]| -> Result<bool> {
                let mut statement = self.db.prepare_cached(
                    "SELECT name FROM entries WHERE scan_id=? AND parent=? LIMIT 2000",
                )?;
                let names =
                    statement.query_map(params![scan, parent], |r| r.get::<_, Vec<u8>>(0))?;
                for name in names {
                    let name = name?;
                    if extensions.iter().any(|e| name.ends_with(e.as_bytes())) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            let (grandparent, parent_name) = crate::index::split_path(&parent);
            let parent_name = parent_name.strip_prefix(b"/").unwrap_or(parent_name);
            let beside_parent = |files: &[&str]| -> Result<bool> {
                for file in files {
                    if exists(join(grandparent, file))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            let evidence = match name {
                b"node_modules" | b".next" | b".nuxt" | b".svelte-kit" | b".turbo"
                | b".parcel-cache" | b".angular" | b".expo" | b".output" | b"storybook-static"
                | b".pnpm-store" => beside(&["package.json"])?,
                b"out" => beside(&[
                    "next.config.js",
                    "next.config.mjs",
                    "next.config.ts",
                    "forge.config.js",
                    "forge.config.ts",
                ])?,
                b".cache" | b"public" => beside(&["gatsby-config.js", "gatsby-config.ts"])?,
                b".docusaurus" => beside(&[
                    "docusaurus.config.js",
                    "docusaurus.config.ts",
                    "docusaurus.config.mjs",
                ])?,
                b".serverless" => beside(&["serverless.yml", "serverless.yaml", "serverless.ts"])?,
                b"bower_components" => beside(&["bower.json"])?,
                b"elm-stuff" => beside(&["elm.json"])?,
                b"vendor" => beside(&["composer.json"])?,
                // Bundler's `--path vendor/bundle`; the rest of `vendor` may be committed code.
                b"bundle" if parent_name == b"vendor" => beside_parent(&["Gemfile"])?,
                b"library" if parent_name == b"renv" => beside_parent(&["renv.lock"])?,
                b"deps" | b".elixir_ls" => beside(&["mix.exs"])?,
                b"bin" | b"obj" => beside_kind(&[".csproj", ".fsproj", ".vbproj"])?,
                b".terraform" => beside_kind(&[".tf"])?,
                b".terragrunt-cache" => beside(&["terragrunt.hcl"])?,
                b"Intermediate" | b"Binaries" | b"DerivedDataCache" => beside_kind(&[".uproject"])?,
                // Unity's own folders, beside the file only Unity writes.
                b"Library" | b"Temp" | b"Obj" => {
                    exists(join(&parent, "ProjectSettings/ProjectVersion.txt"))?
                }
                b".godot" => beside(&["project.godot"])?,
                b".bloop" | b".metals" | b".bsp" => beside(&["build.sbt", "build.sc"])?,
                b".cpcache" => beside(&["deps.edn"])?,
                b".shadow-cljs" => beside(&["shadow-cljs.edn"])?,
                b"_opam" => beside(&["dune-project"])? || beside_kind(&[".opam"])?,
                b".cxx" | b".externalNativeBuild" | b".kotlin" => beside(&[
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                    "settings.gradle.kts",
                ])?,
                b"vcpkg_installed" => beside(&["vcpkg.json"])?,
                b".pytest_cache" | b".mypy_cache" | b".ruff_cache" | b".nox" | b".hypothesis" => {
                    beside(&[
                        "pyproject.toml",
                        "setup.py",
                        "setup.cfg",
                        "requirements.txt",
                        "noxfile.py",
                        "pytest.ini",
                    ])?
                }
                b".direnv" => beside(&[".envrc"])?,
                // Ordinary names: only at the root of a Git project, where the build made them.
                b"dist" | b"build" => {
                    (beside(&["package.json"])? && beside(&[".git"])?)
                        || beside(&[
                            "build.gradle",
                            "build.gradle.kts",
                            "pubspec.yaml",
                            "CMakeLists.txt",
                            "setup.py",
                        ])?
                        || beside_kind(&[".xcodeproj"])?
                }
                b"target" => beside(&["Cargo.toml", "pom.xml", "build.sbt", "project.clj"])?,
                b".gradle" => beside(&[
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                    "settings.gradle.kts",
                ])?,
                b".dart_tool" => beside(&["pubspec.yaml"])?,
                b".tox" => beside(&["tox.ini", "pyproject.toml", "setup.py"])?,
                b"Pods" => beside(&["Podfile"])?,
                b".build" => beside(&["Package.swift"])?,
                b".stack-work" => beside(&["stack.yaml"])?,
                b"dist-newstyle" => beside(&["cabal.project"])?,
                b"_build" => beside(&["mix.exs", "dune-project"])?,
                b"zig-cache" | b".zig-cache" | b"zig-out" => beside(&["build.zig"])?,
                b"cmake-build-debug" | b"cmake-build-release" => beside(&["CMakeLists.txt"])?,
                // Any folder holding a virtual environment's marker is one, whatever its name.
                _ => exists(join(&path, "pyvenv.cfg"))?,
            };
            // Projects live in ordinary folders. Hidden folders hold installed tools (nvm global
            // packages, editor servers, pnpm's own versions), whose dependencies are not stale
            // project output: deleting them would break the tool.
            let hidden = parent
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
