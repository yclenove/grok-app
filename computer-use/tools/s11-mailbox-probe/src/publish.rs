//! Host publish: exclusive temp on the unexported stage, fsync, rename into the slot.
//! A temp that already exists is a stop, not an in-place write.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::osutil::{self, flag_creat, flag_excl, flag_wronly};
use crate::slot::{self, SlotError};

#[derive(Clone, Debug)]
pub struct Times {
    pub write_us: u64,
    pub fsync_us: u64,
    pub rename_us: u64,
}

fn us_since(t: Instant) -> u64 {
    t.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

pub fn publish(
    stage: &Path,
    target_dir: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<Times, &'static str> {
    if !slot::name_ok(name) || name == "probe-result" {
        return Err("publish-name");
    }
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err("publish-size");
    }
    let tmp_name = match name {
        "c1-frame" => "c1-frame.tmp",
        "c2-frame" => "c2-frame.tmp",
        _ => return Err("publish-name"),
    };
    let stage_file = osutil::open_dir_nofollow(stage).map_err(|_| "stage-open")?;
    // O_EXCL covers an existing symlink. Darwin rejects O_NOFOLLOW together with O_CREAT.
    let flags = flag_creat() | flag_excl() | flag_wronly();
    let started = Instant::now();
    let mut tmp =
        osutil::open_at(&stage_file, tmp_name, flags, 0o600).map_err(|_| "stage-create")?;
    tmp.write_all(bytes).map_err(|_| "stage-write")?;
    let write_us = us_since(started);
    let sync_at = Instant::now();
    tmp.sync_all().map_err(|_| "stage-fsync")?;
    let fsync_us = us_since(sync_at);
    let meta = tmp.metadata().map_err(|_| "stage-stat")?;
    if meta.len() != bytes.len() as u64 {
        return Err("stage-size");
    }
    drop(tmp);
    let from = stage.join(tmp_name);
    let to = target_dir.join(name);
    let rename_at = Instant::now();
    std::fs::rename(&from, &to).map_err(|_| "rename")?;
    let rename_us = us_since(rename_at);
    let _ = File::open(target_dir).and_then(|dir| dir.sync_all());
    let _ = File::open(stage).and_then(|dir| dir.sync_all());
    match slot::read_named(target_dir, name, bytes.len() as u64, bytes.len() as u64) {
        Ok(got) if got == bytes => Ok(Times {
            write_us,
            fsync_us,
            rename_us,
        }),
        Ok(_) => Err("torn-readback"),
        Err(SlotError::Symlink) => Err("symlink"),
        Err(SlotError::HardLink) => Err("hardlink"),
        Err(SlotError::Fsid { .. }) => Err("readback-fsid"),
        Err(SlotError::Replaced) => Err("readback-replaced"),
        Err(SlotError::Size) => Err("readback-size"),
        Err(SlotError::NotFile) => Err("readback-notfile"),
        Err(SlotError::Name) => Err("readback-name"),
        Err(SlotError::Io { op, .. }) => Err(match op {
            "open" => "readback-open",
            "read" => "readback-read",
            "fstat" => "readback-fstat",
            "dir-open" => "readback-dir-open",
            "dir-fstat" => "readback-dir-fstat",
            "dir-stat" => "readback-dir-stat",
            "restat" => "readback-restat",
            "reopen-stat" => "readback-reopen-stat",
            "lstat" => "readback-lstat",
            _ => "readback-io",
        }),
    }
}
