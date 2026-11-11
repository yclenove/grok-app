//! Publish via a same-volume staging file and `renameat`. Directory sync errors
//! are returned.

use std::fs::File;
use std::time::Instant;

use crate::osutil;
use crate::slot::{us, SlotError};

#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub ino: Option<u64>,
    pub dev: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct PubTimes {
    pub open_us: u64,
    pub write_us: u64,
    pub fsync_us: u64,
    pub rename_us: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn publish(
    staging: &File,
    dest: &File,
    dest_dev: u64,
    dest_fsid: Option<[i32; 2]>,
    tmp_name: &str,
    final_name: &str,
    bytes: &[u8],
    cursor: &mut Cursor,
    sync_dirs: bool,
) -> Result<PubTimes, SlotError> {
    let Some(expect_fsid) = dest_fsid else {
        return Err(SlotError::FsidUnknown);
    };
    if !osutil::component_ok(tmp_name) || !osutil::component_ok(final_name) {
        return Err(SlotError::Name);
    }
    let stage_id = osutil::id_of(staging).map_err(|_| SlotError::Io)?;
    if stage_id.dev != dest_dev {
        return Err(SlotError::DevMismatch);
    }
    match check_dest(dest, final_name, dest_dev, cursor)? {
        Dest::Absent | Dest::SameSlot => {}
    }
    let t_open = Instant::now();
    let mut file = osutil::create_0600_at(staging, tmp_name).map_err(|_| SlotError::Io)?;
    let open_us = us(t_open.elapsed());
    let t_write = Instant::now();
    osutil::write_all_fd(&mut file, bytes).map_err(|_| SlotError::Io)?;
    let write_us = us(t_write.elapsed());
    let t_sync = Instant::now();
    osutil::fsync_fd(&file).map_err(|_| SlotError::Io)?;
    let fsync_us = us(t_sync.elapsed());
    let src = osutil::id_of(&file).map_err(|_| SlotError::Io)?;
    if !src.is_reg || src.nlink != 1 || src.dev != dest_dev || src.size != bytes.len() as u64 {
        let _ = osutil::unlink_at(staging, tmp_name);
        return Err(SlotError::Replaced);
    }
    let src_fsid = osutil::fsid_of(&file).ok_or(SlotError::FsidUnknown)?;
    if src_fsid != expect_fsid {
        let _ = osutil::unlink_at(staging, tmp_name);
        return Err(SlotError::FsidMismatch);
    }
    let t_ren = Instant::now();
    osutil::rename_at(staging, tmp_name, dest, final_name).map_err(|_| SlotError::Io)?;
    let rename_us = us(t_ren.elapsed());
    let moved = osutil::id_of(&file).map_err(|_| SlotError::Io)?;
    let opened = osutil::open_child(dest, final_name).map_err(|_| SlotError::Io)?;
    let got = osutil::id_of(&opened).map_err(|_| SlotError::Io)?;
    if !got.is_reg || got.nlink != 1 || !got.same_file(&moved) || got.dev != dest_dev {
        return Err(SlotError::Replaced);
    }
    if sync_dirs {
        osutil::fsync_fd(dest).map_err(|_| SlotError::Io)?;
        osutil::fsync_fd(staging).map_err(|_| SlotError::Io)?;
    }
    cursor.dev = Some(got.dev);
    cursor.ino = Some(got.ino);
    Ok(PubTimes {
        open_us,
        write_us,
        fsync_us,
        rename_us,
    })
}

enum Dest {
    Absent,
    SameSlot,
}

fn check_dest(dest: &File, name: &str, dest_dev: u64, cursor: &Cursor) -> Result<Dest, SlotError> {
    match osutil::open_child(dest, name) {
        Err(err) if osutil::is_enoent(&err) => {
            if cursor.ino.is_some() {
                return Err(SlotError::Replaced);
            }
            Ok(Dest::Absent)
        }
        Err(err) if osutil::is_eloop(&err) => Err(SlotError::Symlink),
        Err(_) => Err(SlotError::Io),
        Ok(file) => {
            let id = osutil::id_of(&file).map_err(|_| SlotError::Io)?;
            if !id.is_reg || id.nlink != 1 || id.dev != dest_dev {
                return Err(SlotError::Replaced);
            }
            match (cursor.dev, cursor.ino) {
                (None, None) => Err(SlotError::Replaced),
                (Some(dev), Some(ino)) if id.dev == dev && id.ino == ino => Ok(Dest::SameSlot),
                _ => Err(SlotError::Replaced),
            }
        }
    }
}

pub fn sum_pub(t: &PubTimes) -> u64 {
    t.open_us
        .saturating_add(t.write_us)
        .saturating_add(t.fsync_us)
        .saturating_add(t.rename_us)
}
