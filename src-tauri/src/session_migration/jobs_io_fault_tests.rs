// Included in jobs::tests. Faults are thread-local, one-shot and test-only;
// production publishes still use File I/O, validation, receipts and fsync.

struct PublishIoFault {
    stage: &'static str,
    file_name: Option<&'static str>,
    remaining_successes: usize,
    kind: std::io::ErrorKind,
}

thread_local! {
    static PUBLISH_IO_FAULT: std::cell::RefCell<Option<PublishIoFault>> = const { std::cell::RefCell::new(None) };
}

struct PublishIoFaultGuard;

impl PublishIoFaultGuard {
    fn arm(stage: &'static str, kind: std::io::ErrorKind) -> Self {
        PUBLISH_IO_FAULT.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(PublishIoFault {
                stage,
                file_name: if stage.starts_with("copy_") {
                    Some("events.jsonl")
                } else {
                    None
                },
                remaining_successes: usize::from(stage == "copy_write"),
                kind,
            });
        });
        Self
    }

    fn assert_consumed(&self) {
        PUBLISH_IO_FAULT.with(|slot| assert!(slot.borrow().is_none(), "fault site not reached"));
    }
}

impl Drop for PublishIoFaultGuard {
    fn drop(&mut self) {
        PUBLISH_IO_FAULT.with(|slot| *slot.borrow_mut() = None);
    }
}

pub(super) fn maybe_fail_publish_io(stage: &str, path: &Path) -> std::io::Result<()> {
    PUBLISH_IO_FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(fault) = slot.as_mut() else {
            return Ok(());
        };
        if fault.stage != stage
            || fault
                .file_name
                .is_some_and(|name| path.file_name() != Some(name.as_ref()))
        {
            return Ok(());
        }
        if fault.remaining_successes > 0 {
            fault.remaining_successes -= 1;
            return Ok(());
        }
        let kind = slot.take().unwrap().kind;
        Err(std::io::Error::new(
            kind,
            "bounded synthetic publication I/O fault",
        ))
    })
}

fn publish_io_assert_reader(import: &str, revision: &str, expected: &str) {
    let page = crate::session_migration::reader::event_page(
        &revision_dir(import, revision).unwrap(),
        import,
        revision,
        None,
        None,
        Some(100),
        crate::session_migration::reader::PageDirection::Forward,
    )
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0]["blocks"][0]["text"], expected);
}

fn publish_io_failure_keeps_old_and_retries(stage: &'static str, kind: std::io::ErrorKind) {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("publish-io");
    let conn = open_catalog().unwrap();
    let old_item = sample_item("io-refresh", "complete old revision");
    let old = publish_item(&conn, "old-io-job", &old_item, &|| false).unwrap();
    let import = old.import_id.as_deref().unwrap();
    let old_revision = old.revision_id.as_deref().unwrap();
    let old_dir = revision_dir(import, old_revision).unwrap();
    let old_hashes = published_hashes(&old_dir).unwrap();
    let old_receipts = staged_retry_receipt_rows(&conn, "old-io-job");
    // Larger than two copy buffers, but small enough for a bounded fixture.
    let body = "new complete synthetic body ".repeat(7000);
    let item = sample_item("io-refresh", &body);
    let prepared = item.prepared.as_ref().unwrap();
    let preview_hashes = prepared_hashes(&prepared.path).unwrap();
    let job = "failed-io-job";
    let staging = staging_dir()
        .join(sha256_bytes(job.as_bytes()))
        .join(sha256_bytes(item.candidate_id.as_bytes()));
    let fault = PublishIoFaultGuard::arm(stage, kind);
    let error = publish_item(&conn, job, &item, &|| false).unwrap_err();
    fault.assert_consumed();
    drop(fault);
    let reserved = latest_receipt(&conn, job, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(reserved.import_id.as_deref(), Some(import));
    assert_eq!(reserved.previous_revision.as_deref(), Some(old_revision));
    assert_eq!(
        reserved.phase,
        if stage == "rename" {
            "staged"
        } else {
            "prepared"
        }
    );
    assert!(
        !revision_dir(import, reserved.revision_id.as_deref().unwrap())
            .unwrap()
            .exists()
    );
    if stage == "copy_write" {
        let partial = fs::metadata(staging.join("events.jsonl")).unwrap().len();
        assert_eq!(
            partial,
            64 * 1024,
            "failure must follow a successful real write"
        );
        assert!(
            partial
                < fs::metadata(prepared.path.join("events.jsonl"))
                    .unwrap()
                    .len()
        );
    }
    let receipt_prefix = staged_retry_receipt_rows(&conn, job);
    assert_eq!(published_hashes(&old_dir).unwrap(), old_hashes);
    assert_eq!(prepared_hashes(&prepared.path).unwrap(), preview_hashes);
    assert_eq!(staged_retry_receipt_rows(&conn, "old-io-job"), old_receipts);
    assert_eq!(
        find_entry(&conn, &item.installation_id, &item.provider, "io-refresh")
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(old_revision)
    );
    publish_io_assert_reader(import, old_revision, "complete old revision");
    drop(conn);
    let conn = open_catalog().unwrap();
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let retried = publish_item(&conn, job, &item, &|| false).unwrap();
    assert_eq!(retried.phase, "committed");
    assert_eq!(retried.import_id, reserved.import_id);
    assert_eq!(retried.revision_id, reserved.revision_id);
    let receipts = staged_retry_receipt_rows(&conn, job);
    assert_eq!(
        receipts.get(..receipt_prefix.len()),
        Some(receipt_prefix.as_slice())
    );
    assert_eq!(staged_retry_phases(&conn, job), STAGED_RETRY_PHASES);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM revisions", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        find_entry(&conn, &item.installation_id, &item.provider, "io-refresh")
            .unwrap()
            .unwrap()
            .current_revision,
        retried.revision_id
    );
    let new_revision = retried.revision_id.as_deref().unwrap();
    publish_io_assert_reader(import, new_revision, &body);
    publish_io_assert_reader(import, old_revision, "complete old revision");
    assert_eq!(published_hashes(&old_dir).unwrap(), old_hashes);
    assert_eq!(prepared_hashes(&prepared.path).unwrap(), preview_hashes);
    assert_eq!(staged_retry_receipt_rows(&conn, "old-io-job"), old_receipts);
    assert!(
        !staging.exists(),
        "successful retry consumes only its owned stage"
    );
    let repeated = publish_item(&conn, job, &item, &|| false).unwrap();
    assert_eq!(repeated.revision_id, retried.revision_id);
    assert_eq!(staged_retry_receipt_rows(&conn, job), receipts);
    let expected = if kind == std::io::ErrorKind::StorageFull {
        ErrorCode::DiskSpaceLow
    } else {
        ErrorCode::SnapshotUnavailable
    };
    assert_eq!(error.code, expected, "{stage}: {error}");
    assert_eq!(error.retry, expected.retry_kind());
}

