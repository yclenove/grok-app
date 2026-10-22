use super::*;
use crate::session_migration::{
    catalog, engine, job_control, jobs, scan, test_home::IsolatedAppHome, types::Provider,
};

fn subject(source: &str, cwd: Option<&str>) -> MappingSubject {
    MappingSubject::new(source.into(), "codex".into(), cwd.map(str::to_string))
}

#[test]
fn groups_include_source_and_preserve_path_spelling() {
    let a = subject("a", Some("/repo"));
    let b = subject("b", Some("/repo"));
    let subjects = [
        a.clone(),
        b.clone(),
        a.clone(),
        subject("a", None),
        subject("b", None),
        subject("a", Some("  ")),
    ];
    let rows = propose(&subjects, None);
    assert_eq!(rows.len(), 4);
    assert_ne!(rows[0].root_id, rows[1].root_id);
    assert_ne!(rows[2].root_id, rows[3].root_id);
    assert!(rows
        .iter()
        .all(|row| row.unassigned && row.explicit_target.is_none()));
    assert_ne!(subject("a", Some("/repo ")).root_id(), a.root_id());
    assert_ne!(
        subject("a", Some(r"/repo\a")).root_id(),
        subject("a", Some("/repo/a")).root_id()
    );
    assert_ne!(
        MappingSubject::new("a".into(), "cursor".into(), a.cwd.clone()).root_id(),
        a.root_id()
    );
}

#[test]
fn normalization_rejects_duplicate_missing_mixed_and_wrong_source() {
    let subjects = [subject("a", Some("/repo")), subject("b", Some("/repo"))];
    let rows = propose(&subjects, None);
    assert!(normalize(&subjects, rows.clone()).is_ok());
    let mut wrong = rows.clone();
    wrong[1].source.as_mut().unwrap().source_id = "a".into();
    for invalid in [
        vec![rows[0].clone()],
        vec![rows[0].clone(), rows[0].clone()],
        wrong,
    ] {
        assert_eq!(
            normalize(&subjects, invalid).unwrap_err().code,
            ErrorCode::MappingConflict
        );
    }
    let mut mixed = rows.clone();
    mixed[1].source = None;
    assert!(normalize(&subjects, mixed.clone()).is_err());
    assert!(resolve(&mixed, &subjects[0]).is_err());
    assert!(resolve(&rows[..1], &subjects[1]).is_err());
    assert!(resolve(&[rows[0].clone(), rows[0].clone()], &subjects[0]).is_err());
}

#[test]
fn only_unambiguous_new_legacy_input_is_upgraded() {
    let a = subject("a", Some("/repo"));
    let b = subject("b", Some("/repo"));
    let legacy = workspace::propose_mappings(std::slice::from_ref(&a.cwd), Some("project".into()));
    let before = serde_json::to_value(&legacy).unwrap();
    assert!(before[0].get("source").is_none());
    let rows = normalize(std::slice::from_ref(&a), legacy.clone()).unwrap();
    assert_eq!(rows[0].source, Some(a.source.clone()));
    assert_eq!(rows[0].target_project_id.as_deref(), Some("project"));
    assert!(normalize(&[a.clone(), b.clone()], legacy.clone()).is_err());
    // Existing durable legacy plans retain their old resolver and options bytes.
    assert_eq!(
        resolve(&legacy, &b)
            .unwrap()
            .unwrap()
            .target_project_id
            .as_deref(),
        Some("project")
    );
    assert_eq!(
        options(resolve(&legacy, &a).unwrap()),
        serde_json::json!({"rawRetention":"normalized-only"})
    );
    assert_eq!(serde_json::to_value(legacy).unwrap(), before);
}

#[test]
fn options_bind_receipt_hash_to_source_and_root() {
    let a = subject("a", Some("/repo"));
    let b = subject("b", Some("/repo"));
    let rows = propose(&[a.clone(), b.clone()], None);
    let options_a = options(Some(&rows[0]));
    let options_b = options(Some(&rows[1]));
    assert_ne!(
        crate::session_migration::hash::options_hash(&options_a),
        crate::session_migration::hash::options_hash(&options_b)
    );
    assert!(validate_options(&options_a, &a).is_ok());
    assert!(validate_options(&options_a, &b).is_err());
    assert!(validate_options(&serde_json::json!({"mappingScope":null}), &a).is_err());
    assert!(validate_options(&options(None), &a).is_ok());
}

