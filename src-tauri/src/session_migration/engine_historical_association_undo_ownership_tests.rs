//! N4/N5 coordinated results and N8 project ownership through public Undo.
//! Genuine independent objects first; only faults are committed through SQL.
use super::*;
use rusqlite::params;

#[path = "engine_historical_association_undo_mixed_tests.rs"]
mod mixed_tests;

#[path = "engine_historical_ordinary_undo_ownership_tests.rs"]
mod ordinary_tests;

#[path = "engine_historical_association_undo_project_result_tests.rs"]
mod project_result_tests;

fn independent(home: &IsolatedAppHome) -> JobRecord {
    let previews = cursor_source(home, 1);
    let pending = plan(previews, vec![]).unwrap();
    let queued = commit(
        &pending.plan_id,
        &pending.plan_hash,
        "synthetic-undo-ownership-independent",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items[0].phase, "committed");
    assert_ne!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    completed
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_coordinated_results_are_atomic() {
    for fault in [
        "old-revision",
        "foreign-ids",
        "foreign-created-entry",
        "foreign-content-result",
    ] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let foreign = independent(&home);
        let foreign_import = foreign.items[0].import_id.as_deref().unwrap();
        let foreign_revision = foreign.items[0].revision_id.as_deref().unwrap();
        let foreign_dir = catalog::revision_dir(foreign_import, foreign_revision).unwrap();
        let foreign_hashes = jobs::published_hashes(&foreign_dir).unwrap();
        let foreign_history =
            events_page(foreign_import, Some(foreign_revision), None, Some(20), None).unwrap();
        assert_eq!(foreign_history.2, 1);
        assert!(serde_json::to_string(&foreign_history.0)
            .unwrap()
            .contains("SYNTHETIC_REGISTERED_HISTORY_"));
        let (_, completed) = publish(&format!("synthetic-undo-ownership-{fault}"));
        let revision = completed.items[0].revision_id.as_deref().unwrap();
        let old_hashes = immutable_hashes(REVISION);
        let new_hashes = immutable_hashes(revision);
        let receipt =
            jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
                .unwrap()
                .unwrap();
        let mut changed_receipt = serde_json::to_value(&receipt).unwrap();
        let mut changed_job = serde_json::to_value(&completed).unwrap();
        let (target_import, target_revision) = if fault == "old-revision" {
            (IMPORT, REVISION)
        } else {
            (foreign_import, foreign_revision)
        };
        changed_receipt["importId"] = json!(target_import);
        changed_receipt["revisionId"] = json!(target_revision);
        changed_job["items"][0]["importId"] = json!(target_import);
        changed_job["items"][0]["revisionId"] = json!(target_revision);
        if matches!(fault, "foreign-created-entry" | "foreign-content-result") {
            changed_receipt["previousRevision"] = json!(null);
            changed_receipt["createdEntry"] = json!(true);
        }
        if fault == "foreign-content-result" {
            let foreign_receipt =
                jobs::latest_receipt(&conn, &foreign.job_id, &foreign.items[0].candidate_id)
                    .unwrap()
                    .unwrap();
            let foreign_value = serde_json::to_value(foreign_receipt).unwrap();
            for field in [
                "snapshotHash",
                "optionsHash",
                "selectionHash",
                "selectionSet",
                "fileHashes",
            ] {
                changed_receipt[field] = foreign_value[field].clone();
            }
        }
        let tx = conn.unchecked_transaction().unwrap();
        assert_eq!(
            tx.execute(
                "UPDATE receipts SET import_id=?1,revision_id=?2,json=?3 WHERE receipt_id=?4",
                params![
                    target_import,
                    target_revision,
                    changed_receipt.to_string(),
                    receipt.receipt_id
                ]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            tx.execute(
                "UPDATE jobs SET json=?1 WHERE job_id=?2",
                params![changed_job.to_string(), completed.job_id]
            )
            .unwrap(),
            1
        );
        tx.commit().unwrap();
        assert!(conn.is_autocommit());
        let after_fault = raw_catalog_rows(&conn);
        // A coordinated receipt can already poison the foreign reader lookup.
        // Undo must preserve that fault-time result, not repair the injected fault.
        let foreign_reader = || {
            events_page(foreign_import, Some(foreign_revision), None, Some(20), None)
                .map(|page| page.0)
                .map_err(|error| error.code)
        };
        let foreign_reader_after_fault = foreign_reader();
        let result = engine::undo_import_job(&completed.job_id);
        let unchanged = raw_catalog_rows(&conn) == after_fault;
        println!(
            "ASSOCIATION_UNDO_COORDINATED={}",
            json!({"fault":fault,"code":result.as_ref().err().map(|e| e.code.as_str()),"catalogUnchanged":unchanged})
        );
        let error =
            result.expect_err("coordinated mutable receipt/result cannot grant another target");
        assert_eq!(
            error.code,
            ErrorCode::HistoryAccessUnavailable,
            "{fault}: {error:?}"
        );
        assert!(unchanged, "{fault}: catalog changed on refusal");
        assert_eq!(
            jobs::published_hashes(&foreign_dir).unwrap(),
            foreign_hashes
        );
        assert_eq!(foreign_reader(), foreign_reader_after_fault);
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(revision), new_hashes);
        require_seed(
            &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
            &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
        );
    }
}

