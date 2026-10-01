//! Rename a fully flushed immutable journal without invalidating old readers.
use super::*;

// winbase.h FILE_RENAME_FLAG_*; deliberately no IGNORE_READONLY_ATTRIBUTE,
// storage-reserve overrides, sharing relaxation, or older-API retry fallback.
const REPLACE_IF_EXISTS: u32 = 0x00000001;
const POSIX_SEMANTICS: u32 = 0x00000002;

pub(super) fn replace(source: &Path, target: &Path) -> std::io::Result<()> {
    let file = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH)
        .open(source)?;
    regular_file(&file)?;
    let name = wide(target);
    let name_bytes = (name.len() - 1)
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| std::io::Error::other("Journal replacement path is too long"))?;
    let size = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(name.len() * std::mem::size_of::<u16>())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| std::io::Error::other("Journal rename buffer is too large"))?;
    // usize provides the native HANDLE alignment required by FILE_RENAME_INFO;
    // extra trailing storage holds the variable-length UTF-16 filename.
    let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let ok = unsafe {
        (*info).Anonymous.Flags = REPLACE_IF_EXISTS | POSIX_SEMANTICS;
        (*info).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfoEx,
            storage.as_ptr().cast(),
            size,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Close DELETE access before the owner's no-delete-share readback opens.
    Ok(())
}
