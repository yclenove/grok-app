//! Restore only authentic archived prepared bytes in an isolated synthetic home.
//! This exercises today's durable worker with original input2 metadata, not an
//! old executable/vendor replay or a revival of the immutable seed's preview.
use super::*;
use crate::session_migration::catalog;
use std::{cell::RefCell, rc::Rc};

#[path = "engine_historical_association_old_owner_boundary_tests.rs"]
mod boundary_tests;

#[path = "engine_historical_publication_exclusion_adoption_tests.rs"]
mod exclusion_adoption_tests;

const INPUT_HASH: &str = "b12ebe0605de519c5e2cfaa44f744ca90ce57b9b00155d5c197f4c133396e86d";
const OLD_PREVIEW: &str = "2d40a30b-0686-47e4-ad15-7daf1190b718";

fn all_receipts(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    conn.prepare("SELECT receipt_id,json FROM receipts ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn field<'a>(input: &'a serde_json::Value, key: &str) -> &'a str {
    input[key].as_str().unwrap()
}

fn original_input() -> serde_json::Value {
    let seed = explicit_path("GROK_IMPORT_HISTORICAL_SEED");
    require_seed(&seed, &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"));
    let conn = rusqlite::Connection::open_with_flags(
        seed.join("imports/catalog.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    conn.pragma_update(None, "query_only", true).unwrap();
    let bytes: i64 = conn
        .query_row(
            "SELECT length(CAST(result_json AS BLOB)) FROM job_items WHERE job_id=?1",
            [OLD_JOB],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bytes, 3591, "pinned original input capacity");
    let raw: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [OLD_JOB],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hash::sha256_bytes(raw.as_bytes()), INPUT_HASH);
    let input: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(input["version"], 2);
    assert!(input.get("sourceMember").is_none());
    assert!(input.get("identityAssociation").is_none());
    assert_eq!(input["installationId"], INSTALLATION);
    assert_eq!(input["sourceId"], INSTALLATION);
    assert_eq!(input["provider"], "cursor");
    assert_eq!(input["adapter"]["graph"]["nativeSessionId"], "transcript");
    input
}

fn restore_old_item(home: &IsolatedAppHome, job: &JobRecord) -> CommitItem {
    let mut input = original_input();
    let original = input.clone();
    let selected: SelectionSet = serde_json::from_value(input["selectionSet"].clone()).unwrap();
    assert_eq!(selected.single_entry().unwrap().preview_id, OLD_PREVIEW);
    assert_eq!(selected.hash(), field(&input, "selectionHash"));
    let hashes: BTreeMap<String, String> =
        serde_json::from_value(input["previewFileHashes"].clone()).unwrap();
    assert_eq!(hashes.len(), 3);
    let preview = catalog::staging_dir().join("previews").join(OLD_PREVIEW);
    assert!(preview.starts_with(&home.path));
    assert!(!preview.exists());
    std::fs::create_dir_all(&preview).unwrap();
    for (name, expected) in &hashes {
        assert!(matches!(
            name.as_str(),
            "assets.json" | "events.jsonl" | "manifest.json"
        ));
        let archive = home.path.join(ARCHIVE).join(name);
        require_hash(&archive, expected);
        std::fs::copy(archive, preview.join(name)).unwrap();
        require_hash(&preview.join(name), expected);
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(preview.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["normalizerVersion"], "1.0.0");
    assert_eq!(jobs::prepared_hashes(&preview).unwrap(), hashes);
    jobs::write_preview_lease(
        &preview,
        &jobs::PreviewLease {
            source_member: None,
            preview_id: OLD_PREVIEW.into(),
            candidate_id: field(&input, "candidateId").into(),
            source_revision: format!("synthetic-archived-fixture-restoration:{INPUT_HASH}"),
            prepare_batch_id: None,
            source_id: field(&input, "sourceId").into(),
            root_path: field(&input, "rootPath").into(),
            legacy_id: field(&input, "legacyId").into(),
            snapshot_digest: field(&input, "previewDigest").into(),
            expires_unix: chrono::Utc::now().timestamp() + 3600,
            retained_by_job: false,
            file_hashes: hashes,
        },
    )
    .unwrap();
    // Only the isolated prepared path differs from the authenticated input.
    input["previewPath"] = json!(preview);
    let relocated = serde_json::to_string(&input).unwrap();
    job_control::inspect_input_metadata(&relocated, job).unwrap();
    let item = job_control::decode_input(&relocated, job).unwrap();
    let encoded: serde_json::Value =
        serde_json::from_str(&job_control::encode_input(&item).unwrap()).unwrap();
    assert_eq!(encoded, input, "complete canonical metadata roundtrip");
    let mut unchanged = encoded;
    unchanged["previewPath"] = original["previewPath"].clone();
    assert_eq!(
        unchanged, original,
        "no current-adapter identity substitution"
    );
    println!("AUTHENTIC_OLD_INPUT_RESTORATION={INPUT_HASH}:input2:normalizer1.0.0:isolated-only");
    item
}

fn queue_old(home: &IsolatedAppHome, reimport: bool) -> (JobRecord, CommitItem) {
    let requested = JobRecord {
        job_id: Uuid::new_v4().to_string(),
        request_id: format!("synthetic-authentic-old-producer-{}", Uuid::new_v4()),
        plan_hash: "synthetic-archived-prepared-restoration".into(),
        plan_id: Uuid::new_v4().to_string(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    let mut item = restore_old_item(home, &requested);
    assert!(!item.explicit_reimport);
    // Intentional second scenario, after the exact original metadata control.
    item.explicit_reimport = reimport;
    let queued = job_control::enqueue(requested, vec![item.clone()]).unwrap();
    let conn = open_catalog().unwrap();
    let durable: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&durable).unwrap(),
        serde_json::from_str::<serde_json::Value>(&job_control::encode_input(&item).unwrap())
            .unwrap()
    );
    assert!(
        jobs::read_preview_lease(&item.prepared.as_ref().unwrap().path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    (queued, item)
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_authentic_old_producer_control() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let (queued, item) = queue_old(&home, false);
    drop(item);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let completed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(completed.items[0].phase, "committed");
    assert_eq!(completed.items[0].import_id.as_deref(), Some(IMPORT));
    let revision = completed.items[0].revision_id.as_deref().unwrap();
    assert_ne!(
        revision, REVISION,
        "old normalizer is not current legacy dedup"
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(text(IMPORT, revision), old_text);
    for original in old_receipts {
        assert!(receipts(&conn).contains(&original));
    }
    println!("AUTHENTIC_OLD_PRODUCER_CONTROL=worker-committed:same-import:new-revision");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_old_first_cas() {
    for reimport in [false, true] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let old_text = text(IMPORT, REVISION);
        let (queued, item) = queue_old(&home, reimport);
        let confirmed = confirmed_plan();
        let result = Rc::new(RefCell::new(None));
        let captured = result.clone();
        let old_job_id = queued.job_id.clone();
        job_control::association_acceptance::test_after_preflight(move || {
            let conn = open_catalog().unwrap();
            let published = jobs::publish_item(&conn, &old_job_id, &item, &|| false).unwrap();
            *captured.borrow_mut() = Some(published);
        });
        let error = acceptance(&confirmed, "synthetic-old-first-cas").unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale, "{reimport}: {error:?}");
        let published = result.borrow();
        let published = published.as_ref().unwrap();
        assert_eq!(published.phase, "committed");
        assert_eq!(published.import_id.as_deref(), Some(IMPORT));
        let revision = published.revision_id.as_deref().unwrap();
        assert_ne!(revision, REVISION);
        assert_eq!(
            get_entry(&conn, IMPORT).unwrap().unwrap().native_session_id,
            "transcript"
        );
        assert_eq!(
            get_entry(&conn, IMPORT)
                .unwrap()
                .unwrap()
                .current_revision
                .as_deref(),
            Some(revision)
        );
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(text(IMPORT, revision), old_text);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert_eq!(job_control::run_pending().unwrap(), 1);
        assert_eq!(
            jobs::get_job(&conn, &queued.job_id).unwrap().unwrap().items[0]
                .revision_id
                .as_deref(),
            Some(revision)
        );
        println!("AUTHENTIC_OLD_FIRST=reimport:{reimport}:actual-publisher:PlanStale:original-CAS-preserved");
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_association_first_old_owner() {
    let mut violations = Vec::new();
    for reimport in [false, true] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let old_text = text(IMPORT, REVISION);
        let (queued, item) = queue_old(&home, reimport);
        let confirmed = confirmed_plan();
        let accepted = acceptance(&confirmed, "synthetic-association-first-old-owner").unwrap();
        let before = all_receipts(&conn);
        let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
        let outcome = jobs::publish_item(&conn, &queued.job_id, &item, &|| false);
        let after = all_receipts(&conn);
        println!("AUTHENTIC_ASSOCIATION_FIRST=reimport:{reimport}:outcome:{outcome:?}:entries:{}:new-receipts:{}", list_entries(&conn).unwrap().len(), after.len() - before.len());
        let refused = outcome.as_ref().is_err_and(|error| {
            error.code == ErrorCode::PlanStale
                && error.message == "historical association no longer owns this entry; reimport through the registered source"
        });
        if !refused || list_entries(&conn).unwrap().len() != 1 || after != before {
            violations.push((reimport, format!("{outcome:?}")));
        }
        assert_eq!(
            serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
            entry
        );
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(
            jobs::get_job(&conn, &accepted.job_id)
                .unwrap()
                .unwrap()
                .state,
            JobState::Queued
        );
        for (name, expected) in ARCHIVE_FILES
            .iter()
            .filter(|(name, _)| !name.starts_with("event-index"))
        {
            require_hash(&home.path.join(ARCHIVE).join(name), expected);
        }
        drop(item);
        assert_eq!(job_control::run_pending().unwrap(), 2);
        let old_result = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
        assert_eq!(old_result.items[0].phase, "failed");
        assert!(old_result.items[0]
            .error
            .as_deref()
            .unwrap()
            .contains("PLAN_STALE"));
        let new_result = jobs::get_job(&conn, &accepted.job_id).unwrap().unwrap();
        assert_eq!(new_result.items[0].phase, "committed");
        assert_eq!(new_result.items[0].import_id.as_deref(), Some(IMPORT));
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
    }
    assert!(violations.is_empty(), "accepted association must refuse the authentic old owner before publication effects: {violations:?}");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_old_owner_index_negatives() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (queued, item) = queue_old(&home, false);
    let confirmed = confirmed_plan();
    acceptance(&confirmed, "synthetic-old-owner-index-negatives").unwrap();
    let before = all_receipts(&conn);
    let lease = std::fs::read(item.prepared.as_ref().unwrap().path.join("lease.json")).unwrap();
    for (fault, expected) in [
        ("missing-index", ErrorCode::HistoryAccessUnavailable),
        ("missing-marker", ErrorCode::HistoryAccessUnavailable),
        ("missing-both", ErrorCode::HistoryAccessUnavailable),
        (
            "selector-key-substitution",
            ErrorCode::HistoryAccessUnavailable,
        ),
        (
            "missing-all-selector-facts",
            ErrorCode::HistoryAccessUnavailable,
        ),
        ("index-blob", ErrorCode::SchemaUnsupported),
        ("index-import", ErrorCode::HistoryAccessUnavailable),
        ("index-digest", ErrorCode::HistoryAccessUnavailable),
        ("marker-blob", ErrorCode::SnapshotUnavailable),
        ("marker-digest", ErrorCode::HistoryAccessUnavailable),
        ("missing-proof", ErrorCode::HistoryAccessUnavailable),
        (
            "missing-accepted-witness",
            ErrorCode::HistoryAccessUnavailable,
        ),
        ("missing-schema", ErrorCode::SchemaUnsupported),
        ("missing-table", ErrorCode::SchemaUnsupported),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "selector-key-substitution" => {
                let unrelated = "1".repeat(64);
                tx.execute(
                    "UPDATE registration_association_old_owners SET tuple_digest=?1",
                    [&unrelated],
                )
                .unwrap();
                tx.execute(
                    "UPDATE meta SET key=?1 WHERE key LIKE 'registration_association_old_owner:%'",
                    [format!("registration_association_old_owner:{unrelated}")],
                )
                .unwrap();
            }
            "missing-all-selector-facts" => {
                tx.execute("DELETE FROM registration_association_old_owners", [])
                    .unwrap();
                tx.execute("DELETE FROM meta WHERE key LIKE 'registration_association_old_owner:%' OR key LIKE 'registration_association_accepted:%'", []).unwrap();
            }
            "missing-index" | "missing-both" => {
                tx.execute("DELETE FROM registration_association_old_owners", [])
                    .unwrap();
                if fault == "missing-both" {
                    tx.execute(
                        "DELETE FROM meta WHERE key LIKE 'registration_association_old_owner:%'",
                        [],
                    )
                    .unwrap();
                }
            }
            "missing-marker" => {
                tx.execute(
                    "DELETE FROM meta WHERE key LIKE 'registration_association_old_owner:%'",
                    [],
                )
                .unwrap();
            }
            "index-blob" => {
                tx.execute("UPDATE registration_association_old_owners SET proof_digest=CAST(proof_digest AS BLOB)", []).unwrap();
            }
            "index-import" => {
                tx.execute(
                    "UPDATE registration_association_old_owners SET import_id=?1",
                    [format!("imp:{}", Uuid::new_v4())],
                )
                .unwrap();
            }
            "index-digest" => {
                tx.execute(
                    "UPDATE registration_association_old_owners SET proof_digest=?1",
                    ["0".repeat(64)],
                )
                .unwrap();
            }
            "marker-blob" => {
                tx.execute("UPDATE meta SET value=CAST(value AS BLOB) WHERE key LIKE 'registration_association_old_owner:%'", []).unwrap();
            }
            "marker-digest" => {
                tx.execute("UPDATE meta SET value=?1 WHERE key LIKE 'registration_association_old_owner:%'", [format!("{IMPORT}:{}", "0".repeat(64))]).unwrap();
            }
            "missing-proof" => {
                tx.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "missing-accepted-witness" => {
                tx.execute(
                    "DELETE FROM meta WHERE key LIKE 'registration_association_accepted:%'",
                    [],
                )
                .unwrap();
            }
            "missing-schema" => {
                tx.execute("DELETE FROM meta WHERE key='registration_association_old_owners_schema_version'", []).unwrap();
            }
            "missing-table" => {
                tx.execute_batch("DROP TABLE registration_association_old_owners")
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let error = jobs::publish_item(&tx, &queued.job_id, &item, &|| false).unwrap_err();
        assert_eq!(error.code, expected, "{fault}: {error:?}");
        tx.rollback().unwrap();
        assert_eq!(all_receipts(&conn), before);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        assert_eq!(
            std::fs::read(item.prepared.as_ref().unwrap().path.join("lease.json")).unwrap(),
            lease
        );
        let healthy = jobs::publish_item(&conn, &queued.job_id, &item, &|| false).unwrap_err();
        assert_eq!(healthy.code, ErrorCode::PlanStale);
        println!(
            "AUTHENTIC_OLD_OWNER_INDEX_REFUSAL={fault}:{}:exact-restoration",
            error.code.as_str()
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_original_committed_recovery() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let old_job = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    let before = receipts(&conn);
    let old_text = text(IMPORT, REVISION);
    let recovered = job_control::recover_test_input(&conn, OLD_JOB, &old_job.items[0])
        .unwrap()
        .unwrap();
    assert_eq!(recovered.import_id.as_deref(), Some(IMPORT));
    assert_eq!(recovered.revision_id.as_deref(), Some(REVISION));
    assert_eq!(receipts(&conn), before);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert!(!catalog::staging_dir()
        .join("previews")
        .join(OLD_PREVIEW)
        .exists());
    println!(
        "AUTHENTIC_OLD_COMMITTED_RECOVERY=original-receipt:no-preview-reopen:no-republication"
    );
}
