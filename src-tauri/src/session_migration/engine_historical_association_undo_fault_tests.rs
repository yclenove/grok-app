//! Committed faults cross the real public Undo transaction, one fresh home each.
use super::*;
use crate::session_migration::publication_protocol::PublicationProtocol;
use rusqlite::params;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_receipt_faults_are_atomic() {
    receipt_faults_are_atomic(PublicationProtocol::SignedOutcomeV1);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_legacy_receipt_faults_are_atomic() {
    receipt_faults_are_atomic(PublicationProtocol::Legacy);
}

fn receipt_faults_are_atomic(protocol: PublicationProtocol) {
    for (fault, expected) in [
        ("created-entry", ErrorCode::HistoryAccessUnavailable),
        ("json-job", ErrorCode::HistoryAccessUnavailable),
        ("json-candidate", ErrorCode::HistoryAccessUnavailable),
        ("json-import", ErrorCode::HistoryAccessUnavailable),
        ("json-revision", ErrorCode::HistoryAccessUnavailable),
        ("coordinated-import", ErrorCode::HistoryAccessUnavailable),
        ("coordinated-revision", ErrorCode::HistoryAccessUnavailable),
        ("phase-json", ErrorCode::HistoryAccessUnavailable),
        ("phase-sql", ErrorCode::HistoryAccessUnavailable),
        ("phase-both", ErrorCode::HistoryAccessUnavailable),
        ("delete-latest", ErrorCode::HistoryAccessUnavailable),
        ("blob", ErrorCode::HistoryAccessUnavailable),
        ("invalid-utf8", ErrorCode::HistoryAccessUnavailable),
        ("oversized", ErrorCode::ResourceLimit),
        ("malformed", ErrorCode::SchemaUnsupported),
        ("future-version", ErrorCode::SchemaUnsupported),
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let request = format!("synthetic-public-undo-{protocol:?}-{fault}");
        let (_, completed) = match protocol {
            PublicationProtocol::Legacy => publish_legacy(&request),
            PublicationProtocol::SignedOutcomeV1 => publish(&request),
        };
        assert_eq!(
            PublicationProtocol::for_job_id(&completed.job_id).unwrap(),
            protocol
        );
        // The signed prefix parser rejects damaged bytes as unavailable;
        // the historical receipt reader retains its original error taxonomy.
        let expected = if protocol == PublicationProtocol::SignedOutcomeV1
            && matches!(fault, "oversized" | "malformed")
        {
            ErrorCode::HistoryAccessUnavailable
        } else {
            expected
        };
        let revision = completed.items[0].revision_id.as_deref().unwrap();
        let old_hashes = immutable_hashes(REVISION);
        let new_hashes = immutable_hashes(revision);
        let receipt =
            jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
                .unwrap()
                .unwrap();
        let mut value = serde_json::to_value(&receipt).unwrap();
        let foreign_revision = Uuid::new_v4().to_string();
        let foreign_import = format!("imp:{}", Uuid::new_v4());
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "created-entry" => {
                value["previousRevision"] = json!(null);
                value["createdEntry"] = json!(true);
            }
            "json-job" => value["jobId"] = json!(Uuid::new_v4().to_string()),
            "json-candidate" => value["candidateId"] = json!("foreign-candidate"),
            "json-import" | "coordinated-import" => value["importId"] = json!(foreign_import),
            "json-revision" | "coordinated-revision" => {
                value["revisionId"] = json!(foreign_revision);
            }
            "phase-json" | "phase-both" => value["phase"] = json!("prepared"),
            "future-version" => value["version"] = json!(99),
            _ => {}
        }
        let json = match fault {
            "oversized" => " ".repeat(16 * 1024 * 1024 + 1),
            "malformed" => "{".into(),
            _ => value.to_string(),
        };
        match fault {
            "blob" => {
                tx.execute(
                    "UPDATE receipts SET json=zeroblob(1) WHERE receipt_id=?1",
                    [&receipt.receipt_id],
                )
                .unwrap();
            }
            "invalid-utf8" => {
                tx.execute(
                    "UPDATE receipts SET json=CAST(x'ff' AS TEXT) WHERE receipt_id=?1",
                    [&receipt.receipt_id],
                )
                .unwrap();
            }
            "delete-latest" => {
                assert_eq!(
                    tx.execute(
                        "DELETE FROM receipts WHERE receipt_id=?1",
                        [&receipt.receipt_id]
                    )
                    .unwrap(),
                    1
                );
            }
            _ => {
                tx.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    params![json, receipt.receipt_id],
                )
                .unwrap();
            }
        }
        match fault {
            "coordinated-import" => {
                tx.execute(
                    "UPDATE receipts SET import_id=?1 WHERE receipt_id=?2",
                    params![foreign_import, receipt.receipt_id],
                )
                .unwrap();
            }
            "coordinated-revision" => {
                tx.execute(
                    "UPDATE receipts SET revision_id=?1 WHERE receipt_id=?2",
                    params![foreign_revision, receipt.receipt_id],
                )
                .unwrap();
            }
            "phase-sql" | "phase-both" => {
                tx.execute(
                    "UPDATE receipts SET phase='prepared' WHERE receipt_id=?1",
                    [&receipt.receipt_id],
                )
                .unwrap();
            }
            _ => {}
        }
        tx.commit().unwrap();
        assert!(conn.is_autocommit());
        let after_fault = raw_catalog_rows(&conn);
        let result = engine::undo_import_job(&completed.job_id);
        let unchanged = raw_catalog_rows(&conn) == after_fault;
        println!(
            "ASSOCIATION_PUBLIC_UNDO_RECEIPT_FAULT={}",
            json!({"protocol": format!("{protocol:?}"), "fault": fault,
                "code": result.as_ref().err().map(|error| error.code.as_str()),
                "catalogUnchanged": unchanged})
        );
        let error = result.expect_err("damaged receipt must refuse public Undo");
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        assert!(unchanged, "{fault}: catalog row/type/byte changed");
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
fn historical_seed_association_undo_missing_receipt_is_an_explicit_conflict() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (_, completed) = publish_legacy("synthetic-public-undo-all-legacy-receipts-missing");
    assert_eq!(
        PublicationProtocol::for_job_id(&completed.job_id).unwrap(),
        PublicationProtocol::Legacy
    );
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    conn.execute("DELETE FROM receipts WHERE job_id=?1", [&completed.job_id])
        .unwrap();
    let stable_rows = || {
        raw_catalog_rows(&conn)
            .into_iter()
            .filter(|(table, _)| !matches!(table.as_str(), "jobs" | "job_items"))
            .collect::<Vec<_>>()
    };
    let after_fault = stable_rows();
    let conflict = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(conflict.items[0].phase, "conflict");
    assert_eq!(
        conflict.items[0].error.as_deref(),
        Some("ownership receipt missing")
    );
    assert_eq!(conflict.state_version, completed.state_version + 1);
    assert_eq!(stable_rows(), after_fault);
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(revision)
    );
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    println!(
        "ASSOCIATION_PUBLIC_UNDO_MISSING_RECEIPT=explicit-conflict:target-and-archives-preserved"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_signed_missing_receipt_is_atomic() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (_, completed) = publish("synthetic-public-undo-all-signed-receipts-missing");
    assert_eq!(
        PublicationProtocol::for_job_id(&completed.job_id).unwrap(),
        PublicationProtocol::SignedOutcomeV1
    );
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let deleted = conn
        .execute("DELETE FROM receipts WHERE job_id=?1", [&completed.job_id])
        .unwrap();
    assert_eq!(deleted, 5);
    let after_fault = raw_catalog_rows(&conn);
    for attempt in 0..2 {
        let error = engine::undo_import_job(&completed.job_id).unwrap_err();
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(raw_catalog_rows(&conn), after_fault);
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(revision), new_hashes);
        println!(
            "ASSOCIATION_PUBLIC_UNDO_SIGNED_MISSING_RECEIPT={}",
            json!({"attempt": attempt, "code": error.code.as_str(),
                "allCatalogRowsUnchanged": true, "archivesUnchanged": true})
        );
        reset_runtime();
    }
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_cannot_select_an_independent_published_entry() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let independent_plan = plan(previews, vec![]).unwrap();
    let independent = commit(
        &independent_plan.plan_id,
        &independent_plan.plan_hash,
        "synthetic-public-undo-independent-entry",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let independent = job_get(&independent.job_id).unwrap();
    assert_eq!(independent.state, JobState::Completed);
    assert_eq!(independent.items[0].phase, "committed");
    let foreign = &independent.items[0];
    let foreign_import = foreign.import_id.as_deref().unwrap();
    let foreign_revision = foreign.revision_id.as_deref().unwrap();
    assert_ne!(foreign_import, IMPORT);
    let foreign_dir = catalog::revision_dir(foreign_import, foreign_revision).unwrap();
    let foreign_hashes = jobs::published_hashes(&foreign_dir).unwrap();
    let foreign_history =
        events_page(foreign_import, Some(foreign_revision), None, Some(20), None).unwrap();
    assert_eq!(foreign_history.2, 1);
    assert!(serde_json::to_value(&foreign_history.0)
        .unwrap()
        .to_string()
        .contains("SYNTHETIC_REGISTERED_HISTORY_"));
    let (_, completed) = publish("synthetic-public-undo-foreign-result");
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let mut value = serde_json::to_value(&completed).unwrap();
    value["items"][0]["importId"] = json!(foreign_import);
    value["items"][0]["revisionId"] = json!(foreign_revision);
    assert!(conn.is_autocommit());
    conn.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        params![value.to_string(), completed.job_id],
    )
    .unwrap();
    let after_fault = raw_catalog_rows(&conn);
    let error = engine::undo_import_job(&completed.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(raw_catalog_rows(&conn), after_fault);
    assert_eq!(
        jobs::published_hashes(&foreign_dir).unwrap(),
        foreign_hashes
    );
    let history_after =
        events_page(foreign_import, Some(foreign_revision), None, Some(20), None).unwrap();
    assert_eq!(
        serde_json::to_value(history_after.0).unwrap(),
        serde_json::to_value(foreign_history.0).unwrap()
    );
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    println!("ASSOCIATION_PUBLIC_UNDO_FOREIGN_RESULT=HISTORY_ACCESS_UNAVAILABLE:all-catalog-rows-and-three-archives-preserved");
}
