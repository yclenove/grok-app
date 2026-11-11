//! Local volume directories, stat identity, and the 0700 private-root walk.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::osutil;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub dev: u64,
    pub ino: u64,
    pub uid: u32,
    pub perm: u32,
    pub sticky: bool,
    pub nlink: u64,
    pub len: u64,
    pub blocks_bytes: u64,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_link: bool,
}

pub fn lstat(path: &Path) -> Result<FileStat, &'static str> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "stat")?;
    let link = meta.file_type().is_symlink();
    Ok(FileStat {
        dev: meta.dev(),
        ino: meta.ino(),
        uid: meta.uid(),
        perm: meta.mode() & 0o777,
        sticky: meta.mode() & 0o1000 != 0,
        nlink: meta.nlink(),
        len: meta.len(),
        blocks_bytes: meta.blocks().saturating_mul(512),
        is_dir: meta.is_dir() && !link,
        is_file: meta.is_file() && !link,
        is_link: link,
    })
}

pub fn file_id(st: &FileStat) -> crate::identity::FileId {
    crate::identity::FileId {
        dev: st.dev,
        ino: st.ino,
    }
}

/// The leaf must be 0700 and owned by us. Ancestors we own may be 0755 (a home
/// directory) but must not be group- or other-writable unless they are sticky.
/// The first ancestor owned by someone else may be other-writable only when sticky.
pub fn verify_private_leaf(path: &Path) -> Result<(), &'static str> {
    let mut cur = path.to_path_buf();
    let mut own = true;
    loop {
        let st = lstat(&cur)?;
        if st.is_link {
            return Err("symlink");
        }
        if !st.is_dir {
            return Err("not-dir");
        }
        if own && (st.uid != osutil::euid() || st.perm != 0o700) {
            let ancestor = cur.as_os_str() != path.as_os_str();
            if ancestor && st.uid != osutil::euid() {
                own = false;
            } else if ancestor && st.uid == osutil::euid() && st.perm & 0o022 == 0 {
                // Same-user parents such as a 0755 home are not a plant point for other users.
            } else {
                return Err("mode");
            }
        }
        if !own {
            if st.perm & 0o002 != 0 && !st.sticky {
                return Err("exposed-parent");
            }
            return Ok(());
        }
        if cur.parent().is_none() || cur == Path::new("/") {
            return Ok(());
        }
        let parent = cur.parent().ok_or("exposed-parent")?.to_path_buf();
        if lstat(&parent)?.uid != osutil::euid() {
            own = false;
        }
        cur = parent;
        if !own {
            let st = lstat(&cur)?;
            if st.is_link {
                return Err("symlink");
            }
            if st.perm & 0o002 != 0 && !st.sticky {
                return Err("exposed-parent");
            }
            return Ok(());
        }
    }
}

pub fn acl_clean(ls_stdout: &str) -> Result<(), &'static str> {
    let line = ls_stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or("acl-unknown")?;
    let mode = line.split_whitespace().next().ok_or("acl-unknown")?;
    if mode.contains('+') {
        return Err("acl-present");
    }
    if ls_stdout.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("0:") || t.starts_with("1:")
    }) {
        return Err("acl-present");
    }
    Ok(())
}

pub fn scratch_within(st: &FileStat) -> bool {
    const LIMIT: u64 = 80 * 1024 * 1024;
    st.len <= LIMIT && st.blocks_bytes <= LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_leaf_rejects_loose_modes_and_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "s11-priv-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        osutil::mkdir_0700(&root).unwrap();
        let leaf = root.join("leaf");
        osutil::mkdir_0700(&leaf).unwrap();
        assert!(
            verify_private_leaf(&leaf).is_ok(),
            "sticky temp parent should pass"
        );
        std::fs::set_permissions(&leaf, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(verify_private_leaf(&leaf).unwrap_err(), "mode");
        std::fs::set_permissions(&leaf, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&leaf, &link).unwrap();
        assert_eq!(verify_private_leaf(&link).unwrap_err(), "symlink");
        assert!(acl_clean("drwx------  2 role staff 64 Nov 11 00:00 leaf\n").is_ok());
        assert_eq!(
            acl_clean("drwx------+ 2 role staff 64 Nov 11 00:00 leaf\n").unwrap_err(),
            "acl-present"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
