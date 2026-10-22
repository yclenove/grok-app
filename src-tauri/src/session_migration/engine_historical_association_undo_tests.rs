//! Public Undo of genuinely accepted/published historical associations.
//! Faults are committed only in fresh disposable homes, never an outer writer.
use super::*;
use crate::session_migration::{catalog, engine, engine::legacy_job_fixture::historical_plan};
use rusqlite::Connection;
use std::collections::BTreeMap;

#[path = "engine_historical_association_undo_fault_tests.rs"]
mod fault_tests;

#[path = "engine_historical_association_continuation_tests.rs"]
mod continuation_tests;

#[path = "engine_historical_association_refresh_tests.rs"]
mod refresh_tests;

#[path = "engine_historical_association_undo_ownership_tests.rs"]
mod ownership_tests;

fn publish(request: &str) -> (PlanState, JobRecord) {
    publish_plan(confirmed_plan(), request)
}

fn publish_legacy(request: &str) -> (PlanState, JobRecord) {
    publish_plan(historical_plan(&confirmed_plan()), request)
}

fn publish_plan(confirmed: PlanState, request: &str) -> (PlanState, JobRecord) {
    let queued = acceptance(&confirmed, request).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let conn = open_catalog().unwrap();
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items.len(), 1);
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    assert_ne!(completed.items[0].revision_id.as_deref(), Some(REVISION));
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &queued.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
    assert!(!receipt.created_entry);
    (confirmed, completed)
}

fn immutable_hashes(revision: &str) -> BTreeMap<String, String> {
    jobs::published_hashes(&catalog::revision_dir(IMPORT, revision).unwrap()).unwrap()
}

