use super::{
    fixture::{self, Fixture},
    *,
};
use crate::session_migration::{catalog, jobs::JobState};
use rusqlite::params;

#[test]
fn acceptance_producer_rejects_plan_input_membership_before_loading_key() {
    let _home = IsolatedAppHome::acquire("acceptance-preflight");
    let fixture = Fixture::new(2);
    let scope = hash::sha256_bytes(fixture.scope.as_bytes());
    let conn = catalog::open_catalog().unwrap();
    let capture = |job: &crate::session_migration::jobs::JobRecord, plan: &str, inputs: &[_]| {
        producer::PreparedAcceptance::capture(
            &conn,
            job,
            plan,
            &scope,
            inputs,
            &Fixture::identities(inputs),
        )
    };
    let key = catalog::imports_dir().join(".cursor-hmac-secret");
    assert!(!key.exists());
    assert!(capture(&fixture.job, &fixture.plan, &fixture.prepared[..1]).is_err());
    let mut other = fixture.job.clone();
    other.job_id = uuid::Uuid::new_v4().to_string();
    assert!(capture(&other, &fixture.plan, &fixture.prepared).is_err());
    other = fixture.job.clone();
    other.state = JobState::Running;
    assert!(capture(&other, &fixture.plan, &fixture.prepared).is_err());
    other = fixture.job.clone();
    other.state_version = 2;
    assert!(capture(&other, &fixture.plan, &fixture.prepared).is_err());
    let mut plan: Value = serde_json::from_str(&fixture.plan).unwrap();
    plan["selectionHash"] = json!("9".repeat(64));
    assert!(capture(&fixture.job, &plan.to_string(), &fixture.prepared).is_err());
    assert!(
        !key.exists(),
        "invalid preflight must not issue or load new key"
    );
}

#[test]
fn acceptance_allocation_rejects_missing_reordered_or_changed_preflight_identities() {
    let _home = IsolatedAppHome::acquire("allocation-original-identities");
    let fixture = Fixture::new(2);
    let conn = catalog::open_catalog().unwrap();
    for fault in ["missing", "reordered", "changed"] {
        let mut identities = Fixture::identities(&fixture.prepared);
        match fault {
            "missing" => {
                identities.pop();
            }
            "reordered" => identities.swap(0, 1),
            "changed" => {
                let crate::session_migration::job_control::association_acceptance::Identity::Legacy(
                    ref mut input,
                    _,
                ) = identities[0]
                else {
                    unreachable!()
                };
                input.digest = "0".repeat(64);
            }
            _ => unreachable!(),
        }
        let before = conn.total_changes();
        assert!(
            producer::PreparedAcceptance::capture(
                &conn,
                &fixture.job,
                &fixture.plan,
                &hash::sha256_bytes(fixture.scope.as_bytes()),
                &fixture.prepared,
                &identities,
            )
            .is_err(),
            "{fault}"
        );
        assert_eq!(conn.total_changes(), before);
        assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
    }
}

