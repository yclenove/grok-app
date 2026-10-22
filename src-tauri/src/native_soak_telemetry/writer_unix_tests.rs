use super::*;
use std::os::unix::fs::symlink;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("grok-telemetry-unix-race-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn home_replacement_after_pinning_never_creates_under_symlink_target() {
    let fixture = Fixture::new();
    let home = fixture.0.join("home");
    let moved = fixture.0.join("moved-owned-home");
    let outside = fixture.0.join("outside");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&outside).unwrap();
    let pinned = pinned_home_directories(&home).unwrap();
    // Deterministic injection precisely after parent acquisition, before the
    // production creation helper. No race probabilism or sleep is involved.
    fs::rename(&home, &moved).unwrap();
    symlink(&outside, &home).unwrap();
    assert!(TelemetryWriter::open_pinned(&home, 100, pinned).is_err());
    assert!(!outside.join("diagnostics").exists());
    assert!(moved.join("diagnostics").is_dir());
    assert_eq!(fs::read_dir(moved.join("diagnostics")).unwrap().count(), 0);
}

#[test]
fn child_open_remains_relative_to_pinned_parent_after_ancestor_replacement() {
    let fixture = Fixture::new();
    let parent = fixture.0.join("parent");
    let outside = fixture.0.join("outside");
    fs::create_dir_all(parent.join("home")).unwrap();
    fs::create_dir_all(outside.join("home")).unwrap();
    let pinned_parent = Directory::open(&parent).unwrap();
    fs::rename(&parent, fixture.0.join("moved-parent")).unwrap();
    symlink(&outside, &parent).unwrap();
    let pinned_home =
        super::super::unix_io::open_directory(&pinned_parent.file, std::ffi::OsStr::new("home"))
            .unwrap();
    let _diagnostics =
        super::super::unix_io::create_directory(&pinned_home, std::ffi::OsStr::new("diagnostics"))
            .unwrap();
    assert!(fixture.0.join("moved-parent/home/diagnostics").is_dir());
    assert!(!outside.join("home/diagnostics").exists());
}

#[test]
fn budget_scan_uses_pinned_directory_not_replacement_and_rewinds_each_scan() {
    let fixture = Fixture::new();
    let path = fixture.0.join("diagnostics");
    let outside = fixture.0.join("outside");
    fs::create_dir(&path).unwrap();
    fs::create_dir(&outside).unwrap();
    File::create(path.join("native-soak-1.jsonl"))
        .unwrap()
        .set_len(MAX_BYTES)
        .unwrap();
    let directory = Directory::open(&path).unwrap();
    fs::rename(&path, fixture.0.join("moved-diagnostics")).unwrap();
    symlink(&outside, &path).unwrap();
    assert_eq!(directory.bytes().unwrap(), MAX_BYTES);
    assert_eq!(directory.bytes().unwrap(), MAX_BYTES);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

#[test]
fn fd_budget_scan_refuses_matching_symlink_instead_of_following_it() {
    let fixture = Fixture::new();
    let diagnostics = fixture.0.join("diagnostics");
    fs::create_dir(&diagnostics).unwrap();
    let external = fixture.0.join("external");
    fs::write(&external, b"untouched").unwrap();
    symlink(&external, diagnostics.join("native-soak-1.jsonl")).unwrap();
    let directory = Directory::open(&diagnostics).unwrap();
    assert_eq!(directory.bytes().unwrap_err(), "telemetry_unsafe_file");
    assert_eq!(fs::read(external).unwrap(), b"untouched");
}
