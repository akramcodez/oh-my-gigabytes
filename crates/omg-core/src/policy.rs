use crate::{Entry, platform};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// A Git repository untouched this many days counts as stale: deletable after a warning.
pub const STALE_DAYS: i64 = 30;

/// Build and dependency folders that their tools recreate. Inside a repository they are never
/// locked (a warning is shown instead); outside one they are ordinary folders.
pub const REGENERABLE: [&str; 22] = [
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".angular",
    ".expo",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".gradle",
    "coverage",
    ".dart_tool",
    "DerivedData",
];

/// Names that are sensitive wherever they appear, even inside caches or old repositories. Lower case.
pub const ALWAYS_SENSITIVE: [&str; 14] = [
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    "keychains",
    "credentials",
    ".git-credentials",
    ".pgpass",
    "cloudstorage",
    "mobile documents",
    "onedrive",
    "dropbox",
    "google drive",
];

pub fn always_sensitive(name: &str) -> bool {
    ALWAYS_SENSITIVE.contains(&name.to_ascii_lowercase().as_str())
}

/// Newest modification time (Unix seconds) inside a directory, when known.
pub type AgeLookup<'a> = &'a dyn Fn(&Path) -> Option<i64>;

/// Memo of repository-marker answers for one bulk evaluation, plus an optional lookup of the
/// newest modification time (seconds) inside a directory, used to tell stale repositories apart.
#[derive(Default)]
pub struct RepositoryMemo<'a> {
    repos: HashMap<PathBuf, bool>,
    home: Option<Option<PathBuf>>,
    pub newest: Option<AgeLookup<'a>>,
}
impl<'a> RepositoryMemo<'a> {
    /// A memo that can tell stale repositories apart using `newest(dir)` (Unix seconds).
    pub fn with_ages(newest: AgeLookup<'a>) -> Self {
        Self {
            newest: Some(newest),
            ..Default::default()
        }
    }
    fn is_repository(&mut self, dir: &Path) -> bool {
        let home = self
            .home
            .get_or_insert_with(|| platform::home().ok())
            .clone();
        // A repository at (or above) the home folder is a dotfiles repository; it does not make
        // everything in the home folder "repository data".
        if home.as_deref().is_some_and(|h| h.starts_with(dir)) {
            return false;
        }
        *self.repos.entry(dir.to_path_buf()).or_insert_with(|| {
            dir.is_dir()
                && (dir.join(".git").symlink_metadata().is_ok()
                    || (dir.join("HEAD").is_file()
                        && dir.join("objects").is_dir()
                        && dir.join("refs").is_dir()))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatus {
    Outside,
    /// Inside a build/dependency folder of a repository.
    Regenerable(PathBuf),
    Stale {
        repo: PathBuf,
        days: i64,
    },
    Active {
        repo: PathBuf,
        days: Option<i64>,
    },
}

/// Where an entry stands relative to the nearest project repository.
pub fn repository_status(root: &Path, entry: &Entry, memo: &mut RepositoryMemo) -> RepoStatus {
    let Some(repo) = repository_of(root, entry, memo) else {
        return RepoStatus::Outside;
    };
    let absolute = match platform::path(&entry.path) {
        Ok(relative) => root.join(relative),
        Err(_) => return RepoStatus::Active { repo, days: None },
    };
    if absolute.strip_prefix(&repo).is_ok_and(|inner| {
        inner
            .components()
            .any(|c| REGENERABLE.contains(&c.as_os_str().to_string_lossy().as_ref()))
    }) {
        return RepoStatus::Regenerable(repo);
    }
    let days = memo
        .newest
        .and_then(|lookup| lookup(&repo))
        .map(|newest| (crate::now() - newest).max(0) / 86_400);
    match days {
        Some(days) if days >= STALE_DAYS => RepoStatus::Stale { repo, days },
        days => RepoStatus::Active { repo, days },
    }
}

/// When the repository at `repo` is an untouched clone, the time its index was written (nanoseconds
/// since the epoch); `None` when that cannot be shown. Reads only Git's own config and references,
/// bounded and without following links, and never runs Git. Fetched objects stay in packs, so any
/// loose object means something was committed, staged or pulled here. The caller still has to
/// compare the working tree with the returned time.
pub fn pristine_clone(repo: &Path) -> Option<i128> {
    let git = repo.join(".git");
    if !git.symlink_metadata().ok()?.is_dir() {
        return None;
    }
    // In-progress work, or local state these checks cannot see into.
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
        "modules",
        "worktrees",
        "lfs",
        "reftable",
        "logs/refs/stash",
    ] {
        if git.join(name).symlink_metadata().is_ok() {
            return None;
        }
    }
    for object in std::fs::read_dir(git.join("objects")).ok()? {
        let name = object.ok()?.file_name();
        if name != "info" && name != "pack" {
            return None;
        }
    }
    let config = read_small(&git.join("config"))?;
    let mut in_remote = false;
    let mut has_remote = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_remote = line.starts_with("[remote ");
        } else if in_remote && line.starts_with("url") && line.contains('=') {
            has_remote = true;
        }
    }
    if !has_remote {
        return None;
    }
    let mut refs: HashMap<String, String> = HashMap::new();
    if git.join("packed-refs").symlink_metadata().is_ok() {
        for line in read_small(&git.join("packed-refs"))?.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let (hash, name) = line.trim().split_once(' ')?;
            refs.insert(name.to_string(), hash.to_string());
        }
    }
    let mut loose = vec![(git.join("refs"), "refs".to_string())];
    let mut visited = 0;
    while let Some((directory, prefix)) = loose.pop() {
        for item in std::fs::read_dir(directory).ok()? {
            let item = item.ok()?;
            visited += 1;
            if visited > 10_000 {
                return None;
            }
            let name = format!("{prefix}/{}", item.file_name().into_string().ok()?);
            let kind = item.file_type().ok()?;
            if kind.is_dir() {
                loose.push((item.path(), name));
            } else if kind.is_file() {
                let value = read_small(&item.path())?;
                let value = value.trim();
                // A loose tag was made here: a clone keeps fetched tags in packed-refs.
                if name.starts_with("refs/tags/") {
                    return None;
                }
                if !value.starts_with("ref:") {
                    refs.insert(name, value.to_string());
                }
            } else {
                return None;
            }
        }
    }
    let on_remote = |branch: &str, hash: &str| {
        refs.iter().any(|(name, value)| {
            value == hash
                && name
                    .strip_prefix("refs/remotes/")
                    .and_then(|rest| rest.split_once('/'))
                    .is_some_and(|(_, remote_branch)| remote_branch == branch)
        })
    };
    for (name, hash) in &refs {
        if !(hash.len() == 40 || hash.len() == 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            if !on_remote(branch, hash) {
                return None;
            }
        } else if !name.starts_with("refs/remotes/") && !name.starts_with("refs/tags/") {
            return None;
        }
    }
    let head = read_small(&git.join("HEAD"))?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(target) => {
            refs.get(target.trim())?;
        }
        None => {
            refs.iter()
                .find(|(name, hash)| name.starts_with("refs/remotes/") && *hash == head)?;
        }
    }
    let written = git.join("index").symlink_metadata().ok()?.modified().ok()?;
    let since = written.duration_since(std::time::UNIX_EPOCH).ok()?;
    i128::try_from(since.as_nanos()).ok()
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = path.symlink_metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 4 << 20 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Independent policy: catalog tier/action is never cleanup authority.
pub fn protection(root: &Path, entry: &Entry) -> Option<String> {
    protection_with(root, entry, &mut RepositoryMemo::default())
}

