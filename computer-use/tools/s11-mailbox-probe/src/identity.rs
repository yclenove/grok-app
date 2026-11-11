//! Destructive checklist. Detach and unlink run only when the ledger identity
//! still matches a complete readback. A miss quarantines and does not delete.

use crate::plist::{self, ImageAttach, Plist};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachId {
    pub image_path: String,
    pub whole: String,
    pub slice: Option<String>,
    pub mount: String,
    pub uuid: String,
    pub fsid: [u8; 8],
    pub image: FileId,
    pub mount_underlying: FileId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destroy {
    Proceed,
    SkipAbsent,
    Quarantine { reason: &'static str },
}

pub fn decide_detach(
    recorded: &AttachId,
    info: &Result<Vec<ImageAttach>, &'static str>,
    diskutil: &Result<Plist, &'static str>,
    fsid_now: Option<[u8; 8]>,
) -> Destroy {
    if !plist::whole_disk(&recorded.whole) || recorded.uuid.is_empty() {
        return Destroy::Quarantine {
            reason: "identity-incomplete",
        };
    }
    let images = match info {
        Ok(images) => images,
        Err(_) => {
            return Destroy::Quarantine {
                reason: "readback-incomplete",
            }
        }
    };
    let Some(hit) = images
        .iter()
        .find(|img| img.image_path == recorded.image_path)
    else {
        return Destroy::Quarantine {
            reason: "readback-incomplete",
        };
    };
    if hit.whole != recorded.whole {
        return Destroy::Quarantine {
            reason: "identity-mismatch",
        };
    }
    if hit.mount.as_deref() != Some(recorded.mount.as_str()) && hit.mount.is_some() {
        return Destroy::Quarantine {
            reason: "identity-mismatch",
        };
    }
    let doc = match diskutil {
        Ok(doc) => doc,
        Err(_) => {
            return Destroy::Quarantine {
                reason: "readback-incomplete",
            }
        }
    };
    let uuid = doc.get("VolumeUUID").and_then(Plist::string).unwrap_or("");
    let node = doc.get("DeviceNode").and_then(Plist::string).unwrap_or("");
    if uuid.is_empty() || node.is_empty() {
        return Destroy::Quarantine {
            reason: "readback-incomplete",
        };
    }
    if uuid != recorded.uuid {
        return Destroy::Quarantine {
            reason: "identity-mismatch",
        };
    }
    let node_ok = node == recorded.whole || recorded.slice.as_deref() == Some(node);
    if !node_ok {
        return Destroy::Quarantine {
            reason: "identity-mismatch",
        };
    }
    match fsid_now {
        Some(id) if id == recorded.fsid => Destroy::Proceed,
        _ => Destroy::Quarantine {
            reason: "fsid-mismatch",
        },
    }
}

pub struct UnlinkView<'a> {
    pub info: &'a Result<Vec<ImageAttach>, &'static str>,
    pub mount_text: &'a Result<String, &'static str>,
    pub image_now: Option<FileId>,
    pub image_missing: bool,
}

pub fn decide_unlink(recorded: &AttachId, view: &UnlinkView<'_>) -> Destroy {
    let images = match view.info {
        Ok(images) => images,
        Err(_) => {
            return Destroy::Quarantine {
                reason: "readback-incomplete",
            }
        }
    };
    let mentioned = images.iter().any(|img| {
        img.image_path == recorded.image_path
            || img.whole == recorded.whole
            || recorded
                .slice
                .as_ref()
                .is_some_and(|s| img.slice.as_ref() == Some(s) || img.whole == *s)
    });
    if mentioned {
        return Destroy::Quarantine {
            reason: "possibly-attached",
        };
    }
    let mount = match view.mount_text {
        Ok(text) => text,
        Err(_) => {
            return Destroy::Quarantine {
                reason: "readback-incomplete",
            }
        }
    };
    if mount_contains(mount, &recorded.mount) {
        return Destroy::Quarantine {
            reason: "possibly-attached",
        };
    }
    if view.image_missing {
        return Destroy::SkipAbsent;
    }
    match view.image_now {
        Some(id) if id == recorded.image => Destroy::Proceed,
        Some(_) => Destroy::Quarantine {
            reason: "identity-mismatch",
        },
        None => Destroy::Quarantine {
            reason: "readback-incomplete",
        },
    }
}

pub fn decide_unlink_unattached(
    recorded: &FileId,
    info: &Result<Vec<ImageAttach>, &'static str>,
    image_path: &str,
    now: Option<FileId>,
    missing: bool,
) -> Destroy {
    let images = match info {
        Ok(images) => images,
        Err(_) => {
            return Destroy::Quarantine {
                reason: "readback-incomplete",
            }
        }
    };
    if images.iter().any(|img| img.image_path == image_path) {
        return Destroy::Quarantine {
            reason: "possibly-attached",
        };
    }
    if missing {
        return Destroy::SkipAbsent;
    }
    match now {
        Some(id) if id == *recorded => Destroy::Proceed,
        Some(_) => Destroy::Quarantine {
            reason: "identity-mismatch",
        },
        None => Destroy::Quarantine {
            reason: "readback-incomplete",
        },
    }
}

pub fn decide_rmdir(recorded: FileId, now: Option<FileId>, empty: bool) -> Destroy {
    match now {
        None => Destroy::SkipAbsent,
        Some(id) if id == recorded && empty => Destroy::Proceed,
        Some(id) if id == recorded && !empty => Destroy::Quarantine {
            reason: "dir-not-empty",
        },
        Some(_) => Destroy::Quarantine {
            reason: "identity-mismatch",
        },
    }
}

/// Exact mount-point match. Our paths contain no spaces.
pub fn mount_contains(table: &str, mount: &str) -> bool {
    if mount.is_empty() || mount.contains(' ') || mount.contains('\t') {
        return true;
    }
    for line in table.lines() {
        let Some((_, rest)) = line.split_once(" on ") else {
            continue;
        };
        let point = rest.split(" (").next().unwrap_or("");
        if point == mount {
            return true;
        }
    }
    false
}

pub fn mount_flags(table: &str, mount: &str) -> Option<Vec<String>> {
    for line in table.lines() {
        let Some((_, rest)) = line.split_once(" on ") else {
            continue;
        };
        let mut parts = rest.split(" (");
        let point = parts.next().unwrap_or("");
        if point != mount {
            continue;
        }
        let flags = parts.next().unwrap_or("").trim_end_matches(')');
        return Some(
            flags
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        );
    }
    None
}

pub fn flags_have(flags: &[String], need: &[&str]) -> bool {
    need.iter().all(|n| flags.iter().any(|f| f == n))
}

pub fn excluded_exact(stdout: &str, path: &str) -> bool {
    stdout.lines().any(|line| {
        let line = line.trim();
        line.starts_with("[Excluded]") && line.split_whitespace().any(|tok| tok == path)
    }) && !stdout
        .lines()
        .any(|line| line.contains("[Included]") && line.split_whitespace().any(|tok| tok == path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded() -> AttachId {
        AttachId {
            image_path: "/vol/mailbox.sparseimage".into(),
            whole: "/dev/disk4".into(),
            slice: Some("/dev/disk4s1".into()),
            mount: "/vol/mnt".into(),
            uuid: "UUID-1".into(),
            fsid: [1, 2, 3, 4, 5, 6, 7, 8],
            image: FileId { dev: 9, ino: 10 },
            mount_underlying: FileId { dev: 3, ino: 4 },
        }
    }

    fn image() -> ImageAttach {
        ImageAttach {
            image_path: "/vol/mailbox.sparseimage".into(),
            whole: "/dev/disk4".into(),
            slice: Some("/dev/disk4s1".into()),
            mount: Some("/vol/mnt".into()),
        }
    }

    fn diskutil() -> Plist {
        Plist::Dict(vec![
            ("VolumeUUID".into(), Plist::String("UUID-1".into())),
            ("DeviceNode".into(), Plist::String("/dev/disk4s1".into())),
        ])
    }

    #[test]
    fn mismatch_or_incomplete_readback_does_not_detach_or_unlink() {
        let rec = recorded();
        assert_eq!(
            decide_detach(&rec, &Ok(vec![image()]), &Ok(diskutil()), Some(rec.fsid)),
            Destroy::Proceed
        );
        let mut bad = diskutil();
        if let Plist::Dict(pairs) = &mut bad {
            pairs[0].1 = Plist::String("UUID-2".into());
        }
        assert_eq!(
            decide_detach(&rec, &Ok(vec![image()]), &Ok(bad), Some(rec.fsid)),
            Destroy::Quarantine {
                reason: "identity-mismatch"
            }
        );
        assert_eq!(
            decide_detach(&rec, &Err("plist-limit"), &Ok(diskutil()), Some(rec.fsid)),
            Destroy::Quarantine {
                reason: "readback-incomplete"
            }
        );
        assert_eq!(
            decide_detach(&rec, &Ok(vec![image()]), &Ok(diskutil()), None),
            Destroy::Quarantine {
                reason: "fsid-mismatch"
            }
        );

        let empty: Result<Vec<ImageAttach>, &'static str> = Ok(vec![]);
        let mount_ok = Ok("/dev/disk1 on / (apfs, local)\n".to_string());
        assert_eq!(
            decide_unlink(
                &rec,
                &UnlinkView {
                    info: &empty,
                    mount_text: &mount_ok,
                    image_now: Some(rec.image),
                    image_missing: false,
                }
            ),
            Destroy::Proceed
        );
        let still = Ok(vec![image()]);
        assert_eq!(
            decide_unlink(
                &rec,
                &UnlinkView {
                    info: &still,
                    mount_text: &mount_ok,
                    image_now: Some(rec.image),
                    image_missing: false,
                }
            ),
            Destroy::Quarantine {
                reason: "possibly-attached"
            }
        );
        let mounted =
            Ok("/dev/disk4s1 on /vol/mnt (hfs, local, nodev, nosuid, noexec, rw)\n".into());
        assert_eq!(
            decide_unlink(
                &rec,
                &UnlinkView {
                    info: &empty,
                    mount_text: &mounted,
                    image_now: Some(rec.image),
                    image_missing: false,
                }
            ),
            Destroy::Quarantine {
                reason: "possibly-attached"
            }
        );
        let other = FileId { dev: 1, ino: 2 };
        assert_eq!(
            decide_unlink(
                &rec,
                &UnlinkView {
                    info: &empty,
                    mount_text: &mount_ok,
                    image_now: Some(other),
                    image_missing: false,
                }
            ),
            Destroy::Quarantine {
                reason: "identity-mismatch"
            }
        );
        assert_eq!(
            decide_unlink(
                &rec,
                &UnlinkView {
                    info: &Err("overflow"),
                    mount_text: &mount_ok,
                    image_now: None,
                    image_missing: true,
                }
            ),
            Destroy::Quarantine {
                reason: "readback-incomplete"
            }
        );
        assert_eq!(
            decide_rmdir(rec.mount_underlying, Some(rec.mount_underlying), false),
            Destroy::Quarantine {
                reason: "dir-not-empty"
            }
        );
    }

    #[test]
    fn mount_flags_and_exclusion_are_exact() {
        let table = "/dev/disk4s1 on /vol/mnt (hfs, local, nodev, nosuid, noexec, rw)\n";
        let flags = mount_flags(table, "/vol/mnt").unwrap();
        assert!(flags_have(&flags, &["rw", "nosuid", "noexec", "nodev"]));
        assert!(!mount_contains(table, "/vol/mnt-other"));
        assert!(excluded_exact("[Excluded] /vol/a\n", "/vol/a"));
        assert!(!excluded_exact("[Included] /vol/a\n", "/vol/a"));
        assert!(!excluded_exact("Excluded /vol/a\n", "/vol/a"));
    }
}
