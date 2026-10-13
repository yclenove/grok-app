//! Real Windows files and owned kernel-process fixtures, not an App crash probe.

use super::*;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    temp: PathBuf,
    children: Vec<Child>,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cu-native-journal-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let app = root.join("app");
        let temp = root.join("temp");
        std::fs::create_dir(&app).unwrap();
        std::fs::create_dir(&temp).unwrap();
        Self {
            root,
            app,
            temp,
            children: Vec::new(),
        }
    }

    fn child(&mut self) -> std::sync::Arc<owner::Witness> {
        let child = Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let witness = owner::Witness::capture(child.id()).unwrap();
        self.children.push(child);
        witness
    }

    fn owners(&mut self, foreign: bool) -> Owners {
        if foreign {
            Owners {
                app_root: self.app.clone(),
                app: self.child(),
                browser: self.child(),
            }
        } else {
            Owners::capture(std::process::id(), self.app.clone()).unwrap()
        }
    }

    fn publish(&mut self, foreign: bool) -> Lease {
        let owners = self.owners(foreign);
        let buffer = PrivateBuffer::in_parent(&self.temp).unwrap();
        let lease = Lease::publish(&owners, 7, 11, buffer).unwrap();
        std::fs::write(lease.buffer_path.join("download.bin"), b"original bytes").unwrap();
        lease
    }

    fn stop(&mut self, index: usize) {
        self.children[index].kill().unwrap();
        self.children[index].wait().unwrap();
    }

    fn stop_all(&mut self) {
        for child in &mut self.children {
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
            }
            child.wait().unwrap();
        }
    }
}

fn remove_fixture(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    // Never follow even a test-created link. Directory-link tests explicitly
    // remove their link before teardown, and unknown reparse evidence is kept.
    if metadata.file_attributes() & 0x400 != 0 {
        return;
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            remove_fixture(&entry.unwrap().path());
        }
        let _ = std::fs::remove_dir(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop_all();
        remove_fixture(&self.root);
    }
}

#[test]
fn normal_joint_cleanup_retires_exact_files_and_never_revisits_replacement() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(false);
    let directory = lease.buffer_path.clone();
    assert!(lease.record_path.exists());
    lease.cleanup().unwrap();
    assert!(!directory.exists());
    assert!(!lease.record_path.exists());
    assert!(!lease.receipt_path.exists());
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("download.bin"), b"replacement").unwrap();
    lease.cleanup().unwrap();
    drop(lease);
    assert_eq!(
        std::fs::read(directory.join("download.bin")).unwrap(),
        b"replacement"
    );
}

#[test]
fn lease_drop_is_not_cleanup_and_live_app_prevents_maintenance() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(false);
    let path = lease.buffer_path.join("download.bin");
    drop(lease);
    assert!(maintain(&fixture.app).unwrap_err().contains("still live"));
    assert_eq!(std::fs::read(path).unwrap(), b"original bytes");
}

#[test]
fn app_retirement_alone_keeps_original_browser_file_then_both_retired_can_clean() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let directory = lease.buffer_path.clone();
    let record = lease.record_path.clone();
    drop(lease);
    fixture.stop(0);
    assert!(maintain(&fixture.app).unwrap_err().contains("still live"));
    assert!(directory.join("download.bin").exists());
    assert!(record.exists());
    fixture.stop(1);
    maintain(&fixture.app).unwrap();
    assert!(!directory.exists());
    assert!(!record.exists());
    maintain(&fixture.app).unwrap();
}

#[test]
fn disposition_failure_keeps_original_handle_and_identity() {
    let fixture = Fixture::new();
    let mut original = Some(safety::directory(&fixture.temp, false).unwrap());
    let identity = safety::identity(original.as_ref().unwrap(), true).unwrap();
    assert!(safety::delete(&mut original).is_err());
    assert_eq!(
        safety::identity(original.as_ref().unwrap(), true).unwrap(),
        identity
    );
    drop(original);
}

#[test]
fn writer_occupancy_rejects_cleanup_without_deleting_any_files() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(false);
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(3)
        .open(lease.buffer_path.join("download.bin"))
        .unwrap();
    assert!(lease.cleanup().is_err());
    assert!(lease.record_path.exists());
    assert!(lease.buffer_path.join(MARKER).exists());
    drop(writer);
    lease.cleanup().unwrap();
    assert!(!lease.buffer_path.exists());
}

