//! Auth seed copy. Validation and the byte read share one file handle.
//!
//! A pathname cannot prove an evaluation-only identity.

use crate::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Copy the evaluation login into a task's temporary `GROK_HOME`.
///
/// The seed file is opened once. On Unix that open uses `O_NOFOLLOW` and
/// `O_NONBLOCK`, then `fstat` on the same handle must show a regular file,
/// link count 1, owner equal to the effective uid, and size at most 1 MiB.
/// The bytes are read from that handle. A hard link, FIFO, or symlink is
/// refused. A canonical path under a personal config directory is refused.
/// The file contents are not logged. A pathname cannot prove an evaluation-only
/// identity. Only `auth.json` is taken from `seed`.
pub(crate) fn seed_auth(seed: &Path, grok_home: &Path) -> Result<(), Error> {
    seed_auth_in(seed, grok_home, &crate::access::user_config_dirs())
}

pub(crate) fn seed_auth_in(
    seed: &Path,
    grok_home: &Path,
    configs: &[PathBuf],
) -> Result<(), Error> {
    if symlink_path(seed)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    let from = seed.join("auth.json");
    if symlink_path(&from)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    let seed_canon =
        std::fs::canonicalize(seed).map_err(|_| Error::msg("auth seed has no auth.json"))?;
    let file_canon =
        std::fs::canonicalize(&from).map_err(|_| Error::msg("auth seed has no auth.json"))?;
    if crate::access::canonical_under_configs(&seed_canon, configs)?
        || crate::access::canonical_under_configs(&file_canon, configs)?
    {
        return Err(Error::msg(
            "refusing an auth seed under a personal config directory",
        ));
    }
    let bytes = read_seed_file(&from)?;
    let to = grok_home.join("auth.json");
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(Error::msg("could not copy the auth seed"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&to)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    file.write_all(&bytes)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    Ok(())
}

/// Write a refreshed login back to the seed after an unverified run.
///
/// The Grok login rotates its refresh token, so the copy a task used is the
/// only valid one once it refreshed. Only used with `--unverified-environment`.
/// The task copy is read with the same checks as the seed; the seed file is
/// replaced through a new `0600` temporary file and a rename. Returns whether
/// anything was written.
pub(crate) fn write_back_auth(seed: &Path, grok_home: &Path) -> Result<bool, Error> {
    if symlink_path(seed)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    let used = grok_home.join("auth.json");
    if symlink_path(&used)? || std::fs::symlink_metadata(&used).is_err() {
        return Ok(false);
    }
    let fresh = read_seed_file(&used)?;
    let target = seed.join("auth.json");
    if symlink_path(&target)? {
        return Err(Error::msg("refusing a symlink auth seed"));
    }
    if read_seed_file(&target).is_ok_and(|old| old == fresh) {
        return Ok(false);
    }
    let tmp = seed.join(".auth.json.writeback");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&tmp)
        .map_err(|_| Error::msg("could not write the auth seed back"))?;
    file.write_all(&fresh)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::msg("could not write the auth seed back"))?;
    drop(file);
    std::fs::rename(&tmp, &target).map_err(|_| Error::msg("could not write the auth seed back"))?;
    Ok(true)
}

const SEED_MAX: u64 = 1024 * 1024;

fn read_seed_file(path: &Path) -> Result<Vec<u8>, Error> {
    #[cfg(unix)]
    {
        read_seed_unix(path)
    }
    #[cfg(not(unix))]
    {
        read_seed_plain(path)
    }
}

#[cfg(unix)]
fn read_seed_unix(path: &Path) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    let file = options
        .open(path)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    let meta = file
        .metadata()
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    if !meta.file_type().is_file() || meta.nlink() != 1 || meta.uid() != crate::ffi::current_euid()
    {
        return Err(Error::msg("refusing a non-regular or linked auth seed"));
    }
    if meta.len() > SEED_MAX {
        return Err(Error::msg("auth seed exceeds the size cap"));
    }
    let mut bytes = Vec::new();
    file.take(SEED_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > SEED_MAX {
        return Err(Error::msg("auth seed exceeds the size cap"));
    }
    Ok(bytes)
}

#[cfg(windows)]
fn read_seed_plain(path: &Path) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    // Do not follow a reparse point. `MetadataExt::number_of_links` is unstable
    // (`windows_by_handle`) on the Windows toolchain, so the link count comes
    // from `GetFileInformationByHandle` on this same handle.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options
        .open(path)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    let meta = file
        .metadata()
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    let links = crate::ffi::file_link_count(&file)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    if !meta.is_file() || meta.file_type().is_symlink() || links != 1 {
        return Err(Error::msg("refusing a non-regular or linked auth seed"));
    }
    if meta.len() > SEED_MAX {
        return Err(Error::msg("auth seed exceeds the size cap"));
    }
    let mut bytes = Vec::new();
    file.take(SEED_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::msg("could not copy the auth seed"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > SEED_MAX {
        return Err(Error::msg("auth seed exceeds the size cap"));
    }
    Ok(bytes)
}

#[cfg(all(not(unix), not(windows)))]
fn read_seed_plain(path: &Path) -> Result<Vec<u8>, Error> {
    let _ = path;
    Err(Error::msg("could not copy the auth seed"))
}

fn symlink_path(path: &Path) -> Result<bool, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_symlink()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::msg("auth seed has no auth.json")),
    }
}
