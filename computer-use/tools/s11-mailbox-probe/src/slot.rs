//! Result-slot read: O_NOFOLLOW, regular file, 0 < size <= 4096, nlink == 1.
//! The slot name is a fixed test whitelist entry, not a production frame.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::budget::{SLOT_CAP, SLOT_NAME};
use crate::osutil::{self, flag_creat, flag_excl, flag_nofollow, flag_nonblock, flag_wronly};

#[derive(Debug, PartialEq, Eq)]
pub enum SlotError {
    Name,
    Symlink,
    NotFile,
    HardLink,
    Size,
    Replaced,
    /// `dir_dev` / `file_dev` are the two `st_dev` values that failed to bind.
    Fsid {
        dir_dev: u64,
        file_dev: u64,
    },
    /// `op` is the step (`create`, `sync`, `open`, …). `errno` is `raw_os_error`, or 0.
    Io {
        op: &'static str,
        errno: i32,
    },
}

fn io(op: &'static str, err: &std::io::Error) -> SlotError {
    SlotError::Io {
        op,
        errno: err.raw_os_error().unwrap_or(0),
    }
}

pub fn read_named(dir: &Path, name: &str, min: u64, max: u64) -> Result<Vec<u8>, SlotError> {
    if min == 0 || min > max || max > SLOT_CAP as u64 {
        return Err(SlotError::Size);
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|err| io("dir-stat", &err))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(SlotError::Symlink);
    }
    let parent = osutil::open_dir_nofollow(dir).map_err(|err| io("dir-open", &err))?;
    let bytes = read_slot_at(&parent, dir, name, max)?;
    if (bytes.len() as u64) < min {
        return Err(SlotError::Size);
    }
    Ok(bytes)
}

pub fn read_result_slot(dir: &Path) -> Result<Vec<u8>, SlotError> {
    let meta = std::fs::symlink_metadata(dir).map_err(|err| io("dir-stat", &err))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(SlotError::Symlink);
    }
    let parent = osutil::open_dir_nofollow(dir).map_err(|err| io("dir-open", &err))?;
    read_slot_at(&parent, dir, SLOT_NAME, SLOT_CAP as u64)
}

pub fn read_slot_at(
    dir: &File,
    dir_path: &Path,
    name: &str,
    max: u64,
) -> Result<Vec<u8>, SlotError> {
    if !name_ok(name) {
        return Err(SlotError::Name);
    }
    let dir_meta = dir.metadata().map_err(|err| io("dir-fstat", &err))?;
    let flags = flag_nofollow() | flag_nonblock();
    let mut file = match osutil::open_at(dir, name, flags, 0) {
        Ok(file) => file,
        Err(err) => {
            if err.raw_os_error() == Some(eloop()) {
                return Err(SlotError::Symlink);
            }
            // A FIFO opened O_NONBLOCK with no writer returns ENXIO before fstat.
            let meta =
                std::fs::symlink_metadata(dir_path.join(name)).map_err(|e| io("lstat", &e))?;
            if meta.file_type().is_symlink() {
                return Err(SlotError::Symlink);
            }
            if !meta.is_file() {
                return Err(SlotError::NotFile);
            }
            return Err(io("open", &err));
        }
    };
    let meta = file.metadata().map_err(|err| io("fstat", &err))?;
    if !meta.file_type().is_file() {
        return Err(SlotError::NotFile);
    }
    // nlink 0 means the name was replaced after this fd was opened. That is a retry,
    // not an extra hard link (nlink > 1).
    if meta.nlink() == 0 {
        return Err(SlotError::Replaced);
    }
    if meta.nlink() != 1 {
        return Err(SlotError::HardLink);
    }
    if meta.len() == 0 || meta.len() > max {
        return Err(SlotError::Size);
    }
    // st_dev binds the file to the directory's device. macOS firmlinks can make
    // f_fsid bytes differ for a directory fd and a file fd even when st_dev matches.
    if meta.dev() != dir_meta.dev() {
        return Err(SlotError::Fsid {
            dir_dev: dir_meta.dev(),
            file_dev: meta.dev(),
        });
    }
    let need = meta.len() as usize;
    let mut buf = vec![0u8; need + 1];
    let mut got = 0;
    while got < buf.len() {
        match file.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(err) => return Err(io("read", &err)),
        }
    }
    if got != need {
        return Err(SlotError::Size);
    }
    buf.truncate(need);
    let again = file.metadata().map_err(|err| io("restat", &err))?;
    if again.dev() != meta.dev()
        || again.ino() != meta.ino()
        || again.nlink() != 1
        || again.len() != meta.len()
    {
        return Err(SlotError::Replaced);
    }
    let again_name = osutil::open_at(dir, name, flags, 0).map_err(|_| SlotError::Replaced)?;
    let named = again_name
        .metadata()
        .map_err(|err| io("reopen-stat", &err))?;
    if named.ino() != meta.ino() || named.dev() != meta.dev() {
        return Err(SlotError::Replaced);
    }
    Ok(buf)
}

pub fn write_result_slot(dir: &Path, bytes: &[u8]) -> Result<(), SlotError> {
    if bytes.is_empty() || bytes.len() > SLOT_CAP {
        return Err(SlotError::Size);
    }
    let parent = osutil::open_dir_nofollow(dir).map_err(|err| io("dir-open", &err))?;
    // O_EXCL refuses an existing symlink, so O_NOFOLLOW is not combined with O_CREAT
    // (Darwin returns ELOOP for that pair when the name is absent).
    let flags = flag_creat() | flag_excl() | flag_wronly();
    let mut file =
        osutil::open_at(&parent, SLOT_NAME, flags, 0o600).map_err(|err| io("create", &err))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|err| io("write", &err))?;
    file.sync_all().map_err(|err| io("sync", &err))?;
    Ok(())
}

pub fn name_ok(name: &str) -> bool {
    matches!(name, "probe-result" | "c1-frame" | "c2-frame")
}

fn eloop() -> i32 {
    #[cfg(target_os = "linux")]
    {
        40
    }
    #[cfg(target_os = "macos")]
    {
        62
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("s11-slot-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        osutil::mkdir_0700(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_symlink_fifo_oversize_and_hardlink() {
        let dir = scratch();
        let link = dir.join(SLOT_NAME);
        symlink("/etc/hosts", &link).unwrap();
        assert_eq!(read_result_slot(&dir).unwrap_err(), SlotError::Symlink);
        std::fs::remove_file(&link).unwrap();

        osutil::make_fifo(&link).unwrap();
        assert_eq!(read_result_slot(&dir).unwrap_err(), SlotError::NotFile);
        std::fs::remove_file(&link).unwrap();

        {
            let mut f = std::fs::File::create(&link).unwrap();
            f.write_all(&vec![b'x'; 4097]).unwrap();
        }
        assert_eq!(read_result_slot(&dir).unwrap_err(), SlotError::Size);
        std::fs::remove_file(&link).unwrap();

        std::fs::write(&link, b"ok").unwrap();
        let extra = dir.join("extra");
        std::fs::hard_link(&link, &extra).unwrap();
        assert_eq!(read_result_slot(&dir).unwrap_err(), SlotError::HardLink);
        std::fs::remove_file(&extra).unwrap();
        std::fs::remove_file(&link).unwrap();

        write_result_slot(&dir, b"{\"v\":1}").unwrap();
        let mode = std::fs::symlink_metadata(&link)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read_result_slot(&dir).unwrap(), b"{\"v\":1}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
