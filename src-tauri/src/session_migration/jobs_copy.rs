//! Bounded, cancellable copy from an immutable preview to its owned stage.

use super::super::*;

pub(super) fn copy_dir(
    src: &Path,
    dest: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<bool, ImportError> {
    #[cfg(test)]
    let _span = crate::session_migration::codec_profile::Span::start("archive-copy", 0);
    if !copy_files(src, dest, cancel, true)? {
        return Ok(false);
    }
    // This destination is the unpublished, job-owned staging directory. A
    // previous interrupted copy must not carry derived recovery files forward.
    for name in index_artifacts().filter(|name| *name != "event-index.sqlite") {
        let path = dest.join(name);
        crate::session_migration::catalog::ensure_inside_imports(&path)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_storage_error(error)),
        }
    }
    let target = dest.join("event-index.sqlite");
    let result = crate::session_migration::search::with_index_snapshot(src, cancel, |path| {
        copy_file(path, &target, cancel)
    });
    match result {
        // Ownership and index rebuilding use the same cancellation checkpoint;
        // publishing must retain its established cancelled-item result.
        Err(error) if error.code == ErrorCode::PlanStale => Ok(false),
        result => result,
    }
}

fn index_artifacts() -> impl Iterator<Item = String> {
    [
        "event-index.sqlite",
        "event-index.sqlite.tmp",
        "event-index.sqlite.bak",
    ]
    .into_iter()
    .flat_map(|base| ["", "-journal", "-wal", "-shm"].map(|suffix| format!("{base}{suffix}")))
}

