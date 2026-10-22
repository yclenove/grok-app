use super::{
    fixture::{self, Fixture},
    *,
};
use crate::session_migration::catalog;
use rusqlite::params;

#[test]
fn acceptance_point_input_reader_checks_full_membership_but_only_selected_body() {
    let _home = IsolatedAppHome::acquire("acceptance-point-input");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let candidate = &fixture.prepared[0].0.candidate_id;
    let expected = fixture.prepared[0].0.input_identity().unwrap().digest;
    assert_eq!(
        journal::validate_input_snapshot(&tx, &auth, &fixture.job.job_id, candidate).unwrap(),
        expected
    );
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1 AND candidate_id=?2",
        params![fixture.job.job_id, fixture.prepared[1].0.candidate_id],
    )
    .unwrap();
    let changes = tx.total_changes();
    // An unrelated body is not materialized/hashed by a point-input read. Its
    // own point read and full snapshot validation must still reject corruption.
    assert_eq!(
        journal::validate_input_snapshot(&tx, &auth, &fixture.job.job_id, candidate).unwrap(),
        expected
    );
    assert!(journal::validate_input_snapshot(
        &tx,
        &auth,
        &fixture.job.job_id,
        &fixture.prepared[1].0.candidate_id
    )
    .is_err());
    assert!(journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).is_err());
    assert_eq!(tx.total_changes(), changes);
    tx.execute("DELETE FROM meta WHERE key=?1", [fixture.member_key(1)])
        .unwrap();
    let changes = tx.total_changes();
    assert!(journal::validate_input_snapshot(&tx, &auth, &fixture.job.job_id, candidate).is_err());
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn acceptance_snapshot_validation_is_read_only_and_rejects_changed_input() {
    let _home = IsolatedAppHome::acquire("acceptance-read-snapshot");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.commit().unwrap();
    let before = fixture::sql_digest(&conn);
    let changes = conn.total_changes();
    assert!(journal::validate_snapshot(&conn, &auth, &fixture.job.job_id).is_err());
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&conn), before);
    let tx = conn.unchecked_transaction().unwrap();
    journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    assert_eq!(tx.total_changes(), changes);
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1 AND candidate_id=?2",
        params![fixture.job.job_id, fixture.prepared[1].0.candidate_id],
    )
    .unwrap();
    let damaged = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    assert!(journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).is_err());
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), damaged);
    tx.rollback().unwrap();
    assert_eq!(fixture::sql_digest(&conn), before);
}