/// Hard protection, plus "active repository" protection. Build/dependency folders and stale
/// repositories are not protected (deletion still warns); an unknown age counts as active.
pub fn protection_with(root: &Path, entry: &Entry, memo: &mut RepositoryMemo) -> Option<String> {
    hard_protection(root, entry).or_else(|| match repository_status(root, entry, memo) {
        RepoStatus::Active { repo, days } => Some(format!(
            "active Git repository at {}{}; only build/dependency folders inside are deletable",
            repo.display(),
            days.map(|d| format!(" (changed {d} day(s) ago)"))
                .unwrap_or_default()
        )),
        _ => None,
    })
}

/// Personal-data folders below a macOS `Library` folder (mail, messages, Safari, calendars, address
/// book, iPhone backups, app containers and cloud folders). Matched only directly below `Library`,
/// so an ordinary project folder such as `src/mail` is not caught.
fn private_library(parts: &[String]) -> bool {
    let Some(at) = parts.iter().position(|part| part == "library") else {
        return false;
    };
    let below = &parts[at + 1..];
    match below.first().map(String::as_str) {
        Some(
            "mail" | "messages" | "safari" | "calendars" | "cookies" | "containers"
            | "group containers" | "keychains" | "mobile documents" | "cloudstorage",
        ) => true,
        Some("application support") => matches!(
            below.get(1).map(String::as_str),
            Some("addressbook" | "mobilesync" | "icloud" | "cloudstorage")
        ),
        _ => false,
    }
}

