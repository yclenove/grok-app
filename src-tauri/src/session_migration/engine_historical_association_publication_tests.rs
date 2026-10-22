//! Genuine accepted association through the existing worker and publisher.
use super::*;
use crate::session_migration::catalog;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_publication_refuses_detached_or_changed_owner() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    // Legacy fault probes intentionally run inside a rollback-only snapshot.
    // Signed publisher positives/negatives use the real worker ACK protocol.
    let confirmed = historical_legacy_plan(&confirmed_plan());
    let queued = acceptance(&confirmed, "synthetic-publication-owner-negatives").unwrap();
    let item =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let prepared = item.prepared.as_ref().unwrap();
    let lease = std::fs::read(prepared.path.join("lease.json")).unwrap();
    let hashes = jobs::prepared_hashes(&prepared.path).unwrap();
    let old_receipts = receipts(&conn);
    let old_text = text(IMPORT, REVISION);
    for (fault, expected) in [
        ("missing-proof", ErrorCode::HistoryAccessUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("input-bytes", ErrorCode::PlanStale),
        ("hidden", ErrorCode::HistoryAccessUnavailable),
        ("missing-target", ErrorCode::HistoryAccessUnavailable),
        ("target-native", ErrorCode::HistoryAccessUnavailable),
        ("target-revision", ErrorCode::MappingConflict),
        ("source-host", ErrorCode::MappingConflict),
        ("old-tombstone", ErrorCode::HistoryAccessUnavailable),
        ("new-tombstone", ErrorCode::HistoryAccessUnavailable),
        ("item-title", ErrorCode::HistoryAccessUnavailable),
        ("foreign-job", ErrorCode::PlanStale),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        let mut changed = item.clone();
        let mut job_id = queued.job_id.clone();
        match fault {
            "missing-proof" => {
                tx.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "missing-witness" => {
                assert_eq!(
                    tx.execute(
                        "DELETE FROM meta WHERE key LIKE ?1",
                        [format!(
                            "registration_association_accepted:{}:%",
                            queued.job_id
                        )]
                    )
                    .unwrap(),
                    1
                );
            }
            "input-bytes" => {
                tx.execute(
                    "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
                    [&queued.job_id],
                )
                .unwrap();
            }
            "hidden" => {
                tx.execute("UPDATE entries SET hidden=1 WHERE import_id=?1", [IMPORT])
                    .unwrap();
            }
            "missing-target" => {
                tx.execute("DELETE FROM entries WHERE import_id=?1", [IMPORT])
                    .unwrap();
            }
            "target-native" => {
                tx.execute(
                    "UPDATE entries SET native_session_id='foreign' WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "target-revision" => {
                tx.execute(
                    "UPDATE entries SET current_revision=NULL WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "source-host" => {
                tx.execute(
                    "UPDATE sources SET host='remote' WHERE installation_id=?1",
                    [INSTALLATION],
                )
                .unwrap();
            }
            "old-tombstone" | "new-tombstone" => {
                let native = if fault == "old-tombstone" {
                    "transcript"
                } else {
                    &item.adapter.graph.native_session_id
                };
                tx.execute("INSERT INTO tombstones(installation_id,provider,native_session_id,deleted_at) VALUES(?1,'cursor',?2,'synthetic')", rusqlite::params![INSTALLATION, native]).unwrap();
            }
            "item-title" => {
                changed.adapter.graph.title = Some("synthetic-detached-title".into());
            }
            "foreign-job" => {
                job_id = Uuid::new_v4().to_string();
            }
            _ => unreachable!(),
        }
        let error = jobs::publish_item(&tx, &job_id, &changed, &|| false).unwrap_err();
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        assert!(
            jobs::latest_receipt(&tx, &queued.job_id, &item.candidate_id)
                .unwrap()
                .is_none()
        );
        tx.rollback().unwrap();
        assert_eq!(receipts(&conn), old_receipts);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(
            std::fs::read(prepared.path.join("lease.json")).unwrap(),
            lease
        );
        assert_eq!(jobs::prepared_hashes(&prepared.path).unwrap(), hashes);
        println!(
            "ASSOCIATION_PUBLICATION_OWNER_REFUSAL={fault}:{}",
            error.code.as_str()
        );
    }
    drop(item);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    assert_eq!(text(IMPORT, REVISION), old_text);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publishes_existing_target() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let old_job = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    let confirmed = confirmed_plan();
    let queued = commit(
        &confirmed.plan_id,
        &confirmed.plan_hash,
        "synthetic-real-association-publication",
    )
    .unwrap();
    assert_eq!(queued.items.len(), 1);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(text(IMPORT, REVISION), old_text);
    let current_receipts = receipts(&conn);
    for original in &old_receipts {
        assert!(
            current_receipts.contains(original),
            "old receipt bytes changed"
        );
    }
    assert_eq!(
        serde_json::to_value(jobs::get_job(&conn, OLD_JOB).unwrap().unwrap()).unwrap(),
        serde_json::to_value(old_job).unwrap()
    );
    for (name, expected) in ARCHIVE_FILES
        .iter()
        .filter(|(name, _)| !name.starts_with("event-index"))
    {
        require_hash(&home.path.join(ARCHIVE).join(name), expected);
    }
    println!(
        "ASSOCIATION_REAL_PUBLICATION={}",
        json!({"state": completed.state, "phase": completed.items[0].phase,
            "error": completed.items[0].error, "oldHistoryReadable": true,
            "oldReceiptsUnchanged": true, "catalogEntries": 1})
    );
    assert_eq!(
        completed.items[0].phase, "committed",
        "real worker must publish accepted association"
    );
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    assert_ne!(revision, REVISION);
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &queued.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.import_id.as_deref(), Some(IMPORT));
    assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
    assert!(!receipt.created_entry);
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(revision)
    );
    assert_eq!(text(IMPORT, revision), old_text);
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(text(IMPORT, revision), old_text);
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(
        commit(
            &confirmed.plan_id,
            &confirmed.plan_hash,
            "synthetic-real-association-publication",
        )
        .unwrap()
        .job_id,
        queued.job_id
    );
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_real_publication_crash_recovery() {
    for (point, phase) in [
        ("prepared", "prepared"),
        ("staged", "staged"),
        ("after_rename", "staged"),
        ("after_receipt_published", "published"),
        ("linked", "linked"),
        ("after_catalog_commit", "committed"),
    ] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let old_text = text(IMPORT, REVISION);
        let old_receipts = receipts(&conn);
        let report = crate::session_migration::preview_lifecycle_tests::association_crash_report_at(
            &home,
            &format!("association-publication-{point}"),
        );
        let requested: JobRecord = serde_json::from_value(report["job"].clone()).unwrap();
        let running = jobs::get_job(&conn, &requested.job_id).unwrap().unwrap();
        assert_eq!(running.state, JobState::Running);
        assert_eq!(
            running.items[0].phase,
            if phase == "committed" {
                "committed"
            } else {
                "queued"
            },
            "completion ACK must be atomic with the final catalog receipt"
        );
        let receipt =
            jobs::latest_receipt(&conn, &requested.job_id, &running.items[0].candidate_id)
                .unwrap()
                .unwrap();
        assert_eq!(receipt.phase, phase);
        assert_eq!(receipt.import_id.as_deref(), Some(IMPORT));
        assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
        assert!(!receipt.created_entry);
        let revision = receipt.revision_id.as_deref().unwrap();
        let dest = catalog::revision_dir(IMPORT, revision).unwrap();
        let committed_receipts = receipts(&conn);
        if phase == "committed" {
            // Detach only this isolated completed preview, never the original
            // immutable seed. Recovery must use durable metadata + published files.
            let preview = jobs::find_preview_dir(report["previewId"].as_str().unwrap())
                .unwrap()
                .unwrap();
            std::fs::rename(preview, home.path.join("completed-preview-detached")).unwrap();
            assert!(dest.is_dir());
        }
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 1, "{point}");
        let completed = jobs::get_job(&conn, &requested.job_id).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed, "{point}");
        assert_eq!(completed.items[0].phase, "committed", "{point}");
        assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
        assert_eq!(completed.items[0].revision_id.as_deref(), Some(revision));
        assert_eq!(
            completed.state_version,
            running.state_version + if phase == "committed" { 2 } else { 3 },
            "restart must not repeat an ACK already committed with publication"
        );
        if phase == "committed" {
            assert_eq!(
                receipts(&conn),
                committed_receipts,
                "completed recovery must not republish"
            );
        }
        for original in &old_receipts {
            assert!(receipts(&conn).contains(original));
        }
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(text(IMPORT, revision), old_text);
        assert_eq!(job_control::run_pending().unwrap(), 0);
        assert_eq!(
            job_control::enqueue_confirmed_associations(requested.clone(), vec![])
                .unwrap()
                .job_id,
            requested.job_id
        );
        println!(
            "ASSOCIATION_REAL_PUBLICATION_CRASH={point}:{phase}:exit77:same-import-and-revision"
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_publication_receipt_boundaries() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let confirmed = confirmed_plan();
    let confirmed = historical_legacy_plan(&confirmed);
    let queued = acceptance(&confirmed, "synthetic-publication-receipt-negatives").unwrap();
    // Probe the real metadata recovery owner after normal completed-preview GC;
    // never restore a disposed preview merely to reach the receipt fault.
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    let previous = &completed.items[0];
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &previous.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "committed");
    let before = receipts(&conn);
    let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    for (fault, expected) in [
        ("previous-revision", ErrorCode::HistoryAccessUnavailable),
        ("created-entry", ErrorCode::HistoryAccessUnavailable),
        ("import-id", ErrorCode::HistoryAccessUnavailable),
        ("phase-column", ErrorCode::HistoryAccessUnavailable),
        ("json-type", ErrorCode::HistoryAccessUnavailable),
        ("invalid-utf8", ErrorCode::HistoryAccessUnavailable),
        ("oversized-json", ErrorCode::ResourceLimit),
        ("malformed-json", ErrorCode::SchemaUnsupported),
        ("future-version", ErrorCode::SchemaUnsupported),
        ("missing-receipt", ErrorCode::MappingConflict),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        let mut value = serde_json::to_value(&receipt).unwrap();
        match fault {
            "previous-revision" | "created-entry" | "import-id" | "future-version" => {
                match fault {
                    "previous-revision" => value["previousRevision"] = json!(null),
                    "created-entry" => value["createdEntry"] = json!(true),
                    "import-id" => {
                        let foreign = format!("imp:{}", Uuid::new_v4());
                        value["importId"] = json!(foreign);
                        tx.execute(
                            "UPDATE receipts SET import_id=?1 WHERE receipt_id=?2",
                            rusqlite::params![foreign, receipt.receipt_id],
                        )
                        .unwrap();
                    }
                    "future-version" => value["version"] = json!(99),
                    _ => unreachable!(),
                }
                tx.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    rusqlite::params![value.to_string(), receipt.receipt_id],
                )
                .unwrap();
            }
            "phase-column" => {
                tx.execute(
                    "UPDATE receipts SET phase='prepared' WHERE receipt_id=?1",
                    [&receipt.receipt_id],
                )
                .unwrap();
            }
            "json-type" => {
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
            "oversized-json" => {
                tx.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    rusqlite::params![" ".repeat(16 * 1024 * 1024 + 1), receipt.receipt_id],
                )
                .unwrap();
            }
            "malformed-json" => {
                tx.execute(
                    "UPDATE receipts SET json='{' WHERE receipt_id=?1",
                    [&receipt.receipt_id],
                )
                .unwrap();
            }
            "missing-receipt" => {
                tx.execute("DELETE FROM receipts WHERE job_id=?1", [&queued.job_id])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let error = job_control::recover_test_input(&tx, &queued.job_id, previous).unwrap_err();
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        tx.rollback().unwrap();
        assert_eq!(receipts(&conn), before);
        assert_eq!(
            serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
            entry
        );
        println!(
            "ASSOCIATION_PUBLICATION_RECEIPT_REFUSAL={fault}:{}",
            error.code.as_str()
        );
    }
    let repeated = job_control::recover_test_input(&conn, &queued.job_id, previous)
        .unwrap()
        .unwrap();
    assert_eq!(repeated.import_id, receipt.import_id);
    assert_eq!(repeated.revision_id, receipt.revision_id);
    assert_eq!(receipts(&conn), before);
}
