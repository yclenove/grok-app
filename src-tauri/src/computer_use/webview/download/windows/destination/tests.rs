//! Real file transfer, cancellation, size and non-overwrite regressions.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    directories: Vec<PathBuf>,
    destination: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cu-dl-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut directories = vec![root.clone()];
        let mut parent = root;
        for index in 0..8 {
            parent = parent.join(format!("{index}-{}", "x".repeat(40)));
            std::fs::create_dir(&parent).unwrap();
            directories.push(parent.clone());
        }
        Self {
            directories,
            destination: std::fs::canonicalize(parent).unwrap().join("中文-😀.bin"),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.destination);
        for directory in self.directories.iter().rev() {
            let _ = std::fs::remove_dir(directory);
        }
    }
}

#[test]
fn native_buffer_preserves_long_broker_path_bytes_and_retires_exact_files() {
    let fixture = Fixture::new();
    assert!(fixture.destination.to_string_lossy().encode_utf16().count() > 400);
    let native = NativeDestination::new(&fixture.destination).unwrap();
    assert!(native.buffered());
    let private_directory = native.path().parent().unwrap().to_path_buf();
    assert!(native.path().to_string_lossy().encode_utf16().count() < 260);
    let bytes = "真实下载内容 中文 😀".as_bytes();
    std::fs::write(native.path(), bytes).unwrap();
    native.copy_into(&fixture.destination, || Ok(())).unwrap();
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), bytes);
    drop(native);
    assert!(!private_directory.exists());
    assert_eq!(std::fs::read(&fixture.destination).unwrap(), bytes);
}

#[test]
fn cancellation_during_copy_removes_partial_destination_and_retains_original_buffer() {
    let fixture = Fixture::new();
    let native = NativeDestination::new(&fixture.destination).unwrap();
    std::fs::write(native.path(), vec![b'x'; 128 * 1024]).unwrap();
    let checks = AtomicUsize::new(0);
    assert!(native
        .copy_into(&fixture.destination, || {
            if checks.fetch_add(1, Ordering::SeqCst) == 2 {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        })
        .is_err());
    assert_eq!(checks.load(Ordering::SeqCst), 3);
    assert!(!fixture.destination.exists());
    assert_eq!(std::fs::metadata(native.path()).unwrap().len(), 128 * 1024);
}

#[test]
fn copying_does_not_overwrite_an_existing_destination() {
    let fixture = Fixture::new();
    let native = NativeDestination::new(&fixture.destination).unwrap();
    std::fs::write(native.path(), b"new content").unwrap();
    std::fs::write(&fixture.destination, b"existing content").unwrap();
    assert!(native.copy_into(&fixture.destination, || Ok(())).is_err());
    assert_eq!(
        std::fs::read(&fixture.destination).unwrap(),
        b"existing content"
    );
}

#[test]
fn empty_and_oversized_downloads_never_create_a_broker_file() {
    let fixture = Fixture::new();
    let native = NativeDestination::new(&fixture.destination).unwrap();
    let file = File::create(native.path()).unwrap();
    assert!(native.copy_into(&fixture.destination, || Ok(())).is_err());
    assert!(!fixture.destination.exists());
    file.set_len(grok_computer_use_core::staging::MAX_DOWNLOAD_BYTES + 1)
        .unwrap();
    drop(file);
    assert!(native.copy_into(&fixture.destination, || Ok(())).is_err());
    assert!(!fixture.destination.exists());
}
