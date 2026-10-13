//! Refuse links/reparse points and never truncate or replace existing files.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path};

pub(super) fn error(e: impl std::fmt::Display) -> String {
    format!("computer_use_extension_files: {e}")
}
pub(super) fn plain(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !metadata.file_type().is_symlink()
}

// Windows parent handles deny rename/deletion for the whole operation. On Unix
// files use O_NOFOLLOW; callers revalidate the fixed, Host-owned tree each time.
pub(super) fn directories(path: &Path) -> Result<Vec<File>, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(error("absolute non-traversing App path required"));
    }
    let mut pins = Vec::new();
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(error(e)),
        };
        if !metadata.is_dir() || !plain(&metadata) {
            return Err(error("unsafe directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let uid = unsafe { libc::geteuid() };
            if (metadata.uid() != uid && metadata.uid() != 0)
                || (metadata.mode() & 0o022 != 0 && metadata.mode() & libc::S_ISVTX == 0)
            {
                return Err(error("untrusted writable ancestor"));
            }
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3).custom_flags(0x02000000 | 0x00200000); // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(ancestor).map_err(error)?;
        if !plain(&file.metadata().map_err(error)?) {
            return Err(error("directory changed"));
        }
        pins.push(file);
    }
    Ok(pins)
}
pub(super) fn make_directory(path: &Path) -> Result<(), String> {
    let _parents = directories(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && plain(&metadata) => return Ok(()),
        Ok(_) => return Err(error("unsafe directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    let parent = path.parent().ok_or_else(|| error("missing parent"))?;
    make_directory(parent)?;
    let _pins = directories(parent)?;
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(error)?;
    directories(path)?;
    Ok(())
}
pub(super) fn open(path: &Path, create: bool) -> Result<File, String> {
    let _parents = directories(path.parent().ok_or_else(|| error("missing parent"))?)?;
    if !create {
        let meta = fs::symlink_metadata(path).map_err(error)?;
        if !meta.is_file() || !plain(&meta) || meta.len() > 1024 * 1024 {
            return Err(error("unsafe extension file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() != 1 {
                return Err(error("linked file"));
            }
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(create).create_new(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(error)?;
    let meta = file.metadata().map_err(error)?;
    if !meta.is_file() || !plain(&meta) {
        return Err(error("file changed"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut identity = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut identity) }
            .map_err(error)?;
        if identity.nNumberOfLinks != 1 {
            return Err(error("linked file"));
        }
    }
    Ok(file)
}
pub(super) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    open(path, false)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > 1024 * 1024 {
        return Err(error("extension file too large"));
    }
    Ok(bytes)
}
pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open(path, true)?;
    file.write_all(bytes).map_err(error)?;
    file.sync_all().map_err(error)
}
pub(super) fn sync(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(error)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub(super) fn publish(staging: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
        };
        let source: Vec<u16> = staging.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // std::fs::rename uses REPLACE_EXISTING, including an empty directory.
        // Publication must use the real OS no-replace flag, not a pre-check.
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        }
        .map_err(error)?;
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let source = CString::new(staging.as_os_str().as_bytes()).map_err(error)?;
        let target = CString::new(destination.as_os_str().as_bytes()).map_err(error)?;
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result =
            unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
        if result != 0 {
            return Err(error(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}
