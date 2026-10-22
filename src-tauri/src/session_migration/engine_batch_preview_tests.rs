//! Batch preparation must defer display-only bodies without losing the snapshot.

use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

struct Fixture {
    candidate: SourceCandidate,
    source: PathBuf,
    original: Vec<u8>,
    _home: IsolatedAppHome,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire("batch-preview-lazy-body");
        reset_runtime();
        let directory = home.path.join("source");
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("chat.json");
        let messages: Vec<_> = (0..128)
            .map(|index| {
                serde_json::json!({
                    "role": if index % 2 == 0 { "user" } else { "assistant" },
                    "content": format!("event {index:03} — 合成内容 {}", "x".repeat(1400)),
                })
            })
            .collect();
        let original = serde_json::to_vec(&messages).unwrap();
        std::fs::write(&source, &original).unwrap();
        let scan = scan_start(vec![(
            Provider::GenericFile,
            directory.to_string_lossy().into(),
        )])
        .unwrap();
        assert_eq!(scan.candidates.len(), 1);
        Self {
            candidate: scan.candidates[0].clone(),
            source,
            original,
            _home: home,
        }
    }

    fn batch_preview(&self) -> (String, PreviewDetails) {
        preview_reference(
            crate::session_migration::scan::candidate(&self.candidate.candidate_id).unwrap(),
            Some(&self.candidate.source_revision),
            Some(&Uuid::new_v4().to_string()),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        reset_runtime();
    }
}

fn metadata(details: &PreviewDetails) -> serde_json::Value {
    let mut value = serde_json::to_value(details).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("events");
    object.remove("nextCursor");
    value
}

#[test]
fn batch_preparation_reads_no_discarded_first_page_bodies() {
    let fixture = Fixture::new();
    let mut reads = Vec::new();
    for run in 0..3 {
        crate::session_migration::search::take_event_body_reads();
        let started = Instant::now();
        let (_, details) = fixture.batch_preview();
        let elapsed = started.elapsed();
        let body_reads = crate::session_migration::search::take_event_body_reads();
        println!(
            "V132_PREVIEW_MEASUREMENT {}",
            serde_json::json!({"run": run, "elapsedMicros": elapsed.as_micros(),
                "indexedBodyReads": body_reads, "eventCount": details.event_count,
                "displayEvents": details.events.len()})
        );
        assert_eq!(details.event_count, 128);
        assert_eq!(details.default_selection.event_ids.len(), 128);
        reads.push(body_reads);
        reset_runtime();
    }
    assert_eq!(std::fs::read(&fixture.source).unwrap(), fixture.original);
    assert_eq!(reads, vec![0, 0, 0], "batch workers discard display bodies");
}

#[test]
fn batch_metadata_matches_manual_and_deferred_pages_keep_the_fixed_snapshot() {
    let fixture = Fixture::new();
    crate::session_migration::search::take_event_body_reads();
    let (manual_id, manual) = preview(
        &fixture.candidate.candidate_id,
        Some(&fixture.candidate.source_revision),
    )
    .unwrap();
    assert_eq!(
        crate::session_migration::search::take_event_body_reads(),
        100
    );
    assert_eq!(manual.events.len(), 100);
    assert!(manual.events.iter().all(|event| event.truncated));
    let (batch_id, batch) = fixture.batch_preview();
    assert_eq!(metadata(&batch), metadata(&manual));
    assert_eq!(crate::session_migration::search::take_event_body_reads(), 0);
    assert!(batch.events.is_empty());
    let first_cursor = batch.next_cursor.as_deref().expect("unread first page");
    assert_eq!(
        decode_preview_cursor(
            &batch_id,
            batch.active_leaf.as_deref().unwrap(),
            Some(first_cursor)
        )
        .unwrap(),
        0
    );
    let (expected_last, manual_end) =
        preview_events_page(&manual_id, manual.next_cursor.as_deref(), Some(100), None).unwrap();
    assert_eq!(expected_last.len(), 28);
    assert!(manual_end.is_none());

    // Delayed reads must use the fixed prepared snapshot, including after restart.
    std::fs::write(&fixture.source, b"source changed after preparation").unwrap();
    reset_runtime();
    let (first, next) =
        preview_events_page(&batch_id, Some(first_cursor), Some(100), None).unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&manual.events).unwrap()
    );
    let next = next.expect("remaining 28 events");
    let (last, end) = preview_events_page(&batch_id, Some(&next), Some(100), None).unwrap();
    assert_eq!(
        serde_json::to_value(&last).unwrap(),
        serde_json::to_value(&expected_last).unwrap()
    );
    assert!(end.is_none());
    let ids: Vec<_> = first
        .iter()
        .chain(&last)
        .map(|event| event.event_id.clone())
        .collect();
    assert_eq!(ids, batch.default_selection.event_ids);
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 128);
    assert!(preview_events_page(&manual_id, Some(first_cursor), Some(100), None).is_err());
}

