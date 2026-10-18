use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[cfg(windows)]
pub mod windows;
/// Owner-only state directories on Windows. Separate from `windows`, which stays read-only.
#[cfg(windows)]
pub mod windows_state;

/// Native scanning and state work on Linux, macOS and Windows. Other platforms compile the stubs only.
pub fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
}
pub fn home() -> Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("home directory unavailable"))
}
/// `%LOCALAPPDATA%` on Windows (per-user, not roamed). Used for state and configuration.
fn local_app_data() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is not set"))
}
pub fn default_state() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OMG_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(windows) {
        Ok(local_app_data()?.join("omg"))
    } else if cfg!(target_os = "macos") {
        Ok(home()?.join("Library/Application Support/omg"))
    } else {
        Ok(std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".local/state"))
            .join("omg"))
    }
}
pub fn default_config() -> Result<PathBuf> {
    if cfg!(windows) {
        Ok(local_app_data()?.join("omg").join("config.toml"))
    } else if cfg!(target_os = "macos") {
        Ok(home()?.join("Library/Application Support/omg/config.toml"))
    } else {
        Ok(std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or(home()?.join(".config"))
            .join("omg/config.toml"))
    }
}
/// Live (capacity, used) bytes of the filesystem holding `root`, or `None` when unsupported.
/// "Used" matches `df`: blocks holding data, excluding space the filesystem reserves for root.
pub fn volume_sample(root: &Path) -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let sample = rustix::fs::fstatvfs(&linux::root(root).ok()?).ok()?;
        Some((
            sample.f_blocks.saturating_mul(sample.f_frsize),
            sample
                .f_blocks
                .saturating_sub(sample.f_bfree)
                .saturating_mul(sample.f_frsize),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let sample = rustix::fs::fstatvfs(&macos::root(root).ok()?).ok()?;
        Some((
            sample.f_blocks.saturating_mul(sample.f_frsize),
            sample
                .f_blocks
                .saturating_sub(sample.f_bfree)
                .saturating_mul(sample.f_frsize),
        ))
    }
    #[cfg(windows)]
    {
        windows::volume_sample(root)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = root;
        None
    }
}

/// Stored form of a native path: bytes on Unix; WTF-8 with `/` separators on Windows.
pub fn raw(path: &Path) -> Result<Vec<u8>> {
    #[cfg(any(unix, windows))]
    {
        Ok(encode(path))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        bail!("native paths are not certified on this platform")
    }
}
pub fn path(raw: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(raw)))
    }
    #[cfg(windows)]
    {
        windows::decode_path(raw)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = raw;
        bail!("native paths are not certified on this platform")
    }
}
#[cfg(any(unix, windows))]
fn encode(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        windows::encode_path(path)
    }
}

/// Windows names are UTF-16 and may hold unpaired surrogates, which UTF-8 cannot store. They are kept
/// as WTF-8 (UTF-8 that also encodes unpaired surrogates). `\` cannot occur in a Windows name, so the
/// stored `/` separator maps back to `\` without loss.
#[cfg(any(windows, test))]
pub fn wtf8_encode(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let mut code = u32::from(units[i]);
        if (0xD800..0xDC00).contains(&code)
            && let Some(&low) = units.get(i + 1)
            && (0xDC00..0xE000).contains(&u32::from(low))
        {
            code = 0x10000 + ((code - 0xD800) << 10) + (u32::from(low) - 0xDC00);
            i += 1;
        }
        i += 1;
        if code == u32::from(b'\\') {
            out.push(b'/');
            continue;
        }
        match code {
            0..=0x7F => out.push(code as u8),
            0x80..=0x7FF => out.extend([0xC0 | (code >> 6) as u8, 0x80 | (code & 0x3F) as u8]),
            0x800..=0xFFFF => out.extend([
                0xE0 | (code >> 12) as u8,
                0x80 | ((code >> 6) & 0x3F) as u8,
                0x80 | (code & 0x3F) as u8,
            ]),
            _ => out.extend([
                0xF0 | (code >> 18) as u8,
                0x80 | ((code >> 12) & 0x3F) as u8,
                0x80 | ((code >> 6) & 0x3F) as u8,
                0x80 | (code & 0x3F) as u8,
            ]),
        }
    }
    out
}