#[test]
fn persisted_legacy_plan_hash_and_durable_input_remain_usable() {
    let home = IsolatedAppHome::acquire("mapping-legacy-plan");
    engine::reset_runtime();
    let source = home.path.join("provider");
    std::fs::create_dir(&source).unwrap();
    write_codex(&source.join("rollout.jsonl"), "legacy", "/repo");
    let scan = scan::scan_start(vec![(
        Provider::Codex,
        source.to_string_lossy().into_owned(),
    )])
    .unwrap();
    let candidate = &scan.candidates[0];
    let (preview_id, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let plan = engine::plan(vec![preview_id], Vec::new()).unwrap();
    let conn = catalog::open_catalog().unwrap();
    let (_, json) = catalog::load_plan_json(&conn, &plan.plan_id).unwrap();
    let mut record: serde_json::Value = serde_json::from_str(&json).unwrap();
    // A historical v1 record has its own canonical hash, not a v3 descriptor
    // with its lifetime-bound hash computed as if it were an old payload.
    record["version"] = 1.into();
    record.as_object_mut().unwrap().remove("publicationOutcome");
    record["mappings"] =
        serde_json::to_value(workspace::propose_mappings(&[Some("/repo".into())], None)).unwrap();
    let payload = serde_json::json!({
        "version":record["version"], "previewIds":record["previewIds"], "mappings":record["mappings"],
        "snapshots":record["snapshots"], "selectionSet":record["selectionSet"],
        "selectionHash":record["selectionHash"], "options":record["options"],
    });
    let old_hash = crate::session_migration::hash::options_hash(&payload);
    record["planHash"] = old_hash.clone().into();
    let old_json = record.to_string();
    catalog::persist_plan_json(
        &conn,
        &plan.plan_id,
        &old_hash,
        &old_json,
        record["created"].as_str().unwrap(),
        record["expiry"].as_str().unwrap(),
    )
    .unwrap();
    engine::reset_runtime();
    let queued = engine::commit(&plan.plan_id, &old_hash, "legacy-mapping").unwrap();
    let item = job_control::load_input(&conn, &queued.job_id, &candidate.candidate_id).unwrap();
    assert_eq!(item.options, options(None));
    assert_eq!(
        catalog::load_plan_json(&conn, &plan.plan_id).unwrap(),
        (old_hash, old_json)
    );
    job_control::run_pending().unwrap();
    assert_eq!(
        jobs::get_job(&conn, &queued.job_id).unwrap().unwrap().state,
        jobs::JobState::Completed
    );
    engine::reset_runtime();
}

pub(crate) fn write_codex(path: &std::path::Path, id: &str, cwd: &str) {
    let meta = serde_json::json!({"type":"session_meta", "payload":{"id": id, "cwd": cwd}});
    let event = serde_json::json!({"type":"response_item", "payload":{"type":"message", "id":"u1", "role":"user", "content":[{"type":"input_text", "text": id}]}});
    std::fs::write(path, format!("{meta}\n{event}\n")).unwrap();
}

fn same_cwd_plan(home: &IsolatedAppHome) -> (engine::PlanState, String) {
    engine::reset_runtime();
    let mut roots = Vec::new();
    let mut projects = Vec::new();
    for id in ["a", "b"] {
        let source = home.path.join(id);
        std::fs::create_dir(&source).unwrap();
        write_codex(&source.join("rollout.jsonl"), "same-session", "/same/repo");
        roots.push((Provider::Codex, source.to_string_lossy().into_owned()));
        projects
            .push(crate::store::add_project(source.to_string_lossy().into_owned(), false).unwrap());
    }
    let scan = scan::scan_start(roots).unwrap();
    assert_eq!(scan.candidates.len(), 2);
    let ids = scan
        .candidates
        .iter()
        .map(|candidate| {
            engine::preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .collect::<Vec<_>>();
    let mut rows = engine::propose_mappings(&ids, None).unwrap();
    assert_eq!(rows.len(), 2);
    for (row, project) in rows.iter_mut().zip(&projects) {
        row.target_project_id = Some(project.id.clone());
        row.unassigned = false;
    }
    let plan = engine::plan(ids.clone(), rows.clone()).unwrap();
    engine::reset_runtime();
    assert_eq!(
        engine::propose_mappings(&ids, None)
            .unwrap()
            .iter()
            .map(|row| &row.source)
            .collect::<Vec<_>>(),
        rows.iter().map(|row| &row.source).collect::<Vec<_>>()
    );
    (plan, scan.candidates[0].candidate_id.clone())
}

fn take_sources_offline(home: &IsolatedAppHome) {
    for id in ["a", "b"] {
        std::fs::remove_file(home.path.join(id).join("rollout.jsonl")).unwrap();
    }
    engine::reset_runtime();
}

fn assert_scoped_projects(completed: &jobs::JobRecord, rows: &[workspace::WorkspaceMapping]) {
    assert_eq!(completed.state, jobs::JobState::Completed, "{completed:?}");
    let conn = catalog::open_catalog().unwrap();
    for item in &completed.items {
        assert_eq!(item.phase, "committed", "{item:?}");
        assert_eq!(item.outcome.as_deref(), Some("complete"), "{item:?}");
        let entry = catalog::get_entry(&conn, item.import_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        let row = rows
            .iter()
            .find(|row| row.source.as_ref().unwrap().source_id == entry.installation_id)
            .unwrap();
        assert_eq!(item.mapping_project_id, row.target_project_id);
        let receipt = jobs::latest_receipt(&conn, &completed.job_id, &item.candidate_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.options_hash,
            crate::session_migration::hash::options_hash(&options(Some(row)))
        );
    }
    assert_eq!(catalog::list_entries(&conn).unwrap().len(), 2);
}

#[test]
fn signed_same_cwd_sources_preserve_project_mapping_after_restart_and_source_removal() {
    let home = IsolatedAppHome::acquire("signed-mapping-sources");
    let (plan, first_candidate) = same_cwd_plan(&home);
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, "signed-source-scoped").unwrap();
    assert_eq!(
        crate::session_migration::publication_protocol::PublicationProtocol::for_job_id(
            &queued.job_id
        )
        .unwrap(),
        crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
    );
    let original = conn_input(&queued.job_id, &first_candidate);
    let conn = catalog::open_catalog().unwrap();
    let mut altered: serde_json::Value = serde_json::from_str(&original).unwrap();
    altered["options"]["mappingScope"]["source"]["sourceId"] = "wrong-source".into();
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, first_candidate, altered.to_string()],
    )
    .unwrap();
    let changes = conn.total_changes();
    let error = job_control::load_input(&conn, &queued.job_id, &first_candidate)
        .expect_err("wrong source must fail");
    // Accepted raw input authentication precedes the nested mapping codec.
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), changes);
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, first_candidate, original],
    )
    .unwrap();
    // Provider files can disappear after preparation; durable jobs use snapshots.
    take_sources_offline(&home);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_scoped_projects(&completed, &plan.mappings);
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, "signed-source-scoped")
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    engine::reset_runtime();
}

