use super::*;
use crate::session_migration::catalog;

fn key(kind: StageKind) -> String {
    format!(
        "publication_outcome:v1:{JOB}:{}:{}",
        "e".repeat(64),
        kind.key()
    )
}

#[test]
fn outcome_journal_committed_rows_survive_catalog_reopen_and_are_read_only() {
    let _home = IsolatedAppHome::acquire("outcome-journal-reopen");
    let auth = InstallationAuthenticator::load().unwrap();
    let allocated = seal(&auth, binding(), allocation(create(false))).unwrap();
    let a = verified(&auth, &allocated);
    let stored = seal(&auth, binding(), stored(&a, created(PROJECT))).unwrap();
    let s = verified(&auth, &stored);
    let committed = seal(&auth, binding(), committed(&s)).unwrap();
    {
        let conn = catalog::open_catalog().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        for record in [&allocated, &stored, &committed] {
            journal::insert(&tx, &auth, record).unwrap();
            let changes = tx.total_changes();
            journal::insert(&tx, &auth, record).unwrap();
            assert_eq!(tx.total_changes(), changes);
        }
        tx.commit().unwrap();
    }
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let before = tx.total_changes();
    let a = journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).unwrap();
    let s = journal::load_required(&tx, &auth, &binding(), StageKind::Stored).unwrap();
    let c = journal::load_required(&tx, &auth, &binding(), StageKind::Committed).unwrap();
    let complete = journal::load_committed(&tx, &auth, &binding()).unwrap();
    assert!(complete.digest == c.digest);
    s.follows(&a).unwrap();
    c.follows(&s).unwrap();
    assert_eq!(tx.total_changes(), before);
    tx.rollback().unwrap();
}

