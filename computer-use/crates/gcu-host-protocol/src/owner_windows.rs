//! Windows owner-only DACL: current user and SYSTEM, inheritance disabled.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, ERROR_LOCK_VIOLATION, HANDLE, HLOCAL},
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                SetNamedSecurityInfoW, SE_FILE_OBJECT,
            },
            GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
        },
        Storage::FileSystem::{
            LockFileEx, MoveFileExW, UnlockFileEx, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, LOCKFILE_EXCLUSIVE_LOCK,
            LOCKFILE_FAIL_IMMEDIATELY, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub(crate) fn ensure_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "owner-only directory is a symlink",
            ));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "owner-only path is not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::create_dir(path)?;
        }
        Err(error) => return Err(error),
    }
    restrict(path)?;
    verify_dir(path)
}

pub(crate) fn restrict(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a symlink",
        ));
    }
    let file = open_nofollow(path, meta.is_dir())?;
    if reparse_or_hardlink(&file, meta.is_dir())? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a reparse point or extra link",
        ));
    }
    set_dacl(path)?;
    verify_dacl(path)
}

pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let file = options.open(path)?;
    if let Err(error) = restrict(path) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8], require_sync: bool) -> io::Result<()> {
    reject_reparse(path)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no parent")
    })?;
    let parent_meta = fs::symlink_metadata(parent)?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only parent is not a real directory",
        ));
    }
    let temp = unique_temp(path)?;
    let mut file = create_new(&temp)?;
    if let Err(error) = write_synced(&mut file, bytes, require_sync) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);
    if let Err(error) = replace(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    verify_dir_or_file(path, false)
}

pub(crate) fn read_no_follow(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let file = open_nofollow(path, false)?;
    if reparse_or_hardlink(&file, false)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a reparse point or extra link",
        ));
    }
    verify_dacl(path)?;
    let len = file.metadata()?.len();
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "owner-only file exceeds the read limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(len).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(io::Error::other("owner-only file changed while reading"));
    }
    Ok(bytes)
}

pub(crate) fn verify_dir(path: &Path) -> io::Result<()> {
    verify_dir_or_file(path, true)
}

pub(crate) fn security_sddl(path: &Path) -> io::Result<String> {
    read_dacl_sddl(path)
}

pub(crate) fn current_user_sid() -> io::Result<String> {
    user_sid()
}

pub(crate) struct DirLock {
    file: File,
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let mut overlapped = windows::Win32::System::IO::OVERLAPPED::default();
        // SAFETY: the handle is still owned. Unlock releases only this lock.
        unsafe {
            let _ = UnlockFileEx(
                HANDLE(self.file.as_raw_handle()),
                Some(0),
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            );
        }
    }
}

pub(crate) fn hold_dir_lock(dir: &Path) -> io::Result<DirLock> {
    let file = lock_file(dir)?;
    lock_exclusive(&file, true)?;
    Ok(DirLock { file })
}

pub(crate) fn reap_unlocked_children(
    parent: &Path,
    prefix: &str,
    keep: &Path,
    grace: std::time::Duration,
) -> io::Result<()> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                crate::owner_only::warn_meta("orphan_scan", parent, &error);
                continue;
            }
        };
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if !text.starts_with(prefix) && !text.starts_with(".gcu-tmp-") {
            continue;
        }
        let path = entry.path();
        if path == keep {
            continue;
        }
        if text.starts_with(".gcu-tmp-") {
            remove_any(&path);
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                crate::owner_only::warn_meta("orphan_scan", &path, &error);
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            if meta.file_type().is_symlink() {
                crate::owner_only::warn_meta(
                    "orphan_symlink",
                    &path,
                    &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
                );
            }
            continue;
        }
        if recently_touched(&meta, grace) {
            continue;
        }
        match lock_file(&path).and_then(|file| lock_exclusive(&file, false).map(|_| file)) {
            Ok(file) => {
                let trash = unique_temp(&path)?;
                if let Err(error) = fs::rename(&path, &trash) {
                    drop(file);
                    if error.kind() != io::ErrorKind::NotFound {
                        crate::owner_only::warn_meta("orphan_rename", &path, &error);
                    }
                    continue;
                }
                drop(file);
                remove_any(&trash);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => crate::owner_only::warn_meta("orphan_lock", &path, &error),
        }
    }
    Ok(())
}

fn remove_any(path: &Path) {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            crate::owner_only::warn_meta("orphan_delete", path, &error);
            return;
        }
    };
    if meta.file_type().is_symlink() {
        crate::owner_only::warn_meta(
            "orphan_symlink",
            path,
            &io::Error::new(io::ErrorKind::PermissionDenied, "symlink"),
        );
        return;
    }
    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(error) = result {
        if error.kind() != io::ErrorKind::NotFound {
            crate::owner_only::warn_meta("orphan_delete", path, &error);
        }
    }
}

fn lock_file(dir: &Path) -> io::Result<File> {
    let path = dir.join(".owner-lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(&path)?;
    set_dacl(&path)?;
    Ok(file)
}

fn lock_exclusive(file: &File, block: bool) -> io::Result<()> {
    let mut flags = LOCKFILE_EXCLUSIVE_LOCK;
    if !block {
        flags |= LOCKFILE_FAIL_IMMEDIATELY;
    }
    let mut overlapped = windows::Win32::System::IO::OVERLAPPED::default();
    // SAFETY: `file` owns the handle. `overlapped` is writable and outlives the call.
    let result = unsafe {
        LockFileEx(
            HANDLE(file.as_raw_handle()),
            flags,
            Some(0),
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if lock_held(&error) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "directory lock is held",
        )),
        Err(error) => Err(io::Error::other(error)),
    }
}

