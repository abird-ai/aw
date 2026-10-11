use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    process::{Child, Command},
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    },
    Storage::FileSystem::{
        GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING,
        MOVEFILE_WRITE_THROUGH,
    },
    System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, TerminateProcess, PROCESS_TERMINATE,
    },
};

#[path = "scoped_fs.rs"]
pub mod scoped_fs;

pub const FAMILY: &str = "windows";

fn win_error() -> io::Error {
    io::Error::last_os_error()
}

pub fn local_principal() -> io::Result<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(win_error());
        }
        struct Token(HANDLE);
        impl Drop for Token {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let token = Token(token);
        let mut needed = 0u32;
        let _ = GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 || needed > 64 * 1024 {
            return Err(win_error());
        }
        let mut buffer = vec![0u8; needed as usize];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(win_error());
        }
        let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
        let mut wide = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut wide) == 0 || wide.is_null() {
            return Err(win_error());
        }
        struct Local(*mut u16);
        impl Drop for Local {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0.cast());
                }
            }
        }
        let wide = Local(wide);
        let mut len = 0usize;
        while *wide.0.add(len) != 0 {
            len += 1;
            if len > 256 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "SID too long"));
            }
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(wide.0, len))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SID"))?;
        Ok(format!("windows:{sid}"))
    }
}

pub fn effective_uid() -> Option<u32> {
    None
}

pub fn null_device() -> &'static str {
    "NUL"
}

pub fn os_string_from_bytes(bytes: Vec<u8>) -> io::Result<OsString> {
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Git path is not UTF-8"))?;
    Ok(OsString::from(text))
}

pub fn os_str_bytes(value: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    let text = value
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not Unicode"))?;
    Ok(Cow::Owned(text.as_bytes().to_vec()))
}

pub fn configure_child_group(_command: &mut Command) {
    // Higher-level process wrappers own timeout/kill semantics. Windows job
    // object ownership can be added here without changing callers.
}

/// Windows job-object graceful signaling is not currently supported; fail
/// explicitly so a hosted supervisor can attempt a forced termination.
pub fn terminate_process_group(_pid: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows process-group grace unsupported",
    ))
}

pub fn kill_process_group(pid: u32) {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            let _ = TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

pub fn terminate_child_tree(child: &mut Child) -> io::Result<()> {
    unsafe {
        if TerminateProcess(child.as_raw_handle() as HANDLE, 1) == 0 {
            return child.kill();
        }
    }
    Ok(())
}

/// Owner-only ACL validation is intentionally not approximated with readonly or
/// hidden attributes. Authority-bearing durable stores remain disabled until the
/// Windows backend installs and validates a current-user-only DACL.
pub fn validate_private_dir(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private directory ACL validation is not implemented on Windows",
    ))
}

pub fn ensure_private_dir(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private directory ACL validation is not implemented on Windows",
    ))
}
pub fn open_private_new(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private file ACL validation is not implemented on Windows",
    ))
}
pub fn open_private_existing(_path: &Path, _write: bool) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private file ACL validation is not implemented on Windows",
    ))
}
pub fn open_private_rw_create(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private file ACL validation is not implemented on Windows",
    ))
}
pub fn validate_private_file(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure private file ACL validation is not implemented on Windows",
    ))
}
pub fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Open/create a stable advisory-lock file for non-secret user metadata.
pub fn open_user_metadata_lock(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "metadata lock may not be a reparse point",
        ));
    }
    Ok(file)
}

/// Atomically publish non-secret user metadata next to its destination.
/// This intentionally does not claim SecretStore-grade owner-only ACL custody;
/// provider config contains references/metadata, never credential values.
pub fn atomic_write_user_metadata(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    fs::create_dir_all(parent)?;

    let mut opened = None;
    for _ in 0..16 {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => {
                opened = Some((tmp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (tmp, mut file) = opened.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate temporary metadata file",
        )
    })?;

    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let source: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for
        // the duration of MoveFileExW.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(win_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn path_is_link_like(path: &Path) -> io::Result<bool> {
    Ok(std::fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

pub fn open_nofollow_read(path: &Path) -> io::Result<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reparse-point file refused",
        ));
    }
    Ok(file)
}

pub fn file_link_count(file: &File) -> io::Result<u64> {
    unsafe {
        let mut info = std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>();
        if GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) == 0 {
            return Err(win_error());
        }
        Ok(u64::from(info.nNumberOfLinks))
    }
}

pub fn trusted_direct_filesystem_supported(_path: &Path) -> io::Result<bool> {
    Ok(false)
}

pub fn secure_private_files_supported() -> bool {
    false
}
