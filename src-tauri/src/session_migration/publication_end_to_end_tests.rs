//! Real registered source -> plan -> acceptance -> worker -> restart -> Undo.
//! No positive authorization rows, seals or caller-selected protocols.
use crate::session_migration::{
    catalog, engine, errors::ErrorCode, job_control, jobs, publication_crypto::JobCrypto,
    publication_protocol::PublicationProtocol, scan, test_home::IsolatedAppHome, types::Provider,
};
use serde_json::json;

#[path = "publication_lifecycle_end_to_end_tests.rs"]
mod lifecycle;

#[path = "publication_mixed_undo_end_to_end_tests.rs"]
mod mixed_undo;

#[path = "job_action_pipeline_tests.rs"]
mod job_actions;

#[path = "publication_writer_snapshot_tests.rs"]
mod writer_snapshot;

#[path = "publication_quit_boundary_tests.rs"]
mod quit_boundaries;

#[path = "publication_concurrency_boundary_tests.rs"]
mod concurrency_boundaries;

struct ResetRuntime;
impl Drop for ResetRuntime {
    fn drop(&mut self) {
        engine::reset_runtime();
    }
}

fn finish(plan: &engine::PlanState, request: &str) -> jobs::JobRecord {
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, request).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    completed
}

fn plan(home: &IsolatedAppHome, create: bool) -> engine::PlanState {
    engine::reset_runtime();
    let source = home.path.join("source");
    let target = home.path.join("workspace");
    std::fs::create_dir_all(source.join("sessions")).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    let records = [
        json!({"type":"session_meta","payload":{"id":"signed-normal-source","cwd":target}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"正常导入与撤销 🚀"}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"Synthetic reply"}}),
    ];
    let bytes = records
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    std::fs::write(source.join("sessions/rollout.jsonl"), bytes).unwrap();
    let scanned =
        scan::scan_start(vec![(Provider::Codex, source.to_string_lossy().into())]).unwrap();
    assert_eq!(scanned.candidates.len(), 1);
    let candidate = &scanned.candidates[0];
    let (preview, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let initial = engine::plan(vec![preview], vec![]).unwrap();
    if !create {
        return initial;
    }
    let mut mappings = initial.mappings;
    assert_eq!(mappings.len(), 1);
    mappings[0].create_project = true;
    mappings[0].explicit_target = Some(target.to_string_lossy().into());
    mappings[0].unassigned = false;
    mappings[0].user_choice_version += 1;
    engine::plan(initial.preview_ids, mappings).unwrap()
}

#[test]
fn normal_source_produces_signed_plan_publication_restart_and_undo() {
    let home = IsolatedAppHome::acquire("signed-normal-source");
    let _reset = ResetRuntime;
    let plan = plan(&home, true);
    assert_eq!(
        plan.publication_protocol,
        PublicationProtocol::SignedOutcomeV1
    );
    let conn = catalog::open_catalog().unwrap();
    let (_, raw) = catalog::load_plan_json(&conn, &plan.plan_id).unwrap();
    let durable: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(durable["version"], 3);
    assert_eq!(durable["publicationOutcome"], json!({"version":1}));
    let request = "synthetic-signed-normal-source";
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, request).unwrap();
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        plan.publication_protocol
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    assert_eq!(completed.items[0].phase, "committed", "{completed:?}");
    let imported = completed.items[0].import_id.as_deref().unwrap();
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert!(receipt.created_entry && receipt.created_project);
    let project = receipt.mapping_project_id.as_deref().unwrap();
    assert!(crate::store::load_projects_snapshot()
        .unwrap()
        .iter()
        .any(|p| p.id == project));
    engine::reset_runtime();
    jobs::load_published_auth(imported, Some(revision), None).unwrap();
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    let undone = engine::undo_import_job(&queued.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
    assert!(undone.items[0].error.is_none(), "{undone:?}");
    assert!(catalog::get_entry(&conn, imported).unwrap().unwrap().hidden);
    assert!(!crate::store::load_projects_snapshot()
        .unwrap()
        .iter()
        .any(|p| p.id == project));
    assert!(home.path.join("workspace").is_dir());
    let repeated = engine::undo_import_job(&queued.job_id).unwrap();
    assert_eq!(repeated.items[0].phase, "undone");
    assert!(repeated.items[0].error.is_none());
    engine::reset_runtime();
}

