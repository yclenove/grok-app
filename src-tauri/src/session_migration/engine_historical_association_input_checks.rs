//! Input protocol probes use the normal inspect/plan on the immutable seed.
//! Encoding a fixture is not accepted publication or authorization.
use super::*;
use crate::session_migration::{
    engine, identity_association::AssociationPlanRequest, job_control,
    job_input_association::AssociationInput,
};

pub(super) fn exercise(
    plan: &PlanState,
    confirmation: &crate::session_migration::identity_association::AssociationConfirmation,
) -> (usize, usize) {
    let job = JobRecord {
        job_id: plan.publication_protocol.new_job_id(),
        request_id: "explicit-association-input-test".into(),
        plan_hash: plan.plan_hash.clone(),
        plan_id: plan.plan_id.clone(),
        state: JobState::Queued,
        state_version: 1,
        items: Vec::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    let items = engine_commit::commit_items_for_plan(plan, &job, false).unwrap();
    assert_eq!(items.len(), 1);
    let expected = AssociationInput {
        confirmation: confirmation.clone(),
        job_id: job.job_id.clone(),
        request_id: job.request_id.clone(),
        plan_id: plan.plan_id.clone(),
        plan_hash: plan.plan_hash.clone(),
    };
    let plan_transfer = items[0].identity_association.as_ref() == Some(&expected);
    let raw_transfer = items[0].selection_set == plan.selection_set;
    // Probe the real serializer independently of the construction assertion so
    // a red run records both missing transfer and loss at the encoding boundary.
    let mut probe = items[0].clone();
    probe.identity_association = Some(expected.clone());
    probe.selection_set = plan.selection_set.clone();
    let encoded = job_control::encode_input(&probe).unwrap();
    let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        (
            plan_transfer,
            raw_transfer,
            value["version"].clone(),
            value["identityAssociation"].clone(),
        ),
        (true, true, json!(3), json!(expected)),
        "actual plan→item→durable input must retain the explicit raw association"
    );
    let restored = job_control::decode_input(&encoded, &job).unwrap();
    assert_eq!(restored.identity_association.as_ref(), Some(&expected));
    assert_eq!(restored.selection_set, plan.selection_set);
    assert_eq!(job_control::encode_input(&restored).unwrap(), encoded);
    let proof_checks =
        association_proof_checks::exercise(plan, &job, &restored, &encoded, &expected);
    reset_runtime();
    let reopened = job_control::decode_input(&encoded, &job).unwrap();
    assert_eq!(reopened.identity_association.as_ref(), Some(&expected));
    assert_eq!(reopened.selection_set, plan.selection_set);
    assert_eq!(
        job_control::inspect_input_metadata(&encoded, &job).unwrap(),
        hash::sha256_bytes(encoded.as_bytes())
    );
    // Completed-input metadata must not depend on a surviving preview. Move
    // only this isolated disposable preview and restore it before asserting.
    let prepared = reopened.prepared.as_ref().unwrap();
    let moved = prepared.path.with_file_name(Uuid::new_v4().to_string());
    std::fs::rename(&prepared.path, &moved).unwrap();
    let metadata_without_preview = job_control::inspect_input_metadata(&encoded, &job);
    let content_without_preview = job_control::decode_input(&encoded, &job);
    std::fs::rename(&moved, &prepared.path).unwrap();
    assert_eq!(
        metadata_without_preview.unwrap(),
        hash::sha256_bytes(encoded.as_bytes())
    );
    assert!(content_without_preview.is_err());
    let mut checks = 4;

    for fault in [
        "v2",
        "future",
        "missing",
        "null",
        "array",
        "empty",
        "unknown",
        "confirmation-unknown",
        "confirmation-version",
        "confirmation-missing",
        "target",
        "revision",
        "digest",
        "preview",
        "job",
        "request",
        "plan",
        "plan-hash",
        "raw-order",
        "duplicate-selection",
        "candidate",
        "installation",
        "source",
        "provider",
        "root",
        "legacy",
        "native",
        "snapshot",
        "hashes",
        "member",
        "options",
        "hidden",
        "mapping",
        "create-project",
        "unassigned",
        "explicit-target",
        "unknown-top",
    ] {
        let mut changed = value.clone();
        match fault {
            "v2" => changed["version"] = json!(2),
            "future" => changed["version"] = json!(4),
            "missing" => {
                changed
                    .as_object_mut()
                    .unwrap()
                    .remove("identityAssociation");
            }
            "null" => changed["identityAssociation"] = json!(null),
            "array" => changed["identityAssociation"] = json!([]),
            "empty" => changed["identityAssociation"] = json!({}),
            "unknown" => changed["identityAssociation"]["authorized"] = json!(true),
            "confirmation-unknown" => {
                changed["identityAssociation"]["confirmation"]["authorized"] = json!(true)
            }
            "confirmation-version" => {
                changed["identityAssociation"]["confirmation"]["version"] = json!(2)
            }
            "confirmation-missing" => {
                changed["identityAssociation"]
                    .as_object_mut()
                    .unwrap()
                    .remove("confirmation");
            }
            "target" => {
                changed["identityAssociation"]["confirmation"]["importId"] =
                    json!(format!("imp:{}", Uuid::new_v4()))
            }
            "revision" => {
                changed["identityAssociation"]["confirmation"]["expectedRevision"] =
                    json!(Uuid::new_v4().to_string())
            }
            "digest" => {
                changed["identityAssociation"]["confirmation"]["reviewDigest"] =
                    json!("0".repeat(64))
            }
            "preview" => {
                changed["identityAssociation"]["confirmation"]["previewId"] =
                    json!(Uuid::new_v4().to_string())
            }
            "job" => changed["identityAssociation"]["jobId"] = json!(Uuid::new_v4().to_string()),
            "request" => changed["identityAssociation"]["requestId"] = json!("foreign-request"),
            "plan" => changed["identityAssociation"]["planId"] = json!(Uuid::new_v4().to_string()),
            "plan-hash" => changed["identityAssociation"]["planHash"] = json!("0".repeat(64)),
            "raw-order" => changed["selectionSet"]["entries"][0]["selection"]["eventIds"]
                .as_array_mut()
                .unwrap()
                .reverse(),
            "duplicate-selection" => {
                let entry = changed["selectionSet"]["entries"][0].clone();
                changed["selectionSet"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(entry);
            }
            "candidate" => changed["candidateId"] = json!("foreign-candidate"),
            "installation" => changed["installationId"] = json!(Uuid::new_v4().to_string()),
            "source" => changed["sourceId"] = json!(Uuid::new_v4().to_string()),
            "provider" => changed["provider"] = json!("claude"),
            "root" => changed["rootPath"] = json!("synthetic-foreign-root"),
            "legacy" => changed["legacyId"] = json!("synthetic-foreign-locator"),
            "native" => {
                changed["adapter"]["graph"]["nativeSessionId"] = json!("synthetic-foreign-native")
            }
            "snapshot" => changed["previewDigest"] = json!("0".repeat(64)),
            "hashes" => changed["previewFileHashes"]["manifest.json"] = json!("0".repeat(64)),
            "member" => changed["sourceMember"] = json!({"version":0}),
            "options" => changed["options"]["rawRetention"] = json!("verbatim"),
            "hidden" => changed["options"]["identityAssociation"] = json!(null),
            "mapping" => changed["mappingProjectId"] = json!(Uuid::new_v4().to_string()),
            "create-project" => changed["createProject"] = json!(true),
            "unassigned" => changed["importUnassignedAnyway"] = json!(true),
            "explicit-target" => changed["explicitTarget"] = json!("synthetic-target"),
            "unknown-top" => changed["authorized"] = json!(true),
            _ => unreachable!(),
        }
        assert!(
            job_control::decode_input(&changed.to_string(), &job).is_err(),
            "{fault}"
        );
        checks += 1;
    }
    for key in ["version", "identityAssociation"] {
        let duplicate = format!("{{\"{key}\":{},{}", value[key], &encoded[1..]);
        assert!(
            job_control::decode_input(&duplicate, &job).is_err(),
            "duplicate {key}"
        );
        checks += 1;
    }
    for key in ["job", "request", "plan", "hash"] {
        let mut foreign = job.clone();
        match key {
            "job" => foreign.job_id = Uuid::new_v4().to_string(),
            "request" => foreign.request_id = "foreign-context".into(),
            "plan" => foreign.plan_id = Uuid::new_v4().to_string(),
            "hash" => foreign.plan_hash = "0".repeat(64),
            _ => unreachable!(),
        }
        assert!(job_control::decode_input(&encoded, &foreign).is_err());
        checks += 1;
    }

    let conn = open_catalog().unwrap();
    for registered in [false, true] {
        let result = if registered {
            job_control::enqueue_registered(job.clone(), vec![reopened.clone()])
        } else {
            job_control::enqueue(job.clone(), vec![reopened.clone()])
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::FormatUnsupported);
        assert!(jobs::get_job(&conn, &job.job_id).unwrap().is_none());
        checks += 1;
    }
    let before_unaccepted = conn.total_changes();
    let unaccepted = jobs::publish_item(&conn, &job.job_id, &reopened, &|| false).unwrap_err();
    assert_eq!(unaccepted.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), before_unaccepted);
    assert!(jobs::get_job(&conn, &job.job_id).unwrap().is_none());
    assert!(
        jobs::latest_receipt(&conn, &job.job_id, &reopened.candidate_id)
            .unwrap()
            .is_none()
    );
    checks += 1;

    // Historical malformed-job/parser probes need their own real v2 plan and
    // context. A forged signed job cannot be a positive durable-input reader.
    // The signed codec/context/refusal checks above retain the original plan.
    let historical = engine::legacy_job_fixture::historical_plan(plan);
    let plan = &historical;
    let job = JobRecord {
        job_id: plan.publication_protocol.new_job_id(),
        plan_id: plan.plan_id.clone(),
        plan_hash: plan.plan_hash.clone(),
        ..job
    };
    let mut legacy_items = engine_commit::commit_items_for_plan(plan, &job, false).unwrap();
    assert_eq!(legacy_items.len(), 1);
    let reopened = legacy_items.remove(0);
    let expected = reopened.identity_association.as_ref().unwrap().clone();
    let encoded = job_control::encode_input(&reopened).unwrap();
    let unaccepted = jobs::publish_item(&conn, &job.job_id, &reopened, &|| false).unwrap_err();
    assert_eq!(unaccepted.code, ErrorCode::PlanStale);
    assert!(unaccepted.message.contains("no durable job context"));
    assert!(
        jobs::latest_receipt(&conn, &job.job_id, &reopened.candidate_id)
            .unwrap()
            .is_none()
    );
    checks += 1;
    // These deliberately unaccepted job rows test refusal, never a grant.
    let mut malformed_job = job.clone();
    malformed_job
        .items
        .push(crate::session_migration::jobs::JobItem {
            candidate_id: reopened.candidate_id.clone(),
            title: Some("SYNTHETIC_UNACCEPTED_ASSOCIATION".into()),
            phase: "queued".into(),
            import_id: None,
            revision_id: None,
            outcome: None,
            error: None,
            mapping_project_id: None,
        });
    malformed_job.item_count = 1;
    jobs::save_job(&conn, &malformed_job).unwrap();
    conn.execute(
        "INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,'queued',?3)",
        rusqlite::params![job.job_id, reopened.candidate_id, encoded],
    )
    .unwrap();
    let disk_input = job_control::load_input(&conn, &job.job_id, &reopened.candidate_id).unwrap();
    assert_eq!(disk_input.identity_association.as_ref(), Some(&expected));
    assert_eq!(disk_input.selection_set, plan.selection_set);
    // Complete input from actual inspect/plan, not invented positive metadata.
    // Trailing JSON whitespace preserves every field and its plan context.
    // These are still unaccepted fault-job rows, not Proof v2 or publication.
    let boundary = format!("{encoded}{}", " ".repeat(16 * 1024 * 1024 - encoded.len()));
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![job.job_id, reopened.candidate_id, boundary],
    )
    .unwrap();
    assert_eq!(
        job_control::load_input(&conn, &job.job_id, &reopened.candidate_id)
            .unwrap()
            .identity_association
            .as_ref(),
        Some(&expected)
    );
    checks += 1;
    let oversized = format!("{boundary} ").into_bytes();
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![job.job_id, reopened.candidate_id, oversized],
    )
    .unwrap();
    assert_eq!(
        job_control::load_input(&conn, &job.job_id, &reopened.candidate_id)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    checks += 1;
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![job.job_id, reopened.candidate_id, encoded],
    )
    .unwrap();
    let original_job = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    let old_receipt = jobs::latest_receipt(&conn, OLD_JOB, &original_job.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let mismatch =
        job_control::validate_committed_input_metadata(&encoded, &job.job_id, &old_receipt)
            .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::PlanStale);
    assert!(mismatch
        .message
        .contains("committed receipt does not match the durable job input"));
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let refused = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(refused.state, JobState::CompletedWithIssues);
    assert_eq!(refused.items[0].phase, "failed");
    let refusal = refused.items[0].error.as_ref().unwrap();
    assert!(refusal.contains(ErrorCode::HistoryAccessUnavailable.as_str()));
    assert!(refusal.contains("publication requires the exact accepted association owner"));
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert!(
        jobs::latest_receipt(&conn, &job.job_id, &reopened.candidate_id)
            .unwrap()
            .is_none()
    );
    conn.execute("DELETE FROM job_items WHERE job_id=?1", [&job.job_id])
        .unwrap();
    conn.execute("DELETE FROM jobs WHERE job_id=?1", [&job.job_id])
        .unwrap();
    checks += 1;

    // Build a genuine mixed plan from a second isolated source, not edited plan
    // fields or an invented published entry. Ordinary items keep input v2.
    let root = crate::paths::app_data_root().join("synthetic association extra source");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("extra.jsonl"), format!("{}\n", json!({"composerId":"association-extra", "role":"user", "content":"SYNTHETIC_ORDINARY_ITEM"}))).unwrap();
    let scan = scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    let ordinary_id = preview(&candidate.candidate_id, Some(&candidate.source_revision))
        .unwrap()
        .0;
    let ordinary_plan = engine::plan(vec![ordinary_id.clone()], vec![]).unwrap();
    let mut selections = plan.selection_set.clone();
    selections
        .entries
        .push(ordinary_plan.selection_set.single_entry().unwrap().clone());
    let mixed = AssociationPlanRequest {
        version: 1,
        preview_ids: vec![confirmation.preview_id.clone(), ordinary_id.clone()],
        mappings: vec![],
        selection_set: selections,
        identity_associations: vec![confirmation.clone()],
    }
    .build()
    .unwrap();
    let mut mixed_job = job.clone();
    mixed_job.job_id = mixed.publication_protocol.new_job_id();
    mixed_job.plan_id = mixed.plan_id.clone();
    mixed_job.plan_hash = mixed.plan_hash.clone();
    let mixed_items = engine_commit::commit_items_for_plan(&mixed, &mixed_job, false).unwrap();
    assert_eq!(mixed_items.len(), 2);
    for item in &mixed_items {
        let input = job_control::encode_input(item).unwrap();
        let json: serde_json::Value = serde_json::from_str(&input).unwrap();
        if item.identity_association.is_some() {
            assert_eq!(json["version"], 3);
            assert_eq!(
                job_control::decode_input(&input, &mixed_job)
                    .unwrap()
                    .selection_set,
                item.selection_set
            );
        } else {
            assert_eq!(json["version"], 2);
            assert!(json.get("identityAssociation").is_none());
        }
    }
    assert_eq!(
        job_control::enqueue_registered(mixed_job.clone(), mixed_items)
            .unwrap_err()
            .code,
        ErrorCode::FormatUnsupported
    );
    assert!(jobs::get_job(&conn, &mixed_job.job_id).unwrap().is_none());
    (checks + 1, proof_checks)
}