#[test]
fn acceptance_input_range_rejects_changed_body_types_without_mutating_neighbor_jobs() {
    let _home = IsolatedAppHome::acquire("acceptance-input-range-types");
    let fixture = Fixture::new(2);
    let neighbor = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    neighbor.seed(&conn);
    let producer = fixture.capture();
    let neighbor_producer = neighbor.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    neighbor_producer.record(&tx, &neighbor.job).unwrap();
    journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    for candidate in fixture
        .prepared
        .iter()
        .map(|(input, _)| &input.candidate_id)
    {
        for sql in [
            "UPDATE job_items SET result_json=NULL WHERE job_id=?1 AND candidate_id=?2",
            "UPDATE job_items SET result_json=zeroblob(32) WHERE job_id=?1 AND candidate_id=?2",
            "UPDATE job_items SET result_json=CAST(x'ff' AS TEXT) WHERE job_id=?1 AND candidate_id=?2",
            "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1 AND candidate_id=?2",
        ] {
            tx.execute_batch("SAVEPOINT input_range_fault").unwrap();
            assert_eq!(tx.execute(sql, params![fixture.job.job_id, candidate]).unwrap(), 1);
            let before: (String, String) = tx
                .query_row(
                    "SELECT typeof(result_json),hex(CAST(result_json AS BLOB)) FROM job_items
                     WHERE job_id=?1 AND candidate_id=?2",
                    params![fixture.job.job_id, candidate],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let changes = tx.total_changes();
            assert_eq!(
                journal::validate_snapshot(&tx, &auth, &fixture.job.job_id)
                    .unwrap_err()
                    .code,
                ErrorCode::HistoryAccessUnavailable
            );
            journal::validate_snapshot(&tx, &auth, &neighbor.job.job_id).unwrap();
            let after: (String, String) = tx
                .query_row(
                    "SELECT typeof(result_json),hex(CAST(result_json AS BLOB)) FROM job_items
                     WHERE job_id=?1 AND candidate_id=?2",
                    params![fixture.job.job_id, candidate],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(before, after);
            assert_eq!(tx.total_changes(), changes);
            tx.execute_batch("ROLLBACK TO input_range_fault; RELEASE input_range_fault")
                .unwrap();
            journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
        }
    }
}

#[test]
fn warmed_member_commitments_do_not_accept_resealed_input_or_allocation_under_the_old_root() {
    let _home = IsolatedAppHome::acquire("acceptance-commitment-root");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    let key = fixture.member_key(0);
    let original: String = tx
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    for input_changed in [true, false] {
        tx.execute_batch("SAVEPOINT commitment_fault").unwrap();
        let mut envelope: Envelope<Member> = decode(original.as_bytes()).unwrap();
        let changed = if input_changed {
            &mut envelope.body.input_digest
        } else {
            &mut envelope.body.allocation_digest
        };
        *changed = "9".repeat(64);
        let encoded = seal(&auth, MEMBER_DOMAIN, envelope.body).unwrap();
        tx.execute(
            "UPDATE meta SET value=?1 WHERE key=?2",
            [encoded, key.clone()],
        )
        .unwrap();
        let before = fixture::sql_digest(&tx);
        let changes = tx.total_changes();
        assert!(journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).is_err());
        assert_eq!(tx.total_changes(), changes);
        assert_eq!(fixture::sql_digest(&tx), before);
        tx.execute_batch("ROLLBACK TO commitment_fault; RELEASE commitment_fault")
            .unwrap();
        journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    }
}

#[test]
fn acceptance_snapshot_validation_uses_loaded_crypto_with_loader_blocked() {
    let home = IsolatedAppHome::acquire("acceptance-read-loaded-crypto");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.commit().unwrap();
    let blocked = home.path.join("synthetic-blocked-key-home");
    std::fs::write(&blocked, b"synthetic file; not a home").unwrap();
    std::env::set_var("GROK_APP_HOME", &blocked);
    assert!(InstallationAuthenticator::load().is_err());
    let tx = conn.unchecked_transaction().unwrap();
    let before = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
}

#[test]
fn acceptance_outcome_binding_uses_exact_signed_member_not_mutable_results() {
    let _home = IsolatedAppHome::acquire("acceptance-outcome-binding");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let input = &fixture.prepared[0].0;
    let encoded: String = tx
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [fixture.member_key(0)],
            |row| row.get(0),
        )
        .unwrap();
    let changes = tx.total_changes();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    let binding = accepted.outcome_binding(&tx, &input.candidate_id).unwrap();
    assert!(
        serde_json::to_value(&binding).unwrap()
            == json!({
                "jobId": fixture.job.job_id,
                "contextDigest": record::context_digest(&fixture.job),
                "planId": fixture.job.plan_id,
                "planHash": fixture.job.plan_hash,
                "planDigest": hash::sha256_bytes(fixture.plan.as_bytes()),
                "acceptanceDigest": hash::sha256_bytes(encoded.as_bytes()),
                "candidateDigest": hash::sha256_bytes(input.candidate_id.as_bytes()),
                "inputDigest": input.input_identity().unwrap().digest,
            })
    );
    let other = accepted
        .outcome_binding(&tx, &fixture.prepared[1].0.candidate_id)
        .unwrap();
    assert!(binding != other);
    assert_eq!(tx.total_changes(), changes);
    // Mutable display/result fields cannot become the result protocol's root.
    tx.execute("UPDATE jobs SET json=json_set(json,'$.items[0].phase','committed','$.items[0].importId','foreign-import','$.items[0].revisionId','foreign-revision','$.items[0].mappingProjectId','foreign-project') WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
    let fresh = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    assert!(fresh.outcome_binding(&tx, &input.candidate_id).unwrap() == binding);
    // Canonical envelope semantics are deliberate: whitespace changes are not
    // a different acceptance, nor permission to replace the durable record.
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<Value>(&encoded).unwrap()).unwrap();
    tx.execute(
        "UPDATE meta SET value=?2 WHERE key=?1",
        params![fixture.member_key(0), pretty],
    )
    .unwrap();
    let fresh = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    assert!(fresh.outcome_binding(&tx, &input.candidate_id).unwrap() == binding);
}

