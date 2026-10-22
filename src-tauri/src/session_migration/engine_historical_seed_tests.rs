//! Opt-in discovery against an existing, hash-pinned synthetic V243 archive.
//! Ordinary tests need no external files. This is not a vendor fixture, automatic
//! identity migration, or a replay of the historical producer executable.

use super::*;
use crate::session_migration::{hash, sources};

#[path = "engine_historical_association_checks.rs"]
mod association_checks;

#[path = "engine_historical_association_plan_checks.rs"]
mod association_plan_checks;

#[path = "engine_historical_association_input_checks.rs"]
mod association_input_checks;

#[path = "engine_historical_association_proof_checks.rs"]
mod association_proof_checks;

#[path = "engine_historical_association_acceptance_tests.rs"]
mod association_acceptance_tests;
pub(in crate::session_migration) use association_acceptance_tests::association_crash_child;

const IMPORT: &str = "imp:ba6616b6-b9e8-440a-b01e-ab6db40c27a7";
const REVISION: &str = "028cbb7c-61ff-4d05-bd60-601124babd21";
const INSTALLATION: &str = "f138e7c5-6277-41d3-a982-cdfce579af2c";
const OLD_JOB: &str = "dc25f23f-d080-49dc-adbe-c8aa4776de9e";
const FIXTURE_HASH: &str = "7b6fea265b57f5bcdef8d277a705aec0162fd293c2920985699b9294ead15f7a";
const ARCHIVE: &str =
    "imports/entries/imp_ba6616b6-b9e8-440a-b01e-ab6db40c27a7/028cbb7c-61ff-4d05-bd60-601124babd21";
const ARCHIVE_FILES: &[(&str, &str)] = &[
    (
        "assets.json",
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945",
    ),
    (
        "event-index.lock",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        "event-index.sqlite",
        "a7a58bac90ff43833a7890b480a311bb3c44e82ae3e8fd30500bfe23f145cda4",
    ),
    (
        "events.jsonl",
        "928b86289c95be7669d06e70e3718b37adea0b5abaa693f77dc632eab5306eee",
    ),
    (
        "manifest.json",
        "06db165700a627fd8b17e04d79b0e762ca8d8eb4039a10262b1cb8038684728b",
    ),
    (
        "selection.json",
        "c9ed7c0fbe27ee0a55e565bfaeb04704d27e8f45baf3667746a90fa5326d0ecb",
    ),
];
const CATALOG_HASH: &str = "1afe53205040f78825b36035250701804cdbc96137b6f5f5ce079ed27674ef0e";

fn explicit_path(variable: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(variable).expect("explicit synthetic path required"));
    assert!(path.is_absolute(), "{variable} must be absolute");
    assert_plain(&path);
    path
}

fn assert_plain(path: &Path) {
    for component in path.ancestors() {
        let metadata = std::fs::symlink_metadata(component).unwrap();
        assert!(!metadata.file_type().is_symlink(), "symlink refused");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "reparse point refused"
            );
        }
    }
}

fn require_hash(path: &Path, expected: &str) {
    assert_plain(path);
    assert_eq!(
        hash::sha256_file(path).unwrap(),
        expected,
        "{}",
        path.display()
    );
}

fn require_seed(seed: &Path, fixture: &Path) {
    // Never open the seed catalog through a writer, use its verification cache,
    // or copy any settings, credential material, native sessions or old cursors.
    require_hash(&seed.join("imports/catalog.sqlite"), CATALOG_HASH);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!seed
            .join(format!("imports/catalog.sqlite{suffix}"))
            .exists());
    }
    for (name, expected) in ARCHIVE_FILES {
        require_hash(&seed.join(ARCHIVE).join(name), expected);
    }
    require_hash(fixture, FIXTURE_HASH);
}

