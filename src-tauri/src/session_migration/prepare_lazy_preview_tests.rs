//! Exercise the real batch worker and durable handoff after display deferral.

use super::*;
use crate::session_migration::{catalog, engine, job_control, jobs, prepare};

#[test]
fn deferred_worker_preview_survives_restart_and_commits_every_event_once() {
    let fixture = Fixture::new(1);
    let source = fixture.root.join("source/00000.json");
    let messages: Vec<_> = (0..128)
        .map(|index| serde_json::json!({"role":"user", "content":format!("batch event {index}")}))
        .collect();
    let original = serde_json::to_vec(&messages).unwrap();
    std::fs::write(&source, &original).unwrap();
    let batch = start_batch(&fixture);
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.state, PrepareBatchState::Completed);
    assert_eq!((done.prepared, done.failed, done.active), (1, 0, 0));
    let inputs = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(inputs.preview_ids.len(), 1);
    assert_eq!(
        inputs.selection_set.entries[0].selection.event_ids.len(),
        128
    );
    let preview_id = inputs.preview_ids[0].clone();
    prepare::reset_runtime();
    engine::reset_runtime();

    let (first, cursor) = engine::preview_events_page(&preview_id, None, Some(100), None).unwrap();
    assert_eq!(first.len(), 100);
    let (last, end) =
        engine::preview_events_page(&preview_id, cursor.as_deref(), Some(100), None).unwrap();
    assert_eq!(last.len(), 28);
    assert!(end.is_none());
    for (index, event) in first.iter().chain(&last).enumerate() {
        assert_eq!(event.text, format!("batch event {index}"));
        assert!(event.default_selected && event.selectable && !event.subagent);
    }
    let mappings = prepare::mappings(&batch.batch_id, None).unwrap();
    let inputs = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let plan = engine::plan_from_validated_prepare(inputs, mappings).unwrap();
    let job = engine::commit(&plan.plan_id, &plan.plan_hash, "lazy-batch-worker").unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let done = engine::job_get(&job.job_id).unwrap();
    assert_eq!(done.state, jobs::JobState::Completed);
    assert_eq!(done.item_count, 1);
    let item = &done.items[0];
    let import_id = item.import_id.as_deref().unwrap();
    let (first, cursor, total) =
        engine::events_page(import_id, None, None, Some(100), None).unwrap();
    let (last, end, total_again) =
        engine::events_page(import_id, None, cursor.as_deref(), Some(100), None).unwrap();
    assert_eq!(
        (total, total_again, first.len(), last.len()),
        (128, 128, 100, 28)
    );
    assert!(end.is_none());
    let events: Vec<crate::session_migration::types::ImportedEvent> = first
        .into_iter()
        .chain(last)
        .map(|event| serde_json::from_value(event).unwrap())
        .collect();
    let ids: std::collections::HashSet<_> = events.iter().map(|event| event.id()).collect();
    assert_eq!(ids.len(), 128);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.searchable_text(), format!("batch event {index}"));
    }
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut query = conn
        .prepare(
            "SELECT phase,COUNT(*) FROM receipts WHERE job_id=?1 GROUP BY phase ORDER BY phase",
        )
        .unwrap();
    let phases = query
        .query_map([&job.job_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        phases,
        ["committed", "linked", "prepared", "published", "staged"]
            .map(|phase| (phase.to_string(), 1))
    );
    assert_eq!(std::fs::read(source).unwrap(), original);
}