fn verify_dir_or_file(path: &Path, directory: bool) -> io::Result<()> {
    let file = open_nofollow(path, directory)?;
    if reparse_or_hardlink(&file, directory)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a reparse point or extra link",
        ));
    }
    let meta = file.metadata()?;
    if directory != meta.is_dir() || (!directory && !meta.is_file()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path has an unexpected type",
        ));
    }
    verify_dacl(path)
}

fn verify_dacl(path: &Path) -> io::Result<()> {
    let sddl = read_dacl_sddl(path)?;
    let sid = user_sid()?;
    if crate::owner_only::dacl_allows_only_user_and_system(&sddl, &sid) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("owner-only DACL rejected: {sddl}"),
        ))
    }
}

fn open_nofollow(path: &Path, directory: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT.0;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS.0;
    }
    options.custom_flags(flags);
    options.open(path)
}

fn reparse_or_hardlink(file: &File, directory: bool) -> io::Result<bool> {
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is owned and `info` is a writable output of the right size.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(io::Error::other)?;
    let reparse = info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    let extra = !directory && info.nNumberOfLinks != 1;
    Ok(reparse || extra)
}

fn reject_reparse(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only path is a symlink",
        )),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn set_dacl(path: &Path) -> io::Result<()> {
    let sid = user_sid()?;
    let sddl = format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)");
    let descriptor = descriptor_from(&sddl)?;
    let mut acl: *mut ACL = std::ptr::null_mut();
    let mut present = windows::core::BOOL::default();
    let mut defaulted = windows::core::BOOL::default();
    // SAFETY: the descriptor is live and every output pointer is writable.
    unsafe {
        windows::Win32::Security::GetSecurityDescriptorDacl(
            descriptor.0,
            &mut present,
            &mut acl,
            &mut defaulted,
        )
    }
    .map_err(io::Error::other)?;
    let wide = wide(path);
    // SAFETY: `wide` is NUL-terminated and the ACL lives until this call returns.
    unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        )
    }
    .ok()
    .map_err(io::Error::other)
}

fn read_dacl_sddl(path: &Path) -> io::Result<String> {
    let wide = wide(path);
    let mut raw = PSECURITY_DESCRIPTOR::default();
    // SAFETY: path is terminated and `raw` is a writable security-descriptor output.
    unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &mut raw,
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    let descriptor = Descriptor(raw);
    let mut text = PWSTR::null();
    // SAFETY: the descriptor is valid; the API allocates the output string.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: a successful conversion returns a terminated UTF-16 allocation.
    let string = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: `text` was allocated by the LocalAlloc-backed conversion API.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
    }
    string
}

fn user_sid() -> io::Result<String> {
    static SID: OnceLock<String> = OnceLock::new();
    if let Some(sid) = SID.get() {
        return Ok(sid.clone());
    }
    let sid = query_user_sid()?;
    Ok(SID.get_or_init(|| sid.clone()).clone())
}

fn lock_held(error: &windows::core::Error) -> bool {
    let code = error.code().0;
    let raw = ERROR_LOCK_VIOLATION.0;
    code == raw as i32 || code == (0x8007_0000 | raw) as i32
}

fn query_user_sid() -> io::Result<String> {
    // SAFETY: the process pseudo-handle does not need to be closed.
    let process = unsafe { GetCurrentProcess() };
    let mut token = HANDLE::default();
    // SAFETY: `token` is a writable output. The real handle is closed below.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(io::Error::other)?;
    let token = Token(token);
    let mut size = 0u32;
    // SAFETY: a null buffer asks for the required size. Failure here is expected.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut size) };
    if size == 0 {
        return Err(io::Error::other("token user size is empty"));
    }
    // u64 elements keep TOKEN_USER (it contains a pointer) aligned.
    let mut storage = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `storage` is aligned and writable for at least `size` bytes.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: a successful query wrote a TOKEN_USER at the aligned start.
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID remains inside `storage`. `text` receives a fresh allocation.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io::Error::other)?;
    // SAFETY: the conversion returned a terminated UTF-16 string.
    let string = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: `text` was allocated by ConvertSidToStringSidW.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
    }
    string
}

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is not used again.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by a LocalAlloc-backed security API.
        unsafe {
            LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

fn descriptor_from(sddl: &str) -> io::Result<Descriptor> {
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut raw = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `wide` is NUL-terminated and `raw` is writable output storage.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide.as_ptr()),
            1,
            &mut raw,
            None,
        )
    }
    .map_err(io::Error::other)?;
    Ok(Descriptor(raw))
}

fn replace(from: &Path, to: &Path) -> io::Result<()> {
    let source = wide(from);
    let dest = wide(to);
    // SAFETY: both paths are NUL-terminated and live for the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(dest.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::other)
}

fn write_synced(file: &mut File, bytes: &[u8], require_sync: bool) -> io::Result<()> {
    file.write_all(bytes)?;
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(error) if !require_sync => {
            crate::owner_only::warn_meta("owner_sync", Path::new("fd"), &error);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn unique_temp(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no parent")
    })?;
    let base = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "owner-only file has no name")
    })?;
    for _ in 0..8 {
        let mut name = std::ffi::OsString::from(".gcu-tmp-");
        name.push(base);
        name.push(format!(
            "-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let candidate = parent.join(&name);
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(io::Error::other("owner-only temporary name collided"))
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Whether `meta` was modified within `grace`. An unreadable time counts as recent.
fn recently_touched(meta: &fs::Metadata, grace: std::time::Duration) -> bool {
    meta.modified()
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_none_or(|age| age < grace)
}