fn native_store_bytes() -> Vec<Option<Vec<u8>>> {
    [
        crate::paths::projects_file(),
        crate::paths::workspaces_file(),
        crate::paths::sessions_index_file(),
    ]
    .iter()
    .map(|path| match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("isolated native store unreadable: {error}"),
    })
    .collect()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_refuses_foreign_project() {
    for (snapshot, coordinated_item) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let target = home.path.join("independent-project");
        std::fs::create_dir(&target).unwrap();
        let marker = target.join("keep.txt");
        std::fs::write(&marker, "SYNTHETIC_FOREIGN_PROJECT_CONTENT").unwrap();
        let project =
            crate::store::add_project(target.to_string_lossy().into_owned(), false).unwrap();
        crate::session_migration::project_references::ensure_unreferenced(&conn, &project.id)
            .unwrap();
        let (_, completed) = publish("synthetic-undo-ownership-unassigned-project-fault");
        let revision = completed.items[0].revision_id.as_deref().unwrap();
        // Completed workers dispose prepared files; retained metadata remains
        // available without reopening those intentionally collected previews.
        let raw: String = conn
            .query_row(
                "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
                params![completed.job_id, completed.items[0].candidate_id],
                |row| row.get(0),
            )
            .unwrap();
        let input: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(input["createProject"], false);
        assert!(input
            .get("mappingProjectId")
            .is_none_or(serde_json::Value::is_null));
        let receipt =
            jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
                .unwrap()
                .unwrap();
        assert!(!receipt.created_project && receipt.mapping_project_id.is_none());
        assert_eq!(receipt.project_decision.as_deref(), Some("unassigned"));
        let mut changed = serde_json::to_value(&receipt).unwrap();
        changed["createdProject"] = json!(true);
        changed["mappingProjectId"] = json!(project.id);
        changed["projectDecision"] = json!("create");
        if snapshot {
            changed["projectNameAtLink"] = json!(project.name);
            changed["projectPathAtLink"] = json!(project.path);
            changed["projectPinnedAtLink"] = json!(project.pinned);
            changed["projectTrustedAtLink"] = json!(project.trusted);
        }
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
            params![changed.to_string(), receipt.receipt_id],
        )
        .unwrap();
        if coordinated_item {
            let mut value = serde_json::to_value(&completed).unwrap();
            value["items"][0]["mappingProjectId"] = json!(project.id);
            tx.execute(
                "UPDATE jobs SET json=?1 WHERE job_id=?2",
                params![value.to_string(), completed.job_id],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let after_fault = raw_catalog_rows(&conn);
        let native_before = native_store_bytes();
        let old_hashes = immutable_hashes(REVISION);
        let new_hashes = immutable_hashes(revision);
        let result = engine::undo_import_job(&completed.job_id);
        let project_survives = crate::store::load_projects_snapshot()
            .unwrap()
            .iter()
            .any(|p| p.id == project.id);
        let unchanged = raw_catalog_rows(&conn) == after_fault;
        println!(
            "ASSOCIATION_UNDO_FOREIGN_PROJECT={}",
            json!({"exactSnapshot":snapshot,"coordinatedItem":coordinated_item,"code":result.as_ref().err().map(|e| e.code.as_str()),"returnedPhase":result.as_ref().ok().map(|j| &j.items[0].phase),"foreignProjectSurvives":project_survives,"catalogUnchanged":unchanged})
        );
        let error =
            result.expect_err("unassigned accepted import cannot own an independent project");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert!(
            unchanged && project_survives,
            "foreign project refusal must be atomic"
        );
        assert!(
            native_store_bytes() == native_before,
            "native stores changed on refusal"
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "SYNTHETIC_FOREIGN_PROJECT_CONTENT"
        );
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(revision), new_hashes);
        require_seed(
            &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
            &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_genuine_created_project_is_preserved_by_mapping() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let ordinary = confirmed_plan();
    let target = home.path.join("genuine-created-project");
    std::fs::create_dir(&target).unwrap();
    let marker = target.join("keep.txt");
    std::fs::write(&marker, "SYNTHETIC_GENUINE_PROJECT_CONTENT").unwrap();
    assert!(crate::store::load_projects_snapshot().unwrap().is_empty());
    let mut mappings = ordinary.mappings.clone();
    assert_eq!(mappings.len(), 1);
    mappings[0].create_project = true;
    mappings[0].explicit_target = Some(target.to_string_lossy().into_owned());
    mappings[0].unassigned = false;
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
    let queued = acceptance(
        &confirmed,
        "synthetic-undo-ownership-genuine-create-project",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.items[0].phase, "committed");
    let receipt = jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert!(receipt.created_project && !receipt.created_entry);
    assert_eq!(receipt.project_decision.as_deref(), Some("create"));
    let projects = crate::store::load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        receipt.mapping_project_id.as_deref(),
        Some(projects[0].id.as_str())
    );
    assert_eq!(
        completed.items[0].mapping_project_id,
        receipt.mapping_project_id
    );
    let native_before = native_store_bytes();
    let old_hashes = immutable_hashes(REVISION);
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let new_hashes = immutable_hashes(revision);
    let all_receipts = receipts(&conn);
    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("MAPPING_CONFLICT"));
    assert!(undone.items[0]
        .error
        .as_deref()
        .unwrap()
        .contains("imported history mapping"));
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(REVISION)
    );
    assert!(
        native_store_bytes() == native_before,
        "retained mapping must keep genuine project"
    );
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        "SYNTHETIC_GENUINE_PROJECT_CONTENT"
    );
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
    println!(
        "ASSOCIATION_UNDO_GENUINE_PROJECT=undone:retained-mapping-keeps-project-and-directory"
    );
}