fn receipts(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let mut statement = conn
        .prepare("SELECT receipt_id,json FROM receipts WHERE import_id=?1 ORDER BY rowid")
        .unwrap();
    statement
        .query_map([IMPORT], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn normalizer(conn: &rusqlite::Connection, import: &str, revision: &str) -> String {
    conn.query_row(
        "SELECT normalizer_version FROM revisions WHERE import_id=?1 AND revision_id=?2",
        rusqlite::params![import, revision],
        |row| row.get(0),
    )
    .unwrap()
}

fn text(import: &str, revision: &str) -> String {
    let (events, cursor, count) =
        events_page(import, Some(revision), None, Some(20), None).unwrap();
    assert!(cursor.is_none());
    assert_eq!(count, 2);
    assert_eq!(events.len(), 2);
    let encoded = serde_json::to_string(&events).unwrap();
    assert!(encoded.contains("hello one"));
    assert!(encoded.contains("hi two"));
    encoded
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_pipeline_discovery() {
    let seed = explicit_path("GROK_IMPORT_HISTORICAL_SEED");
    let fixture = explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE");
    require_seed(&seed, &fixture);
    let home = IsolatedAppHome::acquire("historical-seed-pipeline");
    reset_runtime();
    let _runtime = RuntimeReset;
    std::fs::copy(
        seed.join("imports/catalog.sqlite"),
        home.path.join("imports/catalog.sqlite"),
    )
    .unwrap();
    let archive = home.path.join(ARCHIVE);
    std::fs::create_dir_all(&archive).unwrap();
    for (name, expected) in ARCHIVE_FILES {
        std::fs::copy(seed.join(ARCHIVE).join(name), archive.join(name)).unwrap();
        require_hash(&archive.join(name), expected);
    }
    let conn = open_catalog().unwrap();
    let old_entry = get_entry(&conn, IMPORT).unwrap().unwrap();
    assert_eq!(old_entry.installation_id, INSTALLATION);
    assert_eq!(old_entry.provider, "cursor");
    assert_eq!(old_entry.native_session_id, "transcript");
    assert_eq!(old_entry.current_revision.as_deref(), Some(REVISION));
    assert_eq!(normalizer(&conn, IMPORT, REVISION), "1.0.0");
    let source = sources::get(&conn, INSTALLATION).unwrap();
    assert_eq!(source.provider, Provider::Cursor);
    assert_eq!(
        std::fs::canonicalize(&source.root_path).unwrap(),
        std::fs::canonicalize(fixture.parent().unwrap()).unwrap(),
        "this fixture must belong to the exact registered synthetic directory"
    );
    let members: Vec<_> = std::fs::read_dir(fixture.parent().unwrap())
        .unwrap()
        .map(|entry| std::fs::canonicalize(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(members, vec![std::fs::canonicalize(&fixture).unwrap()]);
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    let old_receipts = receipts(&conn);
    assert_eq!(old_receipts.len(), 5);
    let old_job: String = conn
        .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get(0)
        })
        .unwrap();
    drop(conn);
    jobs::clear_published_read_verification_proofs();
    let old_text = text(IMPORT, REVISION);
    assert!(jobs::take_full_published_read_verifications() > 0);
    // Explicitly scan the original registered directory, whose only member is
    // the hash-pinned fixture. Cursor discovery takes a directory, not a file.
    // This does not authorize associating two SourceSession identities.
    let started = crate::session_migration::scan::begin(vec![ScanSourcePath {
        provider: "cursor".into(),
        path: source.root_path,
        source_id: Some(INSTALLATION.into()),
        location_id: None,
        host: Some("local".into()),
        profile: source.profile,
    }])
    .unwrap();
    crate::session_migration::scan::run(&started.scan_id).unwrap();
    let scan = scan_pump(&started.scan_id, started.epoch).unwrap();
    assert!(scan.complete);
    assert!(scan.issues.is_empty(), "{:?}", scan.issues);
    assert_eq!(scan.issue_count, 0);
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    let reference = crate::session_migration::scan::candidate(&candidate.candidate_id).unwrap();
    assert_eq!(
        crate::session_migration::scan::candidate(&candidate.candidate_id)
            .unwrap()
            .source_id,
        INSTALLATION
    );
    assert!(candidate
        .native_session_id
        .starts_with("cursor-identity-v1:transcript-locator:"));
    let (preview_id, details) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert_eq!(details.event_count, 2);
    let pending = plan(vec![preview_id], vec![]).unwrap();
    let association_request = crate::session_migration::identity_association::AssociationRequest {
        version: 1,
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
        preview: pending.selection_set.single_entry().unwrap().clone(),
    };
    let review = association_checks::inspect_unchanged(&association_request).unwrap();
    assert_eq!(
        association_checks::command_unchanged(&association_request).unwrap(),
        review
    );
    assert_eq!(review.import_id, IMPORT);
    assert_eq!(review.old_native_session_id, "transcript");
    assert_eq!(
        review.current_native_session_id,
        candidate.native_session_id
    );
    assert_eq!(review.old_normalizer_version, "1.0.0");
    assert_eq!(review.current_normalizer_version, "1.0.6");
    assert_eq!(review.review_digest.len(), 64);
    assert_eq!(
        crate::session_migration::identity_association::inspect(&association_request).unwrap(),
        review
    );
    let rejected_checks = association_checks::exercise(&association_request, &review);
    let (plan_checks, input_checks, proof_checks) =
        association_plan_checks::exercise(&association_request, &review);
    let conn = open_catalog().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(receipts(&conn), old_receipts);
    let proof_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='registration_provenance')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !proof_exists,
        "inspection must not initialize a provenance extension"
    );
    drop(conn);
    reset_runtime();
    let request = Uuid::new_v4().to_string();
    let queued = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    // Same-request retry must resolve the already accepted durable job.
    assert_eq!(
        commit(&pending.plan_id, &pending.plan_hash, &request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    crate::session_migration::job_control::run_pending().unwrap();
    let completed = job_get(&queued.job_id).unwrap();
    assert_eq!(completed.state, JobState::Completed, "{completed:?}");
    assert_eq!(completed.items.len(), 1);
    let current_import = completed.items[0].import_id.as_deref().unwrap();
    let current_revision = completed.items[0].revision_id.as_deref().unwrap();
    assert_ne!(
        current_import, IMPORT,
        "no explicit old/new association was authorized"
    );
    assert_ne!(current_revision, REVISION);
    // A second identity must be an actual independently published entry, not a
    // hand-inserted canonical row. Use a fresh, unaccepted preview for the check.
    let (collision_preview, _) = preview_reference(reference, None, None).unwrap();
    let collision_plan = plan(vec![collision_preview], vec![]).unwrap();
    let mut collision_request = association_request.clone();
    collision_request.preview = collision_plan.selection_set.single_entry().unwrap().clone();
    assert_eq!(
        association_checks::inspect_unchanged(&collision_request)
            .unwrap_err()
            .code,
        ErrorCode::MappingConflict
    );
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), old_text);
    text(current_import, current_revision);
    assert_eq!(
        commit(&pending.plan_id, &pending.plan_hash, &request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    let conn = open_catalog().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 2);
    let old_after = get_entry(&conn, IMPORT).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(old_after).unwrap(),
        serde_json::to_value(old_entry).unwrap()
    );
    assert_eq!(receipts(&conn), old_receipts);
    let job_after: String = conn
        .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(job_after, old_job);
    let entry = get_entry(&conn, current_import).unwrap().unwrap();
    assert_eq!(entry.installation_id, INSTALLATION);
    assert_eq!(entry.native_session_id, candidate.native_session_id);
    let current_normalizer = normalizer(&conn, current_import, current_revision);
    assert_eq!(
        current_normalizer,
        crate::session_migration::types::NORMALIZER_VERSION
    );
    for (name, expected) in ARCHIVE_FILES
        .iter()
        .filter(|(name, _)| !name.starts_with("event-index"))
    {
        require_hash(&archive.join(name), expected);
    }
    require_seed(&seed, &fixture);
    println!(
        "HISTORICAL_SEED_DISCOVERY={}",
        json!({
            "oldImport": IMPORT, "oldRevision": REVISION, "oldNormalizer": "1.0.0",
            "currentImport": current_import, "currentRevision": current_revision,
            "currentNative": entry.native_session_id, "currentNormalizer": current_normalizer,
            "jobId": queued.job_id, "planId": pending.plan_id, "requestId": request,
            "entries": 2, "oldReceiptCount": old_receipts.len(), "messagesPerRevision": 2,
            "oldArchiveUnchanged": true, "sameRequestRetryStable": true,
            "explicitInstallationScan": true,
            "associationPlanChecks": plan_checks,
            "associationInputChecks": input_checks,
            "associationProofDraftChecks": proof_checks,
            "associationReviewDigest": review.review_digest,
            "associationInspectionOnly": true,
            "associationAsyncCommandVerified": true,
            "associationFaultAndDigestChecks": rejected_checks + 1,
            "associationCatalogAndLeaseUnchanged": true,
            "scope": "Existing synthetic archive through current production pipeline; no explicit identity association, vendor data or historical producer execution"
        })
    );
}

struct RuntimeReset;
impl Drop for RuntimeReset {
    fn drop(&mut self) {
        reset_runtime();
    }
}