fn proof_bytes(conn: &Connection) -> (String, String) {
    conn.query_row(
        "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
        [IMPORT],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

// Change mutable results through their existing layout, so ownership probes
// reach their intended fault rather than an unrelated downgraded-header refusal.
fn save_display_fault(conn: &Connection, job: &JobRecord) {
    let split = jobs::item_store::uses_rows(conn, &job.job_id).unwrap();
    jobs::save_job(conn, job).unwrap();
    assert_eq!(
        jobs::item_store::uses_rows(conn, &job.job_id).unwrap(),
        split
    );
    let stored: Vec<jobs::JobItem> =
        jobs::item_store::page(conn, &job.job_id, 0, usize::MAX).unwrap();
    assert_eq!(
        serde_json::to_value(stored).unwrap(),
        serde_json::to_value(&job.items).unwrap()
    );
}

#[derive(Debug, PartialEq)]
enum RawCell {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

// rusqlite's owned Value decodes TEXT as UTF8 and panics on the deliberately
// invalid bytes in N7. Capture type + raw bytes, including malformed TEXT,
// before the public call; do not normalize or repair the injected fault.
fn raw_catalog_rows(conn: &Connection) -> Vec<(String, Vec<Vec<RawCell>>)> {
    let tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let quoted = table.replace('"', "\"\"");
            let mut statement = conn
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| {
                            use rusqlite::types::ValueRef;
                            Ok(match row.get_ref(column)? {
                                ValueRef::Null => RawCell::Null,
                                ValueRef::Integer(value) => RawCell::Integer(value),
                                ValueRef::Real(value) => RawCell::Real(value.to_bits()),
                                ValueRef::Text(value) => RawCell::Text(value.to_vec()),
                                ValueRef::Blob(value) => RawCell::Blob(value.to_vec()),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

// Successful Undo changes the same progress in either layout. Project only
// this job's expected writes; every other cell, including input bytes, stays exact.
fn with_job_progress(
    conn: &Connection,
    mut rows: Vec<(String, Vec<Vec<RawCell>>)>,
    job: &JobRecord,
) -> Vec<(String, Vec<Vec<RawCell>>)> {
    let split = jobs::item_store::uses_rows(conn, &job.job_id).unwrap();
    for (table, records) in &mut rows {
        if !matches!(table.as_str(), "jobs" | "job_items" | "job_item_states") {
            continue;
        }
        for row in records {
            if !matches!(&row[1], RawCell::Text(id) if id == job.job_id.as_bytes()) {
                continue;
            }
            match table.as_str() {
                "jobs" => {
                    row[4] = RawCell::Text(job.state.as_str().as_bytes().to_vec());
                    row[5] = RawCell::Integer(job.state_version.into());
                    row[6] = RawCell::Text(
                        jobs::item_store::encode_header(job, split)
                            .unwrap()
                            .into_bytes(),
                    );
                }
                "job_items" => {
                    let item = job
                        .items
                        .iter()
                        .find(|item| row[2] == RawCell::Text(item.candidate_id.as_bytes().to_vec()))
                        .unwrap();
                    row[3] = RawCell::Text(item.phase.as_bytes().to_vec());
                }
                "job_item_states" => {
                    let RawCell::Integer(index) = &row[2] else {
                        panic!("invalid display item index");
                    };
                    let item = &job.items[usize::try_from(*index).unwrap()];
                    assert_eq!(row[3], RawCell::Text(item.candidate_id.as_bytes().to_vec()));
                    row[4] = RawCell::Text(serde_json::to_vec(item).unwrap());
                }
                _ => unreachable!(),
            }
        }
    }
    rows
}

#[test]
fn historical_job_display_fault_preserves_layout_and_restores_readable_history() {
    for split in [false, true] {
        let home = IsolatedAppHome::acquire("historical-display-fault-layout");
        let _runtime = RuntimeReset;
        let planned = plan(cursor_source(&home, 1), vec![]).unwrap();
        let queued = commit(
            &planned.plan_id,
            &planned.plan_hash,
            "synthetic-display-fault-layout",
        )
        .unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let completed = job_get(&queued.job_id).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.items[0].phase, "committed");
        let conn = open_catalog().unwrap();
        if !split {
            // Exercise the older display layout with the same genuine producer
            // inputs, receipts and signatures; no authority is manufactured.
            let tx = conn.unchecked_transaction().unwrap();
            tx.execute(
                "DELETE FROM job_item_states WHERE job_id=?1",
                [&completed.job_id],
            )
            .unwrap();
            tx.execute(
                "UPDATE jobs SET json=?1 WHERE job_id=?2",
                rusqlite::params![
                    jobs::item_store::encode_header(&completed, false).unwrap(),
                    completed.job_id
                ],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            jobs::item_store::uses_rows(&conn, &completed.job_id).unwrap(),
            split
        );
        let original: String = conn
            .query_row(
                "SELECT json FROM jobs WHERE job_id=?1",
                [&completed.job_id],
                |row| row.get(0),
            )
            .unwrap();
        let before = raw_catalog_rows(&conn);
        let item = &completed.items[0];
        let read = || {
            events_page(
                item.import_id.as_deref().unwrap(),
                item.revision_id.as_deref(),
                None,
                Some(20),
                None,
            )
        };
        let history = serde_json::to_value(read().unwrap()).unwrap();
        let mut changed = completed.clone();
        changed.items[0].mapping_project_id = Some("synthetic-foreign-project".into());
        save_display_fault(&conn, &changed);
        let damaged = raw_catalog_rows(&conn);
        let error = engine::undo_import_job(&completed.job_id).unwrap_err();
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(raw_catalog_rows(&conn), damaged);
        let tx = conn.unchecked_transaction().unwrap();
        save_display_fault(&tx, &completed);
        tx.execute(
            "UPDATE jobs SET json=?1 WHERE job_id=?2",
            rusqlite::params![original, completed.job_id],
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(raw_catalog_rows(&conn), before);
        reset_runtime();
        assert_eq!(serde_json::to_value(read().unwrap()).unwrap(), history);
        assert_eq!(
            engine::undo_import_job(&completed.job_id).unwrap().items[0].phase,
            "undone"
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_restores_old_revision_without_republishing() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let old_job: String = conn
        .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get(0)
        })
        .unwrap();
    let (confirmed, completed) = publish("synthetic-association-public-undo-positive");
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let before = get_entry(&conn, IMPORT).unwrap().unwrap();
    assert_eq!(before.current_revision.as_deref(), Some(revision));
    assert!(before
        .native_session_id
        .starts_with("cursor-identity-v1:transcript-locator:"));
    let all_receipts = receipts(&conn);
    for receipt in &old_receipts {
        assert!(all_receipts.contains(receipt));
    }
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let proof = proof_bytes(&conn);

    let undone = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0].error.is_none());
    assert_eq!(undone.state_version, completed.state_version + 1);
    let check_preserved = || {
        let entry = get_entry(&conn, IMPORT).unwrap().unwrap();
        assert_eq!(entry.current_revision.as_deref(), Some(REVISION));
        assert!(!entry.hidden);
        assert_eq!(entry.native_session_id, before.native_session_id);
        assert_eq!(entry.installation_id, before.installation_id);
        assert_eq!(entry.title, before.title);
        assert_eq!(entry.alias, before.alias);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert!(!catalog::is_tombstoned(
            &conn,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id,
        )
        .unwrap());
        assert_eq!(proof_bytes(&conn), proof);
        assert_eq!(receipts(&conn), all_receipts);
        let current_old_job: String = conn
            .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(current_old_job, old_job);
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(revision), new_hashes);
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(text(IMPORT, revision), old_text);
        let count: u64 = conn
            .query_row(
                "SELECT count(*) FROM revisions WHERE import_id=?1",
                [IMPORT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    };
    check_preserved();
    reset_runtime();
    check_preserved();
    let repeated = engine::undo_import_job(&completed.job_id).unwrap();
    assert_eq!(repeated.items[0].phase, "undone");
    assert_eq!(repeated.state_version, undone.state_version + 1);
    assert_eq!(
        job_control::enqueue_confirmed_associations(
            requested_job(&confirmed, &completed.request_id),
            vec![],
        )
        .unwrap()
        .job_id,
        completed.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    check_preserved();
    for (name, expected) in ARCHIVE_FILES
        .iter()
        .filter(|(name, _)| !name.starts_with("event-index"))
    {
        require_hash(&home.path.join(ARCHIVE).join(name), expected);
    }
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!(
        "ASSOCIATION_PUBLIC_UNDO_POSITIVE={}",
        json!({"oldCurrentRestored": true, "bothArchivesReadable": true,
            "receiptBytesPreserved": true, "proofPreserved": true,
            "restartAndRetry": true, "revisionCount": 2,
            "initialVersion": completed.state_version,
            "undoneVersion": undone.state_version,
            "repeatedVersion": repeated.state_version})
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_refuses_foreign_previous_revision_atomically() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (_, completed) = publish("synthetic-association-public-undo-previous-fault");
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    let old_hashes = immutable_hashes(REVISION);
    let new_hashes = immutable_hashes(revision);
    let receipt = jobs::latest_receipt(&conn, &completed.job_id, &completed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    let foreign = Uuid::new_v4().to_string();
    let mut changed = serde_json::to_value(&receipt).unwrap();
    changed["previousRevision"] = json!(foreign);
    assert!(conn.is_autocommit());
    assert_eq!(
        conn.execute(
            "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
            rusqlite::params![changed.to_string(), receipt.receipt_id],
        )
        .unwrap(),
        1
    );
    assert!(
        conn.is_autocommit(),
        "fault must be committed before public Undo"
    );
    let after_fault = raw_catalog_rows(&conn);
    let result = engine::undo_import_job(&completed.job_id);
    let current = get_entry(&conn, IMPORT).unwrap().unwrap().current_revision;
    let unchanged = raw_catalog_rows(&conn) == after_fault;
    println!(
        "ASSOCIATION_PUBLIC_UNDO_PREVIOUS_FAULT={}",
        json!({"code": result.as_ref().err().map(|error| error.code.as_str()),
            "returnedPhase": result.as_ref().ok().map(|job| &job.items[0].phase),
            "foreignCurrentInstalled": current.as_deref() == Some(foreign.as_str()),
            "catalogUnchanged": unchanged})
    );
    assert_eq!(immutable_hashes(REVISION), old_hashes);
    assert_eq!(immutable_hashes(revision), new_hashes);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    let error = result.expect_err("foreign previousRevision must not authorize Undo");
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(
        unchanged,
        "public refusal must preserve every catalog row/type/byte"
    );
    assert_eq!(current.as_deref(), Some(revision));
}
