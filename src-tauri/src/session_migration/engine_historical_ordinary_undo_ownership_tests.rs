//! Ordinary durable-v2 controls use genuinely registered sources and workers.
//! No positive input, receipt, catalog entry or job is constructed through SQL.
use super::*;
use crate::session_migration::publication_protocol::PublicationProtocol;

fn ordinary_publish(preview: &str, request: &str, protocol: PublicationProtocol) -> JobRecord {
    let pending = plan(vec![preview.into()], vec![]).unwrap();
    assert!(pending.identity_associations.is_empty());
    let queued = match protocol {
        PublicationProtocol::Legacy => engine::enqueue_legacy_plan(&pending, request),
        PublicationProtocol::SignedOutcomeV1 => {
            commit(&pending.plan_id, &pending.plan_hash, request).unwrap()
        }
    };
    assert_eq!(
        PublicationProtocol::for_job_id(&queued.job_id).unwrap(),
        protocol
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items[0].phase, "committed");
    assert_ne!(completed.items[0].outcome.as_deref(), Some("deduplicated"));
    let conn = open_catalog().unwrap();
    let raw: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&completed.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let input: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(input["version"], 2);
    assert!(input.get("identityAssociation").is_none());
    completed
}

fn ordinary_refresh(home: &IsolatedAppHome) -> (JobRecord, JobRecord, JobRecord) {
    ordinary_refresh_with_protocol(home, PublicationProtocol::SignedOutcomeV1)
}

