//! Windows native adapter (`docs/windows-implementation-plan.md`, W02).
//!
//! Objects are opened as verbatim `\\?\` paths with `FILE_FLAG_OPEN_REPARSE_POINT`, so links and
//! mount points are seen, never followed. A directory is accepted only when its final path and file
//! identity still match the enumeration that named it. That refuses a junction swapped into an
//! intermediate component after enumeration. Relative `NtCreateFile` opens (the plan's stronger
//! containment) are not used yet; that gap is recorded in `docs/windows-evidence.md`.
//!
//! Everything here reads; nothing in this module changes the filesystem.

use super::{Meta, wtf8_decode, wtf8_encode};
use crate::capability::Filesystem;
use anyhow::{Result, bail};
use std::{
    ffi::{OsString, c_void},
    io, mem,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO,
        FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileBasicInfo, FileIdExtdDirectoryInfo,
        FileIdExtdDirectoryRestartInfo, FileIdInfo, GetDiskFreeSpaceExW, GetDriveTypeW,
        GetFileInformationByHandleEx, GetFinalPathNameByHandleW, GetVolumeInformationByHandleW,
        OPEN_EXISTING,
    },
};

/// `GetDriveTypeW` result for a network share.
const DRIVE_REMOTE: u32 = 4;
/// Reparse tags that mark ordinary data, not links: deduplicated files and WIM-backed files.
const TAG_DEDUP: u32 = 0x8000_0013;
const TAG_WIM: u32 = 0x8000_0018;
/// Offset between the FILETIME epoch (1601) and the Unix epoch, in 100 ns units.
const FILETIME_UNIX_EPOCH: i64 = 116_444_736_000_000_000;
const DOT: u16 = b'.' as u16;
const BACKSLASH: u16 = b'\\' as u16;
/// Full sharing for read-only opens: other programs may read, write, rename and delete meanwhile.
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Cloud-provider reparse tags (`IO_REPARSE_TAG_CLOUD*`): contents live remotely and must not be read.
fn is_cloud_tag(tag: u32) -> bool {
    tag & 0xFFFF_0FFF == 0x9000_001A
}

/// Stored form of a Windows path: WTF-8 with `/` separators.
pub fn encode_path(path: &Path) -> Vec<u8> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    wtf8_encode(&units)
}

/// Inverse of [`encode_path`].
pub fn decode_path(raw: &[u8]) -> Result<PathBuf> {
    let Some(units) = wtf8_decode(raw) else {
        bail!("stored path is not valid WTF-8");
    };
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

/// An owned kernel handle, closed exactly once.
struct Handle(HANDLE);

// SAFETY: a kernel handle may be used from any thread. Each handle is closed once, by `Drop`, and
// the scanner shares only the root directory's handle, which it does not enumerate concurrently.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by `CreateFileW` and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// A verified directory: its handle, final path (no `\\?\` prefix) and volume serial.
pub struct Dir {
    handle: Handle,
    path: Vec<u16>,
    volume: u64,
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn identity_of(volume: u64, id: [u8; 16]) -> String {
    format!("{volume:x}:{:032x}", u128::from_le_bytes(id))
}

fn filetime_ns(filetime: i64) -> i64 {
    filetime
        .saturating_sub(FILETIME_UNIX_EPOCH)
        .saturating_mul(100)
}

/// Raw facts about one object, from either a directory listing or a handle.
struct Raw {
    attributes: u32,
    tag: u32,
    size: i64,
    allocated: i64,
    modified: i64,
    changed: i64,
    volume: u64,
    id: [u8; 16],
}

fn meta_of(raw: Raw) -> Meta {
    let directory = raw.attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let cloud = is_cloud_tag(raw.tag)
        || raw.attributes
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0;
    let link = raw.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && !cloud
        && !matches!(raw.tag, TAG_DEDUP | TAG_WIM);
    Meta {
        identity: identity_of(raw.volume, raw.id),
        kind: if link {
            "symlink"
        } else if directory {
            "directory"
        } else {
            "file"
        },
        volume: raw.volume,
        apparent: if directory { 0 } else { raw.size.max(0) as u64 },
        allocated: raw.allocated.max(0) as u64,
        // Link counts are not part of directory enumeration, so each name counts once here.
        // Hard-linked data on Windows is therefore counted per name until link counts are read.
        links: 1,
        modified_ns: filetime_ns(raw.modified),
        changed_ns: filetime_ns(raw.changed),
        restricted: cloud.then_some("cloud or offline placeholder; content is not read"),
    }
}

/// Wide path with `/` turned into `\`, without a terminator.
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'/') {
                BACKSLASH
            } else {
                unit
            }
        })
        .collect()
}

