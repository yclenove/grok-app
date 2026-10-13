//! Exact fixture cleanup with failure-time metadata. No retries or input replay.
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::{fs, io, path::Path, time::Instant};

fn inventory(root: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "reject reparse entries before cleanup"
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else {
            assert!(metadata.is_file());
        }
        entries.push(path);
        assert!(
            entries.len() <= 4096,
            "fixture cleanup inventory unexpectedly large"
        );
    }
    Ok(entries)
}

pub(super) fn remove_fixture_root(root: &Path, temporary: &Path) -> io::Result<()> {
    let exact = root.canonicalize()?;
    assert_eq!(exact.parent(), Some(temporary));
    let leaf = exact.file_name().unwrap().to_string_lossy();
    assert!(leaf.starts_with("grok-cu-managed-task-") || leaf.starts_with("grok-cu-app-scope-"));
    let before = inventory(root)?;
    let started = Instant::now();
    let result = fs::remove_dir_all(root);
    if let Err(error) = &result {
        // The original removal result remains the failure. Access probes only
        // acquire and close DELETE access; no disposition or content changes.
        let remaining = inventory(root).map(|entries| entries.into_iter().map(|path| {
            let access = fs::OpenOptions::new().access_mode(0x10000).share_mode(7)
                .custom_flags(0x02200000).open(&path);
            serde_json::json!({"path": path, "deleteAccessError": access.err().and_then(|error| error.raw_os_error())})
        }).collect::<Vec<_>>()).map_err(|error| error.to_string());
        println!(
            "native fixture removal failure: {}",
            serde_json::json!({
                "root": root, "rawOsError": error.raw_os_error(), "kind": format!("{:?}", error.kind()),
                "elapsedMs": started.elapsed().as_millis(), "originalEntries": before.len(), "remaining": remaining,
                "limitation": "post-error metadata/access snapshot; absence of an owner does not exclude a transient image/file lock",
            })
        );
    }
    result
}
