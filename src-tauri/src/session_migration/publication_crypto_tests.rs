//! Operation-local crypto is not a SQL grant or a signed-producer rollout flag.
use super::*;
use crate::session_migration::{
    catalog, job_control, job_progress,
    jobs::{self, JobRecord, JobState},
    test_home::IsolatedAppHome,
};

const SIGNED_JOB: &str = "abcdef12-1234-8567-8123-abcdef123456";

fn seed(conn: &Connection, id: &str) -> JobRecord {
    let job = JobRecord {
        job_id: id.into(),
        request_id: format!("synthetic-{id}"),
        plan_id: "synthetic-plan".into(),
        plan_hash: "a".repeat(64),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: "2026-10-15T00:00:00Z".into(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    jobs::save_job(conn, &job).unwrap();
    job
}

#[test]
fn legacy_read_state_cancel_and_undo_do_not_create_or_load_crypto() {
    let _home = IsolatedAppHome::acquire("publication-crypto-legacy");
    let conn = catalog::open_catalog().unwrap();
    let job = seed(&conn, "synthetic-legacy");
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    for blocked in [false, true] {
        if blocked {
            std::fs::create_dir(&key).unwrap();
            assert!(InstallationAuthenticator::load().is_err());
        }
        let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
        assert!(crypto
            .require(&job.job_id, PublicationProtocol::Legacy)
            .unwrap()
            .is_none());
        assert!(jobs::get_job_with_crypto(&conn, &job.job_id, &crypto)
            .unwrap()
            .is_some());
        job_progress::request_cancel(&conn, &job.job_id).unwrap();
        job_progress::cancel_unstarted_with_crypto(&conn, &job.job_id, &crypto).unwrap();
        job_control::change_job(&conn, &job.job_id, |job| {
            job.state = JobState::Completed;
            Ok(())
        })
        .unwrap();
        jobs::undo_job(&conn, &job.job_id).unwrap();
        assert_eq!(key.exists(), blocked);
        if blocked {
            assert!(key.is_dir());
            assert_eq!(std::fs::read_dir(&key).unwrap().count(), 0);
        }
    }
}

#[test]
fn crypto_preparation_refuses_inside_transaction_without_changes() {
    let _home = IsolatedAppHome::acquire("publication-crypto-transaction");
    let conn = catalog::open_catalog().unwrap();
    seed(&conn, "synthetic-legacy");
    let tx = conn.unchecked_transaction().unwrap();
    let before = tx.total_changes();
    for id in ["synthetic-legacy", "missing", SIGNED_JOB] {
        let error = JobCrypto::prepare(&tx, id)
            .err()
            .expect("prepare outside snapshot only");
        assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
        assert!(error.is_job_local());
    }
    assert_eq!(tx.total_changes(), before);
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}

#[test]
fn absent_future_and_signed_jobs_preserve_not_found_without_crypto() {
    let _home = IsolatedAppHome::acquire("publication-crypto-absent");
    let conn = catalog::open_catalog().unwrap();
    let before = conn.total_changes();
    for id in [
        SIGNED_JOB,
        "abcdef12-1234-9567-8123-abcdef123456",
        "missing",
    ] {
        let crypto = JobCrypto::prepare(&conn, id).unwrap();
        assert!(jobs::get_job_with_crypto(&conn, id, &crypto)
            .unwrap()
            .is_none());
        assert!(jobs::get_job(&conn, id).unwrap().is_none());
    }
    assert_eq!(conn.total_changes(), before);
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}

#[test]
fn crypto_is_bound_to_job_but_current_sql_is_always_rechecked() {
    let _home = IsolatedAppHome::acquire("publication-crypto-binding");
    let conn = catalog::open_catalog().unwrap();
    seed(&conn, "synthetic-first");
    seed(&conn, "synthetic-second");
    let crypto = JobCrypto::prepare(&conn, "synthetic-first").unwrap();
    let changes = conn.total_changes();
    assert!(jobs::get_job_with_crypto(&conn, "synthetic-second", &crypto).is_err());
    assert_eq!(conn.total_changes(), changes);
    conn.execute(
        "UPDATE jobs SET json='invalid' WHERE job_id='synthetic-first'",
        [],
    )
    .unwrap();
    let changes = conn.total_changes();
    assert!(jobs::get_job_with_crypto(&conn, "synthetic-first", &crypto).is_err());
    assert_eq!(conn.total_changes(), changes);
    conn.execute("DELETE FROM jobs WHERE job_id='synthetic-first'", [])
        .unwrap();
    assert!(jobs::get_job_with_crypto(&conn, "synthetic-first", &crypto)
        .unwrap()
        .is_none());
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}

#[test]
fn prepared_absence_cannot_enable_a_later_signed_job() {
    let _home = IsolatedAppHome::acquire("publication-crypto-rollout");
    let conn = catalog::open_catalog().unwrap();
    let crypto = JobCrypto::prepare(&conn, SIGNED_JOB).unwrap();
    seed(&conn, SIGNED_JOB);
    let changes = conn.total_changes();
    let error = jobs::get_job_with_crypto(&conn, SIGNED_JOB, &crypto).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(error.is_job_local());
    let loaded = JobCrypto::prepare(&conn, SIGNED_JOB).unwrap();
    let error = jobs::get_job_with_crypto(&conn, SIGNED_JOB, &loaded).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert_eq!(conn.total_changes(), changes);
    assert!(catalog::imports_dir().join(".cursor-hmac-secret").is_file());
}

#[test]
fn writer_entry_rejects_other_transaction_job_and_mutation() {
    let _home = IsolatedAppHome::acquire("publication-crypto-writer");
    let conn = catalog::open_catalog().unwrap();
    seed(&conn, "synthetic-first");
    let crypto = JobCrypto::prepare(&conn, "synthetic-first").unwrap();
    for fault in ["transaction", "job", "mutation", "savepoint-rollback"] {
        let tx = conn.unchecked_transaction().unwrap();
        let entry = jobs::entry::authenticate_writer(&tx, "synthetic-first", &crypto).unwrap();
        let other = Connection::open_in_memory().unwrap();
        let other_tx = other.unchecked_transaction().unwrap();
        if matches!(fault, "mutation" | "savepoint-rollback") {
            tx.execute_batch("SAVEPOINT changed; UPDATE jobs SET state_version=state_version+1")
                .unwrap();
            if fault == "savepoint-rollback" {
                tx.execute_batch("ROLLBACK TO changed; RELEASE changed")
                    .unwrap();
            }
        }
        let before = tx.total_changes();
        let writer = if fault == "transaction" {
            &other_tx
        } else {
            &tx
        };
        let id = if fault == "job" {
            "synthetic-other"
        } else {
            "synthetic-first"
        };
        let error = entry.require_writer(writer, id).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale, "{fault}");
        assert_eq!(tx.total_changes(), before);
        other_tx.rollback().unwrap();
        tx.rollback().unwrap();
    }
    let tx = conn.unchecked_transaction().unwrap();
    jobs::entry::authenticate_writer(&tx, "synthetic-first", &crypto)
        .unwrap()
        .require_writer(&tx, "synthetic-first")
        .unwrap();
    tx.commit().unwrap();
}