#[test]
fn outcome_journal_store_and_commit_require_durable_exact_predecessors() {
    let _home = IsolatedAppHome::acquire("outcome-journal-predecessors");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let allocated = seal(&auth, binding(), allocation(create(false))).unwrap();
    let a = verified(&auth, &allocated);
    let stored = seal(&auth, binding(), stored(&a, created(PROJECT))).unwrap();
    let s = verified(&auth, &stored);
    let committed = seal(&auth, binding(), committed(&s)).unwrap();
    let before = tx.total_changes();
    assert!(journal::insert(&tx, &auth, &stored).is_err());
    assert!(journal::insert(&tx, &auth, &committed).is_err());
    assert_eq!(tx.total_changes(), before);
    journal::insert(&tx, &auth, &allocated).unwrap();
    let foreign = seal(&auth, binding(), super::stored(&a, created(OTHER))).unwrap();
    let before = tx.total_changes();
    assert!(journal::insert(&tx, &auth, &foreign).is_err());
    assert!(journal::insert(&tx, &auth, &committed).is_err());
    assert_eq!(tx.total_changes(), before);
    journal::insert(&tx, &auth, &stored).unwrap();
    journal::insert(&tx, &auth, &committed).unwrap();
    journal::load_committed(&tx, &auth, &binding()).unwrap();
    // Validly sealed but different allocation cannot be substituted underneath
    // a committed chain, including coordinated previous/createdEntry changes.
    let mut replaced = allocation(create(false));
    if let Stage::Allocated(row) = &mut replaced {
        row.created_entry = false;
        row.previous_revision = Some(OTHER.into());
    }
    let replacement = seal(&auth, binding(), replaced).unwrap();
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key=?2",
        [&replacement.encoded, &key(StageKind::Allocated)],
    )
    .unwrap();
    let before = tx.total_changes();
    assert!(journal::load_committed(&tx, &auth, &binding()).is_err());
    assert!(journal::insert(&tx, &auth, &committed).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn outcome_journal_missing_or_damaged_predecessor_rejects_complete_chain() {
    let _home = IsolatedAppHome::acquire("outcome-journal-damaged-predecessors");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let allocated = seal(&auth, binding(), allocation(create(false))).unwrap();
    let a = verified(&auth, &allocated);
    let stored = seal(&auth, binding(), stored(&a, created(PROJECT))).unwrap();
    let s = verified(&auth, &stored);
    let committed = seal(&auth, binding(), committed(&s)).unwrap();
    for kind in [
        StageKind::Allocated,
        StageKind::Stored,
        StageKind::Committed,
    ] {
        // Each fault is rolled back after observation; no manual repair is
        // used to claim that the failed read itself was atomic.
        tx.execute_batch("SAVEPOINT synthetic_fault").unwrap();
        for record in [&allocated, &stored, &committed] {
            journal::insert(&tx, &auth, record).unwrap();
        }
        tx.execute("DELETE FROM meta WHERE key=?1", [key(kind)])
            .unwrap();
        let before = tx.total_changes();
        assert!(journal::load_committed(&tx, &auth, &binding()).is_err());
        assert_eq!(tx.total_changes(), before);
        tx.execute_batch("ROLLBACK TO synthetic_fault; RELEASE synthetic_fault")
            .unwrap();
        tx.execute_batch("SAVEPOINT synthetic_fault").unwrap();
        for record in [&allocated, &stored, &committed] {
            journal::insert(&tx, &auth, record).unwrap();
        }
        tx.execute("UPDATE meta SET value='{}' WHERE key=?1", [key(kind)])
            .unwrap();
        let before = tx.total_changes();
        assert!(journal::load_committed(&tx, &auth, &binding()).is_err());
        assert_eq!(tx.total_changes(), before);
        tx.execute_batch("ROLLBACK TO synthetic_fault; RELEASE synthetic_fault")
            .unwrap();
    }
}

#[test]
fn outcome_journal_insert_rollback_does_not_publish_partial_records() {
    let _home = IsolatedAppHome::acquire("outcome-journal-rollback");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    {
        let tx = conn.unchecked_transaction().unwrap();
        journal::insert(&tx, &auth, &record).unwrap();
        journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).unwrap();
        tx.rollback().unwrap();
    }
    let tx = conn.unchecked_transaction().unwrap();
    let before = tx.total_changes();
    assert!(journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn outcome_journal_existing_phase_cannot_be_replaced_or_repaired() {
    let _home = IsolatedAppHome::acquire("outcome-journal-immutable");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let original = seal(&auth, binding(), allocation(create(false))).unwrap();
    journal::insert(&tx, &auth, &original).unwrap();
    let changed = seal(&auth, binding(), allocation(create(true))).unwrap();
    let before = tx.total_changes();
    assert!(journal::insert(&tx, &auth, &changed).is_err());
    assert_eq!(tx.total_changes(), before);
    let stored: String = tx
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [key(StageKind::Allocated)],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored == original.encoded);
    tx.execute(
        "UPDATE meta SET value='{}' WHERE key=?1",
        [key(StageKind::Allocated)],
    )
    .unwrap();
    let before = tx.total_changes();
    assert!(journal::insert(&tx, &auth, &original).is_err());
    assert!(journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn outcome_journal_copied_job_candidate_or_phase_rows_are_rejected() {
    let _home = IsolatedAppHome::acquire("outcome-journal-copies");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let original = seal(&auth, binding(), allocation(create(false))).unwrap();
    journal::insert(&tx, &auth, &original).unwrap();
    for changed in [
        Binding {
            job_id: OTHER.into(),
            ..binding()
        },
        Binding {
            candidate_digest: "9".repeat(64),
            ..binding()
        },
    ] {
        let address = format!(
            "publication_outcome:v1:{}:{}:allocated",
            changed.job_id, changed.candidate_digest
        );
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)",
            [&address, &original.encoded],
        )
        .unwrap();
        assert!(journal::load_required(&tx, &auth, &changed, StageKind::Allocated).is_err());
    }
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        [key(StageKind::Committed), original.encoded.clone()],
    )
    .unwrap();
    assert!(journal::load_required(&tx, &auth, &binding(), StageKind::Committed).is_err());
    tx.execute("DELETE FROM meta WHERE key=?1", [key(StageKind::Allocated)])
        .unwrap();
    let before = tx.total_changes();
    assert!(journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn outcome_journal_type_size_future_and_invalid_utf8_are_not_read_or_resigned() {
    let _home = IsolatedAppHome::acquire("outcome-journal-storage-types");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let original = seal(&auth, binding(), allocation(create(false))).unwrap();
    journal::insert(&tx, &auth, &original).unwrap();
    for sql in [
        "UPDATE meta SET value=42 WHERE key=?1", // TEXT affinity still fails codec.
        "UPDATE meta SET value=zeroblob(10) WHERE key=?1",
        "UPDATE meta SET value=CAST(x'ff' AS TEXT) WHERE key=?1",
        "UPDATE meta SET value=CAST(zeroblob(8193) AS TEXT) WHERE key=?1",
    ] {
        tx.execute(sql, [key(StageKind::Allocated)]).unwrap();
        let before = tx.total_changes();
        assert!(journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).is_err());
        assert!(journal::insert(&tx, &auth, &original).is_err());
        assert_eq!(tx.total_changes(), before);
    }
    let mut future: Value = serde_json::from_str(&original.encoded).unwrap();
    future["body"]["version"] = json!(2);
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key=?2",
        [future.to_string(), key(StageKind::Allocated)],
    )
    .unwrap();
    let before = tx.total_changes();
    let err = journal::load_required(&tx, &auth, &binding(), StageKind::Allocated)
        .err()
        .unwrap();
    assert_eq!(err.code, ErrorCode::ImportSchemaTooNew);
    assert!(journal::insert(&tx, &auth, &original).is_err());
    assert_eq!(tx.total_changes(), before);
}

#[test]
fn outcome_journal_point_lookup_does_not_scan_other_members_or_inputs() {
    let _home = IsolatedAppHome::acquire("outcome-journal-point-lookup");
    let auth = InstallationAuthenticator::load().unwrap();
    let conn = catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    // A malformed unrelated candidate must not be parsed by this lookup.
    tx.execute("INSERT INTO meta(key,value) VALUES('publication_outcome:v1:unrelated:member:allocated','{}')", []).unwrap();
    let record = seal(&auth, binding(), allocation(create(false))).unwrap();
    journal::insert(&tx, &auth, &record).unwrap();
    let read = journal::load_required(&tx, &auth, &binding(), StageKind::Allocated).unwrap();
    assert!(read.digest == verified(&auth, &record).digest);
    let plan: String = tx
        .query_row(
            "EXPLAIN QUERY PLAN SELECT value FROM meta WHERE key=?1",
            [key(StageKind::Allocated)],
            |row| row.get(3),
        )
        .unwrap();
    assert!(plan.contains("SEARCH meta USING INDEX"));
}
