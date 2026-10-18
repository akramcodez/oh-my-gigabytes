//! Central capability contract (WP02). Every user-visible claim about what OMG may do comes from
//! here: the backend enforces it, `omg doctor` prints it, and the release manifest declares it.
//!
//! `Support` is deliberately coarse. `DevelopmentUnqualified` means "offered for development use,
//! without recorded native qualification evidence". No profile reaches `Qualified` yet.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Not offered on this platform or filesystem.
    Unsupported,
    /// Offered for development use. Not qualified by native evidence.
    DevelopmentUnqualified,
    /// Qualified by recorded native evidence. No profile has this yet.
    Qualified,
}

impl Support {
    pub fn as_str(self) -> &'static str {
        match self {
            Support::Unsupported => "unsupported",
            Support::DevelopmentUnqualified => "development-unqualified",
            Support::Qualified => "qualified",
        }
    }
}

/// Filesystem families known to OMG, from the kernel's statfs magic number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Filesystem {
    /// The ext-family magic (ext2, ext3, ext4). ext2 has no birth time.
    Ext,
    Tmpfs,
    Btrfs,
    /// XFS (the default on RHEL-family systems). Scanning only; permanent deletion is not offered.
    Xfs,
    /// Windows NTFS. Scanning is supported; permanent deletion is not, until the Windows milestone.
    Ntfs,
    /// Windows ReFS. Scanning only.
    Refs,
    /// Windows FAT32 or exFAT. Scanning only; these carry no ownership or permission data.
    Fat,
    Other,
}

const EXT_SUPER_MAGIC: u64 = 0xef53;
const TMPFS_MAGIC: u64 = 0x0102_1994;
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const XFS_SUPER_MAGIC: u64 = 0x5846_5342;

impl Filesystem {
    pub fn from_magic(magic: u64) -> Self {
        match magic {
            EXT_SUPER_MAGIC => Filesystem::Ext,
            TMPFS_MAGIC => Filesystem::Tmpfs,
            BTRFS_SUPER_MAGIC => Filesystem::Btrfs,
            XFS_SUPER_MAGIC => Filesystem::Xfs,
            _ => Filesystem::Other,
        }
    }
    /// Family from a Windows volume's file-system name (`GetVolumeInformationByHandleW`).
    pub fn from_name(name: &str) -> Self {
        match name {
            "NTFS" => Filesystem::Ntfs,
            "ReFS" => Filesystem::Refs,
            "FAT" | "FAT32" | "exFAT" => Filesystem::Fat,
            _ => Filesystem::Other,
        }
    }
}

/// Permanent deletion (decision D15) on a filesystem. Owner decision 2026-10-15: Btrfs is allowed
/// as `DevelopmentUnqualified`, because the owner's home is Btrfs. Btrfs shared extents and
/// snapshots mean freed space may be less than shown. Unknown filesystems are refused. On Windows
/// only NTFS is offered: it has stable 128-bit file ids and delete-by-handle. ReFS is untested and
/// FAT32/exFAT have no stable file ids, so both are refused.
pub fn permanent_delete(filesystem: Filesystem) -> Support {
    if !crate::PERMANENT_DELETE {
        return Support::Unsupported;
    }
    match filesystem {
        Filesystem::Ext | Filesystem::Tmpfs | Filesystem::Btrfs | Filesystem::Ntfs => {
            Support::DevelopmentUnqualified
        }
        Filesystem::Xfs | Filesystem::Refs | Filesystem::Fat | Filesystem::Other => {
            Support::Unsupported
        }
    }
}

/// OS Trash (reversible) is not implemented. It stays unsupported until WP09 passes.
pub fn os_trash() -> Support {
    Support::Unsupported
}

/// Permanent-delete support of this platform as reported by `omg doctor`, the status JSON and the
/// release manifest. Cleanup code exists on Linux and Windows; other platforms are unsupported.
pub fn platform_permanent_delete() -> Support {
    if crate::PERMANENT_DELETE && cfg!(any(target_os = "linux", windows)) {
        Support::DevelopmentUnqualified
    } else {
        Support::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_delete_is_unqualified_on_known_filesystems_and_refused_elsewhere() {
        // The analysis-only build (feature off) refuses everything; the default build allows the three families.
        let allowed = if crate::PERMANENT_DELETE {
            Support::DevelopmentUnqualified
        } else {
            Support::Unsupported
        };
        for magic in [EXT_SUPER_MAGIC, TMPFS_MAGIC, BTRFS_SUPER_MAGIC] {
            assert_eq!(permanent_delete(Filesystem::from_magic(magic)), allowed);
        }
        // XFS can be scanned but is not offered for deletion.
        assert_eq!(
            permanent_delete(Filesystem::from_magic(XFS_SUPER_MAGIC)),
            Support::Unsupported
        );
        assert_eq!(
            permanent_delete(Filesystem::from_magic(0xdead_beef)),
            Support::Unsupported
        );
    }

    #[test]
    fn only_ntfs_is_offered_among_windows_volumes() {
        let allowed = if crate::PERMANENT_DELETE {
            Support::DevelopmentUnqualified
        } else {
            Support::Unsupported
        };
        assert_eq!(permanent_delete(Filesystem::from_name("NTFS")), allowed);
        for name in ["ReFS", "FAT32", "exFAT", "FAT", "NFS"] {
            assert_eq!(
                permanent_delete(Filesystem::from_name(name)),
                Support::Unsupported,
                "{name}"
            );
        }
    }

    #[test]
    fn os_trash_is_unsupported_and_no_profile_is_qualified() {
        assert_eq!(os_trash(), Support::Unsupported);
        for filesystem in [Filesystem::Ext, Filesystem::Tmpfs, Filesystem::Btrfs] {
            assert_ne!(permanent_delete(filesystem), Support::Qualified);
        }
    }

    #[test]
    fn manifest_strings_are_stable() {
        assert_eq!(
            Support::DevelopmentUnqualified.as_str(),
            "development-unqualified"
        );
        assert_eq!(Support::Unsupported.as_str(), "unsupported");
    }
}
