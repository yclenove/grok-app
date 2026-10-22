//! Real durable rows, receipts and reader proofs must bind the selected member.
use super::*;
use crate::session_migration::{job_control as control, jobs};
use rusqlite::{params, Connection};

fn enqueue_preview(preview_id: String) -> JobRecord {
    let planned = plan(vec![preview_id], vec![]).unwrap();
    commit(
        &planned.plan_id,
        &planned.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap()
}

fn enqueue(f: &Fixture) -> (JobRecord, Value) {
    let preview_id = f.preview();
    let path = jobs::find_preview_dir(&preview_id).unwrap().unwrap();
    let pin = raw(&path.join("manifest.json"))["sourceMember"].clone();
    (enqueue_preview(preview_id), pin)
}

fn input_json(conn: &Connection, job: &JobRecord) -> Value {
    serde_json::from_str(&input_bytes(conn, job)).unwrap()
}

fn input_bytes(conn: &Connection, job: &JobRecord) -> String {
    conn.query_row(
        "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
        params![job.job_id, job.items[0].candidate_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn replace_input(conn: &Connection, job: &JobRecord, value: &Value) {
    replace_input_bytes(conn, job, &value.to_string());
}

fn replace_input_bytes(conn: &Connection, job: &JobRecord, value: &str) {
    assert_eq!(
        conn.execute(
            "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
            params![job.job_id, job.items[0].candidate_id, value],
        )
        .unwrap(),
        1
    );
}

fn receipt_json(conn: &Connection, job: &JobRecord) -> Value {
    serde_json::to_value(
        jobs::latest_receipt(conn, &job.job_id, &job.items[0].candidate_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

fn replace_receipt(conn: &Connection, value: &Value) {
    assert_eq!(
        conn.execute(
            "UPDATE receipts SET json=?2 WHERE receipt_id=?1",
            params![value["receiptId"].as_str().unwrap(), value.to_string()],
        )
        .unwrap(),
        1
    );
}

fn publish(f: &Fixture) -> (JobRecord, Value) {
    let (job, pin) = enqueue(f);
    control::run_pending().unwrap();
    let job = job_get(&job.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    (job, pin)
}

fn publish_legacy_replay_fixture(f: &Fixture) -> (JobRecord, Value) {
    let preview_id = f.preview();
    let path = jobs::find_preview_dir(&preview_id).unwrap().unwrap();
    let pin = raw(&path.join("manifest.json"))["sourceMember"].clone();
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = enqueue_legacy_plan(&plan, &Uuid::new_v4().to_string());
    control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    (job, pin)
}

fn attempt_worker(
    conn: &Connection,
    job: &JobRecord,
    item: &jobs::CommitItem,
) -> Result<(), ImportError> {
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(conn, &job.job_id)?;
    jobs::publish_for_worker(conn, &job.job_id, 0, item, &|| false, &crypto).map(|_| ())
}

fn read_job(job: &JobRecord) -> Result<(), ImportError> {
    let item = &job.items[0];
    let (events, _, count) = events_page(
        item.import_id.as_deref().unwrap(),
        item.revision_id.as_deref(),
        None,
        Some(20),
        None,
    )?;
    assert_eq!(count, 1);
    assert!(serde_json::to_string(&events)
        .unwrap()
        .contains("FIXED_MEMBER_A"));
    Ok(())
}

#[test]
fn enqueued_durable_input_captures_the_complete_fixed_member() {
    let f = Fixture::new();
    let (job, pin) = enqueue(&f);
    assert_eq!(
        input_json(&open_catalog().unwrap(), &job)["sourceMember"],
        pin
    );
}

#[test]
fn reset_durable_input_cannot_drop_the_member_from_a_pinned_preview() {
    let f = Fixture::new();
    let (job, _) = enqueue(&f);
    let conn = open_catalog().unwrap();
    let mut input = input_json(&conn, &job);
    input.as_object_mut().unwrap().remove("sourceMember");
    replace_input(&conn, &job, &input);
    reset_runtime();
    assert!(control::load_input(&conn, &job.job_id, &job.items[0].candidate_id).is_err());
    assert!(list_entries(&conn).unwrap().is_empty());
}

#[test]
fn committed_receipt_captures_the_complete_fixed_member() {
    let f = Fixture::new();
    let (job, pin) = publish(&f);
    assert_eq!(
        receipt_json(&open_catalog().unwrap(), &job)["sourceMember"],
        pin
    );
}

fn reader_rejects_removed_member(clear_ram: bool) {
    let f = Fixture::new();
    let (job, _) = publish(&f);
    read_job(&job).unwrap();
    let conn = open_catalog().unwrap();
    let mut receipt = receipt_json(&conn, &job);
    receipt.as_object_mut().unwrap().remove("sourceMember");
    replace_receipt(&conn, &receipt);
    if clear_ram {
        jobs::clear_published_read_verification_cache();
    }
    assert!(read_job(&job).is_err(), "clear_ram={clear_ram}");
}

#[test]
fn warm_reader_cannot_ignore_a_removed_receipt_member() {
    reader_rejects_removed_member(false);
}

#[test]
fn persisted_reader_proof_cannot_ignore_a_removed_receipt_member() {
    reader_rejects_removed_member(true);
}

/// Historical v1 verifier signed this exact field set, without sourceMember.
/// A valid old proof must not authorize the new, stronger verification contract.
fn install_v1_proof(receipt: &Value) -> PathBuf {
    let proof_dir = catalog::imports_dir().join(".verification-cache");
    let proof_path = std::fs::read_dir(&proof_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let mut proof = raw(&proof_path);
    let mut old_receipt = json!({});
    for field in [
        "candidateId",
        "createdEntry",
        "fileHashes",
        "importId",
        "installationId",
        "jobId",
        "legacyId",
        "nativeSessionId",
        "optionsHash",
        "previousRevision",
        "provider",
        "revisionId",
        "rootPath",
        "selectionHash",
        "selectionSet",
        "snapshotHash",
        "sourceId",
        "version",
    ] {
        old_receipt[field] = receipt[field].clone();
    }
    let digest = crate::session_migration::hash::options_hash(&old_receipt);
    proof["payload"]["version"] = json!(1);
    proof["payload"]["receiptDigest"] = json!(digest);
    let payload = crate::session_migration::hash::canonical_json(&proof["payload"]);
    proof["mac"] = json!(hex::encode(
        crate::session_migration::cursor::sign_installation_payload(
            "session-import-published-verification-v1",
            payload.as_bytes(),
        )
        .unwrap()
    ));
    let identity = json!({
        "receiptDigest":digest, "revisionDir":proof["payload"]["revisionDir"]
    });
    let name = crate::session_migration::hash::options_hash(&identity);
    let path = proof_dir.join(format!("{name}.json"));
    std::fs::write(&path, proof.to_string()).unwrap();
    path
}

#[test]
fn historical_v1_proof_cannot_authorize_a_pinned_manifest_without_receipt_pin() {
    let f = Fixture::new();
    let (job, _) = publish(&f);
    read_job(&job).unwrap();
    let conn = open_catalog().unwrap();
    let mut receipt = receipt_json(&conn, &job);
    receipt.as_object_mut().unwrap().remove("sourceMember");
    install_v1_proof(&receipt);
    replace_receipt(&conn, &receipt);
    jobs::clear_published_read_verification_cache();
    assert!(read_job(&job).is_err());
}

fn alter_pin(record: &mut Value, mutation: &str, other: &Value) {
    match mutation {
        "missing" => {
            record.as_object_mut().unwrap().remove("sourceMember");
        }
        "null" => record["sourceMember"] = Value::Null,
        "version" => record["sourceMember"]["version"] = json!(99),
        "proof-version" => record["sourceMember"]["proofVersion"] = json!(99),
        "proof" => record["sourceMember"]["proof"] = json!("artifact"),
        "location" => {
            record["sourceMember"]["reference"]["location"]["locationId"] =
                json!(Uuid::new_v4().to_string())
        }
        "listing" => {
            record["sourceMember"]["reference"]["sourceRevision"] = json!("other listing revision")
        }
        "seal" => record["sourceMember"]["seal"] = json!("00".repeat(32)),
        "unknown" => record["sourceMember"]["reference"]["unexpected"] = json!(true),
        "other" => record["sourceMember"] = other.clone(),
        _ => unreachable!(),
    }
}

const MUTATIONS: [&str; 10] = [
    "missing",
    "null",
    "version",
    "proof-version",
    "proof",
    "location",
    "listing",
    "seal",
    "unknown",
    "other",
];

fn other_pin(f: &Fixture) -> Value {
    let scan = f.rescan();
    serde_json::to_value(scan::candidate(&scan.candidates[0].candidate_id).unwrap()).unwrap()
        ["sourceMember"]
        .clone()
}

#[test]
fn durable_reload_rejects_all_member_mutations_and_a_legitimate_other_scan() {
    let f = Fixture::new();
    let (job, _) = enqueue(&f);
    let other = other_pin(&f);
    let conn = open_catalog().unwrap();
    let original_bytes = input_bytes(&conn, &job);
    let original: Value = serde_json::from_str(&original_bytes).unwrap();
    for mutation in MUTATIONS {
        let mut altered = original.clone();
        alter_pin(&mut altered, mutation, &other);
        replace_input(&conn, &job, &altered);
        reset_runtime();
        assert!(
            control::load_input(&conn, &job.job_id, &job.items[0].candidate_id).is_err(),
            "{mutation}"
        );
    }
    // Restoring a signed input means its exact original bytes, not a sorted
    // Value serialization that changes the acceptance digest.
    replace_input_bytes(&conn, &job, &original_bytes);
    assert!(control::load_input(&conn, &job.job_id, &job.items[0].candidate_id).is_ok());
    assert!(list_entries(&conn).unwrap().is_empty());
}

#[test]
fn reader_rejects_receipt_member_mutations_with_ram_and_persisted_proofs() {
    let f = Fixture::new();
    let (job, _) = publish(&f);
    let other = other_pin(&f);
    let conn = open_catalog().unwrap();
    let original = receipt_json(&conn, &job);
    for clear_ram in [false, true] {
        for mutation in MUTATIONS {
            replace_receipt(&conn, &original);
            read_job(&job).unwrap();
            let mut altered = original.clone();
            alter_pin(&mut altered, mutation, &other);
            replace_receipt(&conn, &altered);
            if clear_ram {
                jobs::clear_published_read_verification_cache();
            }
            assert!(read_job(&job).is_err(), "clear_ram={clear_ram}, {mutation}");
        }
    }
    replace_receipt(&conn, &original);
    read_job(&job).unwrap();
}

fn enqueue_legacy(f: &Fixture) -> JobRecord {
    let mut reference =
        serde_json::to_value(scan::candidate(&f.scan.candidates[0].candidate_id).unwrap()).unwrap();
    reference.as_object_mut().unwrap().remove("sourceMember");
    let preview_id = preview_reference(serde_json::from_value(reference).unwrap(), None, None)
        .unwrap()
        .0;
    let plan = plan(vec![preview_id], vec![]).unwrap();
    enqueue_legacy_plan(&plan, &Uuid::new_v4().to_string())
}

#[test]
fn old_no_member_records_remain_readable_without_rewriting_history() {
    let f = Fixture::new();
    let queued = enqueue_legacy(&f);
    let conn = open_catalog().unwrap();
    let input_before: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&input_before)
        .unwrap()
        .get("sourceMember")
        .is_none());
    reset_runtime();
    std::fs::rename(&f.root, f.root.with_file_name("legacy source offline")).unwrap();
    assert!(control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).is_ok());
    control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed);
    let receipt = receipt_json(&conn, &job);
    assert!(receipt.get("sourceMember").is_none());
    let receipt_before: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1",
            [receipt["receiptId"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    let archive = catalog::revision_dir(
        job.items[0].import_id.as_deref().unwrap(),
        job.items[0].revision_id.as_deref().unwrap(),
    )
    .unwrap();
    let hashes = jobs::published_hashes(&archive).unwrap();
    read_job(&job).unwrap();
    let old_proof = install_v1_proof(&receipt);
    jobs::clear_published_read_verification_cache();
    reset_runtime();
    read_job(&job).unwrap();
    assert_eq!(
        raw(&old_proof)["payload"]["version"],
        2,
        "old proof must fully revalidate before being replaced"
    );
    let input_after: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |r| r.get(0),
        )
        .unwrap();
    let receipt_after: String = conn
        .query_row(
            "SELECT json FROM receipts WHERE receipt_id=?1",
            [receipt["receiptId"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(input_before, input_after);
    assert_eq!(receipt_before, receipt_after);
    assert_eq!(jobs::published_hashes(&archive).unwrap(), hashes);
}

/// Exercise the existing completed-record replay contract after normal preview
/// cleanup, not a claim that a new process crashed in that artificial ordering.
fn stale_completed_header(conn: &Connection, job: &JobRecord) {
    let mut stale = job.clone();
    stale.state = JobState::Running;
    stale.items[0].phase = "running".into();
    stale.items[0].import_id = None;
    stale.items[0].revision_id = None;
    stale.items[0].outcome = None;
    jobs::save_job(conn, &stale).unwrap();
}

#[test]
fn committed_member_recovery_survives_source_and_preview_removal() {
    let f = Fixture::new();
    // The historical replay contract predates atomic signed terminal+ACK.
    let (job, pin) = publish_legacy_replay_fixture(&f);
    let conn = open_catalog().unwrap();
    let input = input_json(&conn, &job);
    assert!(!Path::new(input["previewPath"].as_str().unwrap()).exists());
    let receipt = receipt_json(&conn, &job);
    let before: u32 = conn
        .query_row("SELECT count(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    stale_completed_header(&conn, &job);
    reset_runtime();
    std::fs::rename(&f.root, f.root.with_file_name("committed source offline")).unwrap();
    control::run_pending().unwrap();
    let recovered = job_get(&job.job_id).unwrap();
    assert_eq!(recovered.state, JobState::Completed, "{recovered:?}");
    assert_eq!(recovered.items[0].revision_id, job.items[0].revision_id);
    assert_eq!(receipt_json(&conn, &recovered), receipt);
    assert_eq!(input_json(&conn, &recovered)["sourceMember"], pin);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        before
    );
    read_job(&recovered).unwrap();
}

#[test]
fn committed_recovery_cannot_drop_or_replace_input_or_receipt_member() {
    let f = Fixture::new();
    let (job, _) = publish_legacy_replay_fixture(&f);
    let other = other_pin(&f);
    let conn = open_catalog().unwrap();
    let input = input_json(&conn, &job);
    let receipt = receipt_json(&conn, &job);
    let archive = catalog::revision_dir(
        job.items[0].import_id.as_deref().unwrap(),
        job.items[0].revision_id.as_deref().unwrap(),
    )
    .unwrap();
    let hashes = jobs::published_hashes(&archive).unwrap();
    std::fs::rename(
        &f.root,
        f.root.with_file_name("tamper recovery source offline"),
    )
    .unwrap();
    for target in ["input", "receipt", "both"] {
        for mutation in MUTATIONS {
            let mut altered_input = input.clone();
            let mut altered_receipt = receipt.clone();
            if target != "receipt" {
                alter_pin(&mut altered_input, mutation, &other);
            }
            if target != "input" {
                alter_pin(&mut altered_receipt, mutation, &other);
            }
            replace_input(&conn, &job, &altered_input);
            replace_receipt(&conn, &altered_receipt);
            stale_completed_header(&conn, &job);
            reset_runtime();
            control::run_pending().unwrap();
            let failed = job_get(&job.job_id).unwrap();
            assert_eq!(
                failed.state,
                JobState::CompletedWithIssues,
                "target={target}, {mutation}: {failed:?}"
            );
            assert_eq!(failed.items[0].phase, "failed");
            assert_eq!(list_entries(&conn).unwrap().len(), 1);
            assert_eq!(jobs::published_hashes(&archive).unwrap(), hashes);
        }
    }
    replace_input(&conn, &job, &input);
    replace_receipt(&conn, &receipt);
    stale_completed_header(&conn, &job);
    control::run_pending().unwrap();
    assert_eq!(job_get(&job.job_id).unwrap().state, JobState::Completed);
}

#[test]
fn lost_dedup_result_replays_offline_without_claiming_old_revision_ownership() {
    let f = Fixture::new();
    let (first, _) = publish_legacy_replay_fixture(&f);
    let conn = open_catalog().unwrap();
    let original_receipt = receipt_json(&conn, &first);
    let scan = f.rescan();
    let preview_id = preview(&scan.candidates[0].candidate_id, None).unwrap().0;
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = enqueue_legacy_plan(&plan, &Uuid::new_v4().to_string());
    let original_input = input_json(&conn, &queued);
    assert_ne!(
        original_input["sourceMember"],
        original_receipt["sourceMember"]
    );
    // Persistent only inside this isolated synthetic catalog: run_pending opens
    // its own connection, so a TEMP trigger would not exercise the real worker.
    conn.execute_batch(&format!(
        "CREATE TRIGGER lose_dedup_result BEFORE UPDATE OF json ON job_item_states
         WHEN NEW.job_id='{}' AND json_extract(NEW.json,'$.outcome')='deduplicated'
         BEGIN SELECT RAISE(ABORT,'synthetic result-save failure'); END;",
        queued.job_id
    ))
    .unwrap();
    assert!(control::run_pending().is_err());
    let interrupted = job_get(&queued.job_id).unwrap();
    assert_eq!(interrupted.state, JobState::Running);
    assert!(Path::new(original_input["previewPath"].as_str().unwrap()).is_dir());
    conn.execute_batch("DROP TRIGGER lose_dedup_result")
        .unwrap();
    reset_runtime();
    std::fs::rename(
        &f.root,
        f.root.with_file_name("dedup replay source offline"),
    )
    .unwrap();
    control::run_pending().unwrap();
    let done = job_get(&queued.job_id).unwrap();
    assert_eq!(done.state, JobState::Completed, "{done:?}");
    assert_eq!(done.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(done.items[0].import_id, first.items[0].import_id);
    assert_eq!(done.items[0].revision_id, first.items[0].revision_id);
    assert_eq!(input_json(&conn, &done), original_input);
    assert_eq!(receipt_json(&conn, &first), original_receipt);
    assert!(
        jobs::latest_receipt(&conn, &done.job_id, &done.items[0].candidate_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM revisions", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    read_job(&done).unwrap();
}

#[test]
fn every_incomplete_receipt_boundary_retains_member_and_preallocated_ids_on_retry() {
    for phase in ["staged", "published", "linked", "committed"] {
        let f = Fixture::new();
        let (queued, pin) = enqueue(&f);
        let conn = open_catalog().unwrap();
        let item =
            control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
        // Real publish uses this connection, so the TEMP trigger intercepts the
        // requested durable receipt boundary without changing product hooks.
        conn.execute_batch(&format!(
            "CREATE TEMP TRIGGER fail_receipt_boundary BEFORE INSERT ON receipts
             WHEN NEW.phase='{phase}' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;"
        ))
        .unwrap();
        assert!(attempt_worker(&conn, &queued, &item).is_err(), "{phase}");
        conn.execute_batch("DROP TRIGGER fail_receipt_boundary")
            .unwrap();
        let before = receipt_json(&conn, &queued);
        assert_ne!(before["phase"], "committed");
        assert_eq!(before["sourceMember"], pin);
        assert!(list_entries(&conn).unwrap().is_empty());
        let source_bytes = std::fs::read(f.root.join("selected.jsonl")).unwrap();
        let offline = f.root.with_file_name(format!("source offline at {phase}"));
        std::fs::rename(&f.root, &offline).unwrap();
        reset_runtime();
        control::run_pending().unwrap();
        let done = job_get(&queued.job_id).unwrap();
        assert_eq!(done.state, JobState::Completed, "{phase}: {done:?}");
        assert_eq!(
            done.items[0].import_id.as_deref(),
            before["importId"].as_str()
        );
        assert_eq!(
            done.items[0].revision_id.as_deref(),
            before["revisionId"].as_str()
        );
        assert_eq!(receipt_json(&conn, &done)["sourceMember"], pin);
        assert_eq!(
            std::fs::read(offline.join("selected.jsonl")).unwrap(),
            source_bytes
        );
        read_job(&done).unwrap();
    }
}

#[test]
fn incomplete_receipt_member_mutations_fail_before_retry_can_publish() {
    for phase in ["staged", "published", "linked", "committed"] {
        let f = Fixture::new();
        let (queued, _) = enqueue(&f);
        let other = other_pin(&f);
        let conn = open_catalog().unwrap();
        let item =
            control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
        conn.execute_batch(&format!(
            "CREATE TEMP TRIGGER fail_receipt_boundary BEFORE INSERT ON receipts
             WHEN NEW.phase='{phase}' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;"
        ))
        .unwrap();
        assert!(attempt_worker(&conn, &queued, &item).is_err());
        // Remove the failure injection before mutation checks; otherwise its
        // error could accidentally make an ignored member look rejected.
        conn.execute_batch("DROP TRIGGER fail_receipt_boundary")
            .unwrap();
        let before = receipt_json(&conn, &queued);
        for mutation in MUTATIONS {
            let mut altered = before.clone();
            alter_pin(&mut altered, mutation, &other);
            replace_receipt(&conn, &altered);
            assert!(
                attempt_worker(&conn, &queued, &item).is_err(),
                "phase={phase}, {mutation}"
            );
            assert!(
                list_entries(&conn).unwrap().is_empty(),
                "phase={phase}, {mutation}"
            );
            let stored: String = conn
                .query_row(
                    "SELECT json FROM receipts WHERE receipt_id=?1",
                    [before["receiptId"].as_str().unwrap()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), altered);
        }
        replace_receipt(&conn, &before);
        reset_runtime();
        control::run_pending().unwrap();
        let done = job_get(&queued.job_id).unwrap();
        assert_eq!(done.state, JobState::Completed, "{phase}: {done:?}");
        assert_eq!(
            done.items[0].revision_id.as_deref(),
            before["revisionId"].as_str()
        );
    }
}
