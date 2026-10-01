use super::runtime_publish::{journal_path, publish_with_rename, write_journal, Publication};
use super::*;

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("grok-cu-publish-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn tree(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("identity"), content).unwrap();
        path
    }
    fn staging(&self) -> PathBuf {
        self.tree(
            &format!(".seed-staging-{}", uuid::Uuid::new_v4()),
            "prepared",
        )
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn recovery_preserves_unowned_prefix_directories_beside_a_seed() {
    let root = Workspace::new();
    let seed = root.tree("seed", "current");
    let foreign_stage = root.tree(".seed-staging-foreign", "foreign staging");
    let foreign_backup = root.tree(".seed-backup-foreign", "foreign backup");
    recover_seed(&seed).unwrap();
    assert_eq!(
        fs::read_to_string(seed.join("identity")).unwrap(),
        "current"
    );
    assert!(
        foreign_stage.join("identity").is_file(),
        "unowned staging was deleted"
    );
    assert!(
        foreign_backup.join("identity").is_file(),
        "unowned backup was deleted"
    );
}

#[test]
fn recovery_never_promotes_an_unowned_prefix_backup() {
    let root = Workspace::new();
    let seed = root.0.join("seed");
    let backup = root.tree(".seed-backup-unowned", "foreign bytes");
    let _ = recover_seed(&seed);
    assert!(
        !seed.exists(),
        "foreign backup was published as the requested seed"
    );
    assert_eq!(
        fs::read_to_string(backup.join("identity")).unwrap(),
        "foreign bytes"
    );
}

#[test]
fn recovery_does_not_guess_between_unjournaled_backups() {
    let root = Workspace::new();
    let seed = root.0.join("seed");
    let first = root.tree(".seed-backup-a", "first");
    let second = root.tree(".seed-backup-b", "second");
    assert!(
        recover_seed(&seed).is_err(),
        "ambiguous legacy backups need explicit recovery"
    );
    assert!(!seed.exists());
    assert!(first.join("identity").is_file());
    assert!(second.join("identity").is_file());
}

#[test]
fn publication_installs_and_replaces_only_the_requested_seed() {
    let root = Workspace::new();
    let seed = root.0.join("seed");
    let foreign = root.tree(".seed-staging-foreign", "keep");
    for value in ["first", "second"] {
        let staging = root.staging();
        fs::write(staging.join("identity"), value).unwrap();
        publish_seed(&staging, &seed).unwrap();
        assert_eq!(fs::read_to_string(seed.join("identity")).unwrap(), value);
        assert!(!staging.exists());
        assert!(!journal_path(&seed).unwrap().exists());
        assert!(foreign.exists());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
    }
}

#[test]
fn recovery_restores_each_interrupted_precommit_state_without_installing_staging() {
    for move_old in [false, true] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let record = Publication::new(&staging, &seed).unwrap();
        write_journal(&seed, &record).unwrap();
        if move_old {
            fs::rename(&seed, record.backup(&seed).unwrap()).unwrap();
        }
        recover_seed(&seed).unwrap();
        assert_eq!(fs::read_to_string(seed.join("identity")).unwrap(), "old");
        assert!(!staging.exists());
        assert!(!journal_path(&seed).unwrap().exists());
        recover_seed(&seed).unwrap();
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }
}

#[test]
fn recovery_finishes_verified_commit_but_preserves_backup_if_live_seed_changed() {
    let root = Workspace::new();
    let seed = root.tree("seed", "old");
    let staging = root.staging();
    let record = Publication::new(&staging, &seed).unwrap();
    let backup = record.backup(&seed).unwrap();
    write_journal(&seed, &record).unwrap();
    fs::rename(&seed, &backup).unwrap();
    fs::rename(&staging, &seed).unwrap();
    fs::write(seed.join("identity"), "unexpected replacement").unwrap();
    assert_eq!(
        recover_seed(&seed).unwrap_err().code,
        "publication_recovery_required"
    );
    assert_eq!(fs::read_to_string(backup.join("identity")).unwrap(), "old");
    assert!(journal_path(&seed).unwrap().is_file());
    fs::write(seed.join("identity"), "prepared").unwrap();
    // A previous cleanup may have removed some of the exact owned backup.
    fs::remove_file(backup.join("identity")).unwrap();
    recover_seed(&seed).unwrap();
    assert!(!backup.exists());
    assert!(!journal_path(&seed).unwrap().exists());
}

