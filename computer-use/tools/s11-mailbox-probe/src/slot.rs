//! Reads relative to a recorded directory fd. A missing fsid is not a pass.

use std::fs::File;
use std::time::Instant;

use crate::osutil::{self, Id};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotError {
    Name,
    Symlink,
    NotFile,
    HardLink,
    Size,
    Replaced,
    Absent,
    FsidUnknown,
    FsidMismatch,
    DevMismatch,
    Io,
}

impl SlotError {
    pub fn integrity(self) -> bool {
        matches!(
            self,
            SlotError::Symlink
                | SlotError::NotFile
                | SlotError::HardLink
                | SlotError::Size
                | SlotError::FsidMismatch
                | SlotError::DevMismatch
        )
    }
}

#[derive(Debug)]
pub struct TimedRead {
    pub bytes: Vec<u8>,
    pub open_us: u64,
    pub read_us: u64,
}

pub fn read_slot(
    dir: &File,
    export_dev: u64,
    export_fsid: Option<[i32; 2]>,
    name: &str,
    cap: usize,
) -> Result<TimedRead, SlotError> {
    if !osutil::component_ok(name) {
        return Err(SlotError::Name);
    }
    let opened = Instant::now();
    let mut file = match osutil::open_child(dir, name) {
        Ok(file) => file,
        Err(err) if osutil::is_enoent(&err) => return Err(SlotError::Absent),
        Err(err) if osutil::is_eloop(&err) => return Err(SlotError::Symlink),
        Err(err) if osutil::is_enxio(&err) => return Err(SlotError::NotFile),
        Err(_) => return Err(SlotError::Io),
    };
    let before = osutil::id_of(&file).map_err(|_| SlotError::Io)?;
    let open_us = us(opened.elapsed());
    check_id(&before, export_dev, cap)?;
    let fsid = osutil::fsid_of(&file).ok_or(SlotError::FsidUnknown)?;
    match export_fsid {
        Some(expected) if expected == fsid => {}
        Some(_) => return Err(SlotError::FsidMismatch),
        None => return Err(SlotError::FsidUnknown),
    }
    let reading = Instant::now();
    let bytes = osutil::read_fd_capped(&mut file, cap).map_err(|_| SlotError::Size)?;
    let read_us = us(reading.elapsed());
    let after = osutil::id_of(&file).map_err(|_| SlotError::Io)?;
    if !same_moment(&before, &after) {
        return Err(SlotError::Replaced);
    }
    Ok(TimedRead {
        bytes,
        open_us,
        read_us,
    })
}

fn check_id(id: &Id, export_dev: u64, cap: usize) -> Result<(), SlotError> {
    if !id.is_reg {
        return Err(SlotError::NotFile);
    }
    if id.nlink == 0 {
        return Err(SlotError::Replaced);
    }
    if id.nlink != 1 {
        return Err(SlotError::HardLink);
    }
    if id.dev != export_dev {
        return Err(SlotError::DevMismatch);
    }
    if id.size as usize > cap {
        return Err(SlotError::Size);
    }
    Ok(())
}

fn same_moment(a: &Id, b: &Id) -> bool {
    a.dev == b.dev
        && a.ino == b.ino
        && a.mtime == b.mtime
        && a.mtime_nsec == b.mtime_nsec
        && a.ctime == b.ctime
        && a.ctime_nsec == b.ctime_nsec
        && a.size == b.size
        && a.nlink == b.nlink
}

pub fn us(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn symlink_and_fsid_are_not_a_normal_read() {
        let root = std::path::Path::new(&std::env::var("HOME").unwrap())
            .join(format!(".s11m2a-slot-{}", std::process::id()));
        let _ = std::fs::remove_dir(&root);
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let held = osutil::verify_private_leaf(&root).unwrap();
        let mut good = osutil::create_0600_at(&held.file, "frame").unwrap();
        osutil::write_all_fd(&mut good, b"abc").unwrap();
        drop(good);
        let read = read_slot(&held.file, held.dev, held.fsid, "frame", 16).unwrap();
        assert_eq!(read.bytes, b"abc");
        assert!(read_slot(&held.file, held.dev, None, "frame", 16).is_err());
        std::os::unix::fs::symlink("frame", root.join("link")).unwrap();
        assert_eq!(
            read_slot(&held.file, held.dev, held.fsid, "link", 16).unwrap_err(),
            SlotError::Symlink
        );
        assert_eq!(
            read_slot(&held.file, held.dev, held.fsid, "missing", 16).unwrap_err(),
            SlotError::Absent
        );
        let _ = std::fs::remove_file(root.join("link"));
        let _ = osutil::unlink_at(&held.file, "frame");
        let _ = std::fs::remove_dir(&root);
    }
}
