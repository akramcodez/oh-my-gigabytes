use crate::{Entry, platform};
use std::path::{Path, PathBuf};

/// Independent policy: catalog tier/action is never cleanup authority.
pub fn protection(root: &Path, entry: &Entry) -> Option<String> {
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
    // Whole known repositories are protected. This observes markers only, never runs Git.
    let mut current = Some(absolute.as_path());
    while let Some(parent) = current {
        if parent.is_dir()
            && (parent.join(".git").symlink_metadata().is_ok()
                || (parent.join("HEAD").is_file()
                    && parent.join("objects").is_dir()
                    && parent.join("refs").is_dir()))
        {
            return Some("repository scope protected".into());
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