/// Inverse of [`wtf8_encode`]. `None` for bytes that were not produced by it.
#[cfg(any(windows, test))]
pub fn wtf8_decode(bytes: &[u8]) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        let (len, mut code, min) = match lead {
            0x00..=0x7F => (1, u32::from(lead), 0),
            0xC0..=0xDF => (2, u32::from(lead & 0x1F), 0x80),
            0xE0..=0xEF => (3, u32::from(lead & 0x0F), 0x800),
            0xF0..=0xF7 => (4, u32::from(lead & 0x07), 0x10000),
            _ => return None,
        };
        if i + len > bytes.len() {
            return None;
        }
        for &continuation in &bytes[i + 1..i + len] {
            if continuation & 0xC0 != 0x80 {
                return None;
            }
            code = (code << 6) | u32::from(continuation & 0x3F);
        }
        if code < min || code > 0x10FFFF {
            return None;
        }
        i += len;
        if code == u32::from(b'/') {
            out.push(u16::from(b'\\'));
        } else if code >= 0x10000 {
            let value = code - 0x10000;
            out.push(0xD800 + (value >> 10) as u16);
            out.push(0xDC00 + (value & 0x3FF) as u16);
        } else {
            out.push(code as u16);
        }
    }
    Some(out)
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
/// Windows and macOS (default volumes) file names are case-insensitive; Linux names are compared
/// exactly. Matching case-insensitively on macOS is deliberate: it can only protect more.
fn same_name(part: &std::ffi::OsStr, name: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        part.to_string_lossy().eq_ignore_ascii_case(name)
    } else {
        part == std::ffi::OsStr::new(name)
    }
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
        // Windows system-managed folders and files: unreadable by design, never user data.
        "$Recycle.Bin",
        "System Volume Information",
        "pagefile.sys",
        "hiberfil.sys",
        "swapfile.sys",
        // macOS volume metadata and cloud-sync folders: system-managed, never user data.
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        ".DocumentRevisions-V100",
        ".TemporaryItems",
    ];
    if path.components().any(|part| {
        mandatory
            .iter()
            .any(|name| same_name(part.as_os_str(), name))
    }) {
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
    // macOS: the sealed system volume, swap and the volfs mount.
    if cfg!(target_os = "macos")
        && ["/System", "/private/var/vm", "/.vol"]
            .iter()
            .any(|root| path.starts_with(root))
    {
        return Some("unsupported or system-managed filesystem".into());
    }
    None
}

/// What the scanner needs about one object, read without following links. Built from `stat` on
/// Linux and from directory and handle information on Windows.
#[derive(Debug, Clone)]
pub struct Meta {
    /// Stable object identity: `device:inode` on Linux, `volume:file-id` on Windows.
    pub identity: String,
    pub kind: &'static str,
    /// Device (Linux) or volume serial (Windows). A directory on another volume is a mount boundary.
    pub volume: u64,
    pub apparent: u64,
    pub allocated: u64,
    pub links: u64,
    pub modified_ns: i64,
    pub changed_ns: i64,
    /// Set when reading would recall or hydrate content (cloud and offline placeholders). Such
    /// objects are recorded as excluded and never read or descended.
    pub restricted: Option<&'static str>,
}

/// Index entry for `relative` (root-relative) built from the scanner's neutral metadata.
pub fn entry_from_meta(relative: &Path, meta: &Meta) -> crate::model::Entry {
    crate::model::Entry {
        path: encode(relative),
        parent: encode(relative.parent().unwrap_or(Path::new(""))),
        kind: meta.kind.into(),
        identity: meta.identity.clone(),
        apparent: meta.apparent,
        allocated: meta.allocated,
        links: meta.links,
        modified_ns: meta.modified_ns,
        changed_ns: meta.changed_ns,
        excluded: None,
    }
}

/// Platform adapter used by the scanner. Linux and macOS anchor every open at a directory descriptor
/// and refuse links on the whole path. Windows
/// opens with reparse points visible and checks each opened directory's final path and file identity
/// against the enumeration that named it.
pub mod native {
    #[cfg(target_os = "linux")]
    pub use super::linux::{
        Dir, dir_meta, for_each_child, inode_usage, open_dir, open_root, volume,
    };
    #[cfg(target_os = "macos")]
    pub use super::macos::{
        Dir, dir_meta, for_each_child, inode_usage, open_dir, open_root, volume,
    };
    #[cfg(windows)]
    pub use super::windows::{
        Dir, dir_meta, for_each_child, inode_usage, open_dir, open_root, volume,
    };
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use crate::model::Entry;
    use rustix::{
        fd::OwnedFd,
        fs::{self, AtFlags, Dir as Listing, FileType, Mode, OFlags, ResolveFlags},
    };
    use std::{
        ffi::{CStr, OsString},
        os::unix::ffi::{OsStrExt, OsStringExt},
    };

    /// An open, verified directory.
    pub type Dir = OwnedFd;

