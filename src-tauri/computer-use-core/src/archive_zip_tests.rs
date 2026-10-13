//! Regression boundaries for pinned runtime ZIPs; no source runtime is executed.

use super::*;
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn archive(directories: &[&str], files: &[(&str, &[u8])], links: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for directory in directories {
        writer.add_directory(*directory, options).unwrap();
    }
    for (name, contents) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(contents).unwrap();
    }
    for (name, target) in links {
        writer.add_symlink(*name, *target, options).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn policy() -> ZipPolicy {
    ZipPolicy {
        expected_root: "root".into(),
        max_entries: 8,
        max_files: 4,
        max_file_bytes: 128,
        max_total_bytes: 256,
        max_archive_bytes: 4096,
        max_path_chars: 80,
        max_compression_ratio: 20.0,
        extract_allowlist: None,
        allowed_symlinks: BTreeMap::new(),
    }
}

fn reject_preserving_previous(bytes: &[u8], policy: &ZipPolicy, code: &str) {
    let root = std::env::temp_dir().join(format!("grok-cu-zip-bounds-{}", uuid::Uuid::new_v4()));
    let dest = root.join("out");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("previous"), b"validated runtime").unwrap();
    let result = extract_zip(bytes, &dest, policy, None).unwrap_err();
    assert_eq!(result.code, code, "{result:?}");
    assert_eq!(
        fs::read(dest.join("previous")).unwrap(),
        b"validated runtime"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1, "no staging leak");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn readonly_archive_resources_support_repeated_bundle_copies() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("cu-zip-permissions-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    for (mode, expected) in [
        (0o444, 0o644),
        (0o555, 0o755),
        (0o640, 0o640),
        (0o6755, 0o755),
    ] {
        let payload = b"fixed archive resource";
        let mut bytes = archive(&[], &[("root/resource", payload)], &[]);
        // Set the actual central-directory mode, including bits stripped by the
        // test ZIP writer. This matches the upstream shader cache's 0100444.
        let header = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[header + 38..header + 42]
            .copy_from_slice(&(((0o100000 | mode) as u32) << 16).to_le_bytes());
        let dest = root.join(format!("mode-{mode:o}"));
        extract_zip(&bytes, &dest, &policy(), None).unwrap();
        let source = dest.join("resource");
        assert_eq!(
            fs::metadata(&source).unwrap().permissions().mode() & 0o7777,
            expected
        );
        assert_eq!(fs::read(&source).unwrap(), payload);

        // Tauri uses fs::copy and retains source permissions. Subsequent
        // build-script executions must be able to replace that same resource.
        let bundled = root.join(format!("bundled-{mode:o}"));
        fs::copy(&source, &bundled).unwrap();
        fs::copy(&source, &bundled).unwrap();
        assert_eq!(fs::read(&bundled).unwrap(), payload);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn directory_entries_have_a_separate_bound() {
    let bytes = archive(
        &["root/", "root/a/", "root/b/"],
        &[("root/node", b"node")],
        &[],
    );
    let mut policy = policy();
    policy.max_entries = 3;
    reject_preserving_previous(&bytes, &policy, "oversize");
}

#[test]
fn root_and_directory_paths_cannot_hide_payloads_or_links() {
    for name in ["root", "root/", "root/sub/"] {
        let bytes = archive(&[], &[(name, b"hidden payload")], &[]);
        reject_preserving_previous(&bytes, &policy(), "path");
    }
    let bytes = archive(&[], &[], &[("root/", "outside")]);
    reject_preserving_previous(&bytes, &policy(), "symlink");
}

#[test]
fn directory_duplicates_and_file_directory_aliases_are_rejected() {
    let bytes = archive(&["root/sub/", "root/SUB/"], &[], &[]);
    reject_preserving_previous(&bytes, &policy(), "duplicate_path");
    let bytes = archive(&["root/sub/"], &[("root/SUB", b"file")], &[]);
    reject_preserving_previous(&bytes, &policy(), "duplicate_path");
}

#[test]
fn skipped_files_still_consume_every_quota() {
    let bytes = archive(
        &["root/"],
        &[("root/node", b"node"), ("root/skip", &[0; 129])],
        &[],
    );
    let mut policy = policy();
    policy.extract_allowlist = Some(vec!["node".into()]);
    reject_preserving_previous(&bytes, &policy, "oversize");
    policy.max_file_bytes = 256;
    policy.max_total_bytes = 128;
    reject_preserving_previous(&bytes, &policy, "oversize");
    policy.max_total_bytes = 256;
    policy.max_files = 1;
    reject_preserving_previous(&bytes, &policy, "oversize");
    policy.max_files = 4;
    policy.max_compression_ratio = 0.01;
    reject_preserving_previous(&bytes, &policy, "oversize");
}

#[test]
fn skipped_file_crc_failure_does_not_replace_previous_destination() {
    let payload = b"unique skipped archive content";
    let mut bytes = archive(&[], &[("root/node", b"node"), ("root/skip", payload)], &[]);
    let offset = bytes
        .windows(payload.len())
        .position(|value| value == payload)
        .unwrap();
    bytes[offset] ^= 1;
    let mut policy = policy();
    policy.extract_allowlist = Some(vec!["node".into()]);
    reject_preserving_previous(&bytes, &policy, "truncated");
}

#[test]
fn valid_directory_entries_replace_only_after_validation() {
    let root = std::env::temp_dir().join(format!("grok-cu-zip-bounds-{}", uuid::Uuid::new_v4()));
    let dest = root.join("out");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("previous"), b"previous").unwrap();
    let bytes = archive(&["root/", "root/sub/"], &[("root/sub/node", b"node")], &[]);
    assert_eq!(extract_zip(&bytes, &dest, &policy(), None).unwrap(), 1);
    assert_eq!(fs::read(dest.join("sub/node")).unwrap(), b"node");
    assert!(!dest.join("previous").exists());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_path_segments_and_path_quota_are_rejected_before_joining() {
    for name in [
        "root//file",
        "root\\\\file",
        "",
        "root/./file",
        "root/../file",
    ] {
        assert_eq!(validate_zip_name(name, 80).unwrap_err().code, "path");
    }
    assert_eq!(validate_zip_name("./root/sub/", 80).unwrap(), "root/sub");
    let name = format!("root/{}", "a".repeat(80));
    assert_eq!(validate_zip_name(&name, 80).unwrap_err().code, "path");
}

#[test]
fn exact_zip_boundaries_succeed_and_each_one_over_preserves_previous() {
    let bytes = archive(
        &["root/"],
        &[("root/12345678", &[1; 4]), ("root/b", &[2; 4])],
        &[],
    );
    let mut policy = policy();
    policy.max_entries = 3;
    policy.max_files = 2;
    policy.max_file_bytes = 4;
    policy.max_total_bytes = 8;
    policy.max_path_chars = 8;
    policy.max_archive_bytes = bytes.len() as u64;
    let root = std::env::temp_dir().join(format!("grok-cu-zip-bounds-{}", uuid::Uuid::new_v4()));
    assert_eq!(
        extract_zip(&bytes, &root.join("out"), &policy, None).unwrap(),
        2
    );
    fs::remove_dir_all(root).unwrap();
    for field in 0..6 {
        let mut reduced = policy.clone();
        match field {
            0 => reduced.max_entries -= 1,
            1 => reduced.max_files -= 1,
            2 => reduced.max_file_bytes -= 1,
            3 => reduced.max_total_bytes -= 1,
            4 => reduced.max_path_chars -= 1,
            _ => reduced.max_archive_bytes -= 1,
        }
        reject_preserving_previous(
            &bytes,
            &reduced,
            if field == 4 { "path" } else { "oversize" },
        );
    }
}

#[test]
fn selected_crc_failure_after_first_file_does_not_publish_partial_tree() {
    let payload = b"selected second file with corrupt CRC";
    let mut bytes = archive(&[], &[("root/a", b"first file"), ("root/b", payload)], &[]);
    let offset = bytes
        .windows(payload.len())
        .position(|value| value == payload)
        .unwrap();
    bytes[offset] ^= 1;
    reject_preserving_previous(&bytes, &policy(), "truncated");
}
