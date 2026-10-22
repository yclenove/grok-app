use super::*;
use crate::session_migration::{
    catalog,
    cursor::InstallationAuthenticator,
    job_control,
    test_home::{job_read::queued, IsolatedAppHome},
};

#[test]
fn durable_metadata_read_never_loads_key_or_reopens_disposed_preview() {
    let home = IsolatedAppHome::acquire("durable-reader-no-files");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate");
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    assert!(!key.exists());
    std::fs::create_dir(&key).unwrap();
    assert!(InstallationAuthenticator::load().is_err());
    let path = &item.prepared.as_ref().unwrap().path;
    let retained = home.path.join("synthetic-retained-preview");
    std::fs::rename(path, &retained).unwrap();
    job_control::association_members::take_complete_member_scans();
    let changes = conn.total_changes();
    for _ in 0..20 {
        assert_eq!(
            load(&conn, &job.job_id, &item.candidate_id, &crypto)
                .unwrap()
                .candidate_id,
            item.candidate_id
        );
    }
    assert_eq!(
        job_control::association_members::take_complete_member_scans(),
        0
    );
    assert_eq!(conn.total_changes(), changes);
    assert!(key.is_dir());
    std::fs::rename(&retained, path).unwrap();
}

#[test]
fn durable_reader_observes_one_snapshot_and_rechecks_after_external_commit() {
    let _home = IsolatedAppHome::acquire("durable-reader-snapshot");
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL").unwrap();
    let (job, item) = queued("candidate");
    let other = catalog::open_catalog().unwrap();
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let changes = conn.total_changes();
    let tx = conn.unchecked_transaction().unwrap();
    load(&tx, &job.job_id, &item.candidate_id, &crypto).unwrap();
    other
        .execute(
            "UPDATE job_items SET result_json='invalid' WHERE job_id=?1",
            [&job.job_id],
        )
        .unwrap();
    load(&tx, &job.job_id, &item.candidate_id, &crypto).unwrap();
    assert_eq!(tx.total_changes(), changes);
    tx.commit().unwrap();
    assert!(load(&conn, &job.job_id, &item.candidate_id, &crypto).is_err());
    assert_eq!(conn.total_changes(), changes);
}

#[test]
fn durable_reader_rejects_wrong_crypto_and_coordinated_candidate_projection() {
    let _home = IsolatedAppHome::acquire("durable-reader-bindings");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate");
    let wrong = JobCrypto::prepare(&conn, "synthetic-other-job").unwrap();
    let before = conn.total_changes();
    assert_eq!(
        load(&conn, &job.job_id, &item.candidate_id, &wrong)
            .err()
            .unwrap()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(conn.total_changes(), before);
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    conn.execute("UPDATE job_items SET result_json=json_set(result_json,'$.candidateId','other') WHERE job_id=?1", [&job.job_id]).unwrap();
    let before = conn.total_changes();
    assert_eq!(
        load(&conn, &job.job_id, &item.candidate_id, &crypto)
            .err()
            .unwrap()
            .code,
        ErrorCode::PlanStale
    );
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn committed_recovery_validates_metadata_then_refuses_archive_io_in_writer() {
    let home = IsolatedAppHome::acquire("durable-recovery-no-writer-files");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate");
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let done = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    let previous = &done.items[0];
    let input = load(&conn, &job.job_id, &item.candidate_id, &crypto).unwrap();
    let archive = catalog::revision_dir(
        previous.import_id.as_deref().unwrap(),
        previous.revision_id.as_deref().unwrap(),
    )
    .unwrap();
    let retained = home.path.join("synthetic-retained-archive");
    std::fs::rename(&archive, &retained).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute_batch("SAVEPOINT future_receipt").unwrap();
    tx.execute(
        "UPDATE receipts SET json=json_set(json,'$.version',99) WHERE job_id=?1",
        [&job.job_id],
    )
    .unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        recover(&tx, &job.job_id, previous, &input, &crypto)
            .unwrap_err()
            .code,
        ErrorCode::SchemaUnsupported
    );
    assert_eq!(tx.total_changes(), changes);
    assert!(!archive.exists());
    tx.execute_batch("ROLLBACK TO future_receipt; RELEASE future_receipt")
        .unwrap();
    let changes = tx.total_changes();
    assert_eq!(
        recover(&tx, &job.job_id, previous, &input, &crypto)
            .unwrap_err()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert_eq!(tx.total_changes(), changes);
    tx.rollback().unwrap();
    std::fs::rename(&retained, &archive).unwrap();
    let recovered = recover(&conn, &job.job_id, previous, &input, &crypto)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.import_id, previous.import_id);
    assert_eq!(recovered.revision_id, previous.revision_id);
}

#[test]
fn durable_signed_job_without_original_crypto_or_input_is_refused() {
    let _home = IsolatedAppHome::acquire("durable-reader-signed-fence");
    let conn = catalog::open_catalog().unwrap();
    let id = "abcdef12-1234-8567-8123-abcdef123456";
    let crypto = JobCrypto::prepare(&conn, id).unwrap();
    let changes = conn.total_changes();
    let error = load(&conn, id, "missing-candidate", &crypto).err().unwrap();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(error.is_job_local());
    assert_eq!(conn.total_changes(), changes);
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}

#[test]
fn recovery_owner_uses_durable_metadata_without_key_or_prepared_preview_io() {
    let home = IsolatedAppHome::acquire("recovery-owner-durable-metadata");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate-original-中文");
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let input = load(&conn, &job.job_id, &item.candidate_id, &crypto).unwrap();
    let original = input.recovery_input();
    let preview = &item.prepared.as_ref().unwrap().path;
    std::fs::rename(preview, home.path.join("unused-retained-preview")).unwrap();
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    std::fs::create_dir(&key).unwrap();
    assert!(InstallationAuthenticator::load().is_err());
    let digest = hash::sha256_bytes(&serde_json::to_vec(&input).unwrap());
    let changes = conn.total_changes();
    for _ in 0..10 {
        original
            .owner(&conn, &job.job_id, &crypto)
            .unwrap()
            .verify(&conn)
            .unwrap();
        assert_eq!(original.digest().unwrap(), digest);
    }
    assert_eq!(conn.total_changes(), changes);
    assert!(!preview.exists());
    assert!(key.is_dir());
}

#[test]
fn legacy_recovery_progress_preserves_existing_setter_and_mutable_display_title() {
    let _home = IsolatedAppHome::acquire("legacy-recovery-progress-compat");
    let conn = catalog::open_catalog().unwrap();
    let (job, item) = queued("candidate");
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let done = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    let previous = &done.items[0];
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let input = load(&conn, &job.job_id, &item.candidate_id, &crypto).unwrap();
    conn.execute(
        "UPDATE entries SET title='user edited display title' WHERE import_id=?1",
        [previous.import_id.as_deref().unwrap()],
    )
    .unwrap();
    let progress = recover_progress(&conn, &job.job_id, 0, previous, &input, &crypto)
        .unwrap()
        .unwrap();
    let before = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    progress.finish(&conn, &job.job_id, 0, &crypto).unwrap();
    let after = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(after.state_version, before.state_version + 1);
    assert_eq!(
        after.items[0].title.as_deref(),
        Some("user edited display title")
    );
    // The signed ACK constructor deliberately uses the ORIGINAL input title,
    // while legacy recovery continues its prior display-title behavior.
    assert_ne!(
        input
            .recovery_input()
            .result(
                previous.import_id.as_deref().unwrap(),
                previous.revision_id.as_deref().unwrap(),
                None,
            )
            .title,
        after.items[0].title
    );
}
