//! Receipt/CAS/ref authority contracts, split without changing test bodies.
use super::*;

#[test]
fn provenance_extension_marker_failure_rolls_back_identity_job_and_ddl() {
    let home = IsolatedAppHome::acquire("legacy-proof-ddl-rollback");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let before = entry_identity(&seed);
    let counts = table_counts();
    let pending = plan(previews, vec![]).unwrap();
    open_catalog()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER synthetic_reject_proof_marker BEFORE INSERT ON meta
         WHEN NEW.key='registration_provenance_schema_version'
         BEGIN SELECT RAISE(ABORT,'synthetic proof marker rejection'); END;",
        )
        .unwrap();
    let error = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap_err();
    assert!(
        error.message.contains("synthetic proof marker rejection"),
        "{error:?}"
    );
    assert_eq!(entry_identity(&seed), before);
    assert_eq!(table_counts(), counts);
    assert_no_provenance_extension();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn preflight_cas_refuses_revision_and_receipt_drift_without_adopting() {
    use crate::session_migration::registration_provenance::{self, InputIdentity};
    let home = IsolatedAppHome::acquire("legacy-proof-preflight-cas");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let before = entry_identity(&seed);
    let pending = plan(previews.clone(), vec![]).unwrap();
    let fixed = resident(&previews[0]);
    let input = InputIdentity {
        candidate_id: fixed.candidate_id,
        source_id: fixed.source_id,
        provider: "cursor".into(),
        native_session_id: fixed.adapter.graph.native_session_id,
        legacy_id: fixed.legacy_id,
        digest: "0".repeat(64),
    };
    let job = JobRecord {
        job_id: Uuid::new_v4().to_string(),
        request_id: "existing opaque request contract".into(),
        plan_id: pending.plan_id,
        plan_hash: pending.plan_hash,
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        item_count: 0,
        next_item_cursor: None,
        progress: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let conn = open_catalog().unwrap();
    let proof = registration_provenance::preflight(&conn, input.clone(), &job)
        .unwrap()
        .unwrap();
    assert_no_provenance_extension();
    conn.execute(
        "UPDATE entries SET current_revision=NULL WHERE import_id=?1",
        [&seed.import_id],
    )
    .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert_eq!(
        registration_provenance::accept(&tx, &input, Some(&proof))
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    tx.rollback().unwrap();
    conn.execute(
        "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
        rusqlite::params![seed.revision, seed.import_id],
    )
    .unwrap();
    let mut late = jobs::latest_receipt(
        &conn,
        &seed.job_id,
        &job_get(&seed.job_id).unwrap().items[0].candidate_id,
    )
    .unwrap()
    .unwrap();
    late.receipt_id = Uuid::new_v4().to_string();
    jobs::write_receipt(&conn, &late).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert!(registration_provenance::accept(&tx, &input, Some(&proof)).is_err());
    tx.rollback().unwrap();
    assert_eq!(entry_identity(&seed), before);
    assert_no_provenance_extension();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn existing_continuation_ref_sidecar_and_grant_survive_adoption() {
    use crate::session_migration::{continuation, mcp_stdio};
    let home = IsolatedAppHome::acquire("legacy-proof-existing-continuation");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let draft = continue_prepare(&seed.import_id, Some(&seed.revision), None, None, None).unwrap();
    let sid = continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        &Uuid::new_v4().to_string(),
        None,
    )
    .unwrap();
    let grant = grant_with_selection(&sid).unwrap();
    let side_path = continuation::sidecar_path(&sid);
    let side_hash = crate::session_migration::hash::sha256_file(&side_path).unwrap();
    let refs =
        continuation::list_refs_for_import(&open_catalog().unwrap(), &seed.import_id).unwrap();
    let refs_value = serde_json::to_value(&refs).unwrap();
    let pending = plan(previews, vec![]).unwrap();
    let accepted = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    reset_runtime();
    let restored = grant_with_selection(&sid).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&grant).unwrap()
    );
    assert_eq!(
        crate::session_migration::hash::sha256_file(&side_path).unwrap(),
        side_hash
    );
    assert_eq!(
        serde_json::to_value(
            continuation::list_refs_for_import(&open_catalog().unwrap(), &seed.import_id).unwrap()
        )
        .unwrap(),
        refs_value
    );
    let found = mcp_stdio::call_tool(
        &grant,
        "import_history_search",
        &json!({"query":"SYNTHETIC_REGISTERED_HISTORY", "limit":20}),
    )
    .unwrap();
    let body: serde_json::Value =
        serde_json::from_str(found["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(!body["hits"].as_array().unwrap().is_empty());
    // A continuation reference still prevents hiding the original creator.
    let _ = jobs::undo_job(&open_catalog().unwrap(), &accepted.job_id).unwrap();
    let blocked = jobs::undo_job(&open_catalog().unwrap(), &seed.job_id).unwrap();
    assert_eq!(blocked.items[0].phase, "conflict");
    assert!(grant.granted_selection().is_ok());
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn existing_registered_and_legacy_entries_refuse_silent_merge() {
    let home = IsolatedAppHome::acquire("legacy-pipeline-collision-control");
    let previews = cursor_source(&home, 1);
    let legacy = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let registered = seed_registration(&previews[0], resident(&previews[0]).source_id);
    assert_ne!(legacy.import_id, registered.import_id);
    let before = (
        entry_identity(&legacy),
        entry_identity(&registered),
        table_counts(),
    );
    let pending = plan(previews, vec![]).unwrap();
    let error = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::MappingConflict);
    assert_eq!(
        (
            entry_identity(&legacy),
            entry_identity(&registered),
            table_counts()
        ),
        before
    );
    reset_runtime();
    for seed in [&legacy, &registered] {
        events_window(
            &seed.import_id,
            Some(&seed.revision),
            None,
            Some(20),
            None,
            None,
        )
        .unwrap();
        assert_archive_unchanged(seed);
    }
    reset_runtime();
}
