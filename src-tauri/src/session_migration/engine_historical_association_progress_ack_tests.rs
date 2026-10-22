//! SAME-writer ACKs, from real accepted jobs and immutable historical targets.
use super::*;
use crate::session_migration::{job_progress, jobs};
use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::progress_tests::raw_catalog;

pub(super) fn mixed_input(
    home: &IsolatedAppHome,
    ordinary_count: usize,
    signed: bool,
) -> (JobRecord, usize, jobs::CommitItem) {
    let association = confirmed_plan();
    let previews = cursor_source(home, ordinary_count);
    let ordinary = plan(previews.clone(), vec![]).unwrap();
    let mut selections = association.selection_set.clone();
    selections.entries.extend(ordinary.selection_set.entries);
    let mut preview_ids = association.preview_ids.clone();
    preview_ids.extend(previews);
    let confirmed = AssociationPlanRequest {
        version: 1,
        preview_ids,
        mappings: vec![],
        selection_set: selections,
        identity_associations: association.identity_associations.clone(),
    }
    .build()
    .unwrap();
    // Only Legacy permits publication without an atomic worker ACK. Preserve
    // that historical cost comparison; signed worker coverage is separate.
    let confirmed = if signed {
        confirmed
    } else {
        historical_legacy_plan(&confirmed)
    };
    let requested = requested_job(&confirmed, "synthetic-atomic-progress");
    let inputs = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
    let selected = inputs
        .iter()
        .find(|item| item.identity_association.is_some())
        .unwrap()
        .candidate_id
        .clone();
    let job = job_control::enqueue_confirmed_associations(requested, inputs).unwrap();
    let index = job
        .items
        .iter()
        .position(|item| item.candidate_id == selected)
        .unwrap();
    let conn = open_catalog().unwrap();
    let input = job_control::load_input(&conn, &job.job_id, &selected).unwrap();
    (job, index, input)
}