#[test]
fn partial_retirement_receipt_recovers_after_failure_and_owner_exit() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let directory = lease.buffer_path.clone();
    let receipt = lease.receipt_path.clone();
    {
        let mut state = lease.state.lock();
        retirement::prepare(&lease, &mut state).unwrap();
        std::fs::write(directory.join("blocker.bin"), b"test-owned blocker").unwrap();
        assert!(retirement::retire(&lease, &mut state).is_err());
        assert!(state.buffer.is_some());
        assert!(state.marker.is_none());
        assert!(state.data.is_none());
        assert!(receipt.exists());
    }
    drop(lease);
    fixture.stop_all();
    assert!(maintain(&fixture.app).is_err());
    assert!(receipt.exists());
    std::fs::remove_file(directory.join("blocker.bin")).unwrap();
    maintain(&fixture.app).unwrap();
    assert!(!directory.exists());
    assert!(!receipt.exists());
}

#[test]
fn later_data_occupant_is_not_deleted_by_crash_retry() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let directory = lease.buffer_path.clone();
    {
        let mut state = lease.state.lock();
        retirement::prepare(&lease, &mut state).unwrap();
        std::fs::write(directory.join("blocker.bin"), b"block").unwrap();
        assert!(retirement::retire(&lease, &mut state).is_err());
    }
    drop(lease);
    fixture.stop_all();
    std::fs::remove_file(directory.join("blocker.bin")).unwrap();
    std::fs::write(directory.join("download.bin"), b"new occupant").unwrap();
    assert!(maintain(&fixture.app)
        .unwrap_err()
        .contains("data replaced"));
    assert_eq!(
        std::fs::read(directory.join("download.bin")).unwrap(),
        b"new occupant"
    );
}

#[test]
fn corrupt_second_candidate_deletes_nothing_from_first_valid_candidate() {
    let mut fixture = Fixture::new();
    let a = fixture.publish(true);
    let b = fixture.publish(true);
    let (valid, corrupt) = if a.record_path < b.record_path {
        (a, b)
    } else {
        (b, a)
    };
    let path = valid.buffer_path.join("download.bin");
    let corrupt_path = corrupt.record_path.clone();
    drop(valid);
    drop(corrupt);
    fixture.stop_all();
    std::fs::write(corrupt_path, b"half written").unwrap();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"original bytes");
}

#[test]
fn hardlinked_download_is_retained_with_alias_unchanged() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let alias = fixture.root.join("alias.bin");
    std::fs::hard_link(lease.buffer_path.join("download.bin"), &alias).unwrap();
    drop(lease);
    fixture.stop_all();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(std::fs::read(alias).unwrap(), b"original bytes");
}

#[test]
fn replaced_buffer_directory_and_unknown_fields_are_retained() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let directory = lease.buffer_path.clone();
    let record_path = lease.record_path.clone();
    drop(lease);
    fixture.stop_all();
    std::fs::rename(&directory, fixture.temp.join("old-original")).unwrap();
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("download.bin"), b"replacement").unwrap();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(
        std::fs::read(directory.join("download.bin")).unwrap(),
        b"replacement"
    );
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["unknown"] = true.into();
    std::fs::write(record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(maintain(&fixture.app).is_err());
}

#[test]
fn bounded_store_rejects_capacity_and_half_publication() {
    let mut fixture = Fixture::new();
    let store = Store::open(&fixture.app, true).unwrap();
    let path = store.path.clone();
    drop(store);
    for _ in 0..MAX_SLOTS {
        std::fs::write(
            path.join(format!("{}.json", uuid::Uuid::new_v4().simple())),
            [],
        )
        .unwrap();
    }
    let owners = fixture.owners(false);
    let buffer = PrivateBuffer::in_parent(&fixture.temp).unwrap();
    let directory = buffer.directory.clone();
    assert!(Lease::publish(&owners, 1, 2, buffer)
        .err()
        .unwrap()
        .contains("capacity"));
    assert!(!directory.exists());
    assert!(maintain(&fixture.app).is_err());
}

