//! Public confirmed-plan commit over the original hash-pinned historical seed.
use super::*;
use crate::session_migration::{
    job_control::ImportLock, publication_protocol::PublicationProtocol,
};
use rusqlite::params;

fn review_request(plan: &PlanState) -> AssociationRequest {
    let confirmation = &plan.identity_associations[0];
    AssociationRequest {
        version: confirmation.version,
        import_id: confirmation.import_id.clone(),
        expected_revision: confirmation.expected_revision.clone(),
        preview: plan.selection_set.single_entry().unwrap().clone(),
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_public_commit_restart_replay_without_preview() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let confirmed = confirmed_plan();
    let request = "synthetic-public-association-replay";
    let queued = commit(&confirmed.plan_id, &confirmed.plan_hash, request).unwrap();
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        PublicationProtocol::SignedOutcomeV1
    );
    let path = jobs::find_preview_dir(&confirmed.preview_ids[0])
        .unwrap()
        .unwrap();
    let offline = home.path.join("public-accepted-preview-offline");
    std::fs::rename(&path, &offline).unwrap();
    reset_runtime();
    let conn = open_catalog().unwrap();
    let before = progress_tests::raw_catalog(&conn);
    for _ in 0..2 {
        let replay = commit(&confirmed.plan_id, &confirmed.plan_hash, request).unwrap();
        assert_eq!(replay.job_id, queued.job_id);
        assert_eq!(replay.state_version, queued.state_version);
        assert_eq!(
            commit(&Uuid::new_v4().to_string(), &confirmed.plan_hash, request)
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
        assert_eq!(
            commit(&confirmed.plan_id, &"0".repeat(64), request)
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
    }
    assert_eq!(progress_tests::raw_catalog(&conn), before);
    std::fs::rename(&offline, &path).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = commit(&confirmed.plan_id, &confirmed.plan_hash, request).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    assert_eq!(job_control::run_pending().unwrap(), 0);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_public_commit_mixed_plan_uses_one_worker() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let association = confirmed_plan();
    let previews = cursor_source(&home, 2);
    let ordinary = plan(previews.clone(), vec![]).unwrap();
    let mut selections = association.selection_set.clone();
    selections.entries.extend(ordinary.selection_set.entries);
    let mut preview_ids = association.preview_ids.clone();
    preview_ids.extend(previews);
    let confirmed = AssociationPlanRequest {
        version: 1,
        preview_ids,
        mappings: vec![],
        selection_set: selections,
        identity_associations: association.identity_associations,
    }
    .build()
    .unwrap();
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let queued = commit(
        &confirmed.plan_id,
        &confirmed.plan_hash,
        "synthetic-public-mixed-association",
    )
    .unwrap();
    assert_eq!(queued.item_count, 3);
    // Only the disposable ordinary source is moved; never the immutable seed
    // or registered historical fixture. All three fixed previews remain usable.
    std::fs::rename(
        home.path.join("synthetic cursor"),
        home.path.join("ordinary-source-offline"),
    )
    .unwrap();
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items.len(), 3);
    assert!(completed.items.iter().all(|item| item.phase == "committed"));
    assert_eq!(
        completed
            .items
            .iter()
            .filter(|item| item.import_id.as_deref() == Some(IMPORT))
            .count(),
        1
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 3);
    for item in &completed.items {
        let receipt = jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id)
            .unwrap()
            .unwrap();
        assert_eq!(receipt.phase, "committed");
        if item.import_id.as_deref() == Some(IMPORT) {
            assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
            assert!(!receipt.created_entry);
            assert_eq!(text(IMPORT, item.revision_id.as_deref().unwrap()), old_text);
        } else {
            assert!(receipt.created_entry);
            let (events, next, total) = events_page(
                item.import_id.as_deref().unwrap(),
                item.revision_id.as_deref(),
                None,
                Some(20),
                None,
            )
            .unwrap();
            assert_eq!(total, 1);
            assert_eq!(events.len(), 1);
            assert!(next.is_none());
            assert!(serde_json::to_string(&events)
                .unwrap()
                .contains("SYNTHETIC_REGISTERED_HISTORY_"));
        }
    }
    assert_eq!(text(IMPORT, REVISION), old_text);
    for receipt in old_receipts {
        assert!(receipts(&conn).contains(&receipt));
    }
    assert_eq!(job_control::run_pending().unwrap(), 0);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_public_commit_busy_and_ordinary_lane_fail_closed() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let confirmed = confirmed_plan();
    let review = review_request(&confirmed);
    let request = "synthetic-public-busy-association";
    let lock = ImportLock::acquire("publish.lock").unwrap();
    let error = association_checks::checked_unchanged(&review, || {
        commit(&confirmed.plan_id, &confirmed.plan_hash, request)
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceBusy);
    drop(lock);
    let ordinary = requested_job(&confirmed, "ordinary-cannot-accept-association");
    let items = engine_commit::commit_items_for_plan(&confirmed, &ordinary, false).unwrap();
    assert_eq!(
        association_checks::checked_unchanged(&review, || {
            job_control::enqueue_registered(ordinary, items)
        })
        .unwrap_err()
        .code,
        ErrorCode::FormatUnsupported
    );
    let queued = commit(&confirmed.plan_id, &confirmed.plan_hash, request).unwrap();
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(job_get(&queued.job_id).unwrap().state, JobState::Completed);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_public_commit_rechecks_target_without_partial_acceptance() {
    for (fault, expected) in [
        ("hidden", ErrorCode::MappingConflict),
        ("revision", ErrorCode::PlanStale),
        ("old-tombstone", ErrorCode::MappingConflict),
        ("new-tombstone", ErrorCode::MappingConflict),
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let confirmed = confirmed_plan();
        let request = review_request(&confirmed);
        let review = identity_association::inspect(&request).unwrap();
        let conn = open_catalog().unwrap();
        match fault {
            "hidden" => {
                conn.execute("UPDATE entries SET hidden=1 WHERE import_id=?1", [IMPORT])
                    .unwrap();
            }
            "revision" => {
                conn.execute(
                    "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
                    params![Uuid::new_v4().to_string(), IMPORT],
                )
                .unwrap();
            }
            _ => {
                let native = if fault == "old-tombstone" {
                    &review.old_native_session_id
                } else {
                    &review.current_native_session_id
                };
                conn.execute("INSERT INTO tombstones(installation_id,provider,native_session_id,deleted_at) VALUES(?1,'cursor',?2,'synthetic-public-fault')",
                    params![INSTALLATION, native]).unwrap();
            }
        }
        let error = association_checks::checked_unchanged(&request, || {
            commit(
                &confirmed.plan_id,
                &confirmed.plan_hash,
                "public-stale-target",
            )
        })
        .unwrap_err();
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        conn.execute(
            "UPDATE entries SET hidden=0,current_revision=?1 WHERE import_id=?2",
            params![REVISION, IMPORT],
        )
        .unwrap();
        conn.execute("DELETE FROM tombstones", []).unwrap();
        let queued = commit(
            &confirmed.plan_id,
            &confirmed.plan_hash,
            "public-stale-target",
        )
        .expect("the same plan and request remain retryable after removing only the fault");
        assert_eq!(queued.state, JobState::Queued);
        assert_eq!(job_control::run_pending().unwrap(), 1);
        assert_eq!(job_get(&queued.job_id).unwrap().state, JobState::Completed);
    }
}