fn take_costs() -> (u32, u32, u32) {
    (
        job_control::association_members::take_complete_member_scans(),
        crate::session_migration::engine::take_durable_plan_loads(),
        jobs::take_job_item_deserializes(),
    )
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_progress_reuses_final_writer_authentication() {
    for ordinary_count in [1, 24] {
        let mut costs = Vec::new();
        for worker_ack in [false, true] {
            let home = seed_home();
            let _runtime = RuntimeReset;
            let conn = open_catalog().unwrap();
            let (job, index, input) = mixed_input(&home, ordinary_count, false);
            let crypto = crate::session_migration::publication_crypto::JobCrypto::prepare(
                &conn,
                &job.job_id,
            )
            .unwrap();
            let old_text = text(IMPORT, REVISION);
            conn.execute_batch("CREATE TABLE ack_writes(n INTEGER NOT NULL); INSERT INTO ack_writes VALUES(0);
                CREATE TRIGGER count_ack AFTER UPDATE ON jobs BEGIN UPDATE ack_writes SET n=n+1; END;").unwrap();
            crate::session_migration::preview_lifecycle_tests::test_publish_point("linked", || {
                take_costs();
            });
            let item = if worker_ack {
                jobs::publish_for_worker(&conn, &job.job_id, index, &input, &|| false, &crypto)
                    .unwrap()
                    .expect_recorded()
            } else {
                jobs::publish_item(&conn, &job.job_id, &input, &|| false).unwrap()
            };
            let cost = take_costs();
            costs.push(cost);
            assert_eq!(item.phase, "committed");
            let writes: u32 = conn
                .query_row("SELECT n FROM ack_writes", [], |row| row.get(0))
                .unwrap();
            assert_eq!(writes, u32::from(worker_ack));
            let current = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
            assert_eq!(current.state, job.state);
            assert_eq!(
                current.state_version,
                job.state_version + u32::from(worker_ack)
            );
            assert_eq!(
                current.items[index].phase,
                if worker_ack { "committed" } else { "queued" }
            );
            assert_eq!(text(IMPORT, REVISION), old_text);
            drop(input);
        }
        assert!(
            costs[0].0 > 0 && costs[0].1 > 0,
            "measure the actual final writer"
        );
        assert_eq!(
            costs[0].0, costs[1].0,
            "ACK must not add a complete member scan"
        );
        assert_eq!(
            costs[0].1, costs[1].1,
            "ACK must not reload a retained plan"
        );
        assert_eq!(
            costs[1].2,
            costs[0].2 + 1,
            "only the selected progress slot is decoded"
        );
        println!(
            "ASSOCIATION_ATOMIC_ACK_COST={}",
            json!({"items": ordinary_count + 1,
            "publicationOnly": costs[0], "publicationAndAck": costs[1], "headerWrites": 1})
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_progress_witness_rejects_after_capture_mutation() {
    for fault in [
        "input-delete",
        "proof-delete",
        "header-change",
        "foreign-job",
        "foreign-connection",
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let confirmed = confirmed_plan();
        let job = acceptance(&confirmed, "synthetic-ack-auth-mutation").unwrap();
        let original = raw_catalog(&conn);
        let crypto =
            crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &job.job_id)
                .unwrap();
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate).unwrap();
        let auth = jobs::entry::authenticate_writer(&tx, &job.job_id, &crypto).unwrap();
        match fault {
            "input-delete" => {
                tx.execute("DELETE FROM job_items WHERE job_id=?1", [&job.job_id])
                    .unwrap();
            }
            "proof-delete" => {
                tx.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "header-change" => {
                tx.execute(
                    "UPDATE jobs SET json=json_set(json,'$.items',json('[]')) WHERE job_id=?1",
                    [&job.job_id],
                )
                .unwrap();
            }
            _ => {}
        }
        let damaged = raw_catalog(&tx);
        jobs::take_job_item_deserializes();
        let other_conn = Connection::open_in_memory().unwrap();
        let other_tx = other_conn.unchecked_transaction().unwrap();
        let writer = if fault == "foreign-connection" {
            &other_tx
        } else {
            &tx
        };
        let writer_id = if fault == "foreign-job" {
            OLD_JOB
        } else {
            &job.job_id
        };
        let error = job_progress::set_item_authenticated(writer, writer_id, 0, &job.items[0], auth)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale, "{fault}: {error:?}");
        assert_eq!(
            jobs::take_job_item_deserializes(),
            0,
            "{fault}: refuse before projection decode"
        );
        assert_eq!(raw_catalog(&tx), damaged, "{fault}: no product SQL writes");
        other_tx.rollback().unwrap();
        tx.rollback().unwrap();
        assert_eq!(raw_catalog(&conn), original);
        job_progress::set_item(&conn, &job.job_id, 0, &job.items[0]).unwrap();
        assert_eq!(
            jobs::get_job(&conn, &job.job_id)
                .unwrap()
                .unwrap()
                .state_version,
            job.state_version + 1
        );
        println!(
            "ASSOCIATION_PROGRESS_AUTH_AFTER_CAPTURE={fault}:refused-before-decode:same-catalog"
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_progress_publication_sql_abort_is_atomic() {
    for fault in ["header", "catalog", "receipt"] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let confirmed = confirmed_plan();
        let job = acceptance(&confirmed, "synthetic-ack-atomic-abort").unwrap();
        let crypto =
            crate::session_migration::publication_crypto::JobCrypto::prepare(&conn, &job.job_id)
                .unwrap();
        let input =
            job_control::load_input(&conn, &job.job_id, &job.items[0].candidate_id).unwrap();
        let old_text = text(IMPORT, REVISION);
        let old_entry: String = conn
            .query_row(
                "SELECT current_revision FROM entries WHERE import_id=?1",
                [IMPORT],
                |row| row.get(0),
            )
            .unwrap();
        let trigger = match fault {
            "header" => "CREATE TEMP TRIGGER abort_ack BEFORE UPDATE ON jobs BEGIN SELECT RAISE(ABORT,'synthetic ACK header abort'); END;",
            "catalog" => "CREATE TEMP TRIGGER abort_ack BEFORE UPDATE ON entries WHEN NEW.current_revision<>OLD.current_revision BEGIN SELECT RAISE(ABORT,'synthetic ACK catalog abort'); END;",
            _ => "CREATE TEMP TRIGGER abort_ack BEFORE INSERT ON receipts WHEN NEW.phase='committed' BEGIN SELECT RAISE(ABORT,'synthetic ACK receipt abort'); END;",
        };
        conn.execute_batch(trigger).unwrap();
        assert!(
            jobs::publish_for_worker(&conn, &job.job_id, 0, &input, &|| false, &crypto).is_err()
        );
        let current = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
        assert_eq!(current.state_version, job.state_version);
        assert_eq!(current.items[0].phase, "queued");
        let phase: String = conn
            .query_row(
                "SELECT phase FROM job_items WHERE job_id=?1",
                [&job.job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "queued");
        let revision: String = conn
            .query_row(
                "SELECT current_revision FROM entries WHERE import_id=?1",
                [IMPORT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, old_entry);
        let receipt = jobs::latest_receipt(&conn, &job.job_id, &job.items[0].candidate_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.phase, "linked",
            "earlier durable file/link boundaries stay recoverable"
        );
        let committed: u32 = conn
            .query_row(
                "SELECT count(*) FROM receipts WHERE job_id=?1 AND phase='committed'",
                [&job.job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(committed, 0);
        conn.execute_batch("DROP TRIGGER abort_ack").unwrap();
        let item = jobs::publish_for_worker(&conn, &job.job_id, 0, &input, &|| false, &crypto)
            .unwrap()
            .expect_recorded();
        assert_eq!(item.import_id, receipt.import_id);
        assert_eq!(item.revision_id, receipt.revision_id);
        assert_eq!(
            jobs::get_job(&conn, &job.job_id)
                .unwrap()
                .unwrap()
                .state_version,
            job.state_version + 1
        );
        let stable_receipts = receipts(&conn);
        drop(input);
        reset_runtime();
        assert_eq!(job_control::run_pending().unwrap(), 1);
        let complete = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
        assert_eq!(complete.state, JobState::Completed);
        assert_eq!(
            complete.state_version,
            job.state_version + 3,
            "restart has no second item ACK"
        );
        assert_eq!(receipts(&conn), stable_receipts);
        assert_eq!(job_control::run_pending().unwrap(), 0);
        assert_eq!(text(IMPORT, REVISION), old_text);
        println!("ASSOCIATION_PROGRESS_ATOMIC_ABORT={fault}:rollback:same-ids:single-ack");
    }
}