#[test]
fn initial_precommit_and_failed_attempt_cleanup_can_be_recovered_without_an_old_seed() {
    for staging_exists in [false, true] {
        let root = Workspace::new();
        let seed = root.0.join("seed");
        let staging = root.staging();
        let record = Publication::new(&staging, &seed).unwrap();
        write_journal(&seed, &record).unwrap();
        if !staging_exists {
            fs::remove_dir_all(&staging).unwrap();
        }
        recover_seed(&seed).unwrap();
        assert!(!seed.exists());
        assert!(!staging.exists());
        assert!(!journal_path(&seed).unwrap().exists());
    }
}

#[test]
fn recovery_refuses_missing_modified_or_ambiguous_prior_seed_copies() {
    for kind in ["missing", "modified", "occupied"] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let record = Publication::new(&staging, &seed).unwrap();
        let backup = record.backup(&seed).unwrap();
        write_journal(&seed, &record).unwrap();
        fs::rename(&seed, &backup).unwrap();
        match kind {
            "missing" => fs::remove_dir_all(&backup).unwrap(),
            "modified" => fs::write(backup.join("identity"), "changed").unwrap(),
            _ => {
                root.tree("seed", "unexpected");
            }
        }
        let before = snapshot_seed(&root.0).unwrap();
        assert_eq!(
            recover_seed(&seed).unwrap_err().code,
            "publication_recovery_required"
        );
        assert_eq!(
            snapshot_seed(&root.0).unwrap(),
            before,
            "{kind} recovery changed evidence"
        );
    }
}

#[test]
fn journals_cannot_escape_paths_cross_seeds_or_overwrite_pending_intent() {
    for field in ["staging_name", "seed_name", "transaction", "schema"] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let record = Publication::new(&staging, &seed).unwrap();
        let mut value = serde_json::to_value(&record).unwrap();
        value[field] = serde_json::json!("../foreign");
        fs::write(
            journal_path(&seed).unwrap(),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let before = snapshot_seed(&root.0).unwrap();
        assert!(recover_seed(&seed).is_err());
        assert!(publish_seed(&staging, &seed).is_err());
        assert_eq!(snapshot_seed(&root.0).unwrap(), before);
    }
    let root = Workspace::new();
    let seed = root.tree("seed", "old");
    let staging = root.staging();
    let record = Publication::new(&staging, &seed).unwrap();
    write_journal(&seed, &record).unwrap();
    let other = root.tree("other", "foreign");
    fs::copy(journal_path(&seed).unwrap(), journal_path(&other).unwrap()).unwrap();
    assert!(recover_seed(&other).is_err());
    assert_eq!(
        fs::read_to_string(other.join("identity")).unwrap(),
        "foreign"
    );
    assert!(staging.exists());
}

#[test]
fn rollback_failure_keeps_both_errors_and_exact_recoverable_journal() {
    let root = Workspace::new();
    let seed = root.tree("seed", "old");
    let staging = root.staging();
    let mut calls = 0;
    let error = publish_with_rename(&staging, &seed, &mut |from, to| {
        calls += 1;
        match calls {
            1 => fs::rename(from, to),
            2 => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture publication denied",
            )),
            3 => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture rollback denied",
            )),
            _ => panic!("publication must not retry native mutation"),
        }
    })
    .unwrap_err();
    assert_eq!(calls, 3);
    assert_eq!(error.code, "publication_rollback_failed");
    assert!(error.message.contains("fixture publication denied"));
    assert!(error.message.contains("fixture rollback denied"));
    assert!(journal_path(&seed).unwrap().exists());
    // The caller's error cleanup can already have removed its staging.
    fs::remove_dir_all(&staging).unwrap();
    recover_seed(&seed).unwrap();
    assert_eq!(fs::read_to_string(seed.join("identity")).unwrap(), "old");
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
}

