use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub fn supported() -> bool {
    cfg!(target_os = "linux")
}
pub fn home() -> Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("home directory unavailable"))
}
pub fn default_state() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OMG_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".local/state"))
        .join("omg"))
}
pub fn default_config() -> Result<PathBuf> {
    Ok(std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(home()?.join(".config"))
        .join("omg/config.toml"))
}
pub fn raw(path: &Path) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        bail!("native Windows paths are not certified")
    }
}
pub fn path(raw: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(raw)))
    }
    #[cfg(not(unix))]
    {
        let _ = raw;
        bail!("native Windows paths are not certified")
    }
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let value = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if value
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        bail!("parent components are not allowed in a scan/state root");
    }
    Ok(value)
}
pub fn excluded(path: &Path, custom: &[PathBuf]) -> Option<String> {
    let mandatory = [
        ".ssh",
        ".gnupg",
        ".aws",
        ".azure",
        ".git",
        ".docker",
        ".kube",
        ".Trash",
        "OneDrive",
        "Dropbox",
        "Google Drive",
        "CloudStorage",
        "Mobile Documents",
        ".git-credentials",
        ".npmrc",
        ".pgpass",
    ];
    if path
        .components()
        .any(|part| mandatory.iter().any(|name| part.as_os_str() == *name))
    {
        return Some("mandatory sensitive, managed or cloud exclusion".into());
    }
    if custom
        .iter()
        .any(|item| path == item || path.starts_with(item))
    {
        return Some("configured exclusion".into());
    }
    if [
        "/proc",
        "/sys",
        "/dev",
        "/run",
        "/var/lib/docker",
        "/var/lib/containers",
    ]
    .iter()
    .any(|root| path.starts_with(root))
    {
        return Some("unsupported or system-managed filesystem".into());
    }
    None
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use crate::model::Entry;
    use rustix::{
        fd::OwnedFd,
        fs::{self, FileType, Mode, OFlags, ResolveFlags},
    };
    use std::{ffi::CStr, os::unix::ffi::OsStrExt};

    pub fn root(path: &Path) -> Result<OwnedFd> {
        let anchor = fs::open(
            "/",
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let relative = path.strip_prefix("/")?;
        let fd = fs::openat2(
            &anchor,
            if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            },
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "root acquisition denied: missing, inaccessible, redirected or unsupported openat2"
            )
        })?;
        let magic = fs::fstatfs(&fd)?.f_type as u64;
        if ![0xef53, 0x01021994].contains(&magic) {
            bail!("unsupported filesystem profile; use a local ext-family/tmpfs Linux root");
        }
        Ok(fd)
    }
    pub fn acquire(root: &OwnedFd, relative: &Path, directory: bool) -> Result<OwnedFd> {
        let flags = if directory {
            OFlags::RDONLY | OFlags::DIRECTORY
        } else {
            OFlags::PATH
        };
        Ok(fs::openat2(
            root,
            if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            },
            flags | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?)
    }
    pub fn entry(relative: &Path, metadata: &fs::Stat) -> Entry {
        let kind = match FileType::from_raw_mode(metadata.st_mode) {
            FileType::RegularFile => "file",
            FileType::Directory => "directory",
            FileType::Symlink => "symlink",
            _ => "special",
        };
        Entry {
            path: relative.as_os_str().as_bytes().to_vec(),
            parent: relative
                .parent()
                .unwrap_or(Path::new(""))
                .as_os_str()
                .as_bytes()
                .to_vec(),
            kind: kind.into(),
            identity: format!("{}:{}", metadata.st_dev, metadata.st_ino),
            apparent: metadata.st_size.max(0) as u64,
            allocated: (metadata.st_blocks.max(0) as u64).saturating_mul(512),
            links: metadata.st_nlink,
            modified_ns: metadata
                .st_mtime
                .saturating_mul(1_000_000_000)
                .saturating_add(metadata.st_mtime_nsec as i64),
            changed_ns: metadata
                .st_ctime
                .saturating_mul(1_000_000_000)
                .saturating_add(metadata.st_ctime_nsec as i64),
            excluded: None,
        }
    }
    pub fn name(value: &CStr) -> PathBuf {
        PathBuf::from(std::ffi::OsStr::from_bytes(value.to_bytes()))
    }
    pub fn volume(fd: &OwnedFd) -> Result<(u64, u64)> {
        let sample = fs::fstatvfs(fd)?;
        Ok((
            sample.f_blocks.saturating_mul(sample.f_frsize),
            sample.f_bavail.saturating_mul(sample.f_frsize),
        ))
    }
}
