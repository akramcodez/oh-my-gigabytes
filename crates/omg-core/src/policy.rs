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

/// Names that are sensitive wherever they appear, even inside caches or old repositories.
pub fn always_sensitive(name: &str) -> bool {
    [
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
    ]
    .contains(&name.to_ascii_lowercase().as_str())
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
    for component in absolute.components() {
        let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
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
        "No identity-safe Trash handler is qualified on this build; cleanup disabled".into()
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
