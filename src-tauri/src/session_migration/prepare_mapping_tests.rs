use super::*;
use crate::session_migration::{engine, prepare, prepare_mapping};

#[test]
fn override_and_worker_keep_equal_cwds_in_different_source_groups() {
    let fixture = Fixture::new(0);
    let mut roots = Vec::new();
    for id in ["a", "b"] {
        let root = fixture.root.join(id);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("rollout.jsonl"), concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"same\",\"cwd\":\"/same/repo\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"u1\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n"
        )).unwrap();
        roots.push((Provider::Codex, root.to_string_lossy().into_owned()));
    }
    let scan = crate::session_migration::scan::scan_start(roots).unwrap();
    let first = &scan.candidates[0];
    let (preview_id, details) =
        engine::preview(&first.candidate_id, Some(&first.source_revision)).unwrap();
    let selection =
        crate::session_migration::scan_selection::freeze_current(&scan.scan_id, scan.epoch)
            .unwrap();
    let batch = prepare::start(
        &scan.scan_id,
        scan.epoch,
        &selection.selection_id,
        vec![prepare::PrepareOverride {
            candidate_id: first.candidate_id.clone(),
            preview_id,
            leaf_id: details.active_leaf.unwrap(),
            selection: details.default_selection,
        }],
    )
    .unwrap();
    assert_eq!(wait_terminal(&batch.batch_id).prepared, 2);
    let rows = prepare::mappings(&batch.batch_id, None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].source, rows[1].source);
    assert_ne!(rows[0].root_id, rows[1].root_id);
    assert!(rows
        .iter()
        .all(|row| row.suggested_target.as_deref() == Some("/same/repo")));
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    assert_eq!(prepare_mapping::load(&conn).unwrap().len(), 2);
    let validated = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    assert!(engine::plan_from_validated_prepare(validated, rows).is_ok());
}

#[test]
fn mapping_summary_survives_restart_and_legacy_batch_uses_fixed_preview() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    assert_eq!(wait_terminal(&batch.batch_id).prepared, 2);
    let before = prepare::mappings(&batch.batch_id, None).unwrap();
    assert_eq!(before.len(), 1);
    assert!(before[0].source.is_some());
    assert!(before[0].suggested_target.is_none());
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    assert_eq!(prepare_mapping::load(&conn).unwrap().len(), 2);
    prepare::reset_runtime();
    engine::reset_runtime();
    std::fs::remove_dir_all(fixture.root.join("source")).unwrap();
    let cached = prepare::mappings(&batch.batch_id, None).unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(cached).unwrap()
    );
    conn.execute("DROP TABLE mapping_subjects", []).unwrap();
    let legacy = prepare::mappings(&batch.batch_id, None).unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(legacy).unwrap()
    );
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mapping_subjects')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!exists, "reading old batches must not migrate them");
}

#[test]
fn corrupt_or_stale_summary_is_not_silently_replaced() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    let original: String = conn
        .query_row("SELECT json FROM mapping_subjects", [], |row| row.get(0))
        .unwrap();
    conn.execute("UPDATE mapping_subjects SET json='not json'", [])
        .unwrap();
    assert!(prepare::mappings(&batch.batch_id, None).is_err());
    conn.execute(
        "UPDATE mapping_subjects SET json=?1,preview_id='wrong'",
        [&original],
    )
    .unwrap();
    assert_eq!(
        prepare::mappings(&batch.batch_id, None).unwrap_err().code,
        ErrorCode::PlanStale
    );
    conn.execute("UPDATE mapping_subjects SET preview_id=(SELECT preview_id FROM items),json=json_set(json,'$.source.sourceId','wrong')", []).unwrap();
    assert_eq!(
        prepare::mappings(&batch.batch_id, None).unwrap_err().code,
        ErrorCode::PlanStale
    );
}

#[test]
fn forged_cached_cwd_cannot_authorize_a_plan() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE mapping_subjects SET json=json_set(json,'$.cwd','/invented')",
        [],
    )
    .unwrap();
    let rows = prepare::mappings(&batch.batch_id, None).unwrap();
    assert_eq!(rows[0].suggested_target.as_deref(), Some("/invented"));
    let validated = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(
        engine::plan_from_validated_prepare(validated, rows)
            .unwrap_err()
            .code,
        ErrorCode::MappingConflict
    );
    assert!(crate::session_migration::catalog::latest_plan_row(
        &crate::session_migration::catalog::open_catalog().unwrap()
    )
    .unwrap()
    .is_none());
}

#[test]
fn summary_uses_normalized_cwd_and_not_scan_hint() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE items SET input_json=json_set(input_json,'$.candidate.cwd','/scan-only')",
        [],
    )
    .unwrap();
    assert!(prepare::mappings(&batch.batch_id, None).unwrap()[0]
        .suggested_target
        .is_none());
    conn.execute("DELETE FROM mapping_subjects", []).unwrap();
    assert!(prepare::mappings(&batch.batch_id, None).unwrap()[0]
        .suggested_target
        .is_none());
}