/// `base` followed by the components of `relative`, with `\` separators.
fn join(base: &[u16], relative: &Path) -> Result<Vec<u16>> {
    let mut out = base.to_vec();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            bail!("unexpected path component in a stored path");
        };
        if out.last() != Some(&BACKSLASH) {
            out.push(BACKSLASH);
        }
        out.extend(name.encode_wide());
    }
    Ok(out)
}

/// Windows paths compare ASCII case-insensitively; a final path always carries on-disk case.
fn same_path(a: &[u16], b: &[u16]) -> bool {
    fn fold(unit: u16) -> u16 {
        if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
            unit + 32
        } else {
            unit
        }
    }
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| fold(x) == fold(y))
}

/// Open a verbatim path with reparse points visible, with the given access and sharing.
fn open_shared(path: &[u16], access: u32, share: u32) -> io::Result<Handle> {
    let mut wide: Vec<u16> = r"\\?\".encode_utf16().collect();
    wide.extend_from_slice(path);
    wide.push(0);
    // SAFETY: `wide` is NUL-terminated and alive for the call. The security attributes and template
    // handle are null, which is valid for an existing object.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            share,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(last_error())
    } else {
        Ok(Handle(handle))
    }
}

/// Read-only open, fully shared with other programs.
fn open(path: &[u16], access: u32) -> io::Result<Handle> {
    open_shared(path, access, SHARE_ALL)
}

fn query<T>(handle: &Handle, class: FILE_INFO_BY_HANDLE_CLASS, info: &mut T) -> io::Result<()> {
    // SAFETY: `info` is a plain `repr(C)` structure of exactly `size_of::<T>()` bytes, and the handle
    // is live for the call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            class,
            (info as *mut T).cast::<c_void>(),
            mem::size_of::<T>() as u32,
        )
    };
    if ok == 0 { Err(last_error()) } else { Ok(()) }
}

fn basic_info(handle: &Handle) -> io::Result<FILE_BASIC_INFO> {
    // SAFETY: all-zero is a valid value for this plain structure, and `query` fills it.
    let mut info: FILE_BASIC_INFO = unsafe { mem::zeroed() };
    query(handle, FileBasicInfo, &mut info)?;
    Ok(info)
}

/// Volume serial and 128-bit file identifier of an open object.
fn identity(handle: &Handle) -> io::Result<(u64, [u8; 16])> {
    // SAFETY: all-zero is a valid value for this plain structure, and `query` fills it.
    let mut info: FILE_ID_INFO = unsafe { mem::zeroed() };
    query(handle, FileIdInfo, &mut info)?;
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

/// Final path of an open object, with the `\\?\` prefix removed. Reflects every redirection.
fn final_path(handle: &Handle) -> io::Result<Vec<u16>> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: `buffer` has `buffer.len()` writable units. Flags 0 request the normalised DOS path.
        let length = unsafe {
            GetFinalPathNameByHandleW(handle.0, buffer.as_mut_ptr(), buffer.len() as u32, 0)
        } as usize;
        if length == 0 {
            return Err(last_error());
        }
        if length < buffer.len() {
            buffer.truncate(length);
            break;
        }
        buffer.resize(length + 1, 0);
    }
    let prefix = [BACKSLASH, BACKSLASH, u16::from(b'?'), BACKSLASH];
    if buffer.starts_with(&prefix) {
        buffer.drain(..prefix.len());
    }
    Ok(buffer)
}

fn filesystem(handle: &Handle) -> io::Result<Filesystem> {
    let mut name = [0u16; 32];
    // SAFETY: the name buffer is writable for `name.len()` units. The other outputs are optional and
    // passed as null.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle.0,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());
    Ok(Filesystem::from_name(&String::from_utf16_lossy(
        &name[..end],
    )))
}

