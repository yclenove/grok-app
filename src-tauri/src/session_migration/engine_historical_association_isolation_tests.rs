//! Genuine independent ordinary jobs beside negatively damaged acceptance.
//! Captured producer rows are restored before assertions; no positive SQL seed.
use super::*;
use rusqlite::{params, types::Value, Connection};

#[path = "engine_historical_publication_exclusion_tests.rs"]
mod exclusion_tests;

pub(super) type CatalogRows = Vec<(String, Vec<Vec<Value>>)>;
type RawProofRows = Vec<(i64, String, String, String)>;

#[derive(Clone, Copy, Debug)]
enum Read {
    Get,
    Page,
    Retry,
}

impl Read {
    fn run(self, plan: &PlanState, job: &JobRecord) -> Result<JobRecord, ImportError> {
        match self {
            Self::Get => job_get(&job.job_id),
            Self::Page => job_items_page(&job.job_id, None, Some(1)),
            Self::Retry => commit(&plan.plan_id, &plan.plan_hash, &job.request_id),
        }
    }
}

// Include all catalog rowids/types/bytes, not just counts or interpreted proofs.
// Identifier names come exclusively from the isolated catalog's own schema.
pub(super) fn catalog_rows(conn: &Connection) -> CatalogRows {
    catalog_table_rows(conn, false)
}

fn catalog_table_rows(conn: &Connection, exclude_proof: bool) -> CatalogRows {
    let tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .filter(|table| !exclude_proof || table != "registration_provenance")
        .map(|table| {
            let quoted = table.replace('"', "\"\"");
            let mut statement = conn
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            let values = statement
                .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
                .unwrap()
                .collect::<Result<Vec<Vec<Value>>, _>>()
                .unwrap();
            (table, values)
        })
        .collect()
}

struct FaultRows {
    proof: (i64, String, String),
    input: (i64, String, String),
    owner: (i64, String, String),
    accepted_key: String,
    accepted: (i64, String),
    owner_key: String,
    owner_marker: (i64, String),
}

