use super::*;
use std::time::SystemTime;

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("grok-cu-extension-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        let root = fs::canonicalize(root).unwrap();
        Self { root }
    }
    fn export_root(&self) -> PathBuf {
        self.root.join("app-owned").join("browser-extension")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn embedded_bundle_has_the_manifest_identity_and_only_production_entries() {
    let info = info().unwrap();
    assert_eq!(info.extension_id, "bgegbabkegkanjbmjbeaockdijnbkjgi");
    assert_eq!(info.version, "1.1.0");
    assert_eq!(info.minimum_browser_version, "140");
    let names: Vec<_> = files::FILES.iter().map(|(name, _)| *name).collect();
    let unique: BTreeSet<_> = names.iter().copied().collect();
    assert_eq!(names, unique.into_iter().collect::<Vec<_>>());
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("_locales/"))
            .count(),
        15
    );
    assert!(names
        .iter()
        .all(|name| !name.contains("test") && !name.contains("fixtures") && !name.contains("..")));
    assert_eq!(info.file_count, names.len());
    assert_eq!(info.sha256.len(), 64);
    // Prove that this executable contains the current checkout's exact bundle,
    // not only an internally consistent older set of embedded locale bytes.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/computer-use-extension");
    for (name, bytes) in files::FILES {
        assert_eq!(fs::read(source.join(name)).unwrap().as_slice(), *bytes);
    }
    println!(
        "embedded extension info: {}",
        serde_json::to_string(&info).unwrap()
    );
}
#[test]
fn export_publishes_exact_embedded_bytes_without_source_paths_or_runtime_boot() {
    let f = Fixture::new();
    let exported = export_at(&f.export_root()).unwrap();
    let path = PathBuf::from(&exported.path);
    assert!(path.starts_with(f.export_root()));
    verify(&path, &exported.info).unwrap();
    for (name, bytes) in files::FILES {
        assert_eq!(fs::read(path.join(name)).unwrap().as_slice(), *bytes);
    }
    assert!(!f.root.join("runtime").exists());
    assert!(!f.root.join("agent-home").exists());
    assert!(!fs::read_dir(f.export_root()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".pending-")));
}
#[test]
fn repeat_export_reuses_only_the_exact_original_contents_without_rewriting() {
    let f = Fixture::new();
    let first = export_at(&f.export_root()).unwrap();
    let file = Path::new(&first.path).join("manifest.json");
    let original: SystemTime = fs::metadata(&file).unwrap().modified().unwrap();
    let second = export_at(&f.export_root()).unwrap();
    assert_eq!(first.path, second.path);
    assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), original);
}
#[test]
fn an_unowned_even_empty_destination_is_not_replaced() {
    let f = Fixture::new();
    let target = destination(&f.export_root(), &info().unwrap());
    fs::create_dir_all(&target).unwrap();
    assert!(export_at(&f.export_root()).is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}
#[test]
fn changed_extension_or_marker_is_not_overwritten() {
    for name in ["sw.js", MARKER] {
        let f = Fixture::new();
        let exported = export_at(&f.export_root()).unwrap();
        let file = Path::new(&exported.path).join(name);
        fs::write(&file, b"foreign changes to retain").unwrap();
        assert!(export_at(&f.export_root()).is_err());
        assert_eq!(fs::read(file).unwrap(), b"foreign changes to retain");
    }
}
#[test]
fn extra_file_or_empty_directory_invalidates_an_existing_export() {
    for directory in [false, true] {
        let f = Fixture::new();
        let exported = export_at(&f.export_root()).unwrap();
        let path = Path::new(&exported.path).join("not-in-the-bundle");
        if directory {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, b"retain").unwrap();
        }
        assert!(export_at(&f.export_root()).is_err());
        assert!(path.exists());
    }
}
#[test]
fn another_original_publisher_blocks_export_instead_of_racing_it() {
    let f = Fixture::new();
    let first = export_at(&f.export_root()).unwrap();
    let lock = safety::open(&f.export_root().join(".publication.lock"), false).unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(export_at(&f.export_root()).unwrap_err().contains("busy"));
    drop(lock);
    assert_eq!(export_at(&f.export_root()).unwrap().path, first.path);
}
#[test]
fn publication_never_replaces_a_foreign_destination() {
    let f = Fixture::new();
    let staging = f.root.join("staging");
    let foreign = f.root.join("foreign");
    fs::create_dir(&staging).unwrap();
    fs::create_dir(&foreign).unwrap();
    fs::write(staging.join("ours"), b"new").unwrap();
    assert!(safety::publish(&staging, &foreign).is_err());
    assert!(staging.join("ours").exists());
    assert_eq!(fs::read_dir(foreign).unwrap().count(), 0);
}
#[test]
fn hardlinked_code_is_never_admitted_as_an_owned_export() {
    let f = Fixture::new();
    let exported = export_at(&f.export_root()).unwrap();
    let original = Path::new(&exported.path).join("sw.js");
    fs::hard_link(&original, f.root.join("unrelated.js")).unwrap();
    assert!(export_at(&f.export_root()).is_err());
    assert_eq!(
        fs::read(original).unwrap(),
        fs::read(f.root.join("unrelated.js")).unwrap()
    );
}
fn link_directory(from: &Path, to: &Path) {
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(from, to).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(from, to).unwrap();
}
#[test]
fn linked_ancestor_is_rejected_without_writing_outside_app_root() {
    let f = Fixture::new();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    link_directory(&outside, &f.root.join("app-owned"));
    assert!(export_at(&f.export_root()).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn a_linked_locale_directory_cannot_be_opened_as_the_export() {
    let f = Fixture::new();
    let exported = export_at(&f.export_root()).unwrap();
    let locales = Path::new(&exported.path).join("_locales");
    let outside = f.root.join("outside-locales");
    fs::rename(&locales, &outside).unwrap();
    link_directory(&outside, &locales);
    assert!(export_at(&f.export_root()).is_err());
    assert!(outside.join("en/messages.json").exists());
}
#[test]
fn traversal_or_relative_app_paths_never_publish() {
    let f = Fixture::new();
    for path in [PathBuf::from("relative"), f.root.join("..").join("escape")] {
        assert!(export_at(&path).is_err());
    }
}
