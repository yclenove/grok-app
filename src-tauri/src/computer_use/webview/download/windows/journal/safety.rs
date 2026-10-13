//! Pinned, plain Windows objects. Deletion addresses held handles, not names.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf, Prefix};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, GetFileInformationByHandle, SetFileInformationByHandle,
    BY_HANDLE_FILE_INFORMATION, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
};

const DELETE_ACCESS: u32 = 0x00010000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    volume: u32,
    index: u64,
    created: u64,
}

pub(super) fn error(_: impl std::fmt::Display) -> String {
    // Do not leak App home, Temp paths or a process command line into diagnostics.
    "native download journal filesystem unavailable or changed".into()
}

pub(super) fn path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(error("relative path"));
    }
    // Components normalizes interior dot segments; reject their original
    // spelling before allowing a path into the durable ownership contract.
    if path
        .as_os_str()
        .to_string_lossy()
        .split(['\\', '/'])
        .any(|part| part == "." || part == "..")
    {
        return Err(error("dot segment"));
    }
    for part in path.components() {
        match part {
            Component::Prefix(prefix)
                if matches!(
                    prefix.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(_, _)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(_, _)
                ) => {}
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if name.ends_with(['.', ' ']) || name.contains([':', '\0']) {
                    return Err(error("unsafe component"));
                }
            }
            _ => return Err(error("unsafe component")),
        }
    }
    Ok(())
}

pub(super) fn identity(file: &File, directory: bool) -> Result<Identity, String> {
    let metadata = file.metadata().map_err(error)?;
    if metadata.file_attributes() & 0x400 != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(error("not plain"));
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(error)?;
    if !directory && info.nNumberOfLinks != 1 {
        return Err(error("hardlinked"));
    }
    let result = Identity {
        volume: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        created: (u64::from(info.ftCreationTime.dwHighDateTime) << 32)
            | u64::from(info.ftCreationTime.dwLowDateTime),
    };
    if result.index == 0 || result.created == 0 {
        return Err(error("missing identity"));
    }
    Ok(result)
}

pub(super) fn directory(path: &Path, deletable: bool) -> Result<File, String> {
    self::path(path)?;
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0 | if deletable { DELETE_ACCESS } else { 0 })
        .share_mode(3)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(error)?;
    identity(&file, true)?;
    Ok(file)
}

pub(super) fn parents(path: &Path, create: bool) -> Result<Vec<File>, String> {
    self::path(path)?;
    let mut prefix = PathBuf::new();
    let mut pins = Vec::new();
    for part in path.components() {
        prefix.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if create && matches!(part, Component::Normal(_)) {
            match std::fs::create_dir(&prefix) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(value) => return Err(error(value)),
            }
        }
        pins.push(directory(&prefix, false)?);
    }
    Ok(pins)
}

pub(super) fn file(path: &Path, deletable: bool) -> Result<File, String> {
    self::path(path)?;
    let file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ.0 | if deletable { DELETE_ACCESS } else { 0 })
        .share_mode(1)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(error)?;
    identity(&file, false)?;
    Ok(file)
}

pub(super) fn read(file: &File, limit: u64) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(error)?.len() > limit {
        return Err(error("oversize record"));
    }
    use std::os::windows::fs::FileExt;
    let mut bytes = vec![0; file.metadata().map_err(error)?.len() as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        let received = file
            .seek_read(&mut bytes[offset..], offset as u64)
            .map_err(error)?;
        if received == 0 {
            return Err(error("truncated record"));
        }
        offset += received;
    }
    Ok(bytes)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<File, String> {
    self::path(path)?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE_ACCESS)
        .share_mode(1)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(error)?;
    identity(&output, false)?;
    output.write_all(bytes).map_err(error)?;
    output.sync_all().map_err(error)?;
    if read(&output, bytes.len() as u64)? != bytes {
        return Err(error("publication changed"));
    }
    Ok(output)
}

pub(super) fn lock(path: &Path, create: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(error)?;
    identity(&file, false)?;
    if file.metadata().map_err(error)?.len() != 0 {
        return Err(error("lock changed"));
    }
    Ok(file)
}

pub(super) fn delete(file: &mut Option<File>) -> Result<(), String> {
    let Some(original) = file.as_ref() else {
        return Ok(());
    };
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(original.as_raw_handle()),
            FileDispositionInfo,
            &disposition as *const _ as *const _,
            std::mem::size_of_val(&disposition) as u32,
        )
    }
    .map_err(error)?;
    // Failed disposition retains the exact original handle for an explicit
    // retry. Never reopen a deletion target after this stage succeeds.
    drop(file.take());
    Ok(())
}