#[test]
fn acceptance_outcome_binding_cannot_reuse_transaction_or_changed_input() {
    let _home = IsolatedAppHome::acquire("acceptance-outcome-snapshot");
    let fixture = Fixture::new(1);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.commit().unwrap();
    let other = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let other_tx = other.unchecked_transaction().unwrap();
    let candidate = &fixture.prepared[0].0.candidate_id;
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    accepted.outcome_binding(&tx, candidate).unwrap();
    assert!(accepted.outcome_binding(&other_tx, candidate).is_err());
    assert!(accepted.outcome_binding(&tx, "missing-candidate").is_err());
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
        [&fixture.job.job_id],
    )
    .unwrap();
    assert!(accepted.outcome_binding(&tx, candidate).is_err());
    let fresh = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    let before = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    assert!(fresh.outcome_binding(&tx, candidate).is_err());
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
}

#[test]
fn acceptance_journal_exact_retry_reopen_and_read_only_candidate_checks() {
    let _home = IsolatedAppHome::acquire("acceptance-reopen");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    {
        let conn = catalog::open_catalog().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        fixture.seed(&tx);
        producer.record(&tx, &fixture.job).unwrap();
        assert_eq!(fixture.metadata_count(&tx), 3);
        let changes = tx.total_changes();
        producer.record(&tx, &fixture.job).unwrap();
        assert_eq!(tx.total_changes(), changes);
        tx.commit().unwrap();
    }
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    for (input, _) in &fixture.prepared {
        let candidate = accepted.candidate(&tx, &input.candidate_id).unwrap();
        assert!(candidate.body.input_digest == input.input_identity().unwrap().digest);
    }
    assert!(accepted.candidate(&tx, "missing").is_err());
    assert_eq!(tx.total_changes(), changes);
    // Synthetic facts do not grant source/proof/native result ownership.
    // Inside this transaction the caller has not preloaded signed crypto.
    let error = crate::session_migration::jobs::get_job(&tx, &fixture.job.job_id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
}

#[test]
fn acceptance_journal_missing_extra_or_corrupt_members_are_not_repaired() {
    let _home = IsolatedAppHome::acquire("acceptance-membership");
    let fixture = Fixture::new(2);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    for fault in [
        "missing-header",
        "missing-member",
        "extra-member",
        "corrupt-member",
        "copied-member",
        "renamed-member",
        "oversized-member-key",
        "missing-input",
        "extra-input",
        "coordinated-candidate",
        "json-membership",
        "duplicate-membership",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "missing-header" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [fixture.header_key()])
                    .unwrap();
            }
            "missing-member" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [fixture.member_key(0)])
                    .unwrap();
            }
            "extra-member" => {
                tx.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,'{}')",
                    [format!(
                        "publication_acceptance:v1:{}:member:extra",
                        fixture.job.job_id
                    )],
                )
                .unwrap();
            }
            "corrupt-member" => {
                tx.execute(
                    "UPDATE meta SET value='{}' WHERE key=?1",
                    [fixture.member_key(0)],
                )
                .unwrap();
            }
            "copied-member" => {
                tx.execute(
                    "UPDATE meta SET value=(SELECT value FROM meta WHERE key=?2) WHERE key=?1",
                    params![fixture.member_key(0), fixture.member_key(1)],
                )
                .unwrap();
            }
            "renamed-member" => {
                tx.execute(
                    "UPDATE meta SET key=key || 'suffix' WHERE key=?1",
                    [fixture.member_key(0)],
                )
                .unwrap();
            }
            "oversized-member-key" => {
                tx.execute(
                    "UPDATE meta SET key=key || ?2 WHERE key=?1",
                    params![fixture.member_key(1), "k".repeat(1024 * 1024)],
                )
                .unwrap();
            }
            "missing-input" => {
                tx.execute(
                    "DELETE FROM job_items WHERE job_id=?1 AND candidate_id=?2",
                    params![fixture.job.job_id, fixture.prepared[0].0.candidate_id],
                )
                .unwrap();
            }
            "extra-input" => {
                tx.execute("INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,'extra','queued','{}')", [&fixture.job.job_id]).unwrap();
            }
            "coordinated-candidate" => {
                tx.execute("UPDATE job_items SET candidate_id='replaced' WHERE job_id=?1 AND candidate_id=?2", params![fixture.job.job_id, fixture.prepared[0].0.candidate_id]).unwrap();
                tx.execute("UPDATE jobs SET json=json_set(json,'$.items[0].candidateId','replaced') WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
            }
            "json-membership" => {
                tx.execute(
                    "UPDATE jobs SET json=json_set(json,'$.items',json('[]')) WHERE job_id=?1",
                    [&fixture.job.job_id],
                )
                .unwrap();
            }
            "duplicate-membership" => {
                tx.execute("UPDATE jobs SET json=json_set(json,'$.items[1].candidateId',json_extract(json,'$.items[0].candidateId')) WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
            }
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        assert!(
            journal::load(&tx, &auth, &fixture.job.job_id).is_err(),
            "{fault}"
        );
        assert_eq!(tx.total_changes(), changes, "reader cannot repair {fault}");
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn acceptance_journal_original_job_plan_scope_and_marker_are_bound() {
    let _home = IsolatedAppHome::acquire("acceptance-original-bindings");
    let fixture = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    for fault in [
        "context",
        "plan",
        "missing-original",
        "original-blob",
        "scope",
        "marker",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "context" => {
                tx.execute("UPDATE jobs SET created_at='2000-01-01T00:00:00Z',json=json_set(json,'$.createdAt','2000-01-01T00:00:00Z') WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
            }
            // Even valid JSON whitespace changes original accepted plan bytes.
            "plan" => {
                tx.execute(
                    "UPDATE meta SET value=value || ' ' WHERE key=?1",
                    [format!("job_entry_plan_snapshot:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            "missing-original" => {
                tx.execute(
                    "DELETE FROM meta WHERE key=?1",
                    [format!("job_entry_plan_snapshot:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            "original-blob" => {
                tx.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [format!("job_entry_plan_snapshot:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            "scope" => {
                tx.execute(
                    "UPDATE meta SET value=value || ' ' WHERE key=?1",
                    [format!("job_entry_scope:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            "marker" => {
                tx.execute(
                    "DELETE FROM meta WHERE key=?1",
                    [format!("job_entry_scope_accepted:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        assert!(
            journal::load(&tx, &auth, &fixture.job.job_id).is_err(),
            "{fault}"
        );
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn acceptance_journal_point_check_binds_exact_input_bytes_without_global_body_scan() {
    let _home = IsolatedAppHome::acquire("acceptance-point-input");
    let fixture = Fixture::new(2);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.execute(
        "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1 AND candidate_id=?2",
        params![fixture.job.job_id, fixture.prepared[1].0.candidate_id],
    )
    .unwrap();
    let changes = tx.total_changes();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    accepted
        .candidate(&tx, &fixture.prepared[0].0.candidate_id)
        .unwrap();
    assert!(accepted
        .candidate(&tx, &fixture.prepared[1].0.candidate_id)
        .is_err());
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn acceptance_journal_same_snapshot_rejects_writes_and_other_connection() {
    let _home = IsolatedAppHome::acquire("acceptance-snapshot");
    let fixture = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.commit().unwrap();
    let other = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let other_tx = other.unchecked_transaction().unwrap();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    let candidate = &fixture.prepared[0].0.candidate_id;
    accepted.candidate(&tx, candidate).unwrap();
    assert!(accepted.candidate(&other_tx, candidate).is_err());
    tx.execute(
        "INSERT INTO meta(key,value) VALUES('unrelated-snapshot-change','true')",
        [],
    )
    .unwrap();
    assert!(accepted.candidate(&tx, candidate).is_err());
}

#[test]
fn acceptance_journal_header_member_copies_between_jobs_rejected() {
    let _home = IsolatedAppHome::acquire("acceptance-copy-jobs");
    let first = Fixture::new(1);
    let second = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    first.seed(&conn);
    second.seed(&conn);
    let a = first.capture();
    let b = second.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    a.record(&tx, &first.job).unwrap();
    b.record(&tx, &second.job).unwrap();
    for (from, to) in [
        (first.header_key(), second.header_key()),
        (first.member_key(0), second.member_key(0)),
    ] {
        tx.execute_batch("SAVEPOINT replay").unwrap();
        tx.execute(
            "UPDATE meta SET value=(SELECT value FROM meta WHERE key=?1) WHERE key=?2",
            [from, to],
        )
        .unwrap();
        let changes = tx.total_changes();
        assert!(journal::load(&tx, &auth, &second.job.job_id).is_err());
        assert_eq!(tx.total_changes(), changes);
        tx.execute_batch("ROLLBACK TO replay; RELEASE replay")
            .unwrap();
    }
}

#[test]
fn acceptance_journal_metadata_types_and_budgets_do_not_repair_conflicts() {
    let _home = IsolatedAppHome::acquire("acceptance-storage-types");
    let fixture = Fixture::new(2);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).unwrap();
    for key in [
        fixture.header_key(),
        fixture.member_key(0),
        fixture.member_key(1),
    ] {
        for sql in [
            "UPDATE meta SET value='{}' WHERE key=?1",
            "UPDATE meta SET value=zeroblob(10) WHERE key=?1",
            "UPDATE meta SET value=CAST(x'ff' AS TEXT) WHERE key=?1",
            "UPDATE meta SET value=CAST(zeroblob(8193) AS TEXT) WHERE key=?1",
        ] {
            tx.execute_batch("SAVEPOINT fault").unwrap();
            tx.execute(sql, [&key]).unwrap();
            let before = fixture::sql_digest(&tx);
            assert!(journal::load(&tx, &auth, &fixture.job.job_id).is_err());
            assert!(producer.record(&tx, &fixture.job).is_err());
            // A late member conflict may have made exact no-op inserts; the
            // persisted conflicting row itself must never be repaired.
            assert_eq!(fixture::sql_digest(&tx), before);
            tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
                .unwrap();
        }
    }
}

#[test]
fn acceptance_member_range_keeps_neighbor_jobs_and_exact_byte_budget() {
    let _home = IsolatedAppHome::acquire("acceptance-member-range");
    let fixture = Fixture::new(2);
    let neighbor = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    neighbor.seed(&conn);
    let producer = fixture.capture();
    let neighbor_producer = neighbor.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    neighbor_producer.record(&tx, &neighbor.job).unwrap();
    let key = fixture.member_key(1);
    let mut encoded: String = tx
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    encoded.push_str(&" ".repeat(RECORD_BYTES - encoded.len()));
    tx.execute(
        "UPDATE meta SET value=?2 WHERE key=?1",
        params![key, encoded],
    )
    .unwrap();
    let before = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    // Noncanonical whitespace is permitted; the digest is still canonical.
    // Both jobs, all bodies and Unicode candidate IDs must remain readable.
    for job in [&fixture.job, &neighbor.job] {
        journal::validate_snapshot(&tx, &auth, &job.job_id).unwrap();
    }
    for (input, _) in &fixture.prepared {
        journal::validate_input_snapshot(&tx, &auth, &fixture.job.job_id, &input.candidate_id)
            .unwrap();
    }
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
    tx.execute("UPDATE meta SET value=value || ' ' WHERE key=?1", [&key])
        .unwrap();
    let before = fixture::sql_digest(&tx);
    let changes = tx.total_changes();
    assert!(journal::validate_snapshot(&tx, &auth, &fixture.job.job_id).is_err());
    journal::validate_snapshot(&tx, &auth, &neighbor.job.job_id).unwrap();
    assert_eq!(tx.total_changes(), changes);
    assert_eq!(fixture::sql_digest(&tx), before);
}

#[test]
fn acceptance_journal_progressed_state_keeps_original_acceptance_but_cannot_issue_again() {
    let _home = IsolatedAppHome::acquire("acceptance-progressed-state");
    let fixture = Fixture::new(1);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    tx.execute("UPDATE jobs SET state='running',state_version=2,json=json_set(json,'$.state','running','$.stateVersion',2) WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
    let changes = tx.total_changes();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    accepted
        .candidate(&tx, &fixture.prepared[0].0.candidate_id)
        .unwrap();
    assert!(producer.record(&tx, &fixture.job).is_err());
    assert_eq!(tx.total_changes(), changes);
}

#[test]
fn candidate_witness_authenticates_its_own_member_and_leaves_the_set_to_snapshots() {
    let _home = IsolatedAppHome::acquire("acceptance-candidate-witness");
    let fixture = Fixture::new(2);
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let job = &fixture.job.job_id;
    let first = &fixture.prepared[0].0.candidate_id;
    let second = &fixture.prepared[1].0.candidate_id;
    let full = journal::load(&tx, &auth, job).unwrap();
    assert!(full.allocated(&tx, first).is_ok());
    drop(full);

    // Another member is corrupted: this candidate's step still authenticates,
    // the corrupted candidate is refused and so is every complete-set check.
    tx.execute_batch("SAVEPOINT other").unwrap();
    tx.execute(
        "UPDATE meta SET value='{}' WHERE key=?1",
        [fixture.member_key(1)],
    )
    .unwrap();
    let witness = journal::load_candidate(&tx, &auth, job).unwrap();
    assert!(witness.allocated(&tx, first).is_ok());
    assert!(witness.allocated(&tx, second).is_err());
    assert!(journal::load(&tx, &auth, job).is_err());
    assert!(journal::validate_snapshot(&tx, &auth, job).is_err());
    tx.execute_batch("ROLLBACK TO other; RELEASE other")
        .unwrap();

    for fault in [
        "missing-header",
        "corrupt-header",
        "own-member-corrupt",
        "own-member-missing",
        "own-input-changed",
    ] {
        tx.execute_batch("SAVEPOINT fault").unwrap();
        match fault {
            "missing-header" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [fixture.header_key()])
                    .unwrap();
            }
            "corrupt-header" => {
                tx.execute(
                    "UPDATE meta SET value='{}' WHERE key=?1",
                    [fixture.header_key()],
                )
                .unwrap();
            }
            "own-member-corrupt" => {
                tx.execute(
                    "UPDATE meta SET value=(SELECT value FROM meta WHERE key=?2) WHERE key=?1",
                    params![fixture.member_key(0), fixture.member_key(1)],
                )
                .unwrap();
            }
            "own-member-missing" => {
                tx.execute("DELETE FROM meta WHERE key=?1", [fixture.member_key(0)])
                    .unwrap();
            }
            "own-input-changed" => {
                tx.execute(
                    "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1 AND candidate_id=?2",
                    params![job, first],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let changes = tx.total_changes();
        let refused = journal::load_candidate(&tx, &auth, job)
            .and_then(|witness| witness.allocated(&tx, first).map(|_| ()));
        assert!(refused.is_err(), "{fault}");
        assert_eq!(tx.total_changes(), changes, "reader cannot repair {fault}");
        tx.execute_batch("ROLLBACK TO fault; RELEASE fault")
            .unwrap();
    }
}

#[test]
fn candidate_witness_rejects_a_header_copied_from_another_job() {
    let _home = IsolatedAppHome::acquire("acceptance-candidate-copy");
    let first = Fixture::new(1);
    let second = Fixture::new(1);
    let conn = catalog::open_catalog().unwrap();
    first.seed(&conn);
    second.seed(&conn);
    let a = first.capture();
    let b = second.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    a.record(&tx, &first.job).unwrap();
    b.record(&tx, &second.job).unwrap();
    for (from, to) in [
        (first.header_key(), second.header_key()),
        (first.member_key(0), second.member_key(0)),
    ] {
        tx.execute_batch("SAVEPOINT replay").unwrap();
        tx.execute(
            "UPDATE meta SET value=(SELECT value FROM meta WHERE key=?1) WHERE key=?2",
            [from, to],
        )
        .unwrap();
        let refused = journal::load_candidate(&tx, &auth, &second.job.job_id).and_then(|witness| {
            witness
                .allocated(&tx, &second.prepared[0].0.candidate_id)
                .map(|_| ())
        });
        assert!(refused.is_err());
        tx.execute_batch("ROLLBACK TO replay; RELEASE replay")
            .unwrap();
    }
}