#[test]
fn normal_signed_dedup_and_recovery_do_not_need_source_or_retained_preview() {
    let home = IsolatedAppHome::acquire("signed-normal-dedup");
    let _reset = ResetRuntime;
    let first_plan = plan(&home, false);
    let first = finish(&first_plan, "synthetic-normal-first");
    let second_plan = plan(&home, false);
    let duplicate = finish(&second_plan, "synthetic-normal-duplicate");
    assert_eq!(duplicate.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(duplicate.items[0].import_id, first.items[0].import_id);
    assert_eq!(duplicate.items[0].revision_id, first.items[0].revision_id);
    engine::reset_runtime();
    std::fs::rename(home.path.join("source"), home.path.join("offline-source")).unwrap();
    for preview in first_plan
        .preview_ids
        .iter()
        .chain(&second_plan.preview_ids)
    {
        if let Some(path) = jobs::find_preview_dir(preview).unwrap() {
            std::fs::rename(path, home.path.join(format!("offline-preview-{preview}"))).unwrap();
        }
    }
    let conn = catalog::open_catalog().unwrap();
    for job in [&first, &duplicate] {
        let input: String = conn
            .query_row(
                "SELECT result_json FROM job_items WHERE job_id=?1",
                [&job.job_id],
                |row| row.get(0),
            )
            .unwrap();
        let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
        engine::reset_runtime();
        let changes = conn.total_changes();
        for _ in 0..3 {
            let (recovered, version) = job_control::inspect_recovery_input(&input, |original| {
                super::recovery::recover(&conn, &job.job_id, 0, &original, &crypto)
            })
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(
                serde_json::to_value(recovered).unwrap(),
                serde_json::to_value(&job.items[0]).unwrap()
            );
            assert_eq!(version, job.state_version);
        }
        assert_eq!(conn.total_changes(), changes);
    }
    let unchanged = serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap();
    let undo_duplicate = engine::undo_import_job(&duplicate.job_id).unwrap();
    assert_eq!(
        undo_duplicate.items[0].outcome.as_deref(),
        Some("deduplicated")
    );
    assert_eq!(
        serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap(),
        unchanged
    );
    assert_eq!(
        engine::undo_import_job(&first.job_id).unwrap().items[0].phase,
        "undone"
    );
}

#[test]
fn normal_signed_explicit_reimport_after_undo_publishes_fresh_owned_revision() {
    let home = IsolatedAppHome::acquire("signed-normal-explicit-reimport");
    let _reset = ResetRuntime;
    let initial = plan(&home, false);
    let first = finish(&initial, "synthetic-reimport-first");
    let conn = catalog::open_catalog().unwrap();
    let import = first.items[0].import_id.as_deref().unwrap();
    let old_revision = first.items[0].revision_id.as_deref().unwrap();
    conn.execute(
        "UPDATE entries SET alias='SYNTHETIC_REIMPORT_ALIAS' WHERE import_id=?1",
        [import],
    )
    .unwrap();
    let original_receipt = jobs::latest_receipt(&conn, &first.job_id, &first.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let undone = engine::undo_import_job(&first.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
    let next_plan = plan(&home, false);
    let queued = engine::commit_with_reimport(
        &next_plan.plan_id,
        &next_plan.plan_hash,
        "synthetic-reimport-restored",
        true,
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let restored = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(restored.state, jobs::JobState::Completed, "{restored:?}");
    assert_eq!(restored.items[0].import_id.as_deref(), Some(import));
    assert_ne!(restored.items[0].revision_id, first.items[0].revision_id);
    assert_ne!(restored.items[0].outcome.as_deref(), Some("deduplicated"));
    let entry = catalog::get_entry(&conn, import).unwrap().unwrap();
    assert!(!entry.hidden);
    assert_eq!(entry.alias.as_deref(), Some("SYNTHETIC_REIMPORT_ALIAS"));
    assert_eq!(entry.current_revision, restored.items[0].revision_id);
    assert!(!catalog::is_tombstoned(
        &conn,
        &entry.installation_id,
        &entry.provider,
        &entry.native_session_id,
    )
    .unwrap());
    // The historical signed result and Undo ACK are untouched, not re-signed.
    assert_eq!(
        serde_json::to_value(
            jobs::latest_receipt(&conn, &first.job_id, &first.items[0].candidate_id,)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(original_receipt).unwrap()
    );
    assert_eq!(
        serde_json::to_value(engine::job_get(&first.job_id).unwrap()).unwrap(),
        serde_json::to_value(undone).unwrap()
    );
    jobs::load_published_auth(import, Some(old_revision), None).unwrap();
    let new_receipt =
        jobs::latest_receipt(&conn, &restored.job_id, &restored.items[0].candidate_id)
            .unwrap()
            .unwrap();
    assert!(!new_receipt.created_entry && !new_receipt.created_project);
    assert_eq!(new_receipt.previous_revision.as_deref(), Some(old_revision));
    engine::reset_runtime();
    jobs::load_published_auth(import, restored.items[0].revision_id.as_deref(), None).unwrap();
}

#[test]
fn normal_signed_same_path_match_and_explicit_fallback_never_own_a_project() {
    for fallback in [false, true] {
        let home = IsolatedAppHome::acquire("signed-normal-project-disposition");
        let _reset = ResetRuntime;
        let initial = plan(&home, true);
        let plan = if fallback {
            let mut mappings = initial.mappings;
            mappings[0].import_unassigned_anyway = true;
            mappings[0].user_choice_version += 1;
            engine::plan(initial.preview_ids, mappings).unwrap()
        } else {
            initial
        };
        let queued =
            engine::commit(&plan.plan_id, &plan.plan_hash, "synthetic-disposition").unwrap();
        let target = home.path.join("workspace");
        let existing = if fallback {
            assert!(target.starts_with(&home.path));
            assert!(!std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink());
            std::fs::remove_dir(&target).unwrap();
            None
        } else {
            Some(crate::store::add_project(target.to_string_lossy().into(), false).unwrap())
        };
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let completed = engine::job_get(&queued.job_id).unwrap();
        assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
        let conn = catalog::open_catalog().unwrap();
        let receipt = jobs::latest_receipt(&conn, &queued.job_id, &completed.items[0].candidate_id)
            .unwrap()
            .unwrap();
        assert!(!receipt.created_project);
        assert_eq!(
            receipt.project_decision.as_deref(),
            Some(if fallback {
                "unassigned-reported"
            } else {
                "match"
            })
        );
        let undone = engine::undo_import_job(&queued.job_id).unwrap();
        assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
        assert!(undone.items[0].error.is_none(), "{undone:?}");
        let projects = crate::store::load_projects_snapshot().unwrap();
        assert_eq!(projects.len(), usize::from(!fallback));
        if let Some(existing) = existing {
            assert_eq!(
                serde_json::to_value(&projects[0]).unwrap(),
                serde_json::to_value(existing).unwrap()
            );
        }
    }
}

#[test]
fn normal_signed_undo_refuses_coordinated_foreign_project_result_without_writes() {
    let home = IsolatedAppHome::acquire("signed-normal-foreign-result");
    let _reset = ResetRuntime;
    let plan = plan(&home, true);
    let completed = finish(&plan, "synthetic-normal-foreign-result");
    let conn = catalog::open_catalog().unwrap();
    let receipt = jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let target = home.path.join("foreign");
    std::fs::create_dir(&target).unwrap();
    let foreign = crate::store::add_project(target.to_string_lossy().into(), false).unwrap();
    let mut receipt_value = serde_json::to_value(&receipt).unwrap();
    receipt_value["mappingProjectId"] = json!(foreign.id);
    receipt_value["projectNameAtLink"] = json!(foreign.name);
    receipt_value["projectPathAtLink"] = json!(foreign.path);
    receipt_value["projectPinnedAtLink"] = json!(foreign.pinned);
    receipt_value["projectTrustedAtLink"] = json!(foreign.trusted);
    let mut job_value = serde_json::to_value(&completed).unwrap();
    job_value["items"][0]["mappingProjectId"] = json!(foreign.id);
    conn.execute(
        "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
        rusqlite::params![receipt_value.to_string(), receipt.receipt_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        rusqlite::params![job_value.to_string(), completed.job_id],
    )
    .unwrap();
    let projects = std::fs::read(crate::paths::projects_file()).unwrap();
    let entries = serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap();
    let changes = conn.total_changes();
    let error = engine::undo_import_job(&completed.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(
        serde_json::to_value(catalog::list_entries(&conn).unwrap()).unwrap(),
        entries
    );
    assert!(std::fs::read(crate::paths::projects_file()).unwrap() == projects);
}
