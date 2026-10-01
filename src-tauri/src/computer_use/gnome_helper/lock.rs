//! Fixed per-user cross-process lock. Reading status never creates this file.
use super::bundle::{safe_path, UUID};
use std::fs::{self, File, OpenOptions};
use std::os::{
    fd::AsRawFd,
    unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
};
use std::path::Path;

pub(super) struct PublicationLock(File);
impl PublicationLock {
    pub(super) fn acquire(root: &Path) -> Result<Self, String> {
        safe_path(root)?;
        // Create every missing ancestor privately even with a login umask of
        // 0002. Never chmod or adopt an existing writable directory.
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)
            .map_err(|e| e.to_string())?;
        safe_path(root)?;
        if fs::metadata(root).map_err(|e| e.to_string())?.uid() != unsafe { libc::geteuid() } {
            return Err("computer_use_helper_files: extension root belongs to another user".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(root.join(format!(".{UUID}.lock")))
            .map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err("computer_use_helper_files: unsafe publication lock".into());
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("computer_use_helper_busy: another App owns publication".into());
        }
        Ok(Self(file))
    }
}
impl Drop for PublicationLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_lock_excludes_other_handles_then_releases() {
        let root = std::env::temp_dir().join(format!("grok-helper-lock-{}", uuid::Uuid::new_v4()));
        let first = PublicationLock::acquire(&root).unwrap();
        assert!(PublicationLock::acquire(&root).is_err());
        drop(first);
        drop(PublicationLock::acquire(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn symlink_lock_does_not_modify_target() {
        let root = std::env::temp_dir().join(format!("grok-helper-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let target = root.join("foreign");
        fs::write(&target, "preserve").unwrap();
        std::os::unix::fs::symlink(&target, root.join(format!(".{UUID}.lock"))).unwrap();
        assert!(PublicationLock::acquire(&root).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
        fs::remove_dir_all(root).unwrap();
    }
}
