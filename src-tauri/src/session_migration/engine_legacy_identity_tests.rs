//! Registration-layout compatibility through the actual durable import pipeline.
//! Seeds are valid current-schema archives produced by enqueue/run_pending, not
//! edited receipts. They reproduce the historical local:SHA(locator) registration
//! shape only; they do not claim to replay a 1.0.0 Cursor normalizer or vendor data.

use super::*;
use crate::session_migration::jobs::{self, CommitItem};
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::json;
use std::collections::BTreeMap;

#[path = "engine_legacy_project_boundary_tests.rs"]
mod project_boundary_tests;
#[path = "engine_legacy_provenance_boundary_tests.rs"]
mod provenance_boundary_tests;

#[path = "engine_historical_seed_tests.rs"]
mod historical_seed_tests;
pub(in crate::session_migration) use historical_seed_tests::association_crash_child;

#[path = "engine_association_plan_tests.rs"]
mod association_plan_tests;

#[path = "engine_association_input_tests.rs"]
mod association_input_tests;

#[path = "engine_legacy_identity_authority_tests.rs"]
mod authority_tests;

struct Seed {
    import_id: String,
    revision: String,
    installation: String,
    job_id: String,
    hashes: BTreeMap<String, String>,
}

fn cursor_source(home: &IsolatedAppHome, count: usize) -> Vec<String> {
    reset_runtime();
    let root = home.path.join("synthetic cursor");
    std::fs::create_dir_all(&root).unwrap();
    for index in 0..count {
        let record = json!({
            "composerId": format!("legacy-pipeline-{index}"),
            "role": "user", "content": format!("SYNTHETIC_REGISTERED_HISTORY_{index}")
        });
        std::fs::write(root.join(format!("{index}.jsonl")), format!("{record}\n")).unwrap();
    }
    let scan = scan_start(vec![(
        Provider::Cursor,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert!(scan.complete);
    assert_eq!(scan.candidates.len(), count);
    scan.candidates
        .iter()
        .map(|candidate| {
            preview(&candidate.candidate_id, Some(&candidate.source_revision))
                .unwrap()
                .0
        })
        .collect()
}

fn resident(preview_id: &str) -> PreviewState {
    rt(|runtime| runtime.previews.get(preview_id).cloned()).unwrap()
}

fn legacy_registration(preview_id: &str) -> String {
    format!(
        "local:{}",
        crate::session_migration::hash::sha256_bytes(resident(preview_id).legacy_id.as_bytes())
    )
}

fn queue_registration(
    preview_id: &str,
    installation: String,
    explicit_reimport: bool,
    options: serde_json::Value,
) -> (String, CommitItem) {
    queue_registration_custom(preview_id, installation, explicit_reimport, options, |_| {})
}

fn queue_registration_custom(
    preview_id: &str,
    installation: String,
    explicit_reimport: bool,
    options: serde_json::Value,
    configure: impl FnOnce(&mut CommitItem),
) -> (String, CommitItem) {
    let fixed = resident(preview_id);
    let mut adapter = fixed.adapter;
    adapter.graph.events = vec![serde_json::from_slice(
        &std::fs::read(fixed.prepared.path.join("events.jsonl")).unwrap(),
    )
    .unwrap()];
    let mut item = jobs::prepare_test_commit_item(CommitItem {
        candidate_id: fixed.candidate_id,
        installation_id: installation.clone(),
        provider: "cursor".into(),
        root_path: fixed.root_path,
        adapter,
        prepared: None,
        options,
        mapping_project_id: None,
        explicit_reimport,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::default(),
    });
    // Populate a coherent legacy producer lease before publication. Nothing in
    // the committed manifest/receipt/archive is rewritten after publication.
    let prepared = item.prepared.as_ref().unwrap();
    let mut lease = jobs::read_preview_lease(&prepared.path).unwrap().unwrap();
    lease.source_id = installation.clone();
    lease.legacy_id = fixed.legacy_id;
    jobs::write_preview_lease(&prepared.path, &lease).unwrap();
    item.adapter.graph.events.clear();
    configure(&mut item);
    let job_id = Uuid::new_v4().to_string();
    let job = JobRecord {
        job_id: job_id.clone(),
        request_id: Uuid::new_v4().to_string(),
        plan_hash: "synthetic-registration-layout-seed".into(),
        plan_id: "synthetic-registration-layout-seed".into(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: crate::session_migration::clock::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    crate::session_migration::job_control::enqueue(job, vec![item.clone()]).unwrap();
    (job_id, item)
}

fn seed_registration(preview_id: &str, installation: String) -> Seed {
    let (job_id, _) = queue_registration(preview_id, installation.clone(), false, json!({}));
    crate::session_migration::job_control::run_pending().unwrap();
    let completed = job_get(&job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed, "{completed:?}");
    let import_id = completed.items[0].import_id.clone().unwrap();
    let revision = completed.items[0].revision_id.clone().unwrap();
    let page = events_window(&import_id, Some(&revision), None, Some(20), None, None).unwrap();
    assert_eq!(page.events.len(), 1, "seed is readable before migration");
    let hashes = jobs::published_hashes(
        &crate::session_migration::catalog::revision_dir(&import_id, &revision).unwrap(),
    )
    .unwrap();
    Seed {
        import_id,
        revision,
        installation,
        job_id,
        hashes,
    }
}

fn entry_identity(seed: &Seed) -> (String, String) {
    let entry = get_entry(&open_catalog().unwrap(), &seed.import_id)
        .unwrap()
        .unwrap();
    (entry.installation_id, entry.native_session_id)
}

fn assert_archive_unchanged(seed: &Seed) {
    let directory =
        crate::session_migration::catalog::revision_dir(&seed.import_id, &seed.revision).unwrap();
    assert_eq!(jobs::published_hashes(&directory).unwrap(), seed.hashes);
}

fn table_counts() -> (i64, i64, i64, i64) {
    let conn = open_catalog().unwrap();
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions),
         (SELECT COUNT(*) FROM jobs), (SELECT COUNT(*) FROM receipts)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .unwrap()
}

#[test]
fn registered_retry_retains_original_revision_and_receipt() {
    let home = IsolatedAppHome::acquire("legacy-pipeline-registered-control");
    let previews = cursor_source(&home, 1);
    let installation = resident(&previews[0]).source_id;
    let initial = plan(previews.clone(), vec![]).unwrap();
    let accepted = commit(
        &initial.plan_id,
        &initial.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let initial_job = job_get(&accepted.job_id).unwrap();
    assert_eq!(initial_job.state, JobState::Completed);
    let import_id = initial_job.items[0].import_id.clone().unwrap();
    let revision = initial_job.items[0].revision_id.clone().unwrap();
    let hashes = jobs::published_hashes(
        &crate::session_migration::catalog::revision_dir(&import_id, &revision).unwrap(),
    )
    .unwrap();
    let seed = Seed {
        import_id,
        revision,
        installation,
        job_id: initial_job.job_id,
        hashes,
    };
    let identity = entry_identity(&seed);
    // A completed publication releases its old prepared input. Reimport obtains
    // a fresh fixed preview through scan/preview, never a disposed preview path.
    let pending = plan(cursor_source(&home, 1), vec![]).unwrap();
    let request = Uuid::new_v4().to_string();
    let queued = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    assert_eq!(
        commit(&pending.plan_id, &pending.plan_hash, &request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    crate::session_migration::job_control::run_pending().unwrap();
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        completed.items[0].import_id.as_deref(),
        Some(seed.import_id.as_str())
    );
    assert_eq!(
        completed.items[0].revision_id.as_deref(),
        Some(seed.revision.as_str())
    );
    reset_runtime();
    assert_eq!(entry_identity(&seed), identity);
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn legacy_registration_adoption_keeps_old_revision_readable() {
    let home = IsolatedAppHome::acquire("legacy-pipeline-old-reader");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let before = entry_identity(&seed);
    assert_eq!(before.0, seed.installation);
    let pending = plan(previews, vec![]).unwrap();
    assert_eq!(
        entry_identity(&seed),
        before,
        "planning never adopts identity"
    );
    let queued = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    let after = entry_identity(&seed);
    assert_archive_unchanged(&seed);
    reset_runtime();
    let old_read = events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    );
    println!(
        "LEGACY_PIPELINE_FACTS {}",
        json!({
            "seedJob": seed.job_id, "acceptedJob": queued.job_id,
            "before": before, "after": after, "archiveUnchanged": true,
            "oldReadError": old_read.as_ref().err().map(|error| json!({"code":error.code.as_str(),"message":error.message})),
            "normalizer": crate::session_migration::types::NORMALIZER_VERSION,
            "scope": "synthetic current-schema legacy registration layout, not an old-normalizer replay"
        })
    );
    old_read.expect("accepted registration adoption must preserve the immutable old reader");
    crate::session_migration::job_control::run_pending().unwrap();
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed, "{completed:?}");
    assert_eq!(
        completed.items[0].import_id.as_deref(),
        Some(seed.import_id.as_str())
    );
    reset_runtime();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    events_window(&seed.import_id, None, None, Some(20), None, None).unwrap();
    let conn = open_catalog().unwrap();
    let original = jobs::latest_receipt(
        &conn,
        &seed.job_id,
        &job_get(&seed.job_id).unwrap().items[0].candidate_id,
    )
    .unwrap()
    .unwrap();
    jobs::recover_committed_receipt(&conn, &original).unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn later_item_snapshot_change_does_not_adopt_an_earlier_item_without_a_job() {
    let home = IsolatedAppHome::acquire("legacy-pipeline-batch-validation");
    let previews = cursor_source(&home, 2);
    let pending = plan(previews, vec![]).unwrap();
    let first = &pending.preview_ids[0];
    let second = &pending.preview_ids[1];
    let seed = seed_registration(first, legacy_registration(first));
    let before = entry_identity(&seed);
    let counts = table_counts();
    // The first immutable selection remains valid. Deliberately change only the
    // second owned prepared snapshot after plan, never the valid seed/archive.
    // This exercises the production rejection boundary, not seed corruption.
    let second_fixed = resident(second);
    let second_events = second_fixed.prepared.path.join("events.jsonl");
    let mut bytes = std::fs::read(&second_events).unwrap();
    bytes.push(b' ');
    std::fs::write(second_events, bytes).unwrap();
    let error = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceChanged);
    let after = entry_identity(&seed);
    assert_eq!(
        table_counts(),
        counts,
        "failed batch must not create a durable job"
    );
    assert_archive_unchanged(&seed);
    reset_runtime();
    let restarted = entry_identity(&seed);
    println!(
        "LEGACY_PIPELINE_FACTS {}",
        json!({
            "failure": {"code":error.code.as_str(),"message":error.message},
            "before": before, "after": after, "restarted": restarted,
            "countsUnchanged": true, "archiveUnchanged": true,
            "scope": "synthetic production commit with later-item snapshot change before enqueue"
        })
    );
    assert_eq!(
        after, before,
        "no accepted job means no partial identity adoption"
    );
    assert_eq!(restarted, before);
    assert_no_provenance_extension();
    reset_runtime();
}

#[test]
fn enqueue_storage_failure_rolls_back_identity_with_the_job() {
    let home = IsolatedAppHome::acquire("legacy-pipeline-storage-rollback");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let before = entry_identity(&seed);
    let counts = table_counts();
    let pending = plan(previews, vec![]).unwrap();
    open_catalog()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER synthetic_reject_registration_job BEFORE INSERT ON job_items
         BEGIN SELECT RAISE(ABORT,'synthetic registration enqueue rejection'); END;",
        )
        .unwrap();
    let error = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap_err();
    assert!(error
        .message
        .contains("synthetic registration enqueue rejection"));
    reset_runtime();
    assert_eq!(entry_identity(&seed), before);
    assert_eq!(table_counts(), counts);
    assert_no_provenance_extension();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

fn assert_no_provenance_extension() {
    let conn = open_catalog().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name='registration_provenance')+
         (SELECT COUNT(*) FROM meta WHERE key='registration_provenance_schema_version')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "rejected enqueue/read must not leave extension DDL"
    );
}

fn accept_legacy(previews: Vec<String>) -> (Seed, JobRecord, PlanState, String) {
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let pending = plan(previews, vec![]).unwrap();
    let request = Uuid::new_v4().to_string();
    let accepted = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    (seed, accepted, pending, request)
}

#[test]
fn adopted_proof_is_exact_once_after_lost_ack_restart_and_worker() {
    let home = IsolatedAppHome::acquire("legacy-proof-replay");
    let previews = cursor_source(&home, 1);
    let (seed, accepted, pending, request) = accept_legacy(previews);
    reset_runtime();
    let replay = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    assert_eq!(replay.job_id, accepted.job_id);
    crate::session_migration::job_control::run_pending().unwrap();
    reset_runtime();
    assert_eq!(
        commit(&pending.plan_id, &pending.plan_hash, &request)
            .unwrap()
            .job_id,
        accepted.job_id
    );
    let conn = open_catalog().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn missing_proof_refuses_old_reader_and_recovery_without_read_ddl() {
    let home = IsolatedAppHome::acquire("legacy-proof-missing");
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    conn.execute_batch(
        "DROP TABLE registration_provenance;
        DELETE FROM meta WHERE key='registration_provenance_schema_version';",
    )
    .unwrap();
    let receipt = jobs::latest_receipt(
        &conn,
        &seed.job_id,
        &job_get(&seed.job_id).unwrap().items[0].candidate_id,
    )
    .unwrap()
    .unwrap();
    assert!(jobs::recover_committed_receipt(&conn, &receipt).is_err());
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    assert_no_provenance_extension();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn proof_digest_and_exact_bindings_are_not_a_reader_bypass() {
    let home = IsolatedAppHome::acquire("legacy-proof-tampered");
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    let (raw, digest): (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1",
            [&seed.import_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    for path in [
        "/originalNative",
        "/anchorReceipt",
        "/anchorDigest",
        "/expectedRevision",
        "/requestId",
        "/planId",
        "/input/sourceId",
        "/input/nativeSessionId",
        "/input/provider",
        "/input/legacyId",
        "/input/digest",
        "/host",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let replacement = match path {
            "/requestId" | "/input/sourceId" | "/anchorReceipt" => Uuid::new_v4().to_string(),
            "/anchorDigest" | "/input/digest" => "0".repeat(64),
            "/input/provider" => "codex".into(),
            _ => "synthetic foreign binding".into(),
        };
        *value.pointer_mut(path).unwrap() = json!(replacement);
        conn.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
            rusqlite::params![
                value.to_string(),
                crate::session_migration::hash::options_hash(&value),
                seed.import_id
            ],
        )
        .unwrap();
        assert!(
            events_window(
                &seed.import_id,
                Some(&seed.revision),
                None,
                Some(20),
                None,
                None
            )
            .is_err(),
            "{path}"
        );
    }
    conn.execute(
        "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
        rusqlite::params![raw, "0".repeat(64), seed.import_id],
    )
    .unwrap();
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    conn.execute(
        "UPDATE registration_provenance SET digest=?1 WHERE import_id=?2",
        rusqlite::params![digest, seed.import_id],
    )
    .unwrap();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn incompatible_provenance_schema_fails_closed_without_repairs() {
    let home = IsolatedAppHome::acquire("legacy-proof-schema");
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    for version in ["2", "0", "unknown"] {
        conn.execute(
            "UPDATE meta SET value=?1 WHERE key='registration_provenance_schema_version'",
            [version],
        )
        .unwrap();
        let error = open_catalog().unwrap_err();
        assert_eq!(
            error.code,
            if version == "2" {
                ErrorCode::ImportSchemaTooNew
            } else {
                ErrorCode::SchemaUnsupported
            }
        );
        let stored: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='registration_provenance_schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, version);
    }
    conn.execute(
        "UPDATE meta SET value='1' WHERE key='registration_provenance_schema_version'",
        [],
    )
    .unwrap();
    conn.execute_batch("CREATE TRIGGER synthetic_proof_trigger AFTER INSERT ON registration_provenance BEGIN SELECT 1; END;").unwrap();
    assert_eq!(
        open_catalog().unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    conn.execute_batch(
        "DROP TRIGGER synthetic_proof_trigger;
        DELETE FROM meta WHERE key='registration_provenance_schema_version';",
    )
    .unwrap();
    assert_eq!(
        open_catalog().unwrap_err().code,
        ErrorCode::SchemaUnsupported
    );
    conn.execute_batch(
        "INSERT INTO meta(key,value) VALUES('registration_provenance_schema_version','1');",
    )
    .unwrap();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn published_receipt_recovery_uses_proof_after_old_job_ack_loss() {
    let home = IsolatedAppHome::acquire("legacy-proof-worker-recovery");
    let (seed, accepted, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    crate::session_migration::job_control::change_job(&conn, &seed.job_id, |job| {
        job.state = JobState::Queued;
        job.items[0].phase = "queued".into();
        Ok(())
    })
    .unwrap();
    reset_runtime();
    crate::session_migration::job_control::run_pending().unwrap();
    let old = job_get(&seed.job_id).unwrap();
    assert_eq!(old.state, JobState::Completed, "{old:?}");
    assert_eq!(
        old.items[0].revision_id.as_deref(),
        Some(seed.revision.as_str())
    );
    assert_eq!(
        job_get(&accepted.job_id).unwrap().state,
        JobState::Completed
    );
    assert_archive_unchanged(&seed);
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    reset_runtime();
}

#[test]
fn alias_undo_tombstone_and_explicit_reimport_preserve_canonical_owner() {
    let home = IsolatedAppHome::acquire("legacy-proof-lifecycle");
    let (seed, accepted, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET alias='SYNTHETIC_ALIAS' WHERE import_id=?1",
        [&seed.import_id],
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let canonical = entry_identity(&seed);
    if get_entry(&conn, &seed.import_id)
        .unwrap()
        .unwrap()
        .current_revision
        .as_deref()
        != Some(seed.revision.as_str())
    {
        // A changed producer can publish a new revision. Undo its owning job
        // first; the original creator must never remove a later revision.
        let newer = jobs::undo_job(&conn, &accepted.job_id).unwrap();
        assert_eq!(newer.items[0].phase, "undone", "{newer:?}");
        assert_eq!(
            get_entry(&conn, &seed.import_id)
                .unwrap()
                .unwrap()
                .current_revision
                .as_deref(),
            Some(seed.revision.as_str())
        );
        events_window(&seed.import_id, None, None, Some(20), None, None).unwrap();
    }
    // Undo the original creator, not a deduplicated reimport. It hides the same
    // stable import and writes its tombstone under the accepted canonical owner.
    let undone = jobs::undo_job(&conn, &seed.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone", "{undone:?}");
    assert!(crate::session_migration::catalog::is_tombstoned(
        &conn,
        &canonical.0,
        "cursor",
        &canonical.1
    )
    .unwrap());
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    let pending = plan(cursor_source(&home, 1), vec![]).unwrap();
    let ordinary = commit(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    assert_eq!(
        job_get(&ordinary.job_id).unwrap().items[0]
            .outcome
            .as_deref(),
        Some("tombstoned")
    );
    let pending = plan(cursor_source(&home, 1), vec![]).unwrap();
    let restored = commit_with_reimport(
        &pending.plan_id,
        &pending.plan_hash,
        &Uuid::new_v4().to_string(),
        true,
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let restored_job = job_get(&restored.job_id).unwrap();
    assert_eq!(restored_job.state, JobState::Completed, "{restored_job:?}");
    let entry = get_entry(&conn, &seed.import_id).unwrap().unwrap();
    assert!(!entry.hidden);
    assert_eq!(entry.alias.as_deref(), Some("SYNTHETIC_ALIAS"));
    assert_eq!(entry_identity(&seed), canonical);
    assert!(!crate::session_migration::catalog::is_tombstoned(
        &conn,
        &canonical.0,
        "cursor",
        &canonical.1
    )
    .unwrap());
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    assert_eq!(
        job_get(&accepted.job_id).unwrap().items[0]
            .import_id
            .as_deref(),
        Some(seed.import_id.as_str())
    );
    reset_runtime();
}

#[test]
fn accepted_job_input_and_registration_are_required_for_old_authority() {
    let home = IsolatedAppHome::acquire("legacy-proof-input-binding");
    let (seed, accepted, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    let json: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&accepted.job_id],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE job_items SET result_json=json_set(result_json,'$.sourceId',?1) WHERE job_id=?2",
        rusqlite::params![Uuid::new_v4().to_string(), accepted.job_id],
    )
    .unwrap();
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    conn.execute(
        "UPDATE job_items SET result_json=?1 WHERE job_id=?2",
        rusqlite::params![json, accepted.job_id],
    )
    .unwrap();
    let target = entry_identity(&seed).0;
    for field in ["host", "provider"] {
        let original = if field == "host" { "local" } else { "cursor" };
        let changed = if field == "host" { "foreign" } else { "codex" };
        conn.execute(
            &format!("UPDATE sources SET {field}=?1 WHERE installation_id=?2"),
            rusqlite::params![changed, target],
        )
        .unwrap();
        assert!(events_window(
            &seed.import_id,
            Some(&seed.revision),
            None,
            Some(20),
            None,
            None
        )
        .is_err());
        conn.execute(
            &format!("UPDATE sources SET {field}=?1 WHERE installation_id=?2"),
            rusqlite::params![original, target],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE jobs SET request_id=?1 WHERE job_id=?2",
        rusqlite::params![Uuid::new_v4().to_string(), accepted.job_id],
    )
    .unwrap();
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    assert_archive_unchanged(&seed);
    reset_runtime();
}

#[test]
fn old_anchor_and_post_adoption_receipts_cannot_forge_provenance() {
    let home = IsolatedAppHome::acquire("legacy-proof-anchor-binding");
    let (seed, _, _, _) = accept_legacy(cursor_source(&home, 1));
    let conn = open_catalog().unwrap();
    let receipt = jobs::latest_receipt(
        &conn,
        &seed.job_id,
        &job_get(&seed.job_id).unwrap().items[0].candidate_id,
    )
    .unwrap()
    .unwrap();
    let raw = serde_json::to_string(&receipt).unwrap();
    conn.execute("UPDATE receipts SET json=json_set(json,'$.snapshotHash','synthetic wrong snapshot') WHERE receipt_id=?1",
        [&receipt.receipt_id]).unwrap();
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    conn.execute(
        "UPDATE receipts SET json=?1 WHERE receipt_id=?2",
        rusqlite::params![raw, receipt.receipt_id],
    )
    .unwrap();
    let mut late = receipt.clone();
    late.receipt_id = Uuid::new_v4().to_string();
    jobs::write_receipt(&conn, &late).unwrap();
    assert!(
        jobs::recover_committed_receipt(&conn, &late).is_err(),
        "post-adoption old publication has no implicit authority"
    );
    assert!(events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None
    )
    .is_err());
    conn.execute(
        "DELETE FROM receipts WHERE receipt_id=?1",
        [&late.receipt_id],
    )
    .unwrap();
    events_window(
        &seed.import_id,
        Some(&seed.revision),
        None,
        Some(20),
        None,
        None,
    )
    .unwrap();
    assert_archive_unchanged(&seed);
    reset_runtime();
}
