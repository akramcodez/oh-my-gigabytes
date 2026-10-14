use crate::{Entry, platform};
use std::path::{Path, PathBuf};

/// Memo of "is this directory a repository marker" answers, valid for one bulk evaluation (a scan
/// rollup) so that thousands of entries do not repeat the same ancestor filesystem checks.
#[derive(Default)]
pub struct RepositoryMemo(std::collections::HashMap<PathBuf, bool>);
impl RepositoryMemo {
    fn is_repository(&mut self, dir: &Path) -> bool {
        *self.0.entry(dir.to_path_buf()).or_insert_with(|| {
            dir.is_dir()
                && (dir.join(".git").symlink_metadata().is_ok()
                    || (dir.join("HEAD").is_file()
                        && dir.join("objects").is_dir()
                        && dir.join("refs").is_dir()))
        })
    }
}

/// Independent policy: catalog tier/action is never cleanup authority.
pub fn protection(root: &Path, entry: &Entry) -> Option<String> {
    protection_with(root, entry, &mut RepositoryMemo::default())
}

pub fn protection_with(root: &Path, entry: &Entry, memo: &mut RepositoryMemo) -> Option<String> {
    hard_protection(root, entry).or_else(|| {
        repository_of(root, entry, memo).map(|_| "repository scope protected".to_string())
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