#[test]
fn failed_publication_rolls_back_without_repeating_the_failed_rename() {
    for fail_call in [1, 2] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let mut calls = 0;
        let error = publish_with_rename(&staging, &seed, &mut |from, to| {
            calls += 1;
            if calls == fail_call {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "fixture denied",
                ))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();
        assert_eq!(error.code, "io");
        assert_eq!(calls, if fail_call == 1 { 1 } else { 3 });
        assert_eq!(fs::read_to_string(seed.join("identity")).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(staging.join("identity")).unwrap(),
            "prepared"
        );
        assert!(!journal_path(&seed).unwrap().exists());
    }
}

#[test]
fn recovery_cleans_only_the_staging_marker_recorded_in_its_journal() {
    for changed in [false, true] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let owner = new_owner(&seed, &root.0, "owned fixture");
        write_staging_owner(&staging, &owner).unwrap();
        let record = Publication::new(&staging, &seed).unwrap();
        write_journal(&seed, &record).unwrap();
        let marker = crate::runtime_mutation::staging_owner_path(&staging);
        if changed {
            fs::write(&marker, "replacement owner").unwrap();
            assert!(recover_seed(&seed).is_err());
            assert!(staging.exists());
            assert!(journal_path(&seed).unwrap().exists());
            assert_eq!(fs::read_to_string(&marker).unwrap(), "replacement owner");
        } else {
            recover_seed(&seed).unwrap();
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn recovery_refuses_linked_publication_paths_without_following_them() {
    use std::os::unix::fs::symlink;
    for linked_path in ["backup", "staging", "journal", "seed"] {
        let root = Workspace::new();
        let seed = root.tree("seed", "old");
        let staging = root.staging();
        let record = Publication::new(&staging, &seed).unwrap();
        let backup = record.backup(&seed).unwrap();
        write_journal(&seed, &record).unwrap();
        let foreign = root.tree("foreign", "untouched");
        let path = match linked_path {
            "backup" => backup,
            "staging" => {
                fs::remove_dir_all(&staging).unwrap();
                staging.clone()
            }
            "seed" => {
                fs::remove_dir_all(&seed).unwrap();
                seed.clone()
            }
            _ => {
                let path = journal_path(&seed).unwrap();
                fs::remove_file(&path).unwrap();
                path
            }
        };
        symlink(&foreign, &path).unwrap();
        assert!(recover_seed(&seed).is_err(), "{linked_path}");
        assert_eq!(
            fs::read_to_string(foreign.join("identity")).unwrap(),
            "untouched"
        );
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
    }
}

#[cfg(windows)]
#[test]
fn native_backup_cleanup_failure_is_not_misreported_as_success() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = Workspace::new();
    let seed = root.tree("seed", "old");
    let staging = root.staging();
    let mut held = None;
    let mut backup = None;
    let error = publish_with_rename(&staging, &seed, &mut |from, to| {
        fs::rename(from, to)?;
        if from == seed {
            held = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(to.join("identity"))?,
            );
            backup = Some(to.to_path_buf());
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "io", "{error:?}");
    assert!(error
        .message
        .contains("remove exact journal-owned directory"));
    assert_eq!(
        fs::read_to_string(seed.join("identity")).unwrap(),
        "prepared"
    );
    assert!(backup.as_ref().unwrap().exists());
    assert!(journal_path(&seed).unwrap().exists());
    drop(held);
    recover_seed(&seed).unwrap();
    assert!(!backup.unwrap().exists());
    assert!(!journal_path(&seed).unwrap().exists());
}

#[cfg(windows)]
#[test]
fn native_windows_sharing_violation_preserves_the_prior_seed_and_allows_explicit_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = Workspace::new();
    let seed = root.tree("seed", "old");
    let staging = root.staging();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(staging.join("identity"))
        .unwrap();
    let error = publish_seed(&staging, &seed).unwrap_err();
    assert_eq!(error.code, "io", "{error:?}");
    assert!(
        error.message.contains("move prepared staging to seed"),
        "{error:?}"
    );
    assert_eq!(fs::read_to_string(seed.join("identity")).unwrap(), "old");
    drop(held);
    publish_seed(&staging, &seed).unwrap();
    assert_eq!(
        fs::read_to_string(seed.join("identity")).unwrap(),
        "prepared"
    );
}