#[test]
fn acceptance_producer_large_original_input_keeps_unbounded_streaming_identity() {
    let _home = IsolatedAppHome::acquire("acceptance-large-input");
    let mut fixture = Fixture::new(1);
    // Display metadata belongs to the original input hash, not the fixed
    // acceptance record budget. Keep mapping/selection semantically unchanged.
    fixture.prepared[0].0.adapter.graph.title = Some("合成大输入".repeat(1_500_000));
    let original = serde_json::to_string(&fixture.prepared[0].0).unwrap();
    assert!(original.len() > 16 * 1024 * 1024);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let digest = journal::input_digest(
        &tx,
        &fixture.job.job_id,
        &fixture.prepared[0].0.candidate_id,
    )
    .unwrap();
    assert_eq!(digest, hash::sha256_bytes(original.as_bytes()));
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    accepted
        .candidate(&tx, &fixture.prepared[0].0.candidate_id)
        .unwrap();
    let header_bytes: u32 = tx
        .query_row(
            "SELECT length(CAST(value AS BLOB)) FROM meta WHERE key=?1",
            [fixture.header_key()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(header_bytes < RECORD_BYTES as u32);
}

#[test]
fn acceptance_producer_original_facts_and_initial_state_rechecked_before_write() {
    let _home = IsolatedAppHome::acquire("acceptance-current-facts");
    let fixture = Fixture::new(1);
    let producer = fixture.capture();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    for fault in ["input", "plan", "scope", "state", "version"] {
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "input" => {
                tx.execute(
                    "UPDATE job_items SET result_json=result_json || ' ' WHERE job_id=?1",
                    [&fixture.job.job_id],
                )
                .unwrap();
            }
            "plan" => {
                tx.execute(
                    "UPDATE plans SET json=json || ' ' WHERE plan_id=?1",
                    [&fixture.job.plan_id],
                )
                .unwrap();
            }
            "scope" => {
                tx.execute(
                    "UPDATE meta SET value='changed' WHERE key=?1",
                    [format!("job_entry_scope:{}", fixture.job.job_id)],
                )
                .unwrap();
            }
            "state" => {
                tx.execute("UPDATE jobs SET state='running',json=json_set(json,'$.state','running') WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
            }
            "version" => {
                tx.execute("UPDATE jobs SET state_version=2,json=json_set(json,'$.stateVersion',2) WHERE job_id=?1", [&fixture.job.job_id]).unwrap();
            }
            _ => unreachable!(),
        }
        let before = tx.total_changes();
        assert!(producer.record(&tx, &fixture.job).is_err(), "{fault}");
        assert_eq!(tx.total_changes(), before);
        assert_eq!(fixture.metadata_count(&tx), 0);
        tx.rollback().unwrap();
    }
    let _clock =
        crate::session_migration::clock::Fixed::at(chrono::Utc::now() + chrono::Duration::hours(2));
    let tx = conn.unchecked_transaction().unwrap();
    let before = tx.total_changes();
    assert!(producer.record(&tx, &fixture.job).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn acceptance_producer_late_input_scope_and_state_trigger_fault_rolls_back_all_sql() {
    let _home = IsolatedAppHome::acquire("acceptance-late-trigger");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let before = fixture::sql_digest(&conn);
    for action in [
        "UPDATE job_items SET result_json=result_json || ' ' WHERE candidate_id=(SELECT max(candidate_id) FROM job_items);",
        "UPDATE meta SET value=value || ' ' WHERE key LIKE 'job_entry_scope:%';",
        "UPDATE jobs SET state='running',json=json_set(json,'$.state','running');",
        "DELETE FROM meta WHERE key LIKE 'job_entry_scope_accepted:%';",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute_batch(&format!("CREATE TEMP TRIGGER acceptance_late_fault AFTER INSERT ON meta WHEN NEW.key='{}' BEGIN {action} END;", fixture.member_key(1))).unwrap();
        assert!(producer.record(&tx, &fixture.job).is_err());
        tx.rollback().unwrap();
        assert_eq!(fixture::sql_digest(&conn), before);
        assert_eq!(fixture.metadata_count(&conn), 0);
    }
}

#[test]
fn acceptance_producer_late_conflict_rolls_back_partial_header_and_member_inserts() {
    let _home = IsolatedAppHome::acquire("acceptance-late-conflict");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,'{}')",
        [fixture.member_key(1)],
    )
    .unwrap();
    let before = fixture::sql_digest(&conn);
    {
        let tx = conn.unchecked_transaction().unwrap();
        assert!(producer.record(&tx, &fixture.job).is_err());
        tx.rollback().unwrap();
    }
    assert_eq!(fixture::sql_digest(&conn), before);
    assert_eq!(fixture.metadata_count(&conn), 1);
    let value: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            params![fixture.member_key(1)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "{}");
}

#[test]
fn acceptance_producer_writer_uses_preloaded_crypto_when_key_loader_is_unavailable() {
    let home = IsolatedAppHome::acquire("acceptance-preloaded-crypto");
    let fixture = Fixture::new(2);
    let producer = fixture.capture();
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    fixture.seed(&conn);
    let blocked = home.path.join("synthetic-blocked-key-home");
    std::fs::write(&blocked, b"synthetic regular file; no credentials").unwrap();
    // IsolatedAppHome owns this process-global override and restores it even
    // on unwind. The loader cannot create imports under a regular file, so a
    // hidden load would fail before reaching its shared cache.
    std::env::set_var("GROK_APP_HOME", &blocked);
    assert!(InstallationAuthenticator::load().is_err());
    let tx = conn.unchecked_transaction().unwrap();
    producer.record(&tx, &fixture.job).unwrap();
    let accepted = journal::load(&tx, &auth, &fixture.job.job_id).unwrap();
    for (input, _) in &fixture.prepared {
        accepted.candidate(&tx, &input.candidate_id).unwrap();
        accepted.outcome_binding(&tx, &input.candidate_id).unwrap();
    }
    assert_eq!(fixture.metadata_count(&tx), 3);
    assert_eq!(
        std::fs::read(&blocked).unwrap(),
        b"synthetic regular file; no credentials"
    );
}

#[test]
fn acceptance_producer_mapping_snapshot_selection_and_duplicate_candidates_rejected() {
    let _home = IsolatedAppHome::acquire("acceptance-input-preflight");
    let conn = catalog::open_catalog().unwrap();
    for fault in ["mapping", "snapshot", "selection", "candidate"] {
        let mut fixture = Fixture::new(2);
        match fault {
            "mapping" => fixture.prepared[0].0.create_project = true,
            "snapshot" => fixture.prepared[0].0.preview_digest = "9".repeat(64),
            "selection" => fixture.prepared[0].0.selection_hash = "9".repeat(64),
            "candidate" => {
                fixture.prepared[1].0.candidate_id = fixture.prepared[0].0.candidate_id.clone()
            }
            _ => unreachable!(),
        }
        assert!(
            producer::PreparedAcceptance::capture(
                &conn,
                &fixture.job,
                &fixture.plan,
                &hash::sha256_bytes(fixture.scope.as_bytes()),
                &fixture.prepared,
                &Fixture::identities(&fixture.prepared),
            )
            .is_err(),
            "{fault}"
        );
    }
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}
