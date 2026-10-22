//! Bounded faults after a real archive write; no host disk exhaustion or ACL changes.

use std::cell::RefCell;
use std::io::{self, ErrorKind};

use super::*;
use crate::session_migration::{adapters, jobs, search, test_home::IsolatedAppHome};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    Write,
    Flush,
    Sync,
}

thread_local! {
    static FAULT: RefCell<Option<(Point, ErrorKind)>> = const { RefCell::new(None) };
}

struct FaultGuard;

impl FaultGuard {
    fn arm(point: Point, kind: ErrorKind) -> Self {
        FAULT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "fault must not replace an armed point"
            );
            *slot.borrow_mut() = Some((point, kind));
        });
        Self
    }

    fn assert_consumed(&self) {
        FAULT.with(|slot| assert!(slot.borrow().is_none(), "fault site was not reached"));
    }
}

impl Drop for FaultGuard {
    fn drop(&mut self) {
        FAULT.with(|slot| *slot.borrow_mut() = None);
    }
}

pub(super) fn maybe_fail(point: Point) -> io::Result<()> {
    FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.as_ref().is_some_and(|(armed, _)| *armed == point) {
            return Ok(());
        }
        let (_, kind) = slot.take().unwrap();
        Err(io::Error::new(kind, "bounded synthetic preview I/O fault"))
    })
}

fn adapter() -> AdapterOutput {
    let bytes = serde_json::to_vec(&serde_json::json!([
        {"role": "user", "content": "complete synthetic first event ".repeat(1024)},
        {"role": "assistant", "content": "complete synthetic second event"},
    ]))
    .unwrap();
    adapters::generic::normalize_bytes(&bytes, "synthetic.json").unwrap()
}

fn build(adapter: &AdapterOutput, id: &str) -> PreparedRevisionOutput {
    let mut builder = PreparedRevisionBuilder::new_in_preview_dir(id).unwrap();
    for event in &adapter.graph.events {
        builder.push_event(event).unwrap();
    }
    builder.finalize(adapter, &serde_json::json!({})).unwrap()
}

fn failure_discards_only_owned_preview_and_retries(point: Point, kind: ErrorKind) {
    let _home = IsolatedAppHome::acquire("prepare-io-fault");
    let adapter = adapter();
    assert_eq!(adapter.graph.events.len(), 2);
    let old = build(&adapter, &Uuid::new_v4().to_string());
    let old_hashes = prepared_hashes(&old.prepared.path).unwrap();
    let id = Uuid::new_v4().to_string();
    let mut builder = PreparedRevisionBuilder::new_in_preview_dir(&id).unwrap();
    let failed_path = builder.path().to_path_buf();
    builder.push_event(&adapter.graph.events[0]).unwrap();
    assert!(
        fs::metadata(failed_path.join("events.jsonl"))
            .unwrap()
            .len()
            > 8192,
        "the fault follows a real write larger than the buffered writer"
    );
    let fault = FaultGuard::arm(point, kind);
    let error = builder
        .push_event(&adapter.graph.events[1])
        .and_then(|()| builder.finalize(&adapter, &serde_json::json!({})))
        .unwrap_err();
    fault.assert_consumed();
    drop(fault);
    assert!(
        !failed_path.exists(),
        "failed construction must not become a preview"
    );
    assert!(jobs::find_preview_dir(&id).unwrap().is_none());
    assert_eq!(prepared_hashes(&old.prepared.path).unwrap(), old_hashes);

    let retried = build(&adapter, &id);
    assert_eq!(retried.prepared.path, failed_path);
    assert_eq!(retried.event_headers.len(), adapter.graph.events.len());
    assert_eq!(
        prepared_hashes(&retried.prepared.path).unwrap(),
        retried.prepared.file_hashes
    );
    assert_eq!(
        serde_json::to_value(
            search::load_archive_events(&retried.prepared.path, &|| false).unwrap()
        )
        .unwrap(),
        serde_json::to_value(&adapter.graph.events).unwrap()
    );
    assert_eq!(prepared_hashes(&old.prepared.path).unwrap(), old_hashes);
    let expected = if kind == ErrorKind::StorageFull {
        ErrorCode::DiskSpaceLow
    } else {
        ErrorCode::SnapshotUnavailable
    };
    assert_eq!(error.code, expected, "{point:?}: {error}");
    assert_eq!(error.retry, expected.retry_kind());
}

#[test]
fn full_event_write_discards_incomplete_preview_and_retries() {
    failure_discards_only_owned_preview_and_retries(Point::Write, ErrorKind::StorageFull);
}

#[test]
fn full_event_flush_discards_incomplete_preview_and_retries() {
    failure_discards_only_owned_preview_and_retries(Point::Flush, ErrorKind::StorageFull);
}

#[test]
fn full_event_sync_discards_incomplete_preview_and_retries() {
    failure_discards_only_owned_preview_and_retries(Point::Sync, ErrorKind::StorageFull);
}

#[test]
fn denied_event_write_is_not_classified_as_disk_full() {
    failure_discards_only_owned_preview_and_retries(Point::Write, ErrorKind::PermissionDenied);
}