/// One child as its directory lists it, before any open.
struct Child {
    name: Vec<u16>,
    meta: Meta,
}

/// List `handle`'s children (no `.` or `..`) from the start, calling `each` until it returns false.
/// The listing carries names, identifiers, attributes, sizes and times, so no child is opened.
fn read_listing(
    handle: &Handle,
    volume: u64,
    mut each: impl FnMut(Child) -> Result<bool>,
) -> Result<bool> {
    // u64 storage keeps the buffer 8-byte aligned, as the directory records require.
    let mut buffer = vec![0u64; 64 * 1024 / 8];
    // The first call restarts, so a handle that was listed before is read again from the top.
    let mut class = FileIdExtdDirectoryRestartInfo;
    loop {
        // SAFETY: the buffer is writable for its full length in bytes, and the handle is live.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                class,
                buffer.as_mut_ptr().cast::<c_void>(),
                (buffer.len() * mem::size_of::<u64>()) as u32,
            )
        };
        class = FileIdExtdDirectoryInfo;
        if ok == 0 {
            let error = last_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(true);
            }
            bail!("directory could not be read: {error}");
        }
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            // SAFETY: the operating system wrote a complete record at `offset` inside the buffer. The
            // record is read unaligned because the wire layout does not guarantee more.
            let info: FILE_ID_EXTD_DIR_INFO =
                unsafe { ptr::read_unaligned(base.add(offset).cast::<FILE_ID_EXTD_DIR_INFO>()) };
            // SAFETY: the name follows the fixed header and spans `FileNameLength` bytes in the buffer.
            let units = unsafe {
                std::slice::from_raw_parts(
                    base.add(offset + mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName))
                        .cast::<u16>(),
                    info.FileNameLength as usize / 2,
                )
            };
            if units != [DOT] && units != [DOT, DOT] {
                let child = Child {
                    name: units.to_vec(),
                    meta: meta_of(Raw {
                        attributes: info.FileAttributes,
                        tag: info.ReparsePointTag,
                        size: info.EndOfFile,
                        allocated: info.AllocationSize,
                        modified: info.LastWriteTime,
                        changed: info.ChangeTime,
                        volume,
                        id: info.FileId.Identifier,
                    }),
                };
                if !each(child)? {
                    return Ok(false);
                }
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
    }
}

/// Open the scan root. It must be a local drive path, not a link, not a network share, and on a
/// filesystem OMG supports.
pub fn open_root(path: &Path) -> Result<Dir> {
    let wanted = wide_path(path);
    let drive_letter = wanted
        .first()
        .is_some_and(|&unit| unit < 0x80 && (unit as u8).is_ascii_alphabetic());
    if !drive_letter || wanted.get(1) != Some(&u16::from(b':')) {
        bail!("only local drive paths are supported (for example C:\\Folder)");
    }
    let handle = open(&wanted, FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .map_err(|error| anyhow::anyhow!("folder could not be opened: {error}"))?;
    let basic = basic_info(&handle)?;
    if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!("scan root is a link or mount point; scan the folder it points to instead");
    }
    if basic.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        bail!("scan root is not a folder");
    }
    let path = final_path(&handle)?;
    if path.len() < 3 || path[1] != u16::from(b':') {
        bail!("scan root must be on a drive letter volume");
    }
    let drive: Vec<u16> = path[..2].iter().copied().chain([BACKSLASH, 0]).collect();
    // SAFETY: `drive` is NUL-terminated and alive for the call.
    if unsafe { GetDriveTypeW(drive.as_ptr()) } == DRIVE_REMOTE {
        bail!("network drives are not supported");
    }
    if filesystem(&handle)? == Filesystem::Other {
        bail!("unsupported filesystem profile; use a local NTFS, ReFS or FAT32 volume");
    }
    let (volume, _) = identity(&handle)?;
    Ok(Dir {
        handle,
        path,
        volume,
    })
}

