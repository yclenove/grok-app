//! Real accepted jobs through enabled public/entry reads, not an opened commit lane.
//! The state-entry closure aborts after read; this is NOT full worker evidence.
use super::*;
use rusqlite::{params, Connection};

const SENTINEL: &str = "synthetic-state-entry-after-read";
type StoredJob = (i64, String, String, String, u32, String, String);

#[derive(Clone, Copy, Debug)]
enum Entry {
    PublicRetry,
    PublicPage,
    StateRead,
}

impl Entry {
    fn read(
        self,
        conn: &Connection,
        plan: &PlanState,
        job: &JobRecord,
    ) -> Result<JobRecord, ImportError> {
        match self {
            Self::PublicRetry => commit(&plan.plan_id, &plan.plan_hash, &job.request_id),
            Self::PublicPage => job_items_page(&job.job_id, None, Some(1)),
            Self::StateRead => job_control::change_job(conn, &job.job_id, |_| {
                Err(ImportError::new(ErrorCode::MappingConflict, SENTINEL))
            }),
        }
    }

    fn healthy(self, conn: &Connection, plan: &PlanState, job: &JobRecord) {
        let outcome = self.read(conn, plan, job);
        if matches!(self, Self::StateRead) {
            let error = outcome.unwrap_err();
            assert_eq!(error.code, ErrorCode::MappingConflict);
            assert_eq!(error.message, SENTINEL);
        } else {
            let returned = outcome.unwrap();
            assert_eq!(returned.job_id, job.job_id);
            assert_eq!(returned.state, JobState::Queued);
            assert_eq!(returned.items.len(), 1);
            assert_eq!(returned.items[0].candidate_id, job.items[0].candidate_id);
        }
    }
}

fn job_bytes(conn: &Connection, job: &JobRecord) -> StoredJob {
    conn.query_row("SELECT rowid,request_id,plan_hash,state,state_version,json,created_at FROM jobs WHERE job_id=?1",
        [&job.job_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))).unwrap()
}