    /// Filesystem family of an open root. Scanning accepts only the families in `capability`.
    pub fn filesystem(fd: &OwnedFd) -> Result<crate::capability::Filesystem> {
        Ok(crate::capability::Filesystem::from_magic(
            fs::fstatfs(fd)?.f_type as u64,
        ))
    }

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
        .map_err(|error| {
            if error == rustix::io::Errno::NOSYS {
                anyhow::anyhow!("openat2 is unavailable: omg needs Linux 5.6 or newer")
            } else {
                anyhow::anyhow!(
                    "root acquisition denied: missing, inaccessible, redirected or unsupported openat2"
                )
            }
        })?;
        if filesystem(&fd)? == crate::capability::Filesystem::Other {
            bail!(
                "unsupported filesystem profile; use a local ext-family, XFS, tmpfs or Btrfs Linux root"
            );
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
    /// Neutral metadata from one `stat` (never follows links).
    pub fn meta(stat: &fs::Stat) -> Meta {
        let kind = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => "file",
            FileType::Directory => "directory",
            FileType::Symlink => "symlink",
            _ => "special",
        };
        // `st_nlink` is u64 on x86_64 and u32 on aarch64.
        #[allow(clippy::unnecessary_cast)]
        let links = stat.st_nlink as u64;
        Meta {
            identity: format!("{}:{}", stat.st_dev, stat.st_ino),
            kind,
            volume: stat.st_dev,
            apparent: stat.st_size.max(0) as u64,
            allocated: (stat.st_blocks.max(0) as u64).saturating_mul(512),
            links,
            modified_ns: stat
                .st_mtime
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.st_mtime_nsec as i64),
            changed_ns: stat
                .st_ctime
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.st_ctime_nsec as i64),
            restricted: None,
        }
    }
    pub fn entry(relative: &Path, metadata: &fs::Stat) -> Entry {
        entry_from_meta(relative, &meta(metadata))
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

    /// Open a directory below `root` and check that it is still the object the scan enumerated.
    pub fn open_root(path: &Path) -> Result<Dir> {
        root(path)
    }
    pub fn open_dir(root: &Dir, relative: &Path, expected: &str) -> Result<Dir> {
        let fd = acquire(root, relative, true)?;
        if meta(&fs::fstat(&fd)?).identity != expected {
            bail!("directory identity changed");
        }
        Ok(fd)
    }
    pub fn dir_meta(dir: &Dir) -> Result<Meta> {
        Ok(meta(&fs::fstat(dir)?))
    }
    /// Visit every child of `dir` (no `.` or `..`) with metadata read relative to the open
    /// directory. Returns `Ok(false)` if `visit` stopped the walk early.
    pub fn for_each_child(
        dir: &Dir,
        mut visit: impl FnMut(OsString, Result<Meta>) -> Result<bool>,
    ) -> Result<bool> {
        let mut listing = Listing::read_from(dir)?;
        while let Some(value) = listing.read() {
            let value = value?;
            let bytes = value.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = OsString::from_vec(bytes.to_vec());
            // One no-follow stat relative to the open directory; it describes the object that
            // currently has this name and never follows a link.
            let metadata = fs::statat(dir, value.file_name(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| meta(&stat))
                .map_err(anyhow::Error::from);
            if !visit(name, metadata)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Used inodes on the filesystem holding `dir`, for progress estimates.
    pub fn inode_usage(dir: &Dir) -> Result<Option<u64>> {
        let stat = fs::fstatvfs(dir)?;
        Ok(Some(stat.f_files.saturating_sub(stat.f_ffree)))
    }

    #[cfg(test)]
    mod tests {
        use crate::capability::Filesystem;

        #[test]
        fn filesystem_allowlist_includes_btrfs_and_rejects_unknown_profiles() {
            assert_eq!(Filesystem::from_magic(0xef53), Filesystem::Ext);
            assert_eq!(Filesystem::from_magic(0x0102_1994), Filesystem::Tmpfs);
            assert_eq!(Filesystem::from_magic(0x9123_683e), Filesystem::Btrfs);
            assert_eq!(Filesystem::from_magic(0x5846_5342), Filesystem::Xfs);
            assert_eq!(Filesystem::from_magic(0xdead_beef), Filesystem::Other);
        }
    }
}

/// Darwin kernel major version from `uname -r` (macOS 15 is Darwin 24, macOS 26 is Darwin 25).
#[cfg(any(target_os = "macos", test))]
fn darwin_major(release: &str) -> Option<u32> {
    release.split('.').next()?.parse().ok()
}

/// macOS adapter. Every open is anchored at a directory descriptor with `O_NOFOLLOW_ANY` (no link
/// anywhere in the path) and `O_RESOLVE_BENEATH` (the path cannot leave the anchor), the macOS
/// equivalents of Linux `RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH`. Behaviour measured on macOS 15 and
/// 26 (arm64 and Intel) in `docs/macos-evidence.md`. Analysis only: nothing here removes or
/// changes a file.
#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;
    use crate::capability::Filesystem;
    use rustix::{
        fd::OwnedFd,
        fs::{self, AtFlags, Dir as Listing, FileType, Mode, OFlags},
    };
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    /// An open, verified directory.
    pub type Dir = OwnedFd;

    // Values from the macOS SDK, confirmed by the R03 probe. rustix does not name them.
    const O_NOFOLLOW_ANY: u32 = 0x2000_0000;
    const O_RESOLVE_BENEATH: u32 = 0x1000;
    /// `statfs.f_flags`: the filesystem is local.
    const MNT_LOCAL: u32 = 0x1000;
    /// `statfs.f_flags`: the sealed, read-only system volume.
    const MNT_ROOTFS: u32 = 0x4000;
    /// `st_flags`: contents live in the cloud (iCloud "dataless"); reading would download them.
    const SF_DATALESS: u32 = 0x4000_0000;
    /// Oldest Darwin kernel that enforces the two open flags above (macOS 15). An older kernel
    /// could ignore an unknown flag without error and silently lose the protection.
    const MIN_DARWIN: u32 = 24;

    fn anchored() -> OFlags {
        OFlags::from_bits_retain(O_NOFOLLOW_ANY | O_RESOLVE_BENEATH)
    }

    fn require_supported_kernel() -> Result<()> {
        let uname = rustix::system::uname();
        let release = uname.release().to_string_lossy();
        match darwin_major(&release) {
            Some(major) if major >= MIN_DARWIN => Ok(()),
            _ => bail!("macOS 15 or newer is required (kernel {release})"),
        }
    }

    /// Filesystem family and mount flags of an open directory.
    pub fn filesystem(fd: &OwnedFd) -> Result<(Filesystem, u32)> {
        let stat = fs::fstatfs(fd)?;
        let name: Vec<u8> = stat
            .f_fstypename
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte as u8)
            .collect();
        Ok((
            Filesystem::from_name(&String::from_utf8_lossy(&name)),
            stat.f_flags,
        ))
    }

    pub fn root(path: &Path) -> Result<OwnedFd> {
        require_supported_kernel()?;
        let fd = fs::open(
            path,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::CLOEXEC
                | OFlags::from_bits_retain(O_NOFOLLOW_ANY),
            Mode::empty(),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP {
                anyhow::anyhow!(
                    "root acquisition denied: the path passes through a symbolic link (on macOS /tmp, /var and /etc are links); give the real path, for example /private/tmp"
                )
            } else {
                anyhow::anyhow!("root acquisition denied: missing, inaccessible or redirected path")
            }
        })?;
        let (filesystem, flags) = filesystem(&fd)?;
        if flags & MNT_ROOTFS != 0 {
            bail!(
                "the sealed macOS system volume is not scanned; choose a folder in your home folder"
            );
        }
        if flags & MNT_LOCAL == 0 || filesystem == Filesystem::Other {
            bail!("unsupported filesystem profile; use a local APFS, HFS+, exFAT or FAT volume");
        }
        Ok(fd)
    }
    fn acquire(root: &OwnedFd, relative: &Path) -> Result<OwnedFd> {
        Ok(fs::openat(
            root,
            if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            },
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | anchored(),
            Mode::empty(),
        )?)
    }
    /// Neutral metadata from one `stat` (never follows links).
    pub fn meta(stat: &fs::Stat) -> Meta {
        let kind = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => "file",
            FileType::Directory => "directory",
            FileType::Symlink => "symlink",
            _ => "special",
        };
        let device = u64::from(stat.st_dev as u32);
        Meta {
            identity: format!("{}:{}", device, stat.st_ino),
            kind,
            volume: device,
            apparent: stat.st_size.max(0) as u64,
            allocated: (stat.st_blocks.max(0) as u64).saturating_mul(512),
            links: u64::from(stat.st_nlink),
            modified_ns: stat
                .st_mtime
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.st_mtime_nsec),
            changed_ns: stat
                .st_ctime
                .saturating_mul(1_000_000_000)
                .saturating_add(stat.st_ctime_nsec),
            restricted: (stat.st_flags & SF_DATALESS != 0)
                .then_some("cloud placeholder (dataless) not read"),
        }
    }
    pub fn volume(fd: &OwnedFd) -> Result<(u64, u64)> {
        let sample = fs::fstatvfs(fd)?;
        Ok((
            sample.f_blocks.saturating_mul(sample.f_frsize),
            sample.f_bavail.saturating_mul(sample.f_frsize),
        ))
    }

    pub fn open_root(path: &Path) -> Result<Dir> {
        root(path)
    }
    /// Open a directory below `root` and check that it is still the object the scan enumerated.
    pub fn open_dir(root: &Dir, relative: &Path, expected: &str) -> Result<Dir> {
        let fd = acquire(root, relative)?;
        if meta(&fs::fstat(&fd)?).identity != expected {
            bail!("directory identity changed");
        }
        Ok(fd)
    }
    pub fn dir_meta(dir: &Dir) -> Result<Meta> {
        Ok(meta(&fs::fstat(dir)?))
    }
    /// Visit every child of `dir` (no `.` or `..`) with metadata read relative to the open
    /// directory. Returns `Ok(false)` if `visit` stopped the walk early.
    pub fn for_each_child(
        dir: &Dir,
        mut visit: impl FnMut(OsString, Result<Meta>) -> Result<bool>,
    ) -> Result<bool> {
        let mut listing = Listing::read_from(dir)?;
        while let Some(value) = listing.read() {
            let value = value?;
            let bytes = value.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = OsString::from_vec(bytes.to_vec());
            // One no-follow stat relative to the open directory; it describes the object that
            // currently has this name and never follows a link.
            let metadata = fs::statat(dir, value.file_name(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| meta(&stat))
                .map_err(anyhow::Error::from);
            if !visit(name, metadata)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Used inodes, for progress estimates. Only APFS reports a meaningful figure; HFS+ allocates
    /// inodes dynamically and exFAT/FAT report none, so those are unknown rather than zero.
    pub fn inode_usage(dir: &Dir) -> Result<Option<u64>> {
        if filesystem(dir)?.0 != Filesystem::Apfs {
            return Ok(None);
        }
        let stat = fs::fstatvfs(dir)?;
        Ok(Some(stat.f_files.saturating_sub(stat.f_ffree)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn open_flags_match_the_sdk_values_seen_on_the_probe() {
            assert_eq!(O_NOFOLLOW_ANY, 0x2000_0000);
            assert_eq!(O_RESOLVE_BENEATH, 0x1000);
        }

        #[test]
        fn a_link_in_the_root_path_is_refused_with_a_hint() {
            let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
            let target = base.join(format!("omg-mac-target-{}", std::process::id()));
            let link = base.join(format!("omg-mac-link-{}", std::process::id()));
            std::fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, &link).unwrap();
            let error = root(&link).unwrap_err().to_string();
            std::fs::remove_file(&link).unwrap();
            std::fs::remove_dir(&target).unwrap();
            assert!(error.contains("symbolic link"), "{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{darwin_major, wtf8_decode, wtf8_encode};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn wtf8_round_trips_names_with_separators_emoji_and_lone_surrogates() {
        for units in [
            wide("C:\\Users\\SK AKRAM\\Downloads"),
            wide("café 😀 報告"),
            vec![0x0061, 0xD800, 0x0062],
            vec![0xDC00, 0x0063],
            vec![0xD83D, 0xDE00],
        ] {
            let bytes = wtf8_encode(&units);
            let decoded = wtf8_decode(&bytes).expect("own output decodes");
            // Separators are the only change: `\` is stored as `/` and restored.
            assert_eq!(decoded, units, "round trip of {units:x?}");
        }
    }

    #[test]
    fn separators_are_stored_as_slash_and_lone_surrogates_take_three_bytes() {
        assert_eq!(wtf8_encode(&wide("a\\b")), b"a/b");
        assert_eq!(wtf8_encode(&[0xD800]), vec![0xED, 0xA0, 0x80]);
        assert_eq!(wtf8_encode(&[0xD83D, 0xDE00]), "😀".as_bytes());
    }

    #[test]
    fn malformed_stored_bytes_are_rejected() {
        assert_eq!(wtf8_decode(&[0xFF]), None);
        assert_eq!(wtf8_decode(&[0xC0, 0x80]), None, "overlong encoding");
        assert_eq!(wtf8_decode(&[0xE2, 0x82]), None, "truncated sequence");
    }

    #[test]
    fn darwin_kernel_versions_map_to_macos_releases() {
        assert_eq!(darwin_major("24.6.0"), Some(24));
        assert_eq!(darwin_major("25.0.0"), Some(25));
        assert_eq!(darwin_major("23.6.0"), Some(23));
        assert_eq!(darwin_major("junk"), None);
        assert_eq!(darwin_major(""), None);
    }
}