fn ordinary_refresh_with_protocol(
    home: &IsolatedAppHome,
    protocol: PublicationProtocol,
) -> (JobRecord, JobRecord, JobRecord) {
    let previews = cursor_source(home, 2);
    let first = ordinary_publish(&previews[0], "synthetic-ordinary-undo-original", protocol);
    let foreign = ordinary_publish(
        &previews[1],
        "synthetic-ordinary-undo-independent",
        protocol,
    );
    let conn = open_catalog().unwrap();
    let receipt = jobs::latest_receipt(&conn, &first.job_id, &first.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let source = sources::get(&conn, &receipt.source_id).unwrap();
    let path = home.path.join("synthetic cursor/0.jsonl");
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original.replace(
        "SYNTHETIC_REGISTERED_HISTORY_0",
        "ORDINARY_UNDO_CHANGED_BODY",
    );
    assert_ne!(original, changed);
    std::fs::write(&path, changed).unwrap();
    reset_runtime();
    let started = crate::session_migration::scan::begin(vec![ScanSourcePath {
        provider: "cursor".into(),
        path: source.root_path,
        source_id: Some(source.source_id),
        location_id: None,
        host: Some("local".into()),
        profile: source.profile,
    }])
    .unwrap();
    crate::session_migration::scan::run(&started.scan_id).unwrap();
    let scanned = scan_pump(&started.scan_id, started.epoch).unwrap();
    assert!(scanned.complete && scanned.issues.is_empty());
    assert_eq!(scanned.candidates.len(), 2);
    let candidate = scanned
        .candidates
        .iter()
        .find(|candidate| candidate.native_session_id == receipt.native_session_id)
        .unwrap();
    let preview_id = preview(&candidate.candidate_id, Some(&candidate.source_revision))
        .unwrap()
        .0;
    let refreshed = ordinary_publish(&preview_id, "synthetic-ordinary-undo-refresh", protocol);
    assert_eq!(first.items[0].import_id, refreshed.items[0].import_id);
    assert_ne!(first.items[0].revision_id, refreshed.items[0].revision_id);
    assert_ne!(first.items[0].import_id, foreign.items[0].import_id);
    let receipt = jobs::latest_receipt(&conn, &refreshed.job_id, &refreshed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert!(!receipt.created_entry && !receipt.created_project);
    assert_eq!(receipt.previous_revision, first.items[0].revision_id);
    assert!(serde_json::to_string(
        &events_page(
            refreshed.items[0].import_id.as_deref().unwrap(),
            refreshed.items[0].revision_id.as_deref(),
            None,
            Some(20),
            None
        )
        .unwrap()
        .0
    )
    .unwrap()
    .contains("ORDINARY_UNDO_CHANGED_BODY"));
    (first, refreshed, foreign)
}

fn archive_hashes(jobs: &[&JobRecord]) -> Vec<(std::path::PathBuf, BTreeMap<String, String>)> {
    jobs.iter()
        .map(|job| {
            let item = &job.items[0];
            let path = catalog::revision_dir(
                item.import_id.as_deref().unwrap(),
                item.revision_id.as_deref().unwrap(),
            )
            .unwrap();
            let hashes = jobs::published_hashes(&path).unwrap();
            (path, hashes)
        })
        .collect()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_ordinary_refresh_and_missing_receipts() {
    refresh_and_missing_receipts(PublicationProtocol::SignedOutcomeV1);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_ordinary_legacy_refresh_and_missing_receipts() {
    refresh_and_missing_receipts(PublicationProtocol::Legacy);
}

fn refresh_and_missing_receipts(protocol: PublicationProtocol) {
    for missing in [false, true] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let (first, refreshed, foreign) = ordinary_refresh_with_protocol(&home, protocol);
        let hashes = archive_hashes(&[&first, &refreshed, &foreign]);
        let import = first.items[0].import_id.as_deref().unwrap();
        let read = |job: &JobRecord| {
            events_page(
                job.items[0].import_id.as_deref().unwrap(),
                job.items[0].revision_id.as_deref(),
                None,
                Some(20),
                None,
            )
            .map(|page| page.0)
            .map_err(|error| error.code)
        };
        assert!([&first, &refreshed, &foreign]
            .map(read)
            .iter()
            .all(Result::is_ok));
        if missing {
            assert!(
                conn.execute("DELETE FROM receipts WHERE job_id=?1", [&refreshed.job_id])
                    .unwrap()
                    > 0
            );
        }
        // Deleting receipts intentionally denies the refresh reader. Undo must
        // preserve its fault-time result, not repair that injected corruption.
        let readers_before = [&first, &refreshed, &foreign].map(read);
        let untouched = || {
            raw_catalog_rows(&conn)
                .into_iter()
                .filter(|(table, _)| table.as_str() != "entries")
                .collect::<Vec<_>>()
        };
        let mut before = untouched();
        let all_rows_before = raw_catalog_rows(&conn);
        let all_receipts = receipts(&conn);
        let native_before = native_store_bytes();
        let phase = if missing && protocol == PublicationProtocol::SignedOutcomeV1 {
            // Signed authentication refuses before changing even job ACK/version;
            // historical v2 still exposes its original per-item conflict below.
            for _ in 0..2 {
                let error = engine::undo_import_job(&refreshed.job_id).unwrap_err();
                assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
                assert_eq!(raw_catalog_rows(&conn), all_rows_before);
                reset_runtime();
            }
            "refused".to_string()
        } else {
            let undone = engine::undo_import_job(&refreshed.job_id).unwrap();
            assert_eq!(
                undone.items[0].phase,
                if missing { "conflict" } else { "undone" }
            );
            assert_eq!(
                undone.items[0].error.as_deref(),
                if missing {
                    Some("ownership receipt missing")
                } else {
                    None
                }
            );
            assert_eq!(undone.state_version, refreshed.state_version + 1);
            before = with_job_progress(&conn, before, &undone);
            undone.items[0].phase.clone()
        };
        let expected = if missing { &refreshed } else { &first };
        let current = get_entry(&conn, import).unwrap().unwrap();
        assert_eq!(current.current_revision, expected.items[0].revision_id);
        assert!(!current.hidden);
        assert_eq!(untouched(), before);
        assert_eq!(receipts(&conn), all_receipts);
        assert!(native_store_bytes() == native_before);
        for (path, expected) in hashes {
            assert_eq!(jobs::published_hashes(&path).unwrap(), expected);
        }
        reset_runtime();
        assert_eq!([&first, &refreshed, &foreign].map(read), readers_before);
        require_seed(
            &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
            &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
        );
        println!(
            "ORDINARY_UNDO_POSITIVE={}",
            json!({"protocol":format!("{protocol:?}"),"missing":missing,"phase":phase,"archivesPreserved":true})
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_ownership_ordinary_refuses_foreign_results() {
    for fault in ["foreign-item", "coordinated-result", "foreign-previous"] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let (first, refreshed, foreign) = ordinary_refresh(&home);
        let hashes = archive_hashes(&[&first, &refreshed, &foreign]);
        let receipt =
            jobs::latest_receipt(&conn, &refreshed.job_id, &refreshed.items[0].candidate_id)
                .unwrap()
                .unwrap();
        let mut changed_job = refreshed.clone();
        let mut changed_receipt = serde_json::to_value(&receipt).unwrap();
        if fault == "foreign-previous" {
            changed_receipt["previousRevision"] = json!(foreign.items[0].revision_id);
        } else {
            changed_job.items[0].import_id = foreign.items[0].import_id.clone();
            changed_job.items[0].revision_id = foreign.items[0].revision_id.clone();
            if fault == "coordinated-result" {
                changed_receipt["importId"] = json!(foreign.items[0].import_id);
                changed_receipt["revisionId"] = json!(foreign.items[0].revision_id);
            }
        }
        let tx = conn.unchecked_transaction().unwrap();
        save_display_fault(&tx, &changed_job);
        tx.execute(
            "UPDATE receipts SET import_id=?1,revision_id=?2,json=?3 WHERE receipt_id=?4",
            params![
                changed_receipt["importId"].as_str().unwrap(),
                changed_receipt["revisionId"].as_str().unwrap(),
                changed_receipt.to_string(),
                receipt.receipt_id
            ],
        )
        .unwrap();
        tx.commit().unwrap();
        assert!(conn.is_autocommit());
        let before = raw_catalog_rows(&conn);
        let native_before = native_store_bytes();
        let result = engine::undo_import_job(&refreshed.job_id);
        let unchanged = raw_catalog_rows(&conn) == before;
        println!(
            "ORDINARY_UNDO_FOREIGN={}",
            json!({"fault":fault,"code":result.as_ref().err().map(|error| error.code.as_str()),"returnedPhase":result.as_ref().ok().map(|job| &job.items[0].phase),"catalogUnchanged":unchanged})
        );
        let error = result.expect_err("ordinary Undo cannot target another entry or revision");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert!(unchanged, "ordinary refusal must be atomic");
        assert!(native_store_bytes() == native_before);
        for (path, expected) in hashes {
            assert_eq!(jobs::published_hashes(&path).unwrap(), expected);
        }
        require_seed(
            &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
            &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
        );
    }
}
