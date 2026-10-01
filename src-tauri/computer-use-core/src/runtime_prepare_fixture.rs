//! Archive-layout fixtures only. These Chromium bytes are never executed.

use super::*;
use crate::archive_zip::write_test_zip;
use std::io::Cursor;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub(super) fn source_lock(target: RuntimeTarget) -> RuntimeLock {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../resources/computer-use")
        .join(target.lock_name());
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

pub(super) fn node_archive(lock: &RuntimeLock, node: &[u8]) -> Vec<u8> {
    let root = &lock.js_runtime.archive_root;
    let executable = format!("{root}/{}", lock.js_runtime.executable_relpath);
    let license = format!("{root}/LICENSE");
    let ignored = format!("{root}/unused-package-manager");
    let entries = [
        (executable.as_str(), node),
        (license.as_str(), b"license".as_slice()),
        (ignored.as_str(), b"skip".as_slice()),
    ];
    if lock.target == RuntimeTarget::WindowsX64.arch() {
        write_test_zip(&entries)
    } else {
        super::tests::gzip_tar(&entries)
    }
}

pub(super) fn chromium_executable(target: RuntimeTarget) -> &'static [u8] {
    match target {
        RuntimeTarget::WindowsX64 => b"MZ\x90\x00tiny-chrome",
        RuntimeTarget::LinuxX64 => b"\x7fELFtiny-chrome",
        RuntimeTarget::MacosArm64 | RuntimeTarget::MacosX64 => b"\xcf\xfa\xed\xfetiny-chrome",
    }
}

pub(super) fn chromium_archive(lock: &RuntimeLock, executable: &[u8]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    let root = &lock.chromium.archive_root;
    zip.start_file(
        format!("{root}/{}", lock.chromium.executable_relpath),
        options,
    )
    .unwrap();
    zip.write_all(executable).unwrap();
    zip.start_file(format!("{root}/CREDITS.html"), options)
        .unwrap();
    zip.write_all(b"credits").unwrap();
    if crate::runtime_chromium::is_macos(&lock.target) {
        let framework = format!(
            "{root}/Chromium.app/Contents/Frameworks/Chromium Framework.framework/Versions/{}",
            lock.chromium.browser_version
        );
        for path in [
            "Resources/fixture",
            "Libraries/fixture",
            "Helpers/fixture",
            "Chromium Framework",
        ] {
            zip.start_file(format!("{framework}/{path}"), options)
                .unwrap();
            zip.write_all(b"framework-layout-fixture").unwrap();
        }
        for (path, target) in crate::runtime_chromium::macos_links() {
            zip.add_symlink(format!("{root}/{path}"), target, options)
                .unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn every_target_fixture_uses_its_locked_archive_layout() {
    for target in [
        RuntimeTarget::WindowsX64,
        RuntimeTarget::MacosArm64,
        RuntimeTarget::MacosX64,
        RuntimeTarget::LinuxX64,
    ] {
        let lock = source_lock(target);
        let node = node_archive(&lock, b"native-node-fixture");
        if target == RuntimeTarget::WindowsX64 {
            let mut archive = zip::ZipArchive::new(Cursor::new(node)).unwrap();
            assert!(archive
                .by_name(&format!("{}/node.exe", lock.js_runtime.archive_root))
                .is_ok());
        } else {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(node)));
            let entries: Vec<_> = archive
                .entries()
                .unwrap()
                .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
                .collect();
            assert!(entries.contains(&format!("{}/bin/node", lock.js_runtime.archive_root)));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(chromium_archive(
            &lock,
            chromium_executable(target),
        )))
        .unwrap();
        assert!(archive
            .by_name(&format!(
                "{}/{}",
                lock.chromium.archive_root, lock.chromium.executable_relpath
            ))
            .is_ok());
        let expected_links = if crate::runtime_chromium::is_macos(target.arch()) {
            5
        } else {
            0
        };
        let mut links = 0;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            if entry.unix_mode().unwrap_or(0) & 0o170000 == 0o120000 {
                links += 1;
                let mut value = String::new();
                entry.read_to_string(&mut value).unwrap();
                let path = entry
                    .name()
                    .strip_prefix(&format!("{}/", lock.chromium.archive_root))
                    .unwrap();
                assert_eq!(
                    crate::runtime_chromium::macos_links().get(path),
                    Some(&value)
                );
            }
        }
        assert_eq!(links, expected_links, "{}", target.arch());
    }
}
