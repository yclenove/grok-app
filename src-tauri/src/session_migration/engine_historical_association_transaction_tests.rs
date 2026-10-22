//! Real writer-stage aborts and detached-fact races, never positive SQL fixtures.
use super::*;
use crate::session_migration::registration_provenance_store as proof_store;
use rusqlite::{params, Connection};

#[path = "engine_historical_association_compatibility_tests.rs"]
mod compatibility_tests;

struct UnacceptedSnapshot {
    entry: serde_json::Value,
    source: serde_json::Value,
    paths: Vec<String>,
    old_job: serde_json::Value,
    receipts: Vec<(String, String)>,
    witnesses: Vec<(String, String)>,
    lease: jobs::PreviewLease,
    preview_path: PathBuf,
    old_text: String,
}

fn witnesses(conn: &Connection) -> Vec<(String, String)> {
    let mut statement = conn
        .prepare(
            "SELECT key,value FROM meta WHERE key LIKE 'registration_association_accepted:%'
         OR key LIKE 'preview_retention_accepted:%' OR key LIKE 'job_entry_scope:%'
         OR key LIKE 'job_entry_scope_accepted:%' ORDER BY key",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

impl UnacceptedSnapshot {
    fn capture(conn: &Connection, plan: &PlanState) -> Self {
        let source = sources::get(conn, INSTALLATION).unwrap();
        let preview_path = jobs::find_preview_dir(&plan.preview_ids[0])
            .unwrap()
            .unwrap();
        Self {
            entry: serde_json::to_value(get_entry(conn, IMPORT).unwrap()).unwrap(),
            source: serde_json::to_value(&source).unwrap(),
            paths: sources::registered_paths(conn, &source).unwrap(),
            old_job: serde_json::to_value(jobs::get_job(conn, OLD_JOB).unwrap()).unwrap(),
            receipts: receipts(conn),
            witnesses: witnesses(conn),
            lease: jobs::read_preview_lease(&preview_path).unwrap().unwrap(),
            preview_path,
            old_text: text(IMPORT, REVISION),
        }
    }

    fn refused(&self, conn: &Connection, requested: &JobRecord) {
        assert!(jobs::get_job(conn, &requested.job_id).unwrap().is_none());
        let count: u64 = conn
            .query_row(
                "SELECT count(*) FROM job_items WHERE job_id=?1",
                [&requested.job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "no partial durable inputs");
        assert_eq!(
            serde_json::to_value(get_entry(conn, IMPORT).unwrap()).unwrap(),
            self.entry
        );
        let source = sources::get(conn, INSTALLATION).unwrap();
        assert_eq!(serde_json::to_value(&source).unwrap(), self.source);
        assert_eq!(
            sources::registered_paths(conn, &source).unwrap(),
            self.paths
        );
        assert_eq!(
            serde_json::to_value(jobs::get_job(conn, OLD_JOB).unwrap()).unwrap(),
            self.old_job
        );
        assert!(!proof_store::verify_schema(conn).unwrap());
        assert_eq!(witnesses(conn), self.witnesses);
        let lease = jobs::read_preview_lease(&self.preview_path)
            .unwrap()
            .unwrap();
        assert_eq!(lease.retained_by_job, self.lease.retained_by_job);
        assert_eq!(lease.expires_unix, self.lease.expires_unix);
        assert_eq!(lease.file_hashes, self.lease.file_hashes);
        assert_eq!(lease.prepare_batch_id, self.lease.prepare_batch_id);
        assert!(!job_control::preview_retention::pending());
    }

    fn restored(&self, conn: &Connection) {
        assert_eq!(receipts(conn), self.receipts);
        assert_eq!(text(IMPORT, REVISION), self.old_text);
    }

    fn retry(&self, conn: &Connection, plan: &PlanState, requested: JobRecord) {
        let items = engine_commit::commit_items_for_plan(plan, &requested, false).unwrap();
        let job = job_control::enqueue_confirmed_associations(requested.clone(), items)
            .expect("same genuine request must accept after removing only its fault");
        assert_eq!(job.job_id, requested.job_id);
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.items.len(), 1);
        assert_eq!(list_entries(conn).unwrap().len(), 1);
        self.restored(conn);
        reset_runtime();
        assert_eq!(text(IMPORT, REVISION), self.old_text);
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_writer_stage_aborts() {
    for stage in [
        "schema",
        "proof-witness",
        "entry",
        "job",
        "input",
        "retention",
        "legacy-scope",
        "legacy-witness",
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let plan = confirmed_plan();
        let requested = requested_job(&plan, &format!("synthetic-writer-stage-{stage}"));
        let conn = open_catalog().unwrap();
        let snapshot = UnacceptedSnapshot::capture(&conn, &plan);
        // A provenance-table trigger would be refused at schema authentication,
        // not reach the desired writer stage. All blockers live on other tables.
        let (event, condition) = match stage {
            "legacy-scope" => (
                "INSERT ON meta",
                format!("NEW.key='job_entry_scope:{OLD_JOB}'"),
            ),
            "legacy-witness" => (
                "INSERT ON meta",
                format!("NEW.key='job_entry_scope_accepted:{OLD_JOB}'"),
            ),
            "schema" => (
                "INSERT ON meta",
                "NEW.key='registration_provenance_schema_version'".into(),
            ),
            "proof-witness" => (
                "INSERT ON meta",
                "NEW.key LIKE 'registration_association_accepted:%'".into(),
            ),
            "entry" => (
                "UPDATE OF native_session_id ON entries",
                format!("OLD.import_id='{IMPORT}'"),
            ),
            "job" => (
                "INSERT ON jobs",
                format!("NEW.job_id='{}'", requested.job_id),
            ),
            "input" => (
                "INSERT ON job_items",
                format!("NEW.job_id='{}'", requested.job_id),
            ),
            "retention" => (
                "INSERT ON meta",
                "NEW.key LIKE 'preview_retention_accepted:%'".into(),
            ),
            _ => unreachable!(),
        };
        let marker = format!("synthetic-association-SQL-abort-{stage}");
        conn.execute_batch(&format!(
            "CREATE TRIGGER synthetic_association_stage_abort BEFORE {event} WHEN {condition} BEGIN SELECT RAISE(ABORT,'{marker}'); END"
        ))
        .unwrap();
        let items = engine_commit::commit_items_for_plan(&plan, &requested, false).unwrap();
        let error =
            job_control::enqueue_confirmed_associations(requested.clone(), items).unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::SnapshotUnavailable,
            "{stage}: {error:?}"
        );
        let expected_message = if stage == "retention" {
            format!("preview retention reconciliation deferred: {marker}")
        } else {
            marker
        };
        assert_eq!(
            error.message, expected_message,
            "must reach the actual {stage} SQL write"
        );
        snapshot.refused(&conn, &requested);
        snapshot.restored(&conn);
        conn.execute_batch("DROP TRIGGER synthetic_association_stage_abort")
            .unwrap();
        snapshot.retry(&conn, &plan, requested);
        println!("ASSOCIATION_WRITER_STAGE_ABORT={stage}");
    }
}

#[derive(Clone)]
struct Facts {
    plan_id: String,
    plan: String,
    plan_hash: String,
    created: String,
    expiry: String,
    anchor_id: String,
    anchor_row: i64,
    anchor: String,
    extra_receipt: String,
    normalizer: String,
}

impl Facts {
    fn capture(conn: &Connection, plan: &PlanState) -> Self {
        let (json, created, expiry): (String, String, String) = conn
            .query_row(
                "SELECT json,created_at,expires_at FROM plans WHERE plan_id=?1",
                [&plan.plan_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let (anchor_id, anchor_row, anchor): (String, i64, String) = conn
            .query_row("SELECT receipt_id,rowid,json FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1", params![IMPORT, REVISION], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap();
        Self {
            plan_id: plan.plan_id.clone(),
            plan: json,
            plan_hash: plan.plan_hash.clone(),
            created,
            expiry,
            anchor_id,
            anchor_row,
            anchor,
            extra_receipt: Uuid::new_v4().to_string(),
            normalizer: conn.query_row("SELECT normalizer_version FROM revisions WHERE import_id=?1 AND revision_id=?2", params![IMPORT, REVISION], |row| row.get(0)).unwrap(),
        }
    }

    fn change(&self, conn: &Connection, fault: &str) {
        match fault {
            "plan-bytes" => {
                conn.execute(
                    "UPDATE plans SET json=?1 WHERE plan_id=?2",
                    params![format!("{} ", self.plan), self.plan_id],
                )
                .unwrap();
            }
            "plan-hash" => {
                conn.execute(
                    "UPDATE plans SET plan_hash=?1 WHERE plan_id=?2",
                    params!["0".repeat(64), self.plan_id],
                )
                .unwrap();
            }
            "plan-created" => {
                conn.execute(
                    "UPDATE plans SET created_at='2000-01-01T00:00:00Z' WHERE plan_id=?1",
                    [&self.plan_id],
                )
                .unwrap();
            }
            "plan-expiry" => {
                conn.execute(
                    "UPDATE plans SET expires_at='2000-01-01T00:00:00Z' WHERE plan_id=?1",
                    [&self.plan_id],
                )
                .unwrap();
            }
            "plan-expired-bytes" => {
                let mut value: serde_json::Value = serde_json::from_str(&self.plan).unwrap();
                value["expiry"] = json!("2000-01-01T00:00:00Z");
                conn.execute(
                    "UPDATE plans SET json=?1,expires_at='2000-01-01T00:00:00Z' WHERE plan_id=?2",
                    params![value.to_string(), self.plan_id],
                )
                .unwrap();
            }
            "anchor-content" => {
                let mut value: serde_json::Value = serde_json::from_str(&self.anchor).unwrap();
                value["snapshotHash"] = json!("0".repeat(64));
                conn.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    params![value.to_string(), self.anchor_id],
                )
                .unwrap();
            }
            "anchor-row" => {
                conn.execute(
                    "UPDATE receipts SET rowid=rowid+1000 WHERE receipt_id=?1",
                    [&self.anchor_id],
                )
                .unwrap();
            }
            "cutoff" => {
                let mut receipt: jobs::Receipt = serde_json::from_str(&self.anchor).unwrap();
                receipt.receipt_id = self.extra_receipt.clone();
                receipt.revision_id = Some(Uuid::new_v4().to_string());
                // Negative competing-history fact only: no positive authority.
                jobs::write_receipt(conn, &receipt).unwrap();
            }
            "normalizer" => {
                conn.execute("UPDATE revisions SET normalizer_version='1.0.1' WHERE import_id=?1 AND revision_id=?2", params![IMPORT, REVISION]).unwrap();
            }
            _ => unreachable!(),
        }
    }

    fn restore(&self, conn: &Connection) {
        conn.execute(
            "UPDATE plans SET json=?1,plan_hash=?2,created_at=?3,expires_at=?4 WHERE plan_id=?5",
            params![
                self.plan,
                self.plan_hash,
                self.created,
                self.expiry,
                self.plan_id
            ],
        )
        .unwrap();
        conn.execute(
            "UPDATE receipts SET rowid=?1,json=?2 WHERE receipt_id=?3",
            params![self.anchor_row, self.anchor, self.anchor_id],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM receipts WHERE receipt_id=?1",
            [&self.extra_receipt],
        )
        .unwrap();
        conn.execute(
            "UPDATE revisions SET normalizer_version=?1 WHERE import_id=?2 AND revision_id=?3",
            params![self.normalizer, IMPORT, REVISION],
        )
        .unwrap();
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_detached_fact_races() {
    for fault in [
        "plan-bytes",
        "plan-hash",
        "plan-created",
        "plan-expiry",
        "plan-expired-bytes",
        "anchor-content",
        "anchor-row",
        "cutoff",
        "normalizer",
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let plan = confirmed_plan();
        let conn = open_catalog().unwrap();
        let snapshot = UnacceptedSnapshot::capture(&conn, &plan);
        let facts = Facts::capture(&conn, &plan);
        let changed = facts.clone();
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = reached.clone();
        job_control::association_acceptance::test_after_preflight(move || {
            changed.change(&open_catalog().unwrap(), fault);
            signal.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let requested = requested_job(&plan, &format!("synthetic-detached-fact-{fault}"));
        let items = engine_commit::commit_items_for_plan(&plan, &requested, false).unwrap();
        let error =
            job_control::enqueue_confirmed_associations(requested.clone(), items).unwrap_err();
        assert!(
            reached.load(std::sync::atomic::Ordering::SeqCst),
            "genuine authenticated preflight must precede {fault}"
        );
        assert_eq!(error.code, ErrorCode::PlanStale, "{fault}: {error:?}");
        snapshot.refused(&conn, &requested);
        facts.restore(&conn);
        snapshot.restored(&conn);
        snapshot.retry(&conn, &plan, requested);
        println!("ASSOCIATION_DETACHED_FACT_RACE={fault}");
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_receipt_boundaries() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let plan = confirmed_plan();
    let conn = open_catalog().unwrap();
    let snapshot = UnacceptedSnapshot::capture(&conn, &plan);
    let facts = Facts::capture(&conn, &plan);
    let queued = acceptance(&plan, "synthetic-accepted-receipt-boundaries").unwrap();
    assert_eq!(queued.state, JobState::Queued);
    // The actual schema prevents SQL NULL JSON. Prove that storage control
    // instead of claiming to exercise a reader with an impossible injected row.
    let tx = conn.unchecked_transaction().unwrap();
    let null_error = tx
        .execute(
            "UPDATE receipts SET json=NULL WHERE receipt_id=?1",
            [&facts.anchor_id],
        )
        .unwrap_err();
    assert_eq!(
        null_error.sqlite_error().unwrap().extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL
    );
    assert!(jobs::load_published_auth_with(&tx, IMPORT, Some(REVISION), None).is_ok());
    tx.rollback().unwrap();
    snapshot.restored(&conn);
    println!("ASSOCIATION_RECEIPT_SQL_NULL_PREVENTED=true");
    for (fault, expected) in [
        ("json-type", ErrorCode::HistoryAccessUnavailable),
        ("import-column-null", ErrorCode::HistoryAccessUnavailable),
        ("invalid-utf8", ErrorCode::HistoryAccessUnavailable),
        ("malformed-json", ErrorCode::SchemaUnsupported),
        ("duplicate-version", ErrorCode::SchemaUnsupported),
        ("future-version", ErrorCode::SchemaUnsupported),
        ("unknown-field", ErrorCode::SchemaUnsupported),
        ("oversized-json", ErrorCode::ResourceLimit),
        ("job-column", ErrorCode::HistoryAccessUnavailable),
        ("phase-column", ErrorCode::HistoryAccessUnavailable),
        ("candidate-type", ErrorCode::HistoryAccessUnavailable),
        ("missing-anchor", ErrorCode::HistoryAccessUnavailable),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "json-type" => {
                tx.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    params![facts.anchor.as_bytes(), facts.anchor_id],
                )
                .unwrap();
            }
            "import-column-null" => {
                tx.execute(
                    "UPDATE receipts SET import_id=NULL WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            "invalid-utf8" => {
                tx.execute(
                    "UPDATE receipts SET json=CAST(X'80' AS TEXT) WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            "malformed-json" | "duplicate-version" | "future-version" | "unknown-field"
            | "oversized-json" => {
                let json = match fault {
                    "malformed-json" => "{".into(),
                    "duplicate-version" => format!("{{\"version\":2,{}", &facts.anchor[1..]),
                    "oversized-json" => format!("{}{}", facts.anchor, " ".repeat(16 * 1024 * 1024)),
                    _ => {
                        let mut value: serde_json::Value =
                            serde_json::from_str(&facts.anchor).unwrap();
                        if fault == "future-version" {
                            value["version"] = json!(u32::MAX);
                        } else {
                            value["syntheticForeignGrant"] = json!(true);
                        }
                        value.to_string()
                    }
                };
                tx.execute(
                    "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
                    params![json, facts.anchor_id],
                )
                .unwrap();
            }
            "job-column" => {
                tx.execute(
                    "UPDATE receipts SET job_id='synthetic-foreign-job' WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            "phase-column" => {
                tx.execute(
                    "UPDATE receipts SET phase='synthetic-foreign-phase' WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            "candidate-type" => {
                tx.execute(
                    "UPDATE receipts SET candidate_id=zeroblob(8) WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            "missing-anchor" => {
                tx.execute(
                    "DELETE FROM receipts WHERE receipt_id=?1",
                    [&facts.anchor_id],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let error = jobs::load_published_auth_with(&tx, IMPORT, Some(REVISION), None).unwrap_err();
        assert_eq!(error.code, expected, "accepted receipt {fault}: {error:?}");
        tx.rollback().unwrap();
        snapshot.restored(&conn);
        println!("ASSOCIATION_ACCEPTED_RECEIPT_BOUNDARY={fault}");
    }
    reset_runtime();
    snapshot.restored(&conn);
}
