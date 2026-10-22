use super::*;
use crate::session_migration::{
    catalog,
    cursor::InstallationAuthenticator,
    errors::ErrorCode,
    job_control,
    test_home::{job_read::queued, IsolatedAppHome},
};

#[test]
fn retention_legacy_request_and_owner_reads_work_with_key_loader_blocked() {
    let _home = IsolatedAppHome::acquire("retention-crypto-no-key");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate");
    let path = item.prepared.as_ref().unwrap().path.clone();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let leases = vec![(path, lease)];
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    assert!(!key.exists());
    std::fs::create_dir(&key).unwrap();
    assert!(InstallationAuthenticator::load().is_err());
    let changes = conn.total_changes();
    assert_eq!(existing(&conn, &job).unwrap().unwrap().job_id, job.job_id);
    assert_eq!(
        owner_results(&conn, &leases).unwrap()[0].as_ref().unwrap(),
        &Some(true)
    );
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    assert_eq!(
        owner_results(&conn, &leases).unwrap()[0].as_ref().unwrap(),
        &Some(false)
    );
    assert!(key.is_dir());
}

#[test]
fn retention_new_owner_after_crypto_selection_refuses_instead_of_unowned() {
    let _home = IsolatedAppHome::acquire("retention-crypto-new-owner");
    let conn = catalog::open_catalog().unwrap();
    let cryptos = JobCryptos::prepare(&conn, &["candidate".into()], &[]).unwrap();
    let (_job, item) = queued("candidate");
    let path = item.prepared.as_ref().unwrap().path.clone();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let leases = vec![(path, lease)];
    let tx = conn.unchecked_transaction().unwrap();
    let before = tx.total_changes();
    let results = owner_results_in_snapshot(&tx, &leases, &cryptos).unwrap();
    assert_eq!(
        results[0].as_ref().unwrap_err().code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(tx.total_changes(), before);
    tx.rollback().unwrap();
    assert_eq!(
        owner_results(&conn, &leases).unwrap()[0].as_ref().unwrap(),
        &Some(true)
    );
}

#[test]
fn retention_refused_signed_owner_does_not_block_independent_legacy_owner() {
    let home = IsolatedAppHome::acquire("retention-crypto-owner-isolation");
    let conn = catalog::open_catalog().unwrap();
    let (_job, item) = queued("candidate");
    let path = item.prepared.as_ref().unwrap().path.clone();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let mut bad_lease = lease.clone();
    bad_lease.candidate_id = "bad".into();
    conn.execute_batch(
        "INSERT INTO jobs(job_id,request_id,plan_hash,state,state_version,json,created_at)
        VALUES('abcdef12-1234-8567-8123-abcdef123456','bad','h','queued',1,'invalid','now');
        INSERT INTO job_items(job_id,candidate_id,phase,result_json)
        VALUES('abcdef12-1234-8567-8123-abcdef123456','bad','queued','invalid');",
    )
    .unwrap();
    let before = conn.total_changes();
    let results = owner_results(
        &conn,
        &[
            (path, lease),
            (home.path.join("synthetic-unread-path"), bad_lease),
        ],
    )
    .unwrap();
    assert_eq!(results[0].as_ref().unwrap(), &Some(true));
    // This owner has invalid header JSON, rejected before acceptance lookup.
    assert_eq!(
        results[1].as_ref().unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    assert_eq!(conn.total_changes(), before);
    assert!(catalog::imports_dir().join(".cursor-hmac-secret").is_file());
}

#[test]
fn retention_malformed_owner_id_refuses_only_its_independent_preview() {
    let home = IsolatedAppHome::acquire("retention-crypto-invalid-owner-id");
    let conn = catalog::open_catalog().unwrap();
    let (_job, item) = queued("candidate");
    let path = item.prepared.as_ref().unwrap().path.clone();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let mut bad_lease = lease.clone();
    bad_lease.candidate_id = "malformed".into();
    conn.execute(
        "INSERT INTO job_items(job_id,candidate_id,phase,result_json)
        VALUES(zeroblob(1),'malformed','queued','invalid')",
        [],
    )
    .unwrap();
    let changes = conn.total_changes();
    let results = owner_results(
        &conn,
        &[
            (path, lease),
            (home.path.join("synthetic-unread-path"), bad_lease),
        ],
    )
    .unwrap();
    assert_eq!(results[0].as_ref().unwrap(), &Some(true));
    assert!(
        results[1].is_err(),
        "malformed owner is not an unowned preview"
    );
    assert_eq!(conn.total_changes(), changes);
}