#[test]
fn deferred_preview_still_rejects_corrupt_index_and_snapshot_tampering() {
    let fixture = Fixture::new();
    let (preview_id, _) = fixture.batch_preview();
    let path = preview_snapshot_path(&preview_id).unwrap();
    std::fs::write(path.join("event-index.sqlite"), b"not a sqlite database").unwrap();
    assert!(preview_events_page(&preview_id, None, Some(10), None).is_err());
    let maps = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();
    assert!(plan(vec![preview_id], maps).is_err());
    assert_eq!(std::fs::read(&fixture.source).unwrap(), fixture.original);
}

#[test]
fn deferred_metadata_keeps_branch_and_non_model_selection_rules() {
    use crate::session_migration::types::{
        EventRelation, ImportedEvent, MessageRole, RelationConfidence, RelationKind,
    };
    let fixture = Fixture::new();
    let mut adapter = adapters::normalize_in_root(
        Provider::GenericFile,
        &fixture.source,
        fixture.source.parent(),
    )
    .unwrap();
    adapter.graph.events.truncate(6);
    assert_eq!(adapter.graph.events.len(), 6);
    if let ImportedEvent::Message { role, .. } = &mut adapter.graph.events[1] {
        *role = MessageRole::System;
    } else {
        panic!("synthetic fixture must contain messages");
    }
    let ids: Vec<_> = adapter
        .graph
        .events
        .iter()
        .map(|event| event.id().to_string())
        .collect();
    adapter.graph.relations = [(1, 0), (2, 1), (3, 1), (4, 3), (5, 4)]
        .map(|(child, parent)| EventRelation {
            kind: RelationKind::Parent,
            from: ids[child].clone(),
            to: ids[parent].clone(),
            confidence: RelationConfidence::Explicit,
        })
        .to_vec();
    adapter.graph.relations.push(EventRelation {
        kind: RelationKind::Subagent,
        from: ids[4].clone(),
        to: ids[3].clone(),
        confidence: RelationConfidence::Explicit,
    });
    adapter.graph.active_leaf = Some(ids[5].clone());
    adapter.validate_graph();
    let headers: Vec<_> = adapter
        .graph
        .events
        .iter()
        .map(
            |event| crate::session_migration::prepared_builder::PreparedEventHeader {
                event_id: event.id().to_string(),
                ordinal: event.base().ordinal,
                model_visible: !crate::session_migration::selection::is_never_model_visible(event),
            },
        )
        .collect();
    let prepared = crate::session_migration::jobs::prepare_revision(
        &mut adapter,
        &serde_json::json!({}),
        fixture.source.parent(),
        None,
    )
    .unwrap();
    let manual = preview_details_from_prepared(&adapter, &prepared.path, &headers, true).unwrap();
    crate::session_migration::search::take_event_body_reads();
    let batch = preview_details_from_prepared(&adapter, &prepared.path, &headers, false).unwrap();
    assert_eq!(crate::session_migration::search::take_event_body_reads(), 0);
    assert_eq!(metadata(&batch), metadata(&manual));
    assert_eq!(
        batch.default_selection.event_ids,
        vec![ids[0].clone(), ids[3].clone()]
    );
    assert_eq!(batch.leaves, vec![ids[2].clone(), ids[5].clone()]);
    assert_eq!(manual.events.len(), 5);
    assert!(!manual.events[1].selectable);
    assert!(manual.events[3..]
        .iter()
        .all(|event| event.subagent && !event.default_selected));

    // Even metadata-only preparation must fail closed on a missing parent.
    let mut missing = headers;
    missing.remove(3);
    for materialize in [false, true] {
        let error = preview_details_from_prepared(&adapter, &prepared.path, &missing, materialize)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SnapshotUnavailable);
    }
    assert_eq!(std::fs::read(&fixture.source).unwrap(), fixture.original);
}

#[test]
fn deferred_batch_still_rejects_a_source_changed_since_discovery() {
    let fixture = Fixture::new();
    std::fs::write(&fixture.source, b"[]").unwrap();
    let error = preview_reference(
        crate::session_migration::scan::candidate(&fixture.candidate.candidate_id).unwrap(),
        Some(&fixture.candidate.source_revision),
        Some(&Uuid::new_v4().to_string()),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceChanged);
    assert!(rt(|runtime| runtime.previews.is_empty()));
}
