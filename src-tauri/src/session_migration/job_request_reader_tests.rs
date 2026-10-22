use super::*;
use crate::session_migration::{
    catalog,
    cursor::InstallationAuthenticator,
    jobs,
    test_home::{job_read::queued, IsolatedAppHome},
};

#[test]
fn request_reader_rechecks_owner_appearance_replacement_and_deletion() {
    let _home = IsolatedAppHome::acquire("request-owner-race");
    let conn = catalog::open_catalog().unwrap();
    let absent = PreparedRequest::prepare(&conn, "synthetic-absent").unwrap();
    let (job, _item) = queued("candidate");
    let prepared = PreparedRequest::prepare(&conn, &job.request_id).unwrap();
    assert_eq!(
        prepared
            .read(&conn, &job.request_id)
            .unwrap()
            .unwrap()
            .job_id,
        job.job_id
    );
    for fault in ["appearance", "replacement", "deletion", "wrong-request"] {
        let tx = conn.unchecked_transaction().unwrap();
        let (probe, request) = if fault == "appearance" {
            tx.execute(
                "UPDATE jobs SET request_id='synthetic-absent' WHERE job_id=?1",
                [&job.job_id],
            )
            .unwrap();
            (&absent, "synthetic-absent")
        } else {
            if fault == "replacement" {
                tx.execute(
                    "UPDATE jobs SET job_id='synthetic-other' WHERE job_id=?1",
                    [&job.job_id],
                )
                .unwrap();
            }
            if fault == "deletion" {
                tx.execute("DELETE FROM jobs WHERE job_id=?1", [&job.job_id])
                    .unwrap();
            }
            (
                &prepared,
                if fault == "wrong-request" {
                    "synthetic-other-request"
                } else {
                    job.request_id.as_str()
                },
            )
        };
        let before = tx.total_changes();
        let error = probe.read(&tx, request).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale, "{fault}");
        assert!(error.is_job_local());
        assert_eq!(tx.total_changes(), before);
        tx.rollback().unwrap();
    }
    assert_eq!(
        jobs::find_job_by_request(&conn, &job.request_id)
            .unwrap()
            .unwrap()
            .job_id,
        job.job_id
    );
}

#[test]
fn request_reader_preserves_absence_legacy_snapshot_and_blocked_key_loader() {
    let _home = IsolatedAppHome::acquire("request-no-key");
    let conn = catalog::open_catalog().unwrap();
    let (job, _item) = queued("candidate");
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    assert!(!key.exists());
    std::fs::create_dir(&key).unwrap();
    assert!(InstallationAuthenticator::load().is_err());
    let prepared = PreparedRequest::prepare(&conn, &job.request_id).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        prepared.read(&tx, &job.request_id).unwrap().unwrap().job_id,
        job.job_id
    );
    assert_eq!(
        jobs::find_job_by_request(&tx, &job.request_id)
            .unwrap()
            .unwrap()
            .job_id,
        job.job_id
    );
    assert!(jobs::find_job_by_request(&tx, "missing").unwrap().is_none());
    assert!(PreparedRequest::prepare(&tx, &job.request_id).is_err());
    assert_eq!(tx.total_changes(), changes);
    assert!(key.is_dir());
}

#[test]
fn request_reader_does_not_trust_prepared_body_or_column_json_disagreement() {
    let _home = IsolatedAppHome::acquire("request-current-body");
    let conn = catalog::open_catalog().unwrap();
    let (job, _item) = queued("candidate");
    let prepared = PreparedRequest::prepare(&conn, &job.request_id).unwrap();
    for field in ["requestId", "jobId"] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "UPDATE jobs SET json=json_set(json,?2,'synthetic-other') WHERE job_id=?1",
            rusqlite::params![job.job_id, format!("$.{field}")],
        )
        .unwrap();
        let changes = tx.total_changes();
        assert_eq!(
            prepared.read(&tx, &job.request_id).unwrap_err().code,
            ErrorCode::PlanStale
        );
        assert_eq!(tx.total_changes(), changes);
        tx.rollback().unwrap();
    }
}

#[test]
fn request_reader_new_signed_owner_requires_acceptance_before_body_decode() {
    let _home = IsolatedAppHome::acquire("request-signed-fence");
    let conn = catalog::open_catalog().unwrap();
    let absent = PreparedRequest::prepare(&conn, "synthetic-signed-request").unwrap();
    conn.execute("INSERT INTO jobs(job_id,request_id,plan_hash,state,state_version,json,created_at)
        VALUES('abcdef12-1234-8567-8123-abcdef123456','synthetic-signed-request','h','queued',1,'invalid','now')", []).unwrap();
    let before = conn.total_changes();
    assert_eq!(
        absent
            .read(&conn, "synthetic-signed-request")
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(
        jobs::find_job_by_request(&conn, "synthetic-signed-request")
            .unwrap_err()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(conn.total_changes(), before);
    assert!(catalog::imports_dir().join(".cursor-hmac-secret").is_file());
}