#[test]
fn publish_io_mid_copy_full_retains_old_revision_and_retries() {
    publish_io_failure_keeps_old_and_retries("copy_write", std::io::ErrorKind::StorageFull);
}

#[test]
fn publish_io_copy_sync_full_does_not_publish_unsynced_data() {
    publish_io_failure_keeps_old_and_retries("copy_sync", std::io::ErrorKind::StorageFull);
}

#[test]
fn publish_io_selection_write_full_does_not_advance_receipt() {
    publish_io_failure_keeps_old_and_retries("selection_write", std::io::ErrorKind::StorageFull);
}

#[test]
fn publish_io_selection_sync_full_does_not_advance_receipt() {
    publish_io_failure_keeps_old_and_retries("selection_sync", std::io::ErrorKind::StorageFull);
}

#[test]
fn publish_io_rename_full_preserves_staged_receipt_and_retries() {
    publish_io_failure_keeps_old_and_retries("rename", std::io::ErrorKind::StorageFull);
}

#[test]
fn publish_io_permission_denied_is_not_misreported_as_disk_full() {
    publish_io_failure_keeps_old_and_retries("rename", std::io::ErrorKind::PermissionDenied);
}

#[test]
fn publish_io_error_kind_is_authoritative_not_error_text() {
    use std::io::{Error, ErrorKind};
    for (kind, message, expected) in [
        (
            ErrorKind::StorageFull,
            "permission denied",
            ErrorCode::DiskSpaceLow,
        ),
        (
            ErrorKind::PermissionDenied,
            "disk full; database is locked",
            ErrorCode::SnapshotUnavailable,
        ),
        (
            ErrorKind::Other,
            "os error 32",
            ErrorCode::SnapshotUnavailable,
        ),
        (
            ErrorKind::NotFound,
            "disk is full",
            ErrorCode::SnapshotUnavailable,
        ),
    ] {
        let result = io_storage_error(Error::new(kind, message));
        assert_eq!(result.code, expected);
        assert_eq!(result.retry, expected.retry_kind());
    }
    #[cfg(windows)]
    for (raw, expected) in [
        (39, ErrorCode::DiskSpaceLow),
        (112, ErrorCode::DiskSpaceLow),
        (32, ErrorCode::SourceBusy),
        (33, ErrorCode::SourceBusy),
        (5, ErrorCode::SnapshotUnavailable),
    ] {
        assert_eq!(
            io_storage_error(Error::from_raw_os_error(raw)).code,
            expected,
            "OS code {raw}"
        );
    }
    #[cfg(unix)]
    assert_eq!(
        io_storage_error(Error::from_raw_os_error(28)).code,
        ErrorCode::DiskSpaceLow
    );
}

#[cfg(windows)]
#[test]
fn publish_io_windows_exclusive_stage_handle_is_retryable_after_release() {
    use std::os::windows::fs::OpenOptionsExt;
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("publish-sharing");
    let conn = open_catalog().unwrap();
    let item = sample_item("sharing", "locked staging remains unpublished");
    let job = "sharing-job";
    let reserved = staged_retry_cancel_at(&conn, job, &item, "staged");
    let before = staged_retry_receipt_rows(&conn, job);
    let stage = staging_dir()
        .join(sha256_bytes(job.as_bytes()))
        .join(sha256_bytes(item.candidate_id.as_bytes()));
    let bytes = fs::read(stage.join("events.jsonl")).unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(stage.join("events.jsonl"))
        .unwrap();
    let error = publish_item(&conn, job, &item, &|| false).unwrap_err();
    assert_eq!(staged_retry_receipt_rows(&conn, job), before);
    staged_retry_assert_no_catalog_rows(&conn);
    drop(handle);
    assert_eq!(fs::read(stage.join("events.jsonl")).unwrap(), bytes);
    staged_retry_assert_finished(&conn, job, &item, &before, &reserved, STAGED_RETRY_PHASES);
    assert_eq!(error.code, ErrorCode::SourceBusy, "{error}");
    assert_eq!(error.retry, "retry");
}