fn entry_probe(entry: Entry) {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let plan = confirmed_plan();
    let queued = acceptance(&plan, &format!("synthetic-job-entry-{entry:?}")).unwrap();
    let original = job_bytes(&conn, &queued);
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let old_entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let source = serde_json::to_value(sources::get(&conn, INSTALLATION).unwrap()).unwrap();
    let (_, original_plan) = load_plan_json(&conn, &plan.plan_id).unwrap();
    let path = jobs::find_preview_dir(&plan.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = std::fs::read(path.join("lease.json")).unwrap();
    let input: (i64, String, String) = conn
        .query_row(
            "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let proof: (i64, String, String) = conn
        .query_row(
            "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
            [IMPORT],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let owner: (i64, String, String) = conn.query_row(
        "SELECT rowid,tuple_digest,proof_digest FROM registration_association_old_owners WHERE import_id=?1", [IMPORT],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    let owner_key = format!("registration_association_old_owner:{}", owner.1);
    let accepted_key = format!(
        "registration_association_accepted:{}:{}",
        queued.job_id,
        crate::session_migration::hash::sha256_bytes(queued.items[0].candidate_id.as_bytes())
    );
    let marker = |key: &str| {
        conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
    };
    let accepted_marker = marker(&accepted_key);
    let owner_marker = marker(&owner_key);
    let mut violations = Vec::new();
    entry.healthy(&conn, &plan, &queued);
    for (fault, expected) in [
        ("oversized-job", ErrorCode::ResourceLimit),
        ("blob-job", ErrorCode::PlanStale),
        ("utf8-job", ErrorCode::PlanStale),
        ("header-plan-id", ErrorCode::PlanStale),
        ("json-members-removed", ErrorCode::SnapshotUnavailable),
        ("missing-input", ErrorCode::SnapshotUnavailable),
        ("null-association", ErrorCode::SchemaUnsupported),
        ("downgraded-input", ErrorCode::SnapshotUnavailable),
        ("missing-proof", ErrorCode::HistoryAccessUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("wrong-witness", ErrorCode::HistoryAccessUnavailable),
        ("missing-owner", ErrorCode::HistoryAccessUnavailable),
        (
            "coordinated-selectors-removed",
            ErrorCode::HistoryAccessUnavailable,
        ),
    ] {
        match fault {
            "oversized-job" => {
                // Valid JSON with whitespace still exceeds the exact stored-byte
                // budget. No huge title is needed for this body-fetch red.
                conn.execute(
                    "UPDATE jobs SET json=?1 WHERE job_id=?2",
                    params![
                        format!("{}{}", original.5, " ".repeat(16 * 1024 * 1024)),
                        queued.job_id
                    ],
                )
                .unwrap();
            }
            "blob-job" => {
                conn.execute(
                    "UPDATE jobs SET json=CAST(json AS BLOB) WHERE job_id=?1",
                    [&queued.job_id],
                )
                .unwrap();
            }
            "utf8-job" => {
                conn.execute(
                    "UPDATE jobs SET json=CAST(x'ff' AS TEXT) WHERE job_id=?1",
                    [&queued.job_id],
                )
                .unwrap();
            }
            "header-plan-id" | "json-members-removed" => {
                let mut value: serde_json::Value = serde_json::from_str(&original.5).unwrap();
                if fault == "header-plan-id" {
                    value["planId"] = json!(Uuid::new_v4().to_string());
                } else {
                    value["items"] = json!([]);
                    value["itemCount"] = json!(0);
                }
                conn.execute(
                    "UPDATE jobs SET json=?1 WHERE job_id=?2",
                    params![value.to_string(), queued.job_id],
                )
                .unwrap();
            }
            "missing-input" => {
                conn.execute("DELETE FROM job_items WHERE job_id=?1", [&queued.job_id])
                    .unwrap();
            }
            "null-association" | "downgraded-input" => {
                let mut value: serde_json::Value = serde_json::from_str(&input.2).unwrap();
                if fault == "null-association" {
                    value["identityAssociation"] = serde_json::Value::Null;
                } else {
                    value["version"] = json!(2);
                    value.as_object_mut().unwrap().remove("identityAssociation");
                }
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
                    params![value.to_string(), queued.job_id],
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
                conn.execute("DELETE FROM meta WHERE key=?1", [&accepted_key])
                    .unwrap();
            }
            "wrong-witness" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params!["0".repeat(64), accepted_key],
                )
                .unwrap();
            }
            "missing-owner" | "coordinated-selectors-removed" => {
                conn.execute(
                    "DELETE FROM registration_association_old_owners WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
                if fault == "coordinated-selectors-removed" {
                    // Removing both selectors cannot turn a surviving proof2
                    // into ordinary absence. This is only a negative mutation.
                    conn.execute(
                        "DELETE FROM meta WHERE key=?1 OR key=?2",
                        params![accepted_key, owner_key],
                    )
                    .unwrap();
                }
            }
            _ => unreachable!(),
        }
        jobs::take_job_item_deserializes();
        let outcome = entry.read(&conn, &plan, &queued);
        let decoded = jobs::take_job_item_deserializes();
        // Restore only the negative bytes before any assertion. The post-read
        // closure errors before saving, so no positive job state is synthesized.
        conn.execute(
            "UPDATE jobs SET json=?1 WHERE job_id=?2",
            params![original.5, queued.job_id],
        )
        .unwrap();
        // Restore exactly the fault-owned authority/input rows and original
        // rowids; these are captured genuine acceptance bytes, not new proofs.
        let restore = conn.unchecked_transaction().unwrap();
        if matches!(
            fault,
            "missing-input" | "null-association" | "downgraded-input"
        ) {
            restore.execute("INSERT OR REPLACE INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4,?5)",
                params![input.0, queued.job_id, queued.items[0].candidate_id, input.1, input.2]).unwrap();
        }
        if fault == "missing-proof" {
            restore.execute("INSERT INTO registration_provenance(rowid,import_id,json,digest) VALUES(?1,?2,?3,?4)",
                params![proof.0, IMPORT, proof.1, proof.2]).unwrap();
        }
        if matches!(fault, "missing-owner" | "coordinated-selectors-removed") {
            restore.execute("INSERT INTO registration_association_old_owners(rowid,tuple_digest,import_id,proof_digest) VALUES(?1,?2,?3,?4)",
                params![owner.0, owner.1, IMPORT, owner.2]).unwrap();
        }
        if matches!(
            fault,
            "missing-witness" | "wrong-witness" | "coordinated-selectors-removed"
        ) {
            restore
                .execute(
                    "INSERT OR REPLACE INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                    params![accepted_marker.0, accepted_key, accepted_marker.1],
                )
                .unwrap();
        }
        if fault == "coordinated-selectors-removed" {
            restore
                .execute(
                    "INSERT INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                    params![owner_marker.0, owner_key, owner_marker.1],
                )
                .unwrap();
        }
        restore.commit().unwrap();
        let observed = outcome.as_ref().err().map(|error| error.code);
        println!(
            "ASSOCIATION_JOB_ENTRY={}",
            json!({
                "entry": format!("{entry:?}"), "fault": fault, "expected": expected,
                "observed": observed, "jobItemDeserializations": decoded,
                "message": outcome.as_ref().err().map(|error| &error.message),
            })
        );
        if observed != Some(expected) || decoded != 0 {
            violations.push((fault, observed, decoded));
        }
        assert_eq!(job_bytes(&conn, &queued), original);
        assert_eq!(marker(&accepted_key), accepted_marker);
        assert_eq!(marker(&owner_key), owner_marker);
        assert_eq!(
            conn.query_row(
                "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
                [IMPORT],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                ))
            )
            .unwrap(),
            proof
        );
        assert_eq!(conn.query_row("SELECT rowid,tuple_digest,proof_digest FROM registration_association_old_owners WHERE import_id=?1", [IMPORT],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).unwrap(), owner);
        assert_eq!(receipts(&conn), old_receipts);
        assert_eq!(
            serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
            old_entry
        );
        assert_eq!(
            serde_json::to_value(sources::get(&conn, INSTALLATION).unwrap()).unwrap(),
            source
        );
        assert_eq!(
            load_plan_json(&conn, &plan.plan_id).unwrap().1,
            original_plan
        );
        assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), lease);
        assert_eq!(
            conn.query_row(
                "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1",
                [&queued.job_id],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                ))
            )
            .unwrap(),
            input
        );
        assert_eq!(text(IMPORT, REVISION), old_text);
        entry.healthy(&conn, &plan, &queued);
        assert_eq!(
            acceptance(&plan, &queued.request_id).unwrap().job_id,
            queued.job_id
        );
    }
    assert!(
        violations.is_empty(),
        "{entry:?} must authenticate before full job/page projection: {violations:?}"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_public_retry_predecode() {
    entry_probe(Entry::PublicRetry);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_public_page_predecode() {
    entry_probe(Entry::PublicPage);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_change_job_predecode() {
    entry_probe(Entry::StateRead);
}