#[test]
fn original_kernel_birth_live_retirement_and_invalid_identity_are_distinct() {
    let mut fixture = Fixture::new();
    let witness = fixture.child();
    assert!(!witness.identity.retired().unwrap());
    let changed_birth = owner::Identity {
        pid: witness.identity.pid,
        created: witness.identity.created + 1,
    };
    assert!(changed_birth.retired().unwrap());
    assert!(owner::Identity { pid: 0, created: 1 }.retired().is_err());
    fixture.stop(0);
    assert!(witness.terminated());
    assert!(witness.identity.retired().unwrap());
}

#[test]
fn unsafe_paths_reparse_and_cross_root_store_are_rejected() {
    let fixture = Fixture::new();
    for path in [
        "relative",
        "C:\\temp\\..\\other",
        "C:\\temp\\.\\child",
        "C:\\temp\\file:stream",
        "C:\\temp\\trailing.",
        "C:\\temp\\trailing ",
    ] {
        assert!(safety::path(Path::new(path)).is_err(), "{path}");
    }
    let target = fixture.root.join("target");
    std::fs::create_dir(&target).unwrap();
    let link = fixture.root.join("directory-link");
    std::os::windows::fs::symlink_dir(&target, &link).unwrap();
    assert!(safety::parents(&link, false).is_err());
    std::fs::remove_dir(link).unwrap();
    let store = Store::open(&fixture.app, true).unwrap();
    let lock_path = store.path.join("store.lock");
    assert!(safety::lock(&lock_path, false).is_err());
    let owner_bytes = safety::read(&store.owner, MAX_RECORD).unwrap();
    drop(store);
    let other = fixture.root.join("other-app");
    std::fs::create_dir(&other).unwrap();
    let other_store = Store::open(&other, true).unwrap();
    let other_marker = other_store.path.join(MARKER);
    drop(other_store);
    std::fs::write(other_marker, owner_bytes).unwrap();
    assert!(maintain(&other).is_err());
}

#[test]
fn identical_record_bytes_at_a_replacement_file_id_are_not_authority() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let record_path = lease.record_path.clone();
    let data = lease.buffer_path.join("download.bin");
    let bytes = json(&lease.record).unwrap();
    drop(lease);
    fixture.stop_all();
    std::fs::remove_file(&record_path).unwrap();
    std::fs::write(record_path, bytes).unwrap();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(std::fs::read(data).unwrap(), b"original bytes");
}

#[test]
fn identical_marker_bytes_at_a_replacement_file_id_are_retained() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let marker_path = lease.buffer_path.join(MARKER);
    let data = lease.buffer_path.join("download.bin");
    let bytes = json(&lease.record.binding).unwrap();
    drop(lease);
    fixture.stop_all();
    std::fs::remove_file(&marker_path).unwrap();
    std::fs::write(marker_path, bytes).unwrap();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(std::fs::read(data).unwrap(), b"original bytes");
}

#[test]
fn incomplete_store_initialization_is_not_silently_repaired() {
    let fixture = Fixture::new();
    let parent = fixture.app.join("computer-use");
    std::fs::create_dir(&parent).unwrap();
    let store = parent.join(DIRECTORY);
    std::fs::create_dir(&store).unwrap();
    std::fs::write(store.join("store.lock"), []).unwrap();
    assert!(Store::open(&fixture.app, true).is_err());
    assert!(!store.join(MARKER).exists());
}

#[test]
fn oversized_record_or_unknown_store_child_prevents_all_deletion() {
    let mut fixture = Fixture::new();
    let lease = fixture.publish(true);
    let record_path = lease.record_path.clone();
    let data = lease.buffer_path.join("download.bin");
    drop(lease);
    fixture.stop_all();
    std::fs::write(&record_path, vec![b'x'; MAX_RECORD as usize + 1]).unwrap();
    assert!(maintain(&fixture.app).is_err());
    let unknown = record_path.parent().unwrap().join("foreign.bin");
    std::fs::write(&unknown, b"fixture foreign child").unwrap();
    assert!(maintain(&fixture.app).is_err());
    assert_eq!(std::fs::read(data).unwrap(), b"original bytes");
    assert_eq!(std::fs::read(unknown).unwrap(), b"fixture foreign child");
}
