//! Actual progress writers over genuine accepted association jobs.
//! Each negative owns a fresh isolated catalog; no positive SQL grant is made.
use super::*;
use crate::session_migration::job_progress;
use rusqlite::{params, Connection};

#[derive(Clone, Copy, Debug)]
enum Writer {
    Item,
    CancelBatch,
}

impl Writer {
    fn run(self, conn: &Connection, job: &JobRecord) -> Result<(), ImportError> {
        match self {
            Self::Item => {
                let mut item = job.items[0].clone();
                item.phase = "running".into();
                job_progress::set_item(conn, &job.job_id, 0, &item)
            }
            Self::CancelBatch => {
                job_progress::cancel_unstarted_queued(conn, &job.job_id).map(|_| ())
            }
        }
    }

    fn healthy(self, conn: &Connection, job: &JobRecord) {
        self.run(conn, job).unwrap();
        let current = jobs::get_job(conn, &job.job_id).unwrap().unwrap();
        assert_eq!(current.state, job.state);
        assert_eq!(current.state_version, job.state_version + 1);
        assert_eq!(current.items.len(), 1);
        assert_eq!(current.items[0].candidate_id, job.items[0].candidate_id);
        let expected = match self {
            Self::Item => "running",
            Self::CancelBatch => "cancelled",
        };
        assert_eq!(current.items[0].phase, expected);
        if matches!(self, Self::CancelBatch) {
            assert_eq!(current.items[0].outcome.as_deref(), Some("cancelled"));
            assert!(job_progress::cancel_unstarted_queued(conn, &job.job_id)
                .unwrap()
                .is_empty());
            assert_eq!(
                jobs::get_job(conn, &job.job_id)
                    .unwrap()
                    .unwrap()
                    .state_version,
                current.state_version
            );
        }
        assert_eq!(
            conn.query_row(
                "SELECT phase FROM job_items WHERE job_id=?1",
                [&job.job_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            expected
        );
    }
}

// Snapshot SQL type and exact cell bytes, including invalid UTF8 TEXT. Value's
// owned String reader cannot faithfully inspect that deliberate negative.
pub(super) fn raw_catalog(conn: &Connection) -> Vec<(String, Vec<Vec<String>>)> {
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
            let table = table.replace('"', "\"\"");
            let columns = conn
                .prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0"))
                .unwrap()
                .column_names()
                .into_iter()
                .map(|name| name.replace('"', "\"\""))
                .collect::<Vec<_>>();
            let projection = columns
                .iter()
                .map(|name| format!("typeof(\"{name}\"),hex(CAST(\"{name}\" AS BLOB))"))
                .collect::<Vec<_>>()
                .join(",");
            let mut statement = conn
                .prepare(&format!(
                    "SELECT CAST(rowid AS TEXT),{projection} FROM \"{table}\" ORDER BY rowid"
                ))
                .unwrap();
            let count = statement.column_count();
            let rows = statement
                .query_map([], |row| (0..count).map(|index| row.get(index)).collect())
                .unwrap()
                .collect::<Result<Vec<Vec<String>>, _>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

struct FaultRows {
    job_json: String,
    display_rows: Vec<(i64, i64, String, String)>,
    input: (i64, String, String),
    proof: (i64, String, String),
    accepted_key: String,
    accepted: (i64, String),
    owner: (i64, String, String),
    owner_key: String,
    owner_marker: (i64, String),
}

impl FaultRows {
    fn capture(conn: &Connection, job: &JobRecord) -> Self {
        let marker = |key: &str| {
            conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
        };
        let accepted_key = format!(
            "registration_association_accepted:{}:{}",
            job.job_id,
            hash::sha256_bytes(job.items[0].candidate_id.as_bytes())
        );
        let owner: (i64, String, String) = conn.query_row(
            "SELECT rowid,tuple_digest,proof_digest FROM registration_association_old_owners WHERE import_id=?1",
            [IMPORT], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        let owner_key = format!("registration_association_old_owner:{}", owner.1);
        Self {
            display_rows: conn
                .prepare("SELECT rowid,item_index,candidate_id,json FROM job_item_states WHERE job_id=?1 ORDER BY item_index")
                .unwrap()
                .query_map([&job.job_id], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            job_json: conn
                .query_row(
                    "SELECT json FROM jobs WHERE job_id=?1",
                    [&job.job_id],
                    |row| row.get(0),
                )
                .unwrap(),
            input: conn
                .query_row(
                    "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1",
                    [&job.job_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap(),
            proof: conn
                .query_row(
                    "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap(),
            accepted: marker(&accepted_key),
            accepted_key,
            owner,
            owner_marker: marker(&owner_key),
            owner_key,
        }
    }

    fn inject(&self, conn: &Connection, job: &JobRecord, fault: &str) {
        match fault {
            "oversized-job" | "header-plan-id" | "members-removed" => {
                let mut json: serde_json::Value = serde_json::from_str(&self.job_json).unwrap();
                let encoded = match fault {
                    "oversized-job" => {
                        format!("{}{}", self.job_json, " ".repeat(16 * 1024 * 1024))
                    }
                    "header-plan-id" => {
                        json["planId"] = json!(Uuid::new_v4().to_string());
                        json.to_string()
                    }
                    _ => {
                        json["items"] = json!([]);
                        json["itemCount"] = json!(0);
                        json.to_string()
                    }
                };
                conn.execute(
                    "UPDATE jobs SET json=?1 WHERE job_id=?2",
                    params![encoded, job.job_id],
                )
                .unwrap();
                if fault == "members-removed" {
                    conn.execute("DELETE FROM job_item_states WHERE job_id=?1", [&job.job_id])
                        .unwrap();
                }
            }
            "blob-job" => {
                conn.execute(
                    "UPDATE jobs SET json=CAST(json AS BLOB) WHERE job_id=?1",
                    [&job.job_id],
                )
                .unwrap();
            }
            "utf8-job" => {
                conn.execute(
                    "UPDATE jobs SET json=CAST(x'ff' AS TEXT) WHERE job_id=?1",
                    [&job.job_id],
                )
                .unwrap();
            }
            "missing-input" => {
                conn.execute("DELETE FROM job_items WHERE job_id=?1", [&job.job_id])
                    .unwrap();
            }
            "null-association" => {
                let mut json: serde_json::Value = serde_json::from_str(&self.input.2).unwrap();
                json["identityAssociation"] = serde_json::Value::Null;
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
                    params![json.to_string(), job.job_id],
                )
                .unwrap();
            }
            "missing-proof" => {
                conn.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "missing-witness" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&self.accepted_key])
                    .unwrap();
            }
            "coordinated-selectors" => {
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
            _ => panic!("unknown negative"),
        }
    }

    fn restore(&self, conn: &Connection, job: &JobRecord, fault: &str) {
        match fault {
            "oversized-job" | "header-plan-id" | "members-removed" | "blob-job" | "utf8-job" => {
                conn.execute(
                    "UPDATE jobs SET json=?1 WHERE job_id=?2",
                    params![self.job_json, job.job_id],
                )
                .unwrap();
                if fault == "members-removed" {
                    for row in &self.display_rows {
                        conn.execute("INSERT INTO job_item_states(rowid,job_id,item_index,candidate_id,json) VALUES(?1,?2,?3,?4,?5)",
                            params![row.0, job.job_id, row.1, row.2, row.3]).unwrap();
                    }
                }
            }
            "missing-input" => {
                conn.execute("INSERT INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4,?5)",
                    params![self.input.0, job.job_id, job.items[0].candidate_id, self.input.1, self.input.2]).unwrap();
            }
            "null-association" => {
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
                    params![self.input.2, job.job_id],
                )
                .unwrap();
            }
            "missing-proof" => {
                conn.execute("INSERT INTO registration_provenance(rowid,import_id,json,digest) VALUES(?1,?2,?3,?4)",
                    params![self.proof.0, IMPORT, self.proof.1, self.proof.2]).unwrap();
            }
            "missing-witness" | "coordinated-selectors" => {
                conn.execute(
                    "INSERT INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                    params![self.accepted.0, self.accepted_key, self.accepted.1],
                )
                .unwrap();
                if fault == "coordinated-selectors" {
                    conn.execute("INSERT INTO registration_association_old_owners(rowid,tuple_digest,import_id,proof_digest) VALUES(?1,?2,?3,?4)",
                        params![self.owner.0, self.owner.1, IMPORT, self.owner.2]).unwrap();
                    conn.execute(
                        "INSERT INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                        params![self.owner_marker.0, self.owner_key, self.owner_marker.1],
                    )
                    .unwrap();
                }
            }
            _ => panic!("unknown negative"),
        }
    }
}

fn writer_probe(writer: Writer) {
    let mut violations = Vec::new();
    for (fault, expected) in [
        ("oversized-job", ErrorCode::ResourceLimit),
        ("blob-job", ErrorCode::PlanStale),
        ("utf8-job", ErrorCode::PlanStale),
        ("header-plan-id", ErrorCode::PlanStale),
        ("members-removed", ErrorCode::SnapshotUnavailable),
        ("missing-input", ErrorCode::SnapshotUnavailable),
        ("null-association", ErrorCode::SchemaUnsupported),
        ("missing-proof", ErrorCode::HistoryAccessUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("coordinated-selectors", ErrorCode::HistoryAccessUnavailable),
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let planned = confirmed_plan();
        let accepted =
            acceptance(&planned, &format!("synthetic-progress-{writer:?}-{fault}")).unwrap();
        let job = if matches!(writer, Writer::CancelBatch) {
            job_control::cancel(&accepted.job_id).unwrap()
        } else {
            accepted
        };
        let original = raw_catalog(&conn);
        let old_text = text(IMPORT, REVISION);
        let rows = FaultRows::capture(&conn, &job);
        rows.inject(&conn, &job, fault);
        let damaged = raw_catalog(&conn);
        jobs::take_job_item_deserializes();
        let result = writer.run(&conn, &job);
        let decoded = jobs::take_job_item_deserializes();
        let unchanged = raw_catalog(&conn) == damaged;
        let observed = result.as_ref().err().map(|error| error.code);
        rows.restore(&conn, &job, fault);
        let restored = raw_catalog(&conn) == original;
        println!(
            "ASSOCIATION_PROGRESS_WRITER={}",
            json!({"writer": format!("{writer:?}"), "fault": fault, "expected": expected,
                "observed": observed, "jobItemDeserializations": decoded,
                "faultCatalogUnchanged": unchanged, "restoredOriginal": restored}),
        );
        if observed != Some(expected) || decoded != 0 || !unchanged || !restored {
            violations.push((fault, observed, decoded, unchanged, restored));
            // Never repair an unexpected product write to manufacture a retry.
            // This isolated negative ends here; the next has a genuine fresh owner.
            continue;
        }
        assert_eq!(text(IMPORT, REVISION), old_text);
        writer.healthy(&conn, &job);
        assert_eq!(text(IMPORT, REVISION), old_text);
        reset_runtime();
        let after_restart = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
        assert_eq!(after_restart.state, job.state);
        assert_eq!(after_restart.state_version, job.state_version + 1);
    }
    assert!(
        violations.is_empty(),
        "{writer:?} writer refused too late or mutated: {violations:?}"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_item_progress_predecode() {
    writer_probe(Writer::Item);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_cancel_batch_predecode() {
    writer_probe(Writer::CancelBatch);
}

fn cancel_mixed_started_recovery(signed: bool) {
    for ordinary_count in [1, 24] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let association = confirmed_plan();
        let previews = cursor_source(&home, ordinary_count);
        let ordinary = plan(previews.clone(), vec![]).unwrap();
        let mut selection = association.selection_set.clone();
        selection.entries.extend(ordinary.selection_set.entries);
        let mut preview_ids = association.preview_ids.clone();
        preview_ids.extend(previews);
        let mixed = AssociationPlanRequest {
            version: 1,
            preview_ids,
            mappings: vec![],
            selection_set: selection,
            identity_associations: association.identity_associations.clone(),
        }
        .build()
        .unwrap();
        // This lost-ACK scenario belongs to the historical protocol; signed
        // publication writes its completion ACK in the final transaction.
        let mixed = if signed {
            mixed
        } else {
            historical_legacy_plan(&mixed)
        };
        let requested = requested_job(&mixed, "synthetic-mixed-cancel-real-started");
        let inputs = engine_commit::commit_items_for_plan(&mixed, &requested, false).unwrap();
        let associated_candidate = inputs
            .iter()
            .find(|item| item.identity_association.is_some())
            .unwrap()
            .candidate_id
            .clone();
        let queued = job_control::enqueue_confirmed_associations(requested, inputs).unwrap();
        assert_eq!(queued.items.len(), ordinary_count + 1);
        let started = queued
            .items
            .iter()
            .find(|item| item.candidate_id != associated_candidate)
            .unwrap();
        let input = job_control::load_input(&conn, &queued.job_id, &started.candidate_id).unwrap();
        assert!(input.identity_association.is_none());
        // Legacy receipts can precede the ACK; signed completion is atomic.
        // Both must preserve their started item through cancellation/restart.
        let published = if signed {
            let crypto = crate::session_migration::publication_crypto::JobCrypto::prepare(
                &conn,
                &queued.job_id,
            )
            .unwrap();
            let index = queued
                .items
                .iter()
                .position(|item| item.candidate_id == started.candidate_id)
                .unwrap();
            jobs::publish_for_worker(&conn, &queued.job_id, index, &input, &|| false, &crypto)
                .unwrap()
                .expect_recorded()
        } else {
            jobs::publish_item(&conn, &queued.job_id, &input, &|| false).unwrap()
        };
        assert_eq!(published.phase, "committed");
        drop(input);
        let before_receipts = receipts(&conn);
        let old_text = text(IMPORT, REVISION);
        let cancelled = job_control::cancel(&queued.job_id).unwrap();
        conn.execute_batch(
            "CREATE TABLE cancel_header_writes(n INTEGER NOT NULL);
             INSERT INTO cancel_header_writes VALUES(0);
             CREATE TRIGGER count_cancel_header AFTER UPDATE ON jobs
             BEGIN UPDATE cancel_header_writes SET n=n+1; END;",
        )
        .unwrap();
        job_control::association_members::take_complete_member_scans();
        jobs::take_job_item_deserializes();
        let unstarted = job_progress::cancel_unstarted_queued(&conn, &queued.job_id).unwrap();
        let scans = job_control::association_members::take_complete_member_scans();
        let decoded = jobs::take_job_item_deserializes();
        assert_eq!(scans, 1, "one complete member scan for the entire batch");
        assert_eq!(decoded as usize, ordinary_count + 1);
        assert_eq!(unstarted.len(), ordinary_count);
        assert!(unstarted.contains(&associated_candidate));
        assert!(!unstarted.contains(&started.candidate_id));
        assert_eq!(
            conn.query_row("SELECT n FROM cancel_header_writes", [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            1
        );
        let current = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(current.state, JobState::Cancelling);
        assert_eq!(current.state_version, cancelled.state_version + 1);
        assert_eq!(receipts(&conn), before_receipts);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let complete = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(complete.state, JobState::Cancelled);
        for item in &complete.items {
            if item.candidate_id == started.candidate_id {
                assert_eq!(item.phase, "committed");
                assert_eq!(item.import_id, published.import_id);
                assert_eq!(item.revision_id, published.revision_id);
            } else {
                assert_eq!(item.phase, "cancelled");
                assert_eq!(item.outcome.as_deref(), Some("cancelled"));
            }
        }
        assert_eq!(receipts(&conn), before_receipts);
        assert_eq!(text(IMPORT, REVISION), old_text);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 0);
        assert_eq!(receipts(&conn), before_receipts);
        let undone = jobs::undo_job(&conn, &queued.job_id).unwrap();
        assert_eq!(undone.state, JobState::Cancelled);
        for item in &undone.items {
            assert_eq!(
                item.phase,
                if item.candidate_id == started.candidate_id {
                    "undone"
                } else {
                    "cancelled"
                }
            );
        }
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(receipts(&conn), before_receipts);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 0);
        let restored = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(restored.state_version, undone.state_version);
        assert_eq!(receipts(&conn), before_receipts);
        println!(
            "ASSOCIATION_CANCEL_BATCH_COMPLEXITY={}",
            json!({
                "items": ordinary_count + 1, "completeMemberScans": scans,
                "signed": signed,
                "jobItemDeserializations": decoded, "headerWrites": 1,
                "cancelled": unstarted.len(), "startedReceiptUnchanged": true,
                "restartExactOnce": true,
            })
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_cancel_mixed_started_recovery() {
    cancel_mixed_started_recovery(false);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_signed_cancel_mixed_started_recovery() {
    cancel_mixed_started_recovery(true);
}
