//! Recorded dev/inode cleanup. Unknown names, links, and mismatches quarantine.
//! There is no volume DeviceNode path in this probe.

use std::fs::File;
use std::path::Path;

use crate::osutil::{self, Id};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjId {
    pub dev: u64,
    pub ino: u64,
}

impl From<&Id> for ObjId {
    fn from(id: &Id) -> Self {
        Self {
            dev: id.dev,
            ino: id.ino,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmdirPlan {
    SkipAbsent,
    Proceed,
    Quarantine(&'static str),
}

/// A directory is removed only when it is empty and still the recorded inode.
/// A replacement inode is not removed.
pub fn decide_rmdir(found: Option<ObjId>, recorded: ObjId, children: usize) -> RmdirPlan {
    match found {
        None => RmdirPlan::SkipAbsent,
        Some(id) if id == recorded && children == 0 => RmdirPlan::Proceed,
        Some(id) if id == recorded => RmdirPlan::Quarantine("dir-not-empty"),
        Some(_) => RmdirPlan::Quarantine("identity-mismatch"),
    }
}

#[derive(Clone, Debug)]
pub struct Rec {
    pub parent_path: String,
    pub parent_dev: u64,
    pub parent_ino: u64,
    pub name: String,
    pub dev: u64,
    pub ino: u64,
    pub is_dir: bool,
    /// Mailbox directories the guest writes into are retained on purpose.
    pub retain: bool,
}

pub struct Tree {
    pub items: Vec<Rec>,
}

impl Tree {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn record_dir(
        &mut self,
        parent: &osutil::Held,
        parent_path: &Path,
        name: &str,
        file: &File,
        retain: bool,
    ) -> Result<(), &'static str> {
        let id = osutil::id_of(file).map_err(|_| "record")?;
        if !id.is_dir {
            return Err("record");
        }
        self.items.push(Rec {
            parent_path: parent_path.display().to_string(),
            parent_dev: parent.dev,
            parent_ino: parent.ino,
            name: name.to_string(),
            dev: id.dev,
            ino: id.ino,
            is_dir: true,
            retain,
        });
        Ok(())
    }

    pub fn record_file(
        &mut self,
        parent: &osutil::Held,
        parent_path: &Path,
        name: &str,
        file: &File,
    ) -> Result<(), &'static str> {
        let id = osutil::id_of(file).map_err(|_| "record")?;
        if !id.is_reg || id.nlink != 1 {
            return Err("record");
        }
        self.items.push(Rec {
            parent_path: parent_path.display().to_string(),
            parent_dev: parent.dev,
            parent_ino: parent.ino,
            name: name.to_string(),
            dev: id.dev,
            ino: id.ino,
            is_dir: false,
            retain: false,
        });
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildKind {
    File(ObjId),
    Dir(ObjId),
    Link,
    Other,
}

fn classify_child(parent: &File, name: &str) -> Result<ChildKind, &'static str> {
    match osutil::open_child(parent, name) {
        Err(err) if osutil::is_eloop(&err) => Ok(ChildKind::Link),
        Err(err) if osutil::is_enoent(&err) => Err("absent"),
        Err(err) if osutil::is_eacces(&err) => Err("eacces"),
        Err(_) => Ok(ChildKind::Other),
        Ok(file) => {
            let id = osutil::id_of(&file).map_err(|_| "stat")?;
            let obj = ObjId::from(&id);
            if id.is_dir {
                Ok(ChildKind::Dir(obj))
            } else if id.is_reg {
                Ok(ChildKind::File(obj))
            } else {
                Ok(ChildKind::Other)
            }
        }
    }
}

fn reopen_parent(rec: &Rec) -> Result<osutil::Held, &'static str> {
    let held = osutil::verify_private_leaf(Path::new(&rec.parent_path))?;
    if held.dev != rec.parent_dev || held.ino != rec.parent_ino {
        return Err("parent-identity");
    }
    Ok(held)
}

/// Delete only recorded names under directories this run marked removable.
/// Any unknown entry, symlink, or identity mismatch leaves that directory
/// untouched and returns quarantine. This never walks recursively.
pub fn cleanup(tree: &Tree) -> Result<(), &'static str> {
    let dirs: Vec<&Rec> = tree
        .items
        .iter()
        .filter(|r| r.is_dir && !r.retain)
        .collect();
    for dir in dirs.into_iter().rev() {
        clean_one_dir(tree, dir)?;
    }
    Ok(())
}

fn clean_one_dir(tree: &Tree, dir: &Rec) -> Result<(), &'static str> {
    let parent = reopen_parent(dir)?;
    let opened = match osutil::open_child_dir(&parent.file, &dir.name) {
        Err(err) if osutil::is_enoent(&err) => return Ok(()),
        Err(err) if osutil::is_eloop(&err) => return Err("symlink"),
        Err(_) => return Err("open-dir"),
        Ok(file) => file,
    };
    let id = osutil::id_of(&opened).map_err(|_| "stat")?;
    if id.dev != dir.dev || id.ino != dir.ino {
        return Err("identity-mismatch");
    }
    let recorded: Vec<&Rec> = tree
        .items
        .iter()
        .filter(|r| r.parent_dev == dir.dev && r.parent_ino == dir.ino)
        .collect();
    let dir_path = Path::new(&dir.parent_path).join(&dir.name);
    let entries = std::fs::read_dir(&dir_path).map_err(|_| "read-dir")?;
    let mut names = Vec::new();
    for ent in entries {
        let ent = ent.map_err(|_| "read-dir")?;
        let name = ent.file_name();
        let Some(name) = name.to_str() else {
            return Err("unknown-entry");
        };
        if !osutil::component_ok(name) {
            return Err("unknown-entry");
        }
        names.push(name.to_string());
    }
    for name in &names {
        if !recorded.iter().any(|r| r.name == *name) {
            return Err("unknown-entry");
        }
        match classify_child(&opened, name) {
            Ok(ChildKind::Link) | Ok(ChildKind::Other) => return Err("unknown-entry"),
            Err("eacces") => return Err("eacces"),
            Err("absent") => {}
            Ok(ChildKind::File(obj)) | Ok(ChildKind::Dir(obj)) => {
                let rec = recorded.iter().find(|r| r.name == *name).unwrap();
                if obj.dev != rec.dev || obj.ino != rec.ino {
                    return Err("identity-mismatch");
                }
                if rec.is_dir != matches!(classify_child(&opened, name), Ok(ChildKind::Dir(_))) {
                    return Err("identity-mismatch");
                }
            }
            Err(_) => return Err("stat"),
        }
    }
    for rec in &recorded {
        if rec.is_dir {
            continue;
        }
        match classify_child(&opened, &rec.name) {
            Err("absent") => {}
            Ok(ChildKind::File(obj)) if obj.dev == rec.dev && obj.ino == rec.ino => {
                osutil::unlink_at(&opened, &rec.name).map_err(|_| "unlink")?;
            }
            Ok(_) => return Err("identity-mismatch"),
            Err("eacces") => return Err("eacces"),
            Err(_) => return Err("unlink"),
        }
    }
    let remaining = std::fs::read_dir(&dir_path)
        .map_err(|_| "read-dir")?
        .count();
    let found = match osutil::open_child_dir(&parent.file, &dir.name) {
        Err(err) if osutil::is_enoent(&err) => None,
        Err(_) => return Err("open-dir"),
        Ok(file) => {
            let id = osutil::id_of(&file).map_err(|_| "stat")?;
            Some(ObjId {
                dev: id.dev,
                ino: id.ino,
            })
        }
    };
    match decide_rmdir(
        found,
        ObjId {
            dev: dir.dev,
            ino: dir.ino,
        },
        remaining,
    ) {
        RmdirPlan::SkipAbsent => Ok(()),
        RmdirPlan::Proceed => osutil::unlink_dir_at(&parent.file, &dir.name).map_err(|_| "rmdir"),
        RmdirPlan::Quarantine(reason) => Err(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn replacement_directory_is_not_removed() {
        let recorded = ObjId { dev: 1, ino: 2 };
        assert_eq!(decide_rmdir(None, recorded, 0), RmdirPlan::SkipAbsent);
        assert_eq!(
            decide_rmdir(Some(recorded), recorded, 0),
            RmdirPlan::Proceed
        );
        assert_eq!(
            decide_rmdir(Some(recorded), recorded, 1),
            RmdirPlan::Quarantine("dir-not-empty")
        );
        assert_eq!(
            decide_rmdir(Some(ObjId { dev: 1, ino: 9 }), recorded, 0),
            RmdirPlan::Quarantine("identity-mismatch")
        );
    }

    #[test]
    fn unknown_canary_quarantines_and_is_left_in_place() {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = Path::new(&std::env::var("HOME").unwrap())
            .join(format!(".s11m2a-id-{}-{n}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let parent = osutil::verify_private_leaf(&root).unwrap();
        let dirf = osutil::mkdir_0700_at(&parent.file, "stage").unwrap();
        let mut tree = Tree::new();
        tree.record_dir(&parent, &root, "stage", &dirf, false)
            .unwrap();
        std::fs::write(root.join("stage").join("canary"), b"keep").unwrap();
        let err = cleanup(&tree).unwrap_err();
        assert_eq!(err, "unknown-entry");
        assert_eq!(
            std::fs::read(root.join("stage").join("canary")).unwrap(),
            b"keep"
        );
        assert!(root.join("stage").is_dir());
        let _ = std::fs::remove_file(root.join("stage").join("canary"));
        let _ = std::fs::remove_dir(root.join("stage"));
        let _ = std::fs::remove_dir(&root);
    }
}