/// Protection that never depends on repository membership: exclusions, scope, entry type,
/// sensitive component names (including `.git`) and database/credential/disk-image names.
pub fn hard_protection(root: &Path, entry: &Entry) -> Option<String> {
    if let Some(reason) = &entry.excluded {
        return Some(reason.clone());
    }
    let relative = match platform::path(&entry.path) {
        Ok(path) => path,
        Err(_) => return Some("unsupported native path".into()),
    };
    if relative.is_absolute()
        || relative
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Some("scope escape".into());
    }
    let absolute = root.join(&relative);
    if let Some(reason) = platform::excluded(&absolute, &[]) {
        return Some(reason);
    }
    if !matches!(entry.kind.as_str(), "file" | "directory") || entry.identity.is_empty() {
        return Some("uncertain type or identity".into());
    }
    let parts: Vec<String> = absolute
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    if private_library(&parts) {
        return Some("personal data folder".into());
    }
    // Media libraries and disk images are packages or containers of personal data.
    if parts.iter().any(|part| {
        [
            ".photoslibrary",
            ".musiclibrary",
            ".tvlibrary",
            ".sparsebundle",
            ".sparseimage",
        ]
        .iter()
        .any(|suffix| part.ends_with(suffix))
    }) {
        return Some("media library or disk image package".into());
    }
    // One disk holds every image, container and volume, so removing it cannot be selective.
    if parts
        .iter()
        .any(|part| [".colima", ".lima", ".orbstack"].contains(&part.as_str()))
    {
        return Some(
            "container or virtual machine disk; free space with `docker system prune` instead"
                .into(),
        );
    }
    // Removing part of a bundle breaks the app, and App Management can stop the removal halfway.
    if cfg!(target_os = "macos") && parts.iter().any(|part| part.ends_with(".app")) {
        return Some("application bundle; uninstall it from Finder".into());
    }
    for value in &parts {
        if [
            ".git",
            ".ssh",
            ".gnupg",
            ".aws",
            ".azure",
            ".docker",
            ".kube",
            "keychains",
            "credentials",
            "cloudstorage",
            "mobile documents",
            "onedrive",
            "dropbox",
        ]
        .contains(&value.as_str())
        {
            return Some("sensitive, repository, managed or cloud path".into());
        }
    }
    let name = relative
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if [
        ".db",
        ".sqlite",
        ".sqlite3",
        "-wal",
        "-shm",
        ".vhd",
        ".vhdx",
        ".qcow2",
        ".vmdk",
        ".dmg",
        ".keychain",
        ".keychain-db",
        ".pem",
        ".key",
        ".partial",
        ".part",
        ".incomplete",
        ".env",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
    {
        return Some("database, disk image, credential or active download".into());
    }
    None
}

/// The nearest enclosing repository (a directory with a `.git` marker or bare-repository layout),
/// if any. Observes markers only, never runs Git.
pub fn repository_of(root: &Path, entry: &Entry, memo: &mut RepositoryMemo) -> Option<PathBuf> {
    let absolute = root.join(platform::path(&entry.path).ok()?);
    let mut current = Some(absolute.as_path());
    while let Some(parent) = current {
        if memo.is_repository(parent) {
            return Some(parent.to_path_buf());
        }
        current = parent.parent();
    }
    None
}

pub fn cleanup_denial(root: &Path, entry: &Entry) -> String {
    protection(root, entry).unwrap_or_else(|| {
        "OS Trash is not implemented on this build; permanent deletion works only from OMG Trash".into()
    })
}

pub fn inside(path: &[u8], scope: &[u8], subtree: bool) -> bool {
    if path == scope {
        return true;
    }
    subtree
        && (scope.is_empty()
            || path
                .strip_prefix(scope)
                .is_some_and(|rest| rest.starts_with(b"/")))
}

pub fn safe_relative(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        && path != PathBuf::new()
}
