use super::*;

struct Fixture {
    root: PathBuf,
    bundle: Bundle,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cu-helper-bundle-{}", uuid::Uuid::new_v4()));
        Self {
            bundle: Bundle::new(root.join("extensions")),
            root,
        }
    }
    fn install(&self) {
        self.bundle.install(false, &shell()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn shell() -> ShellStamp {
    ShellStamp {
        bus: "a".repeat(32),
        owner: ":1.42".into(),
        process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:100:200".into(),
    }
}

#[test]
fn publication_ignores_permissive_umask_without_changing_existing_paths() {
    use super::super::lock::PublicationLock;
    use std::os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        process::CommandExt,
    };

    const CHILD: &str = "GROK_CU_PRIVATE_PUBLICATION_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        for mask in [0o000, 0o002] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "computer_use::gnome_helper::bundle::tests::publication_ignores_permissive_umask_without_changing_existing_paths",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD, "1");
            // umask is process-wide. Change it only in the forked child before
            // exec, never in this parallel test process. No allocation here.
            unsafe {
                child.pre_exec(move || {
                    libc::umask(mask);
                    Ok(())
                });
            }
            let result = child.output().unwrap();
            assert!(
                result.status.success(),
                "umask {mask:04o}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return;
    }

    for use_lock in [true, false] {
        let f = Fixture::new();
        let lock = use_lock.then(|| PublicationLock::acquire(&f.bundle.root).unwrap());
        f.install();
        for path in [&f.root, &f.bundle.root, &f.bundle.current] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        for entry in fs::read_dir(&f.bundle.current).unwrap() {
            assert_eq!(
                entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(f.bundle.inspect().unwrap(), Installation::Current);
        drop(lock);
    }

    let f = Fixture::new();
    fs::DirBuilder::new().mode(0o750).create(&f.root).unwrap();
    let lock = PublicationLock::acquire(&f.bundle.root).unwrap();
    assert_eq!(
        fs::metadata(&f.root).unwrap().permissions().mode() & 0o777,
        0o750
    );
    drop(lock);
    fs::set_permissions(&f.bundle.root, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(PublicationLock::acquire(&f.bundle.root).is_err());
    assert!(f.bundle.install(false, &shell()).is_err());
    assert_eq!(
        fs::metadata(&f.bundle.root).unwrap().permissions().mode() & 0o777,
        0o775
    );
    assert!(!f.bundle.current.exists());
}

#[test]
fn reading_missing_installation_has_no_filesystem_effect() {
    let f = Fixture::new();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Missing);
    assert!(!f.root.exists());
}
#[test]
fn embedded_publication_is_exact_and_requires_a_new_shell() {
    let f = Fixture::new();
    f.install();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Current);
    for (name, bytes) in FILES {
        assert_eq!(fs::read(f.bundle.path().join(name)).unwrap(), *bytes);
    }
    assert!(f.bundle.restart_required(&shell()));
    let next_owner = ShellStamp {
        owner: ":1.43".into(),
        ..shell()
    };
    assert!(
        f.bundle.restart_required(&next_owner),
        "same process can reconnect"
    );
    let next_bus = ShellStamp {
        bus: "b".repeat(32),
        ..shell()
    };
    assert!(
        f.bundle.restart_required(&next_bus),
        "bus restart is not a GJS restart"
    );
    let next_process = ShellStamp {
        process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:100:201".into(),
        ..shell()
    };
    assert!(
        !f.bundle.restart_required(&next_process),
        "new process even with reused PID"
    );
    assert!(f.bundle.install(false, &next_owner).is_err());
    assert!(!f.bundle.journal.exists());
    assert!(!f.bundle.pending.exists());
    assert!(!f.bundle.previous.exists());
}
#[test]
fn explicit_repair_replaces_only_managed_files_and_resets_restart_requirement() {
    let f = Fixture::new();
    f.install();
    fs::write(f.bundle.path().join("policy.js"), "broken").unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Modified);
    let next = ShellStamp {
        owner: ":1.43".into(),
        ..shell()
    };
    f.bundle.install(true, &next).unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Current);
    assert!(f.bundle.restart_required(&next));
}
#[test]
fn journal_recovers_all_publication_checkpoints_only_on_explicit_repair() {
    for step in 0..6 {
        let f = Fixture::new();
        f.install();
        write_new(&f.bundle.journal, OWNER).unwrap();
        if step >= 1 {
            fs::create_dir(&f.bundle.pending).unwrap();
        }
        if step >= 2 {
            write_new(&f.bundle.pending.join(MARKER), OWNER).unwrap();
        }
        if step >= 3 {
            write_new(&f.bundle.pending.join("policy.js"), b"partial").unwrap();
        }
        if step >= 4 {
            fs::rename(&f.bundle.current, &f.bundle.previous).unwrap();
        }
        if step >= 5 {
            fs::rename(&f.bundle.pending, &f.bundle.current).unwrap();
        }
        assert_eq!(
            f.bundle.inspect().unwrap(),
            Installation::RecoveryRequired,
            "step {step}"
        );
        assert!(f.bundle.install(false, &shell()).is_err());
        f.bundle.install(true, &shell()).unwrap();
        assert_eq!(
            f.bundle.inspect().unwrap(),
            Installation::Current,
            "step {step}"
        );
    }
}
#[test]
fn foreign_or_extra_files_are_never_adopted_or_removed() {
    for slot in ["current", "pending", "previous"] {
        let f = Fixture::new();
        let path = match slot {
            "current" => &f.bundle.current,
            "pending" => &f.bundle.pending,
            _ => &f.bundle.previous,
        };
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("keep.txt"), "user data").unwrap();
        assert_eq!(f.bundle.inspect().unwrap(), Installation::Conflict);
        assert!(f.bundle.install(true, &shell()).is_err());
        assert_eq!(
            fs::read_to_string(path.join("keep.txt")).unwrap(),
            "user data"
        );
    }
    let f = Fixture::new();
    f.install();
    fs::write(f.bundle.current.join("custom.js"), "user data").unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Conflict);
    assert!(f.bundle.install(true, &shell()).is_err());
}
#[test]
fn journal_cannot_own_foreign_current_or_unexpected_staging_content() {
    for current in [true, false] {
        let f = Fixture::new();
        fs::create_dir_all(&f.bundle.root).unwrap();
        write_new(&f.bundle.journal, OWNER).unwrap();
        let path = if current {
            &f.bundle.current
        } else {
            &f.bundle.pending
        };
        fs::create_dir(path).unwrap();
        fs::write(path.join("unknown"), "preserve").unwrap();
        assert_eq!(f.bundle.inspect().unwrap(), Installation::Conflict);
        assert!(f.bundle.install(true, &shell()).is_err());
        assert_eq!(
            fs::read_to_string(path.join("unknown")).unwrap(),
            "preserve"
        );
    }
}
#[test]
fn corrupt_or_missing_restart_identity_is_not_current_or_ready() {
    let f = Fixture::new();
    f.install();
    fs::remove_file(f.bundle.current.join(RESTART)).unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Modified);
    assert!(f.bundle.restart_required(&shell()));
    fs::write(f.bundle.current.join(RESTART), "{}").unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Modified);
}
#[test]
fn symlinks_and_traversal_are_rejected_without_changing_targets() {
    let f = Fixture::new();
    f.install();
    fs::remove_file(f.bundle.current.join("policy.js")).unwrap();
    let foreign = f.root.join("foreign");
    fs::write(&foreign, "keep").unwrap();
    std::os::unix::fs::symlink(&foreign, f.bundle.current.join("policy.js")).unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Conflict);
    assert!(f.bundle.install(true, &shell()).is_err());
    assert_eq!(fs::read_to_string(&foreign).unwrap(), "keep");
    assert!(Bundle::new(f.root.join("../elsewhere")).inspect().is_err());
    assert!(Bundle::new(PathBuf::from("relative")).inspect().is_err());
    let link = f.root.join("redirect");
    std::os::unix::fs::symlink(&f.bundle.root, &link).unwrap();
    assert!(Bundle::new(link).inspect().is_err());
}
#[test]
fn malformed_shell_stamp_and_oversized_files_fail_closed() {
    let f = Fixture::new();
    assert!(f
        .bundle
        .install(
            false,
            &ShellStamp {
                bus: "".into(),
                owner: ":1.1".into(),
                process: shell().process
            }
        )
        .is_err());
    assert!(!f.root.exists());
    f.install();
    fs::write(f.bundle.current.join("policy.js"), vec![0; 1024 * 1024 + 1]).unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Modified);
    fs::write(&f.bundle.journal, "malformed transaction").unwrap();
    assert_eq!(f.bundle.inspect().unwrap(), Installation::Conflict);
    assert!(f.bundle.install(true, &shell()).is_err());
}
