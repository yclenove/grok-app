//! Same-handle file IO for the in-process fake.
//!
//! Every open is nonblocking and does not follow the final path component.
//! The same handle must be a regular file before any byte is read or written.
//! A FIFO, symlink, or other special file fails immediately, so the caller
//! cannot block on a reader that never arrives. Missing files stay missing.

use std::io::{Read, Write};
use std::path::Path;

/// Fixed refusal for a FIFO, symlink, device, or other non-regular file.
const NON_REGULAR: &str = "refusing a non-regular file";

const CAP: usize = 1024 * 1024;

/// Replace `path` with `bytes` when it is a regular file, or create one.
pub(crate) fn write_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_write(path, false)?;
    file.write_all(bytes).map_err(|err| err.to_string())
}

/// Append `bytes` to a regular file, creating it when it is absent.
pub(crate) fn append(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = open_write(path, true)?;
    file.write_all(bytes).map_err(|err| err.to_string())
}

/// Read a regular file, capped at 1 MiB. Missing is an error.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    read_capped(open_read(path)?)
}

/// Read a regular file. `Ok(None)` when the path is absent.
///
/// A non-regular file is an error, not an empty read.
pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match open_read(path) {
        Ok(file) => read_capped(file).map(Some),
        Err(err) if err == missing_label() => Ok(None),
        Err(err) => Err(err),
    }
}

fn missing_label() -> String {
    std::io::Error::from(std::io::ErrorKind::NotFound).to_string()
}

fn open_read(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    apply_safe_flags(&mut options);
    let file = options.open(path).map_err(map_open)?;
    ensure_regular(&file)?;
    Ok(file)
}

fn open_write(path: &Path, append: bool) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    apply_safe_flags(&mut options);
    let file = options.open(path).map_err(map_open)?;
    ensure_regular(&file)?;
    Ok(file)
}

fn apply_safe_flags(options: &mut std::fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Do not follow a reparse point. The handle is then type-checked.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
}

fn map_open(err: std::io::Error) -> String {
    if is_refused_open(&err) {
        return NON_REGULAR.into();
    }
    if err.kind() == std::io::ErrorKind::NotFound {
        return missing_label();
    }
    err.to_string()
}

fn is_refused_open(err: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(err.raw_os_error(), Some(code) if code == libc::ENXIO || code == libc::ELOOP)
    }
    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

fn ensure_regular(file: &std::fs::File) -> Result<(), String> {
    let meta = file.metadata().map_err(|err| err.to_string())?;
    let kind = meta.file_type();
    if kind.is_symlink() || !kind.is_file() {
        return Err(NON_REGULAR.into());
    }
    Ok(())
}

fn read_capped(file: std::fs::File) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    file.take(CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() > CAP {
        return Err("file exceeds the size cap".into());
    }
    Ok(bytes)
}