fn copy_files(
    src: &Path,
    dest: &Path,
    cancel: &dyn Fn() -> bool,
    revision_root: bool,
) -> Result<bool, ImportError> {
    if cancel() {
        return Ok(false);
    }
    crate::session_migration::catalog::ensure_inside_imports(src)?;
    crate::session_migration::catalog::ensure_inside_imports(dest)?;
    fs::create_dir_all(dest).map_err(io_storage_error)?;
    for ent in fs::read_dir(src).map_err(io_storage_error)? {
        let ent = ent.map_err(io_storage_error)?;
        if cancel() {
            return Ok(false);
        }
        let from = ent.path();
        let to = dest.join(ent.file_name());
        crate::session_migration::catalog::ensure_inside_imports(&from)?;
        crate::session_migration::catalog::ensure_inside_imports(&to)?;
        // Skip mutable state before inspecting a possibly vanished directory
        // entry. Copy only the stable live index under its owner afterward.
        let file_name = ent.file_name();
        let lease_temporary = file_name.to_str().is_some_and(|name| {
            name.strip_prefix(".lease-")
                .and_then(|name| name.strip_suffix(".tmp"))
                .is_some_and(|id| Uuid::parse_str(id).is_ok())
        });
        if revision_root
            && (file_name == "lease.json"
                || file_name == crate::session_migration::search::INDEX_LOCK_FILE
                || lease_temporary
                || index_artifacts().any(|name| file_name == std::ffi::OsStr::new(&name)))
        {
            continue;
        }
        let file_type = ent.file_type().map_err(io_storage_error)?;
        if file_type.is_dir() && ent.file_name() == "blobs" {
            if !copy_files(&from, &to, cancel, false)? {
                return Ok(false);
            }
            continue;
        }
        if !file_type.is_file() {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "unexpected preview file type",
            ));
        }
        if !copy_file(&from, &to, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn copy_file(from: &Path, to: &Path, cancel: &dyn Fn() -> bool) -> Result<bool, ImportError> {
    if cancel() {
        return Ok(false);
    }
    crate::session_migration::catalog::ensure_inside_imports(from)?;
    crate::session_migration::catalog::ensure_inside_imports(to)?;
    let metadata = fs::symlink_metadata(from).map_err(io_storage_error)?;
    if !metadata.file_type().is_file() {
        return Err(ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "unexpected preview file type",
        ));
    }
    let dest = to.parent().ok_or_else(|| {
        ImportError::new(
            ErrorCode::SnapshotUnavailable,
            "copy destination has no parent",
        )
    })?;
    let available = fs2::available_space(dest).map_err(|e| {
        let error = io_storage_error(e);
        ImportError::new(
            error.code,
            format!("disk space {}: {}", dest.display(), error.message),
        )
    })?;
    require_disk_space(available, metadata.len())?;
    let mut input = fs::File::open(from).map_err(io_storage_error)?;
    let mut output = fs::File::create(to).map_err(io_storage_error)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancel() {
            return Ok(false);
        }
        let count = input.read(&mut buffer).map_err(io_storage_error)?;
        if count == 0 {
            break;
        }
        #[cfg(test)]
        super::super::tests::maybe_fail_publish_io("copy_write", to).map_err(io_storage_error)?;
        output
            .write_all(&buffer[..count])
            .map_err(io_storage_error)?;
    }
    #[cfg(test)]
    super::super::tests::maybe_fail_publish_io("copy_sync", to).map_err(io_storage_error)?;
    output.sync_all().map_err(io_storage_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indexed_fixture() -> (
        crate::session_migration::test_home::IsolatedAppHome,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        use crate::session_migration::types::{
            ContentBlock, EventBase, ImportedEvent, MessageRole, TimeOrigin,
        };
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("copy-index");
        let source = home.path.join("imports/source");
        let target = home.path.join("imports/target");
        let event = ImportedEvent::Message {
            base: EventBase {
                id: "copy-event".into(),
                ordinal: 0,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![ContentBlock::text("immutable copy body")],
            complete: true,
        };
        crate::session_migration::search::build_index(&source, &[event]).unwrap();
        (home, source, target)
    }

    #[test]
    fn copy_defers_temporary_index_artifacts_to_active_builder() {
        let (_home, source, target) = indexed_fixture();
        let original = fs::read(source.join("events.jsonl")).unwrap();
        let builder = crate::session_migration::search::PausedIndexBuilder::start(&source);
        let copied = copy_dir(&source, &target, &|| false);
        builder.finish().unwrap();
        let error = copied.expect_err("copy must not publish another builder's temporary state");
        assert_eq!(error.code, ErrorCode::SourceBusy);
        assert_eq!(error.retry, "retry");
        for name in [
            "event-index.lock",
            "event-index.sqlite.tmp",
            "event-index.sqlite.bak",
        ] {
            assert!(!target.join(name).exists(), "copied mutable state: {name}");
        }
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), original);
    }

    #[test]
    fn copy_retries_live_index_owner_without_changing_archive_bytes() {
        let (_home, source, target) = indexed_fixture();
        let original = fs::read(source.join("events.jsonl")).unwrap();
        let name = crate::session_migration::search::INDEX_LOCK_FILE;
        fs::write(source.join(name), b"synthetic coordination state").unwrap();
        let owner = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(source.join(name))
            .unwrap();
        fs2::FileExt::lock_exclusive(&owner).unwrap();
        let copied = copy_dir(&source, &target, &|| false);
        fs2::FileExt::unlock(&owner).unwrap();
        assert_eq!(copied.unwrap_err().code, ErrorCode::SourceBusy);
        assert!(copy_dir(&source, &target, &|| false).unwrap());
        assert!(!target.join(name).exists());
        assert_eq!(fs::read(target.join("events.jsonl")).unwrap(), original);
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), original);
        assert_eq!(
            fs::read(target.join("event-index.sqlite")).unwrap(),
            fs::read(source.join("event-index.sqlite")).unwrap()
        );
        assert_eq!(
            fs::read(source.join(name)).unwrap(),
            b"synthetic coordination state"
        );
    }

    #[test]
    fn copy_cleans_staged_index_remnants_but_preserves_blob_names() {
        let (_home, source, target) = indexed_fixture();
        fs::create_dir_all(source.join("blobs")).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in index_artifacts().filter(|name| name != "event-index.sqlite") {
            fs::write(target.join(&name), b"abandoned staging index state").unwrap();
            fs::write(source.join("blobs").join(&name), b"immutable asset bytes").unwrap();
        }
        assert!(copy_dir(&source, &target, &|| false).unwrap());
        for name in index_artifacts().filter(|name| name != "event-index.sqlite") {
            assert!(!target.join(&name).exists(), "staged remnant: {name}");
            assert_eq!(
                fs::read(target.join("blobs").join(&name)).unwrap(),
                b"immutable asset bytes"
            );
        }
    }

    #[test]
    fn copy_cancellation_does_not_create_a_destination() {
        let (_home, source, target) = indexed_fixture();
        assert!(!copy_dir(&source, &target, &|| true).unwrap());
        assert!(!target.exists());
    }

    #[test]
    fn copy_rejects_future_index_without_downgrading_source() {
        let (_home, source, target) = indexed_fixture();
        let path = source.join("event-index.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(connection);
        let original = fs::read(&path).unwrap();
        let error = copy_dir(&source, &target, &|| false).unwrap_err();
        assert_eq!(error.code, ErrorCode::ImportSchemaTooNew);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!target.join("event-index.sqlite").exists());
    }

    fn assert_unpublished_index(target: &Path) {
        for name in [
            "event-index.sqlite",
            "event-index.lock",
            "event-index.sqlite.tmp",
            "event-index.sqlite.bak",
        ] {
            assert!(
                !target.join(name).exists(),
                "published mutable or live index: {name}"
            );
        }
    }

    #[test]
    fn copy_legacy_index_without_sentinel_preserves_source_bytes() {
        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        let index = fs::read(source.join("event-index.sqlite")).unwrap();
        let lock = source.join(crate::session_migration::search::INDEX_LOCK_FILE);
        fs::remove_file(&lock).unwrap();
        assert!(copy_dir(&source, &target, &|| false).unwrap());
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(source.join("event-index.sqlite")).unwrap(), index);
        assert_eq!(fs::read(target.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(target.join("event-index.sqlite")).unwrap(), index);
        let created = fs::symlink_metadata(&lock).unwrap();
        assert!(created.is_file());
        assert!(!created.file_type().is_symlink());
        for name in [
            "event-index.lock",
            "event-index.sqlite.tmp",
            "event-index.sqlite.bak",
        ] {
            assert!(!target.join(name).exists(), "{name}");
        }
    }

    #[test]
    fn copy_readonly_sentinel_preserves_coordination_metadata() {
        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        let index = fs::read(source.join("event-index.sqlite")).unwrap();
        let lock = source.join(crate::session_migration::search::INDEX_LOCK_FILE);
        let lock_bytes = fs::read(&lock).unwrap();
        let original = fs::metadata(&lock).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&lock, readonly).unwrap();
        let copied = copy_dir(&source, &target, &|| false);
        let archive_after = fs::read(source.join("events.jsonl")).unwrap();
        let index_after = fs::read(source.join("event-index.sqlite")).unwrap();
        let lock_after = fs::read(&lock).unwrap();
        let still_readonly = fs::metadata(&lock).unwrap().permissions().readonly();
        fs::set_permissions(&lock, original).unwrap();

        assert!(copied.expect("readonly coordination state must not fail publication"));
        assert_eq!(archive_after, archive);
        assert_eq!(index_after, index);
        assert_eq!(lock_after, lock_bytes);
        assert!(still_readonly);
        assert_eq!(fs::read(target.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(target.join("event-index.sqlite")).unwrap(), index);
        assert!(!target
            .join(crate::session_migration::search::INDEX_LOCK_FILE)
            .exists());
    }

    #[test]
    fn copy_nonregular_sentinel_does_not_publish_index() {
        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        let index = fs::read(source.join("event-index.sqlite")).unwrap();
        let lock = source.join(crate::session_migration::search::INDEX_LOCK_FILE);
        fs::remove_file(&lock).unwrap();
        fs::create_dir(&lock).unwrap();
        for _ in 0..2 {
            let error = copy_dir(&source, &target, &|| false)
                .expect_err("a directory sentinel must not publish");
            assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
            assert_eq!(error.retry, "retry");
            assert!(error.message.contains("not a regular file"));
            assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
            assert_eq!(fs::read(source.join("event-index.sqlite")).unwrap(), index);
            assert!(fs::metadata(&lock).unwrap().is_dir());
            assert!(!target.join("event-index.sqlite").exists());
        }
    }

    #[test]
    fn copy_corrupt_index_does_not_publish_or_rewrite_source() {
        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        fs::write(source.join("event-index.sqlite"), b"not-a-sqlite").unwrap();
        let error =
            copy_dir(&source, &target, &|| false).expect_err("corrupt index must not publish");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(error.retry, "retry");
        assert_eq!(
            fs::read(source.join("event-index.sqlite")).unwrap(),
            b"not-a-sqlite"
        );
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert!(!source.join("event-index.sqlite.tmp").exists());
        assert!(!source.join("event-index.sqlite.bak").exists());
        assert_unpublished_index(&target);
    }

    #[test]
    fn copy_locked_index_is_retryable_and_publishes_same_bytes_after_release() {
        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        let connection = Connection::open(source.join("event-index.sqlite")).unwrap();
        connection.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let locked_index = fs::read(source.join("event-index.sqlite")).unwrap();
        let copied = copy_dir(&source, &target, &|| false);
        let during_index = fs::read(source.join("event-index.sqlite")).unwrap();
        let during_archive = fs::read(source.join("events.jsonl")).unwrap();
        connection.execute_batch("ROLLBACK;").unwrap();
        drop(connection);

        let error = copied.expect_err("a locked index must not publish");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(error.retry, "retry");
        assert_eq!(during_index, locked_index);
        assert_eq!(during_archive, archive);
        assert_unpublished_index(&target);
        assert!(copy_dir(&source, &target, &|| false).unwrap());
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(target.join("events.jsonl")).unwrap(), archive);
        assert_eq!(
            fs::read(target.join("event-index.sqlite")).unwrap(),
            fs::read(source.join("event-index.sqlite")).unwrap()
        );
        assert!(!target.join("event-index.lock").exists());
    }

    #[test]
    fn copy_cancel_during_owner_wait_is_not_success_and_retry_matches_index() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let (_home, source, target) = indexed_fixture();
        let archive = fs::read(source.join("events.jsonl")).unwrap();
        let builder = crate::session_migration::search::PausedIndexBuilder::start(&source);
        let temp_path = source.join("event-index.sqlite.tmp");
        let backup_path = source.join("event-index.sqlite.bak");
        let temp_bytes = fs::read(&temp_path).unwrap();
        let backup_bytes = fs::read(&backup_path).unwrap();
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert!(!source.join("event-index.sqlite").is_file());

        let (reached_tx, reached_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let armed = Arc::new(AtomicBool::new(false));
        let cancel_flag = Arc::clone(&armed);
        let source_for_copy = source.clone();
        let target_for_copy = target.clone();
        let helper = std::thread::spawn(move || {
            crate::session_migration::search::observe_revision_owner_wait(
                reached_tx,
                release_rx,
                || {
                    copy_dir(&source_for_copy, &target_for_copy, &|| {
                        cancel_flag.load(Ordering::SeqCst)
                    })
                },
            )
        });
        reached_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("contender did not reach the held revision-owner mutex");
        armed.store(true, Ordering::SeqCst);
        release_tx.send(()).unwrap();
        let copied = helper.join().expect("copy contender thread");

        assert_eq!(fs::read(&temp_path).unwrap(), temp_bytes);
        assert_eq!(fs::read(&backup_path).unwrap(), backup_bytes);
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert!(!source.join("event-index.sqlite").is_file());
        match copied {
            Ok(false) => {}
            other => panic!("revision-owner wait must cancel publication, got {other:?}"),
        }
        assert_unpublished_index(&target);

        builder.finish().unwrap();
        let index = fs::read(source.join("event-index.sqlite")).unwrap();
        assert!(copy_dir(&source, &target, &|| false).unwrap());
        assert_eq!(fs::read(source.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(source.join("event-index.sqlite")).unwrap(), index);
        assert_eq!(fs::read(target.join("events.jsonl")).unwrap(), archive);
        assert_eq!(fs::read(target.join("event-index.sqlite")).unwrap(), index);
        assert!(!target.join("event-index.lock").exists());
        assert!(!target.join("event-index.sqlite.tmp").exists());
        assert!(!target.join("event-index.sqlite.bak").exists());
    }
}