/// Open `relative` below `root` and check that it is still the object `expected` names.
pub fn open_dir(root: &Dir, relative: &Path, expected: &str) -> Result<Dir> {
    let wanted = join(&root.path, relative)?;
    let handle = open(&wanted, FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .map_err(|error| anyhow::anyhow!("folder could not be opened: {error}"))?;
    let basic = basic_info(&handle)?;
    if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || basic.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        bail!("folder is a link, mount point or not a folder; not followed");
    }
    let path = final_path(&handle)?;
    if !same_path(&path, &wanted) {
        bail!("folder moved or redirected; skipped");
    }
    let (volume, id) = identity(&handle)?;
    if identity_of(volume, id) != expected {
        bail!("directory identity changed");
    }
    Ok(Dir {
        handle,
        path,
        volume,
    })
}

/// Metadata of an open directory.
pub fn dir_meta(dir: &Dir) -> Result<Meta> {
    let basic = basic_info(&dir.handle)?;
    let (volume, id) = identity(&dir.handle)?;
    Ok(meta_of(Raw {
        attributes: basic.FileAttributes,
        tag: 0,
        size: 0,
        allocated: 0,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
        volume,
        id,
    }))
}

/// Visit every child of `dir` (no `.` or `..`). Returns `Ok(false)` if `visit` stopped early.
pub fn for_each_child(
    dir: &Dir,
    mut visit: impl FnMut(OsString, Result<Meta>) -> Result<bool>,
) -> Result<bool> {
    read_listing(&dir.handle, dir.volume, |child| {
        visit(OsString::from_wide(&child.name), Ok(child.meta))
    })
}

/// Windows does not expose used inodes, so progress is estimated from the walk alone.
pub fn inode_usage(_dir: &Dir) -> Result<Option<u64>> {
    Ok(None)
}

fn space(path: &[u16]) -> io::Result<(u64, u64, u64)> {
    let mut wide = path.to_vec();
    wide.push(0);
    let (mut caller, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and the output pointers are valid for writes.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut caller, &mut total, &mut free) };
    if ok == 0 {
        Err(last_error())
    } else {
        Ok((total, caller, free))
    }
}

/// (capacity, available to the caller) of the volume holding `dir`.
pub fn volume(dir: &Dir) -> Result<(u64, u64)> {
    let (total, caller, _) = space(&dir.path)?;
    Ok((total, caller))
}

/// Live (capacity, used) bytes of the volume holding `root`. "Used" excludes space reserved for the
/// system, matching `df`.
pub fn volume_sample(root: &Path) -> Option<(u64, u64)> {
    let dir = open_root(root).ok()?;
    let (total, _, free) = space(&dir.path).ok()?;
    Some((total, total.saturating_sub(free)))
}

/// Live (identity, creation time) of a root-relative stored path, read without following links.
pub fn live_identity(root: &Path, relative: &[u8]) -> Result<(String, Option<String>)> {
    let root = open_root(root)?;
    let relative = decode_path(relative)?;
    let handle = open(&join(&root.path, &relative)?, FILE_READ_ATTRIBUTES)?;
    let (volume, id) = identity(&handle)?;
    let created = basic_info(&handle)?.CreationTime;
    Ok((identity_of(volume, id), Some(created.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_tags_are_recognised_across_provider_slots() {
        assert!(is_cloud_tag(0x9000_001A));
        assert!(is_cloud_tag(0x9000_101A));
        assert!(
            !is_cloud_tag(0xA000_000C),
            "symlinks are not cloud placeholders"
        );
    }

    #[test]
    fn identity_and_time_conversions_are_stable() {
        assert_eq!(
            identity_of(0xABCD, [1; 16]),
            "abcd:01010101010101010101010101010101"
        );
        assert_eq!(filetime_ns(FILETIME_UNIX_EPOCH), 0);
        assert_eq!(filetime_ns(FILETIME_UNIX_EPOCH + 10), 1_000);
    }

    #[test]
    fn paths_compare_case_insensitively_and_join_with_backslashes() {
        let a: Vec<u16> = "C:\\Users\\X".encode_utf16().collect();
        let b: Vec<u16> = "c:\\users\\x".encode_utf16().collect();
        assert!(same_path(&a, &b));
        let joined = join(&a, Path::new("Docs/report.txt")).unwrap();
        assert_eq!(
            String::from_utf16_lossy(&joined),
            "C:\\Users\\X\\Docs\\report.txt"
        );
    }
}