impl FaultRows {
    fn capture(conn: &Connection, job: &JobRecord) -> Self {
        let proof = conn
            .query_row(
                "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
                [IMPORT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let input = conn
            .query_row(
                "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1",
                [&job.job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let owner: (i64, String, String) = conn.query_row(
            "SELECT rowid,tuple_digest,proof_digest FROM registration_association_old_owners WHERE import_id=?1",
            [IMPORT], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        let accepted_key = format!(
            "registration_association_accepted:{}:{}",
            job.job_id,
            hash::sha256_bytes(job.items[0].candidate_id.as_bytes())
        );
        let owner_key = format!("registration_association_old_owner:{}", owner.1);
        let marker = |key: &str| {
            conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
        };
        Self {
            proof,
            input,
            owner,
            accepted: marker(&accepted_key),
            owner_marker: marker(&owner_key),
            accepted_key,
            owner_key,
        }
    }

    fn damage(&self, conn: &Connection, job: &JobRecord, fault: &str) {
        match fault {
            "proof-json" => {
                conn.execute(
                    "UPDATE registration_provenance SET json='{' WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "proof-digest" => {
                conn.execute(
                    "UPDATE registration_provenance SET digest=?1 WHERE import_id=?2",
                    params!["0".repeat(64), IMPORT],
                )
                .unwrap();
            }
            "proof-utf8" => {
                conn.execute("UPDATE registration_provenance SET json=CAST(x'ff' AS TEXT) WHERE import_id=?1", [IMPORT]).unwrap();
            }
            "proof-oversize" => {
                conn.pragma_update(None, "ignore_check_constraints", true)
                    .unwrap();
                conn.execute(
                    "UPDATE registration_provenance SET json=?1 WHERE import_id=?2",
                    params![" ".repeat(32769), IMPORT],
                )
                .unwrap();
                conn.pragma_update(None, "ignore_check_constraints", false)
                    .unwrap();
            }
            "missing-proof" => {
                conn.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "null-input" | "downgraded-input" => {
                let mut value: serde_json::Value = serde_json::from_str(&self.input.2).unwrap();
                if fault == "null-input" {
                    value["identityAssociation"] = json!(null);
                } else {
                    value["version"] = json!(2);
                    value.as_object_mut().unwrap().remove("identityAssociation");
                }
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
                    params![value.to_string(), job.job_id],
                )
                .unwrap();
            }
            "missing-witness" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&self.accepted_key])
                    .unwrap();
            }
            "selectors-removed" => {
                conn.execute(
                    "DELETE FROM registration_association_old_owners WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
                conn.execute(
                    "DELETE FROM meta WHERE key=?1 OR key=?2",
                    params![self.accepted_key, self.owner_key],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
    }

    fn restore(&self, conn: &Connection, job: &JobRecord, fault: &str) {
        let tx = conn.unchecked_transaction().unwrap();
        if fault.starts_with("proof-") || fault == "missing-proof" {
            tx.execute("INSERT OR REPLACE INTO registration_provenance(rowid,import_id,json,digest) VALUES(?1,?2,?3,?4)",
                params![self.proof.0, IMPORT, self.proof.1, self.proof.2]).unwrap();
        }
        if matches!(fault, "null-input" | "downgraded-input") {
            tx.execute(
                "UPDATE job_items SET result_json=?1 WHERE rowid=?2 AND job_id=?3 AND phase=?4",
                params![self.input.2, self.input.0, job.job_id, self.input.1],
            )
            .unwrap();
        }
        if matches!(fault, "missing-witness" | "selectors-removed") {
            tx.execute(
                "INSERT INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                params![self.accepted.0, self.accepted_key, self.accepted.1],
            )
            .unwrap();
        }
        if fault == "selectors-removed" {
            tx.execute("INSERT INTO registration_association_old_owners(rowid,tuple_digest,import_id,proof_digest) VALUES(?1,?2,?3,?4)",
                params![self.owner.0, self.owner.1, IMPORT, self.owner.2]).unwrap();
            tx.execute(
                "INSERT INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                params![self.owner_marker.0, self.owner_key, self.owner_marker.1],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
}

fn ordinary_history(job: &JobRecord) -> serde_json::Value {
    let item = &job.items[0];
    let (events, cursor, total) = events_page(
        item.import_id.as_deref().unwrap(),
        item.revision_id.as_deref(),
        None,
        Some(20),
        None,
    )
    .unwrap();
    assert!(cursor.is_none());
    assert_eq!(total, 1);
    assert_eq!(events.len(), 1);
    let value = serde_json::to_value(events).unwrap();
    assert!(value.to_string().contains("SYNTHETIC_REGISTERED_HISTORY_"));
    value
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_independent_job_isolation() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 2);
    let ordinary_plan = plan(vec![previews[0].clone()], vec![]).unwrap();
    let ordinary = commit(
        &ordinary_plan.plan_id,
        &ordinary_plan.plan_hash,
        "synthetic-isolation-completed",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let ordinary = job_get(&ordinary.job_id).unwrap();
    assert_eq!(ordinary.state, JobState::Completed);
    let later_plan = plan(vec![previews[1].clone()], vec![]).unwrap();
    let association_plan = confirmed_plan();
    let association = acceptance(&association_plan, "synthetic-isolation-association").unwrap();
    let later = commit(
        &later_plan.plan_id,
        &later_plan.plan_hash,
        "synthetic-isolation-later",
    )
    .unwrap();
    assert_eq!(later.state, JobState::Queued);
    let rows = FaultRows::capture(&conn, &association);
    let history = ordinary_history(&ordinary);
    let old_history = text(IMPORT, REVISION);
    let archive = jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap();
    let ordinary_archive = jobs::published_hashes(
        &crate::session_migration::catalog::revision_dir(
            ordinary.items[0].import_id.as_deref().unwrap(),
            ordinary.items[0].revision_id.as_deref().unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let leases: Vec<_> = association_plan
        .preview_ids
        .iter()
        .chain(&later_plan.preview_ids)
        .map(|id| {
            let path = jobs::find_preview_dir(id)
                .unwrap()
                .unwrap()
                .join("lease.json");
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    let baseline = catalog_rows(&conn);
    let mut violations = Vec::new();
    for (fault, expected) in [
        ("proof-json", ErrorCode::SchemaUnsupported),
        ("proof-digest", ErrorCode::SchemaUnsupported),
        ("proof-utf8", ErrorCode::SnapshotUnavailable),
        ("proof-oversize", ErrorCode::SchemaUnsupported),
        ("missing-proof", ErrorCode::HistoryAccessUnavailable),
        ("null-input", ErrorCode::SchemaUnsupported),
        ("downgraded-input", ErrorCode::SnapshotUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("selectors-removed", ErrorCode::HistoryAccessUnavailable),
    ] {
        rows.damage(&conn, &association, fault);
        let damaged = catalog_rows_raw_proof_safe(&conn);
        for read in [Read::Get, Read::Page, Read::Retry] {
            jobs::take_job_item_deserializes();
            let bad = read.run(&association_plan, &association);
            let decoded = jobs::take_job_item_deserializes();
            let completed = read.run(&ordinary_plan, &ordinary);
            let queued = read.run(&later_plan, &later);
            let healthy = |result: &Result<JobRecord, ImportError>, job: &JobRecord| {
                result.as_ref().is_ok_and(|returned| {
                    returned.job_id == job.job_id
                        && returned.state == job.state
                        && returned.state_version == job.state_version
                        && serde_json::to_value(&returned.items).unwrap()
                            == serde_json::to_value(&job.items).unwrap()
                })
            };
            println!(
                "ASSOCIATION_JOB_ISOLATION={}",
                json!({"fault": fault, "entry": format!("{read:?}"),
                "associationError": bad.as_ref().err().map(|e| e.code), "associationJobItemDecodes": decoded,
                "ordinaryError": completed.as_ref().err().map(|e| e.code), "laterError": queued.as_ref().err().map(|e| e.code)})
            );
            if bad.as_ref().err().map(|error| error.code) != Some(expected)
                || decoded != 0
                || !healthy(&completed, &ordinary)
                || !healthy(&queued, &later)
            {
                violations.push(format!(
                    "{fault}/{read:?}: association={:?},decodes={decoded},ordinary={:?},later={:?}",
                    bad.as_ref().err().map(|e| e.code),
                    completed.as_ref().err().map(|e| e.code),
                    queued.as_ref().err().map(|e| e.code)
                ));
            }
        }
        let after_read = catalog_rows_raw_proof_safe(&conn);
        rows.restore(&conn, &association, fault);
        assert_eq!(
            after_read, damaged,
            "read/retry must not mutate fault {fault}"
        );
        assert_eq!(
            catalog_rows(&conn),
            baseline,
            "restore exact captured rows for {fault}"
        );
        assert_eq!(ordinary_history(&ordinary), history);
        assert_eq!(text(IMPORT, REVISION), old_history);
        assert_eq!(
            jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap(),
            archive
        );
        assert_eq!(
            jobs::published_hashes(
                &crate::session_migration::catalog::revision_dir(
                    ordinary.items[0].import_id.as_deref().unwrap(),
                    ordinary.items[0].revision_id.as_deref().unwrap()
                )
                .unwrap()
            )
            .unwrap(),
            ordinary_archive
        );
        for (path, bytes) in &leases {
            assert_eq!(std::fs::read(path).unwrap(), *bytes);
        }
        for read in [Read::Get, Read::Page, Read::Retry] {
            assert_eq!(
                read.run(&association_plan, &association).unwrap().job_id,
                association.job_id
            );
            assert_eq!(
                read.run(&ordinary_plan, &ordinary).unwrap().job_id,
                ordinary.job_id
            );
            assert_eq!(read.run(&later_plan, &later).unwrap().job_id, later.job_id);
        }
    }
    // Real worker is only exercised after restoring the acceptance, not passed
    // off as proof that a damaged association cannot starve the queue.
    assert_eq!(job_control::run_pending().unwrap(), 2);
    assert_eq!(job_get(&later.job_id).unwrap().state, JobState::Completed);
    assert_eq!(ordinary_history(&ordinary), history);
    assert!(
        violations.is_empty(),
        "independent job isolation: {violations:?}"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_unrelated_legacy_proof_isolation() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 2);
    // This existing producer publishes a genuine old-registration archive and
    // the real registered preflight then creates proof1. No positive SQL rows.
    let (legacy, accepted, _, _) = accept_legacy(vec![previews[0].clone()]);
    let ordinary_plan = plan(vec![previews[1].clone()], vec![]).unwrap();
    let queued = commit(
        &ordinary_plan.plan_id,
        &ordinary_plan.plan_hash,
        "synthetic-legacy-isolation-ordinary",
    )
    .unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 2);
    assert_eq!(
        job_get(&accepted.job_id).unwrap().state,
        JobState::Completed
    );
    let ordinary = job_get(&queued.job_id).unwrap();
    assert_eq!(ordinary.state, JobState::Completed);
    let association_plan = confirmed_plan();
    let association =
        acceptance(&association_plan, "synthetic-legacy-isolation-association").unwrap();
    let proof: (i64, String, String) = conn
        .query_row(
            "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
            [&legacy.import_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&proof.1).unwrap();
    assert_eq!(value["version"], 1);
    let original_rows = catalog_rows(&conn);
    let old_history = text(IMPORT, REVISION);
    let ordinary_text = ordinary_history(&ordinary);
    let mut violations = Vec::new();
    for fault in ["version1-unknown-field", "legacy-digest", "legacy-utf8"] {
        match fault {
            "version1-unknown-field" => {
                let mut damaged = value.clone();
                damaged["unrecognizedSyntheticField"] = json!(true);
                conn.execute(
                    "UPDATE registration_provenance SET json=?1 WHERE import_id=?2",
                    params![damaged.to_string(), legacy.import_id],
                )
                .unwrap();
            }
            "legacy-digest" => {
                conn.execute(
                    "UPDATE registration_provenance SET digest=?1 WHERE import_id=?2",
                    params!["0".repeat(64), legacy.import_id],
                )
                .unwrap();
            }
            "legacy-utf8" => {
                conn.execute("UPDATE registration_provenance SET json=CAST(x'ff' AS TEXT) WHERE import_id=?1", [&legacy.import_id]).unwrap();
            }
            _ => unreachable!(),
        }
        let damaged = catalog_rows_raw_proof_safe(&conn);
        for read in [Read::Get, Read::Page, Read::Retry] {
            for (plan, job) in [
                (&ordinary_plan, &ordinary),
                (&association_plan, &association),
            ] {
                let outcome = read.run(plan, job);
                let same = outcome.as_ref().is_ok_and(|returned| {
                    returned.job_id == job.job_id
                        && returned.state_version == job.state_version
                        && returned.state == job.state
                        && serde_json::to_value(&returned.items).unwrap()
                            == serde_json::to_value(&job.items).unwrap()
                });
                println!(
                    "LEGACY_PROOF_JOB_ISOLATION={}",
                    json!({"fault": fault, "entry": format!("{read:?}"),
                    "jobKind": if job.job_id == ordinary.job_id {"ordinary"} else {"association"},
                    "error": outcome.as_ref().err().map(|e| e.code), "sameJob": same})
                );
                if !same {
                    violations.push(format!(
                        "{fault}/{read:?}/{}: {:?}",
                        job.job_id,
                        outcome.err().map(|e| e.code)
                    ));
                }
            }
        }
        let after_read = catalog_rows_raw_proof_safe(&conn);
        conn.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE rowid=?3 AND import_id=?4",
            params![proof.1, proof.2, proof.0, legacy.import_id],
        )
        .unwrap();
        assert_eq!(after_read, damaged, "unrelated reads cannot repair {fault}");
        assert_eq!(catalog_rows(&conn), original_rows);
        assert_eq!(text(IMPORT, REVISION), old_history);
        assert_eq!(ordinary_history(&ordinary), ordinary_text);
        assert_archive_unchanged(&legacy);
        for read in [Read::Get, Read::Page, Read::Retry] {
            assert_eq!(
                read.run(&ordinary_plan, &ordinary).unwrap().job_id,
                ordinary.job_id
            );
            assert_eq!(
                read.run(&association_plan, &association).unwrap().job_id,
                association.job_id
            );
        }
    }
    assert!(
        violations.is_empty(),
        "unrelated real legacy proof: {violations:?}"
    );
}

pub(super) fn raw_job_rows(conn: &Connection, job_id: &str) -> (CatalogRows, i64) {
    let rows = catalog_rows_raw_proof_safe(conn)
        .0
        .into_iter()
        .filter(|(table, _)| matches!(table.as_str(), "jobs" | "job_items"))
        .map(|(table, rows)| {
            (
                table,
                rows.into_iter()
                    .filter(|row| row.contains(&Value::Text(job_id.into())))
                    .collect(),
            )
        })
        .collect();
    let receipts = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [job_id],
            |row| row.get(0),
        )
        .unwrap();
    (rows, receipts)
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_independent_publication_isolation() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let later_plan = plan(previews, vec![]).unwrap();
    let confirmed = confirmed_plan();
    let association = acceptance(&confirmed, "synthetic-publication-isolation-owner").unwrap();
    let later = commit(
        &later_plan.plan_id,
        &later_plan.plan_hash,
        "synthetic-publication-isolation-later",
    )
    .unwrap();
    let item = job_control::load_input(&conn, &later.job_id, &later.items[0].candidate_id).unwrap();
    let fault = FaultRows::capture(&conn, &association);
    let original_owner = raw_job_rows(&conn, &association.job_id);
    let old_history = text(IMPORT, REVISION);
    let archive = jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap();
    fault.damage(&conn, &association, "proof-json");
    // Invoke the existing publisher with the genuine accepted durable item.
    // This isolates publication from the separately failing queue scheduler.
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &later.job_id)
            .unwrap();
    let result = jobs::publish_for_worker(&conn, &later.job_id, 0, &item, &|| false, &crypto)
        .map(|progress| progress.expect_recorded());
    let receipt_during_fault =
        jobs::latest_receipt(&conn, &later.job_id, &later.items[0].candidate_id).unwrap();
    let owner_after = raw_job_rows(&conn, &association.job_id);
    fault.restore(&conn, &association, "proof-json");
    assert_eq!(
        owner_after, original_owner,
        "damaged owner is never rewritten"
    );
    assert_eq!(
        jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap(),
        archive
    );
    assert_eq!(text(IMPORT, REVISION), old_history);
    let observed = result.as_ref().err().map(|error| error.code);
    let phase = result.as_ref().ok().map(|item| item.phase.clone());
    // Restore only the fault, then exercise genuine worker recovery. A direct
    // committed receipt must be recovered, not published for a second time.
    drop(item);
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 2);
    let completed = job_get(&later.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    ordinary_history(&completed);
    let final_receipt = jobs::latest_receipt(&conn, &later.job_id, &later.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_receipt.phase, "committed");
    if let Some(before) = receipt_during_fault.as_ref() {
        assert_eq!(before.receipt_id, final_receipt.receipt_id);
        assert_eq!(before.revision_id, final_receipt.revision_id);
    }
    let final_receipts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&later.job_id],
            |row| row.get(0),
        )
        .unwrap();
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
            [&later.job_id],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        final_receipts
    );
    assert_eq!(text(IMPORT, REVISION), old_history);
    println!(
        "INDEPENDENT_PUBLICATION_ISOLATION={}",
        json!({"errorWhileDamaged": observed, "phaseWhileDamaged": phase,
        "damagedOwnerUnchanged": true, "restartDuplicateReceipts": 0})
    );
    assert_eq!(
        phase.as_deref(),
        Some("committed"),
        "a healthy accepted item must publish beside an unrelated malformed proof"
    );
    assert_eq!(receipt_during_fault.unwrap().phase, "committed");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_worker_queue_isolation() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let previews = cursor_source(&home, 1);
    let later_plan = plan(previews, vec![]).unwrap();
    let association_plan = confirmed_plan();
    let association =
        acceptance(&association_plan, "synthetic-worker-isolation-association").unwrap();
    let later = commit(
        &later_plan.plan_id,
        &later_plan.plan_hash,
        "synthetic-worker-isolation-later",
    )
    .unwrap();
    let fault = FaultRows::capture(&conn, &association);
    let association_rows = raw_job_rows(&conn, &association.job_id);
    let old_history = text(IMPORT, REVISION);
    let archive = jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap();
    fault.damage(&conn, &association, "proof-json");
    let outcome = job_control::run_pending();
    let state: String = conn
        .query_row(
            "SELECT state FROM jobs WHERE job_id=?1",
            [&later.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let after_first = raw_job_rows(&conn, &association.job_id);
    reset_runtime();
    let restarted = job_control::run_pending();
    let after_restart = raw_job_rows(&conn, &association.job_id);
    fault.restore(&conn, &association, "proof-json");
    // Restore the negative before asserting or invoking a genuine retry.
    assert_eq!(
        after_first, association_rows,
        "damaged association cannot change state or publish receipts"
    );
    assert_eq!(after_restart, association_rows);
    assert_eq!(
        jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap(),
        archive
    );
    assert_eq!(text(IMPORT, REVISION), old_history);
    let retry = job_control::run_pending().unwrap();
    let completed = job_get(&later.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.items[0].phase, "committed");
    ordinary_history(&completed);
    assert_eq!(text(IMPORT, REVISION), old_history);
    println!(
        "ASSOCIATION_WORKER_QUEUE_ISOLATION={}",
        json!({"firstError": outcome.as_ref().err().map(|e| e.code),
        "restartError": restarted.as_ref().err().map(|e| e.code), "laterStateWhileDamaged": state,
        "associationRowsUnchanged": true, "restoredRetryCompletedJobs": retry})
    );
    assert_eq!(
        state, "completed",
        "one refused association must not starve an independent later job"
    );
    assert_eq!(
        retry, 1,
        "healthy later job must already have published exactly once"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_ordinary_input_failure_queue_control() {
    for missing in [false, true] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let confirmed = confirmed_plan();
        let accepted =
            acceptance(&confirmed, "synthetic-input-failure-association-control").unwrap();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        assert_eq!(
            job_get(&accepted.job_id).unwrap().state,
            JobState::Completed
        );
        let history = text(IMPORT, REVISION);
        let previews = cursor_source(&home, 2);
        let broken_plan = plan(vec![previews[0].clone()], vec![]).unwrap();
        let broken = enqueue_legacy_plan(&broken_plan, "synthetic-input-failure-broken");
        let later_plan = plan(vec![previews[1].clone()], vec![]).unwrap();
        let later = commit(
            &later_plan.plan_id,
            &later_plan.plan_hash,
            "synthetic-input-failure-later",
        )
        .unwrap();
        let input: (i64, String) = conn
            .query_row(
                "SELECT rowid,result_json FROM job_items WHERE job_id=?1",
                [&broken.job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        if missing {
            conn.execute("DELETE FROM job_items WHERE job_id=?1", [&broken.job_id])
                .unwrap();
        } else {
            conn.execute(
                "UPDATE job_items SET result_json='{' WHERE job_id=?1",
                [&broken.job_id],
            )
            .unwrap();
        }
        let result = job_control::run_pending();
        let failed = job_get(&broken.job_id);
        let healthy = job_get(&later.job_id);
        let damaged_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM job_items WHERE job_id=?1",
                [&broken.job_id],
                |row| row.get(0),
            )
            .unwrap();
        let failed_receipts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM receipts WHERE job_id=?1",
                [&broken.job_id],
                |row| row.get(0),
            )
            .unwrap();
        // Restore the captured input bytes/rowid after measuring the failure.
        // The legitimate worker-produced failed phase is retained, not reset.
        if missing {
            conn.execute("INSERT INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,'failed',?4)",
                params![input.0, broken.job_id, broken.items[0].candidate_id, input.1]).unwrap();
        } else {
            conn.execute(
                "UPDATE job_items SET result_json=?1 WHERE rowid=?2",
                params![input.1, input.0],
            )
            .unwrap();
        }
        assert_eq!(result.unwrap(), 2);
        let failed = failed.unwrap();
        assert_eq!(failed.state, JobState::CompletedWithIssues);
        assert_eq!(failed.items[0].phase, "failed");
        assert!(failed.items[0].error.is_some());
        assert!(failed.items[0].import_id.is_none());
        assert_eq!(failed_receipts, 0);
        assert_eq!(
            damaged_count,
            i64::from(!missing),
            "worker cannot synthesize an input"
        );
        let healthy = healthy.unwrap();
        assert_eq!(healthy.state, JobState::Completed);
        assert_eq!(healthy.items[0].phase, "committed");
        ordinary_history(&healthy);
        assert_eq!(text(IMPORT, REVISION), history);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 0);
        assert_eq!(
            job_get(&broken.job_id).unwrap().state,
            JobState::CompletedWithIssues
        );
        println!(
            "ORDINARY_INPUT_FAILURE_QUEUE_CONTROL={}",
            json!({"missing": missing,
            "failedVisible": true, "syntheticInputCreated": false, "laterCompleted": true, "restartRepublishCount": 0})
        );
    }
}

fn catalog_rows_raw_proof_safe(conn: &Connection) -> (CatalogRows, RawProofRows) {
    // SQLite TEXT containing invalid UTF8 is an intentional negative. Keep its
    // bytes as hex, rather than asking rusqlite to decode the corrupt column.
    let proof = conn.prepare("SELECT rowid,import_id,hex(CAST(json AS BLOB)),hex(CAST(digest AS BLOB)) FROM registration_provenance ORDER BY rowid")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    (catalog_table_rows(conn, true), proof)
}