#[test]
fn signed_same_cwd_retry_after_real_receipt_fault_keeps_each_source_project_and_allocation() {
    let home = IsolatedAppHome::acquire("signed-mapping-retry");
    let (plan, _) = same_cwd_plan(&home);
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, "signed-mapping-retry").unwrap();
    let conn = catalog::open_catalog().unwrap();
    let projects_before = std::fs::read(crate::paths::projects_file()).unwrap();
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_second_mapping AFTER INSERT ON receipts
         WHEN NEW.job_id='{}' AND NEW.candidate_id='{}' AND NEW.phase='committed'
         BEGIN SELECT RAISE(ABORT,'synthetic second-source publication failure'); END;",
        queued.job_id, queued.items[1].candidate_id,
    ))
    .unwrap();
    take_sources_offline(&home);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let failed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(
        failed.state,
        jobs::JobState::CompletedWithIssues,
        "{failed:?}"
    );
    assert_eq!(failed.items[0].phase, "committed");
    assert_eq!(failed.items[1].phase, "failed");
    assert!(failed.items[1]
        .error
        .as_ref()
        .unwrap()
        .contains("synthetic second-source"));
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &queued.items[1].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "linked");
    assert_eq!(catalog::list_entries(&conn).unwrap().len(), 1);
    conn.execute_batch("DROP TRIGGER fail_second_mapping")
        .unwrap();
    engine::reset_runtime();
    job_control::retry(&queued.job_id).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let retried = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_scoped_projects(&retried, &plan.mappings);
    assert_eq!(
        serde_json::to_value(&retried.items[0]).unwrap(),
        serde_json::to_value(&failed.items[0]).unwrap()
    );
    assert_eq!(retried.items[1].import_id, receipt.import_id);
    assert_eq!(retried.items[1].revision_id, receipt.revision_id);
    assert_eq!(
        std::fs::read(crate::paths::projects_file()).unwrap(),
        projects_before
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    engine::reset_runtime();
}

#[test]
fn legacy_same_cwd_two_sources_publish_and_retry_their_own_projects_after_restart() {
    let home = IsolatedAppHome::acquire("legacy-mapping-sources");
    let (plan, _) = same_cwd_plan(&home);
    // Historical ACK-loss replay uses a genuine v1 producer, not corruption of
    // an authenticated signed terminal. The signed real-failure control above
    // preserves the normal retry contract without weakening that refusal.
    let queued = engine::enqueue_legacy_plan(&plan, "legacy-source-scoped");
    take_sources_offline(&home);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let conn = catalog::open_catalog().unwrap();
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_scoped_projects(&completed, &plan.mappings);
    let mut failed = completed.clone();
    failed.state = jobs::JobState::CompletedWithIssues;
    for item in &mut failed.items {
        item.phase = "failed".into();
        item.outcome = Some("failed".into());
    }
    jobs::save_job(&conn, &failed).unwrap();
    engine::reset_runtime();
    job_control::retry(&queued.job_id).unwrap();
    job_control::run_pending().unwrap();
    let retried = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(retried.state, jobs::JobState::Completed, "{retried:?}");
    for (before, after) in completed.items.iter().zip(&retried.items) {
        assert_eq!(after.revision_id, before.revision_id);
        assert_eq!(after.mapping_project_id, before.mapping_project_id);
    }
    assert_eq!(catalog::list_entries(&conn).unwrap().len(), 2);
    engine::reset_runtime();
}

fn conn_input(job_id: &str, candidate_id: &str) -> String {
    catalog::open_catalog()
        .unwrap()
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![job_id, candidate_id],
            |row| row.get(0),
        )
        .unwrap()
}
