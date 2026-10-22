//! Test-only, one-shot faults preserve lease publication's old-or-new boundary.

use std::cell::RefCell;
use std::io::{self, ErrorKind};

use super::*;
use crate::session_migration::{
    adapters, jobs, prepared_builder::PreparedRevisionBuilder, test_home::IsolatedAppHome,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session_migration) enum Point {
    Write,
    Sync,
    Rename,
}

#[derive(Debug)]
struct Fault {
    point: Point,
    kind: ErrorKind,
    expected_bytes: Vec<u8>,
}

thread_local! {
    static FAULT: RefCell<Option<Fault>> = const { RefCell::new(None) };
}

pub(in crate::session_migration) struct FaultGuard;

impl FaultGuard {
    pub(in crate::session_migration) fn arm(
        point: Point,
        kind: ErrorKind,
        expected: &PreviewLease,
    ) -> Self {
        FAULT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "fault must not replace an armed point"
            );
            *slot.borrow_mut() = Some(Fault {
                point,
                kind,
                expected_bytes: serde_json::to_vec(expected).unwrap(),
            });
        });
        Self
    }

    pub(in crate::session_migration) fn assert_consumed(&self) {
        FAULT.with(|slot| assert!(slot.borrow().is_none(), "fault site was not reached"));
    }
}

impl Drop for FaultGuard {
    fn drop(&mut self) {
        FAULT.with(|slot| *slot.borrow_mut() = None);
    }
}

pub(super) fn maybe_fail(point: Point, path: &Path) -> io::Result<()> {
    FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.as_ref().is_some_and(|fault| fault.point == point) {
            return Ok(());
        }
        let fault = slot.take().unwrap();
        let actual = fs::read(path).unwrap();
        if point == Point::Write {
            assert!(
                actual.is_empty(),
                "temporary file is opened before the failed write"
            );
        } else {
            assert_eq!(
                actual, fault.expected_bytes,
                "fault follows a complete real write"
            );
        }
        Err(io::Error::new(
            fault.kind,
            "bounded synthetic lease I/O fault",
        ))
    })
}

fn assert_no_temporary(dir: &Path) {
    assert!(!fs::read_dir(dir).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lease-")
    }));
}

fn lease_failure_preserves_old_bytes_and_retries(point: Point, kind: ErrorKind) {
    let _home = IsolatedAppHome::acquire("lease-io-fault");
    let adapter = adapters::generic::normalize_bytes(
        br#"[{"role":"user","content":"complete synthetic lease fixture"}]"#,
        "synthetic.json",
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut builder = PreparedRevisionBuilder::new_in_preview_dir(&id).unwrap();
    for event in &adapter.graph.events {
        builder.push_event(event).unwrap();
    }
    let output = builder.finalize(&adapter, &serde_json::json!({})).unwrap();
    let dir = &output.prepared.path;
    let hashes = jobs::prepared_hashes(dir).unwrap();
    let lease = PreviewLease {
        source_member: None,
        preview_id: id.clone(),
        candidate_id: "synthetic-candidate".into(),
        source_revision: "synthetic-source-revision".into(),
        prepare_batch_id: None,
        source_id: "synthetic-source".into(),
        root_path: "synthetic-root".into(),
        legacy_id: "synthetic-legacy".into(),
        snapshot_digest: output.prepared.digest.clone(),
        expires_unix: chrono::Utc::now().timestamp() + 3600,
        retained_by_job: false,
        file_hashes: hashes.clone(),
    };
    let fault = FaultGuard::arm(point, kind, &lease);
    let initial_error = write(dir, &lease).unwrap_err();
    fault.assert_consumed();
    drop(fault);
    assert!(
        read(dir).unwrap().is_none(),
        "failed first lease is not published"
    );
    assert_eq!(
        jobs::verify_disk_preview_integrity(dir, &id)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(jobs::prepared_hashes(dir).unwrap(), hashes);
    assert_no_temporary(dir);

    write(dir, &lease).unwrap();
    jobs::verify_disk_preview_integrity(dir, &id).unwrap();
    let lease_path = dir.join("lease.json");
    let old_bytes = fs::read(&lease_path).unwrap();
    let mut next = lease.clone();
    next.expires_unix += 3600;
    next.retained_by_job = true;
    let fault = FaultGuard::arm(point, kind, &next);
    let update_error = update(dir, |current| *current = next.clone()).unwrap_err();
    fault.assert_consumed();
    drop(fault);
    assert_eq!(fs::read(&lease_path).unwrap(), old_bytes);
    assert_eq!(jobs::prepared_hashes(dir).unwrap(), hashes);
    jobs::verify_disk_preview_integrity(dir, &id).unwrap();
    assert_no_temporary(dir);

    update(dir, |current| *current = next.clone()).unwrap();
    let new_bytes = serde_json::to_vec(&next).unwrap();
    assert_eq!(fs::read(&lease_path).unwrap(), new_bytes);
    jobs::verify_disk_preview_integrity(dir, &id).unwrap();
    assert_eq!(jobs::prepared_hashes(dir).unwrap(), hashes);
    assert_no_temporary(dir);
    let expected = if kind == ErrorKind::StorageFull {
        ErrorCode::DiskSpaceLow
    } else {
        ErrorCode::SnapshotUnavailable
    };
    for error in [initial_error, update_error] {
        assert_eq!(error.code, expected, "{point:?}: {error}");
        assert_eq!(error.retry, expected.retry_kind());
    }
}

#[test]
fn full_lease_write_preserves_old_lease_and_retries() {
    lease_failure_preserves_old_bytes_and_retries(Point::Write, ErrorKind::StorageFull);
}

#[test]
fn full_lease_sync_preserves_old_lease_and_retries() {
    lease_failure_preserves_old_bytes_and_retries(Point::Sync, ErrorKind::StorageFull);
}

#[test]
fn full_lease_rename_preserves_old_lease_and_retries() {
    lease_failure_preserves_old_bytes_and_retries(Point::Rename, ErrorKind::StorageFull);
}

#[test]
fn denied_lease_rename_is_not_classified_as_disk_full() {
    lease_failure_preserves_old_bytes_and_retries(Point::Rename, ErrorKind::PermissionDenied);
}

#[test]
fn malformed_lease_retains_non_retryable_format_error() {
    let home = IsolatedAppHome::acquire("lease-invalid-json");
    let dir = home.path.join("synthetic-invalid-lease");
    fs::create_dir(&dir).unwrap();
    let raw = b"{ invalid synthetic lease";
    fs::write(dir.join("lease.json"), raw).unwrap();
    let error = read(&dir).unwrap_err();
    assert_eq!(error.code, ErrorCode::SnapshotUnavailable);
    assert_eq!(error.retry, "none");
    assert_eq!(fs::read(dir.join("lease.json")).unwrap(), raw);
}
