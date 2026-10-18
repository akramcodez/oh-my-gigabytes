//! Owner-only state directory on Windows: the counterpart of the Unix "owned by you, mode 0700"
//! check. A new directory gets a protected access list that grants full control to you and SYSTEM
//! only. An existing directory is accepted only if you own it and its access list grants nobody
//! else anything (Administrators, SYSTEM, CREATOR OWNER and OWNER RIGHTS are accepted: an
//! administrator can take ownership anyway). Only OMG's own state directory is changed here.
use anyhow::{Result, bail};
use std::{ffi::c_void, io, mem, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorDacl,
        GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
        TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
        WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid, WinCreatorOwnerSid,
        WinLocalSystemSid,
    },
    Storage::FileSystem::CreateDirectoryW,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;
const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: u8 = 9;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Who "you" are: the token's user and its default owner, each in an 8-byte-aligned buffer. An
/// elevated administrator's new objects are owned by the Administrators group (the token's default
/// owner), so both count. This is the Windows form of Linux's `st_uid == euid`.
struct CurrentUser {
    user: Vec<u64>,
    owner: Vec<u64>,
}

impl CurrentUser {
    fn load() -> io::Result<Self> {
        struct Token(HANDLE);
        impl Drop for Token {
            fn drop(&mut self) {
                // SAFETY: the token handle came from `OpenProcessToken` and is closed only here.
                unsafe { CloseHandle(self.0) };
            }
        }
        let mut raw: HANDLE = ptr::null_mut();
        // SAFETY: the process pseudo-handle needs no closing; `raw` receives an owned token.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Token(raw);
        Ok(Self {
            user: token_info(token.0, TokenUser)?,
            owner: token_info(token.0, TokenOwner)?,
        })
    }
    fn sids(&self) -> [PSID; 2] {
        // SAFETY: the buffers hold a `TOKEN_USER` and a `TOKEN_OWNER` written by
        // `GetTokenInformation`, and stay alive as long as `self`.
        unsafe {
            [
                (*self.user.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                (*self.owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
            ]
        }
    }
    /// Your account SID, which the new access list names.
    fn user_sid(&self) -> PSID {
        self.sids()[0]
    }
}

fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer; it only writes `needed`.
    unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut needed) };
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer is writable for its full length in bytes.
    let ok = unsafe {
        GetTokenInformation(
            token,
            class,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

/// A security descriptor allocated by the system, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer was allocated by the system for this descriptor and is freed once.
        unsafe { LocalFree(self.0) };
    }
}

impl Descriptor {
    /// `D:PAI(A;OICI;FA;;;<you>)(A;OICI;FA;;;SY)`: protected (no inherited entries), full control
    /// for you and SYSTEM, inherited by files and folders created inside.
    fn private() -> io::Result<Self> {
        let user = CurrentUser::load()?;
        let mut text: windows_sys::core::PWSTR = ptr::null_mut();
        // SAFETY: `user_sid` points into a live token buffer; `text` receives a system allocation.
        if unsafe { ConvertSidToStringSidW(user.user_sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `text` is a NUL-terminated string from the call above.
        let sid = unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
        };
        // SAFETY: `text` was allocated by `ConvertSidToStringSidW` and is freed once.
        unsafe { LocalFree(text.cast()) };
        let sddl: Vec<u16> = format!("D:PAI(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `descriptor` receives a system allocation.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

/// Create one directory that only you and SYSTEM can use.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::private()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and `attributes` points to a live descriptor.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Replace the access list of an existing directory with the private one. Used to prepare test
/// directories; `Index::open` never changes the access list of a directory it did not create.
pub fn protect(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::private()?;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: the out-pointers are valid and the descriptor is live.
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
    {
        return Err(io::Error::last_os_error());
    }
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and `dacl` points into the live descriptor.
    let status = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

fn trusted(sid: PSID, user: &CurrentUser) -> bool {
    // SAFETY: `sid` points into a live access list and the token buffers are live.
    unsafe {
        user.sids().into_iter().any(|own| EqualSid(sid, own) != 0)
            || [
                WinLocalSystemSid,
                WinBuiltinAdministratorsSid,
                WinCreatorOwnerSid,
                WinCreatorOwnerRightsSid,
            ]
            .into_iter()
            .any(|kind| IsWellKnownSid(sid, kind) != 0)
    }
}

/// Accept `path` only if you own it and its access list grants nobody else anything.
pub fn verify_private(path: &Path) -> Result<()> {
    let user = CurrentUser::load()?;
    let name = wide(path);
    let mut owner: PSID = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the out-pointers are valid; owner and access list are requested.
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        bail!(
            "cannot read the state directory's permissions: {}",
            io::Error::from_raw_os_error(status as i32)
        );
    }
    // `owner` and `dacl` point into `descriptor`; it is freed when this guard drops.
    let _free = Descriptor(descriptor);
    let fix = format!(
        "fix it with: icacls \"{}\" /inheritance:r /grant:r \"%USERNAME%:(OI)(CI)F\" /grant:r \"SYSTEM:(OI)(CI)F\"",
        path.display()
    );
    // SAFETY: `owner` is a valid SID inside the live descriptor.
    if !user
        .sids()
        .into_iter()
        .any(|own| unsafe { EqualSid(owner, own) } != 0)
    {
        bail!("state directory is not owned by the current user; {fix}");
    }
    if dacl.is_null() {
        bail!("state directory has no access list, so every user can use it; {fix}");
    }
    let mut info = ACL_SIZE_INFORMATION {
        AceCount: 0,
        AclBytesInUse: 0,
        AclBytesFree: 0,
    };
    // SAFETY: `info` is the structure the information class asks for, with its exact size.
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
            mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        bail!(
            "cannot read the state directory's access list: {}",
            io::Error::last_os_error()
        );
    }
    for index in 0..info.AceCount {
        let mut entry: *mut c_void = ptr::null_mut();
        // SAFETY: `index` is below the entry count of the live access list.
        if unsafe { GetAce(dacl, index, &mut entry) } == 0 {
            bail!(
                "cannot read the state directory's access list: {}",
                io::Error::last_os_error()
            );
        }
        // SAFETY: every entry starts with an `ACE_HEADER`.
        match unsafe { (*entry.cast::<ACE_HEADER>()).AceType } {
            ACCESS_ALLOWED_ACE_TYPE | ACCESS_ALLOWED_CALLBACK_ACE_TYPE => {
                // SAFETY: these entry types share the `ACCESS_ALLOWED_ACE` layout; the SID begins
                // at `SidStart` and runs inside the entry.
                let sid = unsafe {
                    (&raw const (*entry.cast::<ACCESS_ALLOWED_ACE>()).SidStart).cast_mut()
                };
                if !trusted(sid.cast(), &user) {
                    bail!("state directory is accessible to other users; {fix}");
                }
            }
            // A denial can only reduce access.
            ACCESS_DENIED_ACE_TYPE => {}
            other => {
                bail!("state directory has an access entry of unsupported type {other}; {fix}")
            }
        }
    }
    Ok(())
}
