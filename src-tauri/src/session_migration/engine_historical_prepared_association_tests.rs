//! A genuine prepare batch and origin receipt over the pinned historical source.
use super::*;
use crate::session_migration::{prepare, prepared_association::*, scan, scan_selection};
use std::time::{Duration, Instant};

#[path = "engine_historical_prepared_mixed_tests.rs"]
mod mixed;

fn prepared_request() -> PreparedAssociationRequest {
    let source = sources::get(&open_catalog().unwrap(), INSTALLATION).unwrap();
    let started = scan::begin(vec![ScanSourcePath {
        provider: "cursor".into(),
        path: source.root_path,
        source_id: Some(INSTALLATION.into()),
        location_id: None,
        host: Some("local".into()),
        profile: source.profile,
    }])
    .unwrap();
    scan::run(&started.scan_id).unwrap();
    let done = scan_pump(&started.scan_id, started.epoch).unwrap();
    assert!(done.complete && done.issues.is_empty());
    assert_eq!(done.candidates.len(), 1);
    let frozen = scan_selection::freeze_current(&started.scan_id, started.epoch).unwrap();
    let batch = prepare::start(
        &started.scan_id,
        started.epoch,
        &frozen.selection_id,
        vec![],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        let status = prepare::status(&batch.batch_id).unwrap();
        if status.state == prepare::PrepareBatchState::Completed
            && !prepare::running_for_test(&batch.batch_id)
        {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "prepare failed or timed out: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let item = prepare::items_page(&batch.batch_id, None, Some(1))
        .unwrap()
        .items
        .remove(0);
    PreparedAssociationRequest {
        version: 1,
        batch_id: batch.batch_id,
        expected_state_version: status.state_version,
        candidate_id: item.candidate_id,
        preview_id: item.preview_id.unwrap(),
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
    }
}

fn plan_request(request: &PreparedAssociationRequest) -> PreparedAssociationPlanRequest {
    let review = request.inspect().unwrap();
    assert_eq!(review.batch_id, request.batch_id);
    assert_eq!(review.state_version, request.expected_state_version);
    assert_eq!(review.candidate_id, request.candidate_id);
    PreparedAssociationPlanRequest {
        version: 1,
        batch_id: request.batch_id.clone(),
        expected_state_version: request.expected_state_version,
        mappings: vec![],
        identity_associations: vec![AssociationConfirmation {
            version: 1,
            import_id: IMPORT.into(),
            expected_revision: REVISION.into(),
            preview_id: request.preview_id.clone(),
            review_digest: review.review.review_digest,
        }],
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_prepared_association_preserves_origin_and_publishes() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let request = prepared_request();
    let old = text(IMPORT, REVISION);
    let confirmed = plan_request(&request).build().unwrap();
    assert_eq!(
        prepare::status(&request.batch_id).unwrap().state,
        prepare::PrepareBatchState::Planned
    );
    reset_runtime();
    prepare::reset_runtime();
    let restored = recovery().unwrap().plan.unwrap();
    assert_eq!(restored.plan_id, confirmed.plan_id);
    assert_eq!(restored.preview_count, 1);
    assert_eq!(
        restored.association_targets,
        confirmed.identity_associations
    );
    let queued = commit(
        &confirmed.plan_id,
        &confirmed.plan_hash,
        "synthetic-prepared-association",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let done = job_get(&queued.job_id).unwrap();
    assert_eq!(done.state, JobState::Completed, "{done:?}");
    assert_eq!(done.items[0].import_id.as_deref(), Some(IMPORT));
    assert_ne!(done.items[0].revision_id.as_deref(), Some(REVISION));
    assert_eq!(text(IMPORT, REVISION), old);
    assert_eq!(list_entries(&open_catalog().unwrap()).unwrap().len(), 1);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_prepared_association_refuses_stale_reviews_and_missing_receipt() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let request = prepared_request();
    let intended = plan_request(&request);
    let conn = open_catalog().unwrap();
    let before = progress_tests::raw_catalog(&conn);
    let mut stale = request.clone();
    stale.expected_state_version += 1;
    assert_eq!(stale.inspect().unwrap_err().code, ErrorCode::PlanStale);
    let mut invalid = intended.clone();
    invalid.expected_state_version += 1;
    assert_eq!(invalid.build().unwrap_err().code, ErrorCode::PlanStale);
    let mut invalid = intended.clone();
    invalid.identity_associations[0].review_digest = "0".repeat(64);
    assert_eq!(invalid.build().unwrap_err().code, ErrorCode::PlanStale);
    assert_eq!(progress_tests::raw_catalog(&conn), before);
    let confirmed = intended.build().unwrap();
    let batch = prepare::open_existing(&request.batch_id).unwrap();
    batch.execute("DELETE FROM plan_receipt", []).unwrap();
    assert!(commit(
        &confirmed.plan_id,
        &confirmed.plan_hash,
        "missing-prepared-receipt"
    )
    .is_err());
    reset_runtime();
    assert!(recovery().unwrap().plan.is_none());
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
}
