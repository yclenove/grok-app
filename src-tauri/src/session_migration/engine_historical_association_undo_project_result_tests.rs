//! Genuine producer project outcomes, not mapping permission as result ownership.
//! All projects and target directories belong to fresh synthetic App homes.
use super::*;

fn create_plan(
    home: &IsolatedAppHome,
    label: &str,
    fallback: bool,
) -> (PlanState, std::path::PathBuf) {
    let ordinary = confirmed_plan();
    let target = home.path.join(label);
    std::fs::create_dir(&target).unwrap();
    let mut mappings = ordinary.mappings;
    assert_eq!(mappings.len(), 1);
    mappings[0].create_project = true;
    mappings[0].explicit_target = Some(target.to_string_lossy().into_owned());
    mappings[0].unassigned = false;
    mappings[0].import_unassigned_anyway = fallback;
    mappings[0].user_choice_version += 1;
    let confirmed = AssociationPlanRequest {
        version: 1,
        preview_ids: ordinary.preview_ids,
        mappings,
        selection_set: ordinary.selection_set,
        identity_associations: ordinary.identity_associations,
    }
    .build()
    .unwrap();
    (confirmed, target)
}

fn finish(queued: &JobRecord) -> (JobRecord, jobs::Receipt) {
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items.len(), 1);
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    let conn = open_catalog().unwrap();
    let receipt = jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
    assert!(!receipt.created_entry);
    (completed, receipt)
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_allowed_create_cannot_own_foreign_project() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (confirmed, target) = create_plan(&home, "owned-created-project", false);
    let own_marker = target.join("keep.txt");
    std::fs::write(&own_marker, "SYNTHETIC_OWN_PROJECT_CONTENT").unwrap();
    let queued = acceptance(&confirmed, "synthetic-undo-allowed-create-foreign-result").unwrap();
    let (completed, receipt) = finish(&queued);
    assert!(receipt.created_project);
    assert_eq!(receipt.project_decision.as_deref(), Some("create"));
    let projects = crate::store::load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 1);
    let own = &projects[0];
    assert_eq!(receipt.mapping_project_id.as_deref(), Some(own.id.as_str()));
    let input: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![completed.job_id, completed.items[0].candidate_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&input).unwrap()["createProject"],
        true
    );

    // A second project is genuinely created, not a fabricated SQL owner. It
    // has no import/workspace/session references and an exact saved snapshot.
    let foreign_target = home.path.join("unrelated-project");
    std::fs::create_dir(&foreign_target).unwrap();
    let foreign_marker = foreign_target.join("keep.txt");
    std::fs::write(&foreign_marker, "SYNTHETIC_FOREIGN_PROJECT_CONTENT").unwrap();
    let foreign =
        crate::store::add_project(foreign_target.to_string_lossy().into_owned(), false).unwrap();
    assert_ne!(foreign.id, own.id);
    crate::session_migration::project_references::ensure_unreferenced(&conn, &foreign.id).unwrap();
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let old_text = text(IMPORT, REVISION);
    let new_text = text(IMPORT, revision);
    let original: (String, String) = conn
        .query_row(
            "SELECT j.json,r.json FROM jobs j JOIN receipts r ON r.job_id=j.job_id
             WHERE j.job_id=?1 AND r.receipt_id=?2",
            params![completed.job_id, receipt.receipt_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let before_fault = raw_catalog_rows(&conn);
    let mut changed_receipt = serde_json::to_value(&receipt).unwrap();
    changed_receipt["mappingProjectId"] = json!(foreign.id);
    changed_receipt["projectNameAtLink"] = json!(foreign.name);
    changed_receipt["projectPathAtLink"] = json!(foreign.path);
    changed_receipt["projectPinnedAtLink"] = json!(foreign.pinned);
    changed_receipt["projectTrustedAtLink"] = json!(foreign.trusted);
    let mut changed_job = completed.clone();
    changed_job.items[0].mapping_project_id = Some(foreign.id.clone());
    let tx = conn.unchecked_transaction().unwrap();
    assert_eq!(
        tx.execute(
            "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
            params![changed_receipt.to_string(), receipt.receipt_id],
        )
        .unwrap(),
        1
    );
    save_display_fault(&tx, &changed_job);
    tx.commit().unwrap();
    assert!(conn.is_autocommit());
    let after_fault = raw_catalog_rows(&conn);
    let native_before = native_store_bytes();
    let result = engine::undo_import_job(&completed.job_id);
    let catalog_unchanged = raw_catalog_rows(&conn) == after_fault;
    let native_unchanged = native_store_bytes() == native_before;
    let after_projects = crate::store::load_projects_snapshot().unwrap();
    let own_survives = after_projects.iter().any(|project| project.id == own.id);
    let foreign_survives = after_projects
        .iter()
        .any(|project| project.id == foreign.id);
    let archives_unchanged =
        immutable_hashes(REVISION) == old_hashes && immutable_hashes(revision) == new_hashes;
    println!(
        "ASSOCIATION_UNDO_ALLOWED_CREATE_FOREIGN_RESULT={}",
        json!({"createdPermissionAuthenticated":true,
            "code":result.as_ref().err().map(|error| error.code.as_str()),
            "returnedPhase":result.as_ref().ok().map(|job| &job.items[0].phase),
            "catalogUnchanged":catalog_unchanged,"nativeStoresUnchanged":native_unchanged,
            "ownProjectSurvives":own_survives,"foreignProjectSurvives":foreign_survives,
            "archivesUnchanged":archives_unchanged,
            "ownMarkerSurvives":own_marker.is_file(),"foreignMarkerSurvives":foreign_marker.is_file()})
    );
    let error = result.expect_err("creation permission cannot authorize another producer result");
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(catalog_unchanged && native_unchanged && own_survives && foreign_survives);
    assert!(archives_unchanged);
    assert_eq!(
        std::fs::read_to_string(own_marker).unwrap(),
        "SYNTHETIC_OWN_PROJECT_CONTENT"
    );
    assert_eq!(
        std::fs::read_to_string(foreign_marker).unwrap(),
        "SYNTHETIC_FOREIGN_PROJECT_CONTENT"
    );
    // The refusal must preserve the fault above. Restore only its original
    // producer bytes before the separate successful historical-reader checks.
    let tx = conn.unchecked_transaction().unwrap();
    save_display_fault(&tx, &completed);
    tx.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        params![original.0, completed.job_id],
    )
    .unwrap();
    tx.execute(
        "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
        params![original.1, receipt.receipt_id],
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(raw_catalog_rows(&conn), before_fault);
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(text(IMPORT, revision), new_text);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_genuine_same_path_match_is_preserved() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (confirmed, target) = create_plan(&home, "existing-project-target", false);
    let queued = acceptance(&confirmed, "synthetic-undo-genuine-same-path-match").unwrap();
    let project = crate::store::add_project(target.to_string_lossy().into_owned(), false).unwrap();
    let (completed, receipt) = finish(&queued);
    assert!(!receipt.created_project);
    assert_eq!(receipt.project_decision.as_deref(), Some("match"));
    assert_eq!(
        receipt.mapping_project_id.as_deref(),
        Some(project.id.as_str())
    );
    let native_before = native_store_bytes();
    let all_receipts = receipts(&conn);
    let old_hashes = immutable_hashes(REVISION);
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let new_hashes = immutable_hashes(revision);
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0].error.is_none());
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(REVISION)
    );
    assert!(native_store_bytes() == native_before);
    assert_eq!(receipts(&conn), all_receipts);
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    reset_runtime();
    text(IMPORT, REVISION);
    text(IMPORT, revision);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!("ASSOCIATION_UNDO_GENUINE_MATCH=undone:existing-project-preserved");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_genuine_unassigned_reported_fallback() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (confirmed, target) = create_plan(&home, "disappearing-project-target", true);
    let queued = acceptance(&confirmed, "synthetic-undo-genuine-unassigned-reported").unwrap();
    // Remove only this exact, empty synthetic directory after acceptance. This
    // is real target unavailability, not a fabricated receipt outcome.
    assert!(target.starts_with(&home.path));
    assert!(!std::fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    std::fs::remove_dir(&target).unwrap();
    let (completed, receipt) = finish(&queued);
    assert!(!receipt.created_project && receipt.mapping_project_id.is_none());
    assert_eq!(
        receipt.project_decision.as_deref(),
        Some("unassigned-reported")
    );
    assert!(completed.items[0].mapping_project_id.is_none());
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    let native_before = native_store_bytes();
    let all_receipts = receipts(&conn);
    let old_hashes = immutable_hashes(REVISION);
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let new_hashes = immutable_hashes(revision);
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0].error.is_none());
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(REVISION)
    );
    assert!(native_store_bytes() == native_before);
    assert_eq!(receipts(&conn), all_receipts);
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    assert!(!target.exists());
    reset_runtime();
    text(IMPORT, REVISION);
    text(IMPORT, revision);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!("ASSOCIATION_UNDO_GENUINE_FALLBACK=undone:unassigned-reported-no-project-created");
}
