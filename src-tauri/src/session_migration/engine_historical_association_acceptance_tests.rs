//! Real accepted-job assertions, using only the original immutable V243 seed.
//! No positive proof/job/input/entry row is manufactured by this harness.
use super::*;
use crate::session_migration::{
    identity_association::{
        self, AssociationConfirmation, AssociationPlanRequest, AssociationRequest,
    },
    job_control,
};

#[path = "engine_historical_association_transaction_tests.rs"]
mod transaction_tests;

#[path = "engine_historical_association_retention_tests.rs"]
mod retention_tests;

#[path = "engine_historical_association_crash_tests.rs"]
mod crash_tests;

#[path = "engine_historical_association_membership_tests.rs"]
mod membership_tests;

#[path = "engine_historical_association_publication_tests.rs"]
mod publication_tests;

#[path = "engine_historical_association_undo_tests.rs"]
mod undo_tests;

#[path = "engine_historical_association_old_producer_tests.rs"]
mod old_producer_tests;

#[path = "engine_historical_association_job_entry_tests.rs"]
mod job_entry_tests;

#[path = "engine_historical_association_isolation_tests.rs"]
mod isolation_tests;

#[path = "engine_historical_association_scope_tests.rs"]
mod scope_tests;

#[path = "engine_historical_association_intent_tests.rs"]
mod intent_tests;

#[path = "engine_historical_association_progress_tests.rs"]
mod progress_tests;

#[path = "engine_historical_association_progress_ack_tests.rs"]
mod progress_ack_tests;

#[path = "engine_historical_signed_protocol_tests.rs"]
mod signed_protocol_tests;

#[path = "engine_historical_association_public_commit_tests.rs"]
mod public_commit_tests;

#[path = "engine_historical_prepared_association_tests.rs"]
mod prepared_tests;

fn seed_home() -> IsolatedAppHome {
    let seed = explicit_path("GROK_IMPORT_HISTORICAL_SEED");
    let fixture = explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE");
    require_seed(&seed, &fixture);
    let home = IsolatedAppHome::acquire("historical-association-acceptance");
    reset_runtime();
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
    home
}

fn confirmed_plan() -> PlanState {
    let conn = open_catalog().unwrap();
    let source = sources::get(&conn, INSTALLATION).unwrap();
    let fixture = explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE");
    assert_eq!(
        std::fs::canonicalize(&source.root_path).unwrap(),
        std::fs::canonicalize(fixture.parent().unwrap()).unwrap()
    );
    drop(conn);
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
    assert!(scan.complete && scan.issues.is_empty());
    assert_eq!(scan.candidates.len(), 1);
    let candidate = &scan.candidates[0];
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let ordinary = plan(vec![preview_id], vec![]).unwrap();
    let mut selection = ordinary.selection_set.single_entry().unwrap().clone();
    selection.selection.event_ids.reverse();
    let review = identity_association::inspect(&AssociationRequest {
        version: 1,
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
        preview: selection.clone(),
    })
    .unwrap();
    AssociationPlanRequest {
        version: 1,
        preview_ids: vec![selection.preview_id.clone()],
        mappings: vec![],
        selection_set: SelectionSet::single(selection),
        identity_associations: vec![AssociationConfirmation {
            version: 1,
            import_id: IMPORT.into(),
            expected_revision: REVISION.into(),
            preview_id: review.preview_id,
            review_digest: review.review_digest,
        }],
    }
    .build()
    .unwrap()
}

fn acceptance(plan: &PlanState, request: &str) -> Result<JobRecord, ImportError> {
    let job = requested_job(plan, request);
    let items = engine_commit::commit_items_for_plan(plan, &job, false)?;
    job_control::enqueue_confirmed_associations(job, items)
}

fn requested_job(plan: &PlanState, request: &str) -> JobRecord {
    JobRecord {
        job_id: plan.publication_protocol.new_job_id(),
        request_id: request.into(),
        plan_hash: plan.plan_hash.clone(),
        plan_id: plan.plan_id.clone(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let old_text = text(IMPORT, REVISION);
    let conn = open_catalog().unwrap();
    let before = get_entry(&conn, IMPORT).unwrap().unwrap();
    let old_receipts = receipts(&conn);
    let old_job = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    let confirmed = confirmed_plan();
    let request = "synthetic-real-association-acceptance";
    let queued = commit(&confirmed.plan_id, &confirmed.plan_hash, request)
        .expect("public commit must accept a genuine confirmed association atomically");
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(queued.items.len(), 1);
    let after = get_entry(&conn, IMPORT).unwrap().unwrap();
    assert_eq!(after.current_revision, before.current_revision);
    assert_eq!(after.installation_id, before.installation_id);
    assert_eq!(after.title, before.title);
    assert_eq!(after.alias, before.alias);
    assert_eq!(after.hidden, before.hidden);
    assert_ne!(after.native_session_id, before.native_session_id);
    assert!(after
        .native_session_id
        .starts_with("cursor-identity-v1:transcript-locator:"));
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    assert_eq!(receipts(&conn), old_receipts);
    assert_eq!(
        serde_json::to_value(jobs::get_job(&conn, OLD_JOB).unwrap()).unwrap(),
        serde_json::to_value(Some(old_job)).unwrap()
    );
    assert_eq!(
        text(IMPORT, REVISION),
        old_text,
        "accepted bridge must preserve old history"
    );
    reset_runtime();
    assert_eq!(
        text(IMPORT, REVISION),
        old_text,
        "restart must reauthenticate durable authority"
    );
    assert_eq!(
        acceptance(&confirmed, request).unwrap().job_id,
        queued.job_id
    );
    accepted_negative_checks(&conn, &queued);
    let preview_id = &confirmed.preview_ids[0];
    let preview_path = jobs::find_preview_dir(preview_id).unwrap().unwrap();
    let moved = home.path.join("disposable-accepted-preview-offline");
    std::fs::rename(&preview_path, &moved).unwrap();
    let without_preview = jobs::load_published_auth(IMPORT, Some(REVISION), None);
    let retry =
        job_control::enqueue_confirmed_associations(requested_job(&confirmed, request), vec![]);
    let mut foreign = requested_job(&confirmed, request);
    foreign.plan_id = Uuid::new_v4().to_string();
    let stale = job_control::enqueue_confirmed_associations(foreign, vec![]);
    std::fs::rename(&moved, &preview_path).unwrap();
    assert!(
        without_preview.is_ok(),
        "accepted reader must not reopen a preview"
    );
    assert_eq!(
        retry.unwrap().job_id,
        queued.job_id,
        "accepted retry precedes input/preview inspection"
    );
    assert_eq!(stale.unwrap_err().code, ErrorCode::PlanStale);
    for (name, expected) in ARCHIVE_FILES
        .iter()
        .filter(|(name, _)| !name.starts_with("event-index"))
    {
        require_hash(&home.path.join(ARCHIVE).join(name), expected);
    }
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    println!(
        "HISTORICAL_ASSOCIATION_ACCEPTANCE={}",
        json!({
            "importId": IMPORT, "oldRevision": REVISION, "jobId": queued.job_id,
            "oldHistoryReadable": true, "oldReceiptsUnchanged": true,
            "publicCommitAccepted": true, "publisherVerified": false,
        })
    );
}

fn accepted_negative_checks(conn: &rusqlite::Connection, queued: &JobRecord) {
    use crate::session_migration::{
        catalog, registration_provenance::ProofRecord, registration_provenance_store as proof_store,
    };
    let raw = serde_json::to_value(proof_store::get(conn, IMPORT).unwrap().unwrap()).unwrap();
    for (pointer, replacement) in [
        ("/anchorDigest", json!("0".repeat(64))),
        ("/anchorReceipt", json!(Uuid::new_v4().to_string())),
        (
            "/receiptCutoff",
            json!(raw["receiptCutoff"].as_i64().unwrap() + 1),
        ),
        ("/input/digest", json!("0".repeat(64))),
        ("/input/candidateId", json!("foreign-candidate")),
        (
            "/association/confirmation/reviewDigest",
            json!("0".repeat(64)),
        ),
        ("/association/requestId", json!("foreign-request")),
        ("/association/planId", json!(Uuid::new_v4().to_string())),
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        let mut value = raw.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let changed = ProofRecord::parse(&value.to_string()).unwrap();
        tx.execute(
            "UPDATE registration_provenance SET json=?1,digest=?2 WHERE import_id=?3",
            rusqlite::params![
                serde_json::to_string(&changed).unwrap(),
                changed.digest().unwrap(),
                IMPORT
            ],
        )
        .unwrap();
        assert!(
            jobs::load_published_auth_with(&tx, IMPORT, Some(REVISION), None).is_err(),
            "checksum-correct {pointer}"
        );
        tx.rollback().unwrap();
    }
    for fault in [
        "input",
        "input-future",
        "input-oversize",
        "job-request",
        "job-member",
        "job-unknown",
        "phase",
        "missing-job",
        "missing-input",
        "matching-old",
        "witness",
        "witness-type",
        "proof-oversize",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        let candidate = &queued.items[0].candidate_id;
        match fault {
            "input" | "input-future" | "input-oversize" => {
                let input: String = tx
                    .query_row(
                        "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
                        rusqlite::params![queued.job_id, candidate],
                        |row| row.get(0),
                    )
                    .unwrap();
                let changed = if fault == "input" {
                    format!("{input} ")
                } else if fault == "input-oversize" {
                    format!("{input}{}", " ".repeat(16 * 1024 * 1024))
                } else {
                    let mut value: serde_json::Value = serde_json::from_str(&input).unwrap();
                    value["version"] = json!(4);
                    value.to_string()
                };
                tx.execute(
                    "UPDATE job_items SET result_json=?1 WHERE job_id=?2 AND candidate_id=?3",
                    rusqlite::params![changed, queued.job_id, candidate],
                )
                .unwrap();
            }
            "job-request" | "job-member" | "job-unknown" => {
                let raw: String = tx
                    .query_row(
                        "SELECT json FROM jobs WHERE job_id=?1",
                        [&queued.job_id],
                        |row| row.get(0),
                    )
                    .unwrap();
                let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
                if fault == "job-request" {
                    value["requestId"] = json!("foreign");
                } else if fault == "job-member" {
                    value["items"] = json!([]);
                    value["itemCount"] = json!(0);
                    if jobs::item_store::uses_rows(&tx, &queued.job_id).unwrap() {
                        tx.execute(
                            "DELETE FROM job_item_states WHERE job_id=?1",
                            [&queued.job_id],
                        )
                        .unwrap();
                    }
                } else {
                    value["authorized"] = json!(true);
                }
                tx.execute(
                    "UPDATE jobs SET json=?1 WHERE job_id=?2",
                    rusqlite::params![value.to_string(), queued.job_id],
                )
                .unwrap();
            }
            "phase" => {
                tx.execute(
                    "UPDATE job_items SET phase='foreign' WHERE job_id=?1",
                    [&queued.job_id],
                )
                .unwrap();
            }
            "missing-job" => {
                tx.execute("DELETE FROM jobs WHERE job_id=?1", [&queued.job_id])
                    .unwrap();
            }
            "missing-input" => {
                tx.execute("DELETE FROM job_items WHERE job_id=?1", [&queued.job_id])
                    .unwrap();
            }
            "matching-old" => {
                tx.execute(
                    "UPDATE entries SET native_session_id='transcript' WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "witness" | "witness-type" => {
                let prefix = format!("registration_association_accepted:{}:%", queued.job_id);
                let sql = if fault == "witness" {
                    "DELETE FROM meta WHERE key LIKE ?1"
                } else {
                    "UPDATE meta SET value=zeroblob(64) WHERE key LIKE ?1"
                };
                assert_eq!(tx.execute(sql, [prefix]).unwrap(), 1);
            }
            "proof-oversize" => {
                tx.execute_batch("PRAGMA ignore_check_constraints=ON")
                    .unwrap();
                tx.execute(
                    "UPDATE registration_provenance SET json=?1 WHERE import_id=?2",
                    rusqlite::params![" ".repeat(32769), IMPORT],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            jobs::load_published_auth_with(&tx, IMPORT, Some(REVISION), None).is_err(),
            "accepted fault {fault}"
        );
        tx.rollback().unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints=OFF")
            .unwrap();
        assert!(catalog::get_entry(conn, IMPORT).unwrap().is_some());
    }
    assert!(jobs::load_published_auth_with(conn, IMPORT, Some(REVISION), None).is_ok());
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_sql_rollback() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let confirmed = confirmed_plan();
    let conn = open_catalog().unwrap();
    let before = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let old_receipts = receipts(&conn);
    let preview_path = jobs::find_preview_dir(&confirmed.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    let reader = conn.unchecked_transaction().unwrap();
    reader
        .query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, u64>(0))
        .unwrap();
    let requested = requested_job(&confirmed, "synthetic-association-real-SQL-rollback");
    let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
    let result = job_control::enqueue_confirmed_associations(requested.clone(), items);
    drop(reader);
    let error = result.unwrap_err();
    assert!(
        error.message.contains("locked"),
        "must reach a genuine SQLite COMMIT failure: {error:?}"
    );
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    assert!(
        !crate::session_migration::registration_provenance_store::verify_schema(&conn).unwrap()
    );
    assert_eq!(
        serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
        before
    );
    assert_eq!(receipts(&conn), old_receipts);
    let restored = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    assert!(!restored.retained_by_job);
    assert_eq!(restored.expires_unix, lease.expires_unix);
    assert_eq!(restored.file_hashes, lease.file_hashes);
    let retry = acceptance(&confirmed, &requested.request_id).unwrap();
    assert_eq!(retry.state, JobState::Queued);
    text(IMPORT, REVISION);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_stale_cas() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let confirmed = confirmed_plan();
    let conn = open_catalog().unwrap();
    let old_receipts = receipts(&conn);
    let profile: String = conn
        .query_row(
            "SELECT profile FROM sources WHERE installation_id=?1",
            [INSTALLATION],
            |row| row.get(0),
        )
        .unwrap();
    let job = requested_job(&confirmed, "synthetic-association-CAS-probe");
    let native = engine_commit::commit_items_for_plan(&confirmed, &job, false).unwrap()[0]
        .adapter
        .graph
        .native_session_id
        .clone();
    for fault in [
        "alias",
        "hidden",
        "revision",
        "source-alias",
        "source-host",
        "tombstone",
        "revision-digest",
        "proof",
    ] {
        let saved_profile = profile.clone();
        let new_native = native.clone();
        job_control::association_acceptance::test_after_preflight(move || {
            let conn = open_catalog().unwrap();
            match fault {
                "alias" => {
                    conn.execute(
                        "UPDATE entries SET alias='synthetic-after-review' WHERE import_id=?1",
                        [IMPORT],
                    )
                    .unwrap();
                }
                "hidden" => {
                    conn.execute("UPDATE entries SET hidden=1 WHERE import_id=?1", [IMPORT])
                        .unwrap();
                }
                "revision" => {
                    conn.execute(
                        "UPDATE entries SET current_revision=NULL WHERE import_id=?1",
                        [IMPORT],
                    )
                    .unwrap();
                }
                "source-alias" => {
                    let mut value: serde_json::Value =
                        serde_json::from_str(&saved_profile).unwrap();
                    value["aliases"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("synthetic-new-alias"));
                    conn.execute(
                        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
                        rusqlite::params![value.to_string(), INSTALLATION],
                    )
                    .unwrap();
                }
                "source-host" => {
                    conn.execute(
                        "UPDATE sources SET host='remote' WHERE installation_id=?1",
                        [INSTALLATION],
                    )
                    .unwrap();
                }
                "tombstone" => {
                    conn.execute("INSERT INTO tombstones(installation_id,provider,native_session_id,deleted_at) VALUES(?1,'cursor',?2,'synthetic')", rusqlite::params![INSTALLATION, new_native]).unwrap();
                }
                "revision-digest" => {
                    conn.execute("UPDATE revisions SET source_snapshot_id='foreign' WHERE import_id=?1 AND revision_id=?2", rusqlite::params![IMPORT, REVISION]).unwrap();
                }
                "proof" => {
                    use crate::session_migration::registration_provenance_store as store;
                    conn.execute_batch(store::SQL).unwrap();
                    conn.execute(
                        "INSERT INTO meta(key,value) VALUES(?1,'1')",
                        [store::VERSION_KEY],
                    )
                    .unwrap();
                    conn.execute("INSERT INTO registration_provenance(import_id,json,digest) VALUES(?1,'{}',?2)", rusqlite::params![IMPORT, "0".repeat(64)]).unwrap();
                }
                _ => unreachable!(),
            }
        });
        let request = requested_job(&confirmed, &format!("synthetic-CAS-{fault}"));
        let items = engine_commit::commit_items_for_plan(&confirmed, &request, false).unwrap();
        let result = job_control::enqueue_confirmed_associations(request.clone(), items);
        assert!(
            result.is_err(),
            "CAS must refuse {fault} after authenticated preflight"
        );
        assert!(jobs::get_job(&conn, &request.job_id).unwrap().is_none());
        assert_eq!(
            get_entry(&conn, IMPORT).unwrap().unwrap().native_session_id,
            "transcript"
        );
        assert_eq!(receipts(&conn), old_receipts);
        conn.execute(
            "UPDATE entries SET alias=NULL,hidden=0,current_revision=?1 WHERE import_id=?2",
            rusqlite::params![REVISION, IMPORT],
        )
        .unwrap();
        conn.execute(
            "UPDATE sources SET profile=?1,host='local' WHERE installation_id=?2",
            rusqlite::params![profile, INSTALLATION],
        )
        .unwrap();
        conn.execute("DELETE FROM tombstones WHERE installation_id=?1 AND provider='cursor' AND native_session_id=?2", rusqlite::params![INSTALLATION, native]).unwrap();
        if fault == "revision-digest" {
            let receipt = jobs::latest_receipt_for_revision(&conn, IMPORT, REVISION)
                .unwrap()
                .unwrap();
            conn.execute(
                "UPDATE revisions SET source_snapshot_id=?1 WHERE import_id=?2 AND revision_id=?3",
                rusqlite::params![receipt.snapshot_hash, IMPORT, REVISION],
            )
            .unwrap();
        }
        if fault == "proof" {
            conn.execute_batch("DROP TABLE registration_provenance")
                .unwrap();
            conn.execute(
                "DELETE FROM meta WHERE key=?1",
                [crate::session_migration::registration_provenance_store::VERSION_KEY],
            )
            .unwrap();
        }
    }
    text(IMPORT, REVISION);
    assert_eq!(
        acceptance(&confirmed, "synthetic-after-all-CAS-refusals")
            .unwrap()
            .state,
        JobState::Queued
    );
    text(IMPORT, REVISION);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_guards() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let confirmed = confirmed_plan();
    let conn = open_catalog().unwrap();
    let before = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let path = jobs::find_preview_dir(&confirmed.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = std::fs::read(path.join("lease.json")).unwrap();
    let lock = jobs::try_adoption_publication_lock().unwrap();
    let error = acceptance(&confirmed, "synthetic-publication-busy").unwrap_err();
    drop(lock);
    assert_eq!(error.code, ErrorCode::SourceBusy);
    assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), lease);
    assert_eq!(
        serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
        before
    );
    assert!(
        !crate::session_migration::registration_provenance_store::verify_schema(&conn).unwrap()
    );

    // Queue a genuine ordinary producer first, without touching the chosen
    // historical entry. Publish it between detached review and the real CAS.
    let probe = requested_job(&confirmed, "synthetic-collision-probe");
    let item = engine_commit::commit_items_for_plan(&confirmed, &probe, false)
        .unwrap()
        .remove(0);
    let reference = crate::session_migration::scan::candidate(&item.candidate_id).unwrap();
    let (ordinary_preview, _) = preview_reference(reference, None, None).unwrap();
    let ordinary = plan(vec![ordinary_preview], vec![]).unwrap();
    let producer = commit(
        &ordinary.plan_id,
        &ordinary.plan_hash,
        "synthetic-genuine-competing-producer",
    )
    .unwrap();
    job_control::association_acceptance::test_after_preflight(|| {
        assert_eq!(job_control::run_pending().unwrap(), 1);
    });
    let requested = requested_job(&confirmed, "synthetic-competing-publication");
    let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
    let rejected =
        job_control::enqueue_confirmed_associations(requested.clone(), items).unwrap_err();
    assert_eq!(rejected.code, ErrorCode::MappingConflict);
    assert!(jobs::get_job(&conn, &requested.job_id).unwrap().is_none());
    assert_eq!(
        jobs::get_job(&conn, &producer.job_id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    assert_eq!(
        list_entries(&conn).unwrap().len(),
        2,
        "collision must be an actually published ordinary entry"
    );
    assert_eq!(
        serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
        before
    );
    assert_eq!(std::fs::read(path.join("lease.json")).unwrap(), lease);
    assert!(
        !crate::session_migration::registration_provenance_store::verify_schema(&conn).unwrap()
    );
    text(IMPORT, REVISION);
}

pub(in crate::session_migration) fn association_crash_child(signal: PathBuf) {
    let root = crate::paths::app_data_root();
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("historical-association-acceptance-"));
    assert_plain(&root);
    require_seed(
        &explicit_path("GROK_IMPORT_HISTORICAL_SEED"),
        &explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE"),
    );
    assert_eq!(signal.parent(), Some(root.as_path()));
    let mode = std::env::var("GROK_TEST_PREVIEW_LIFECYCLE").unwrap();
    let publish_point = match mode.as_str() {
        "association-publication-prepared" => Some("prepared"),
        "association-publication-staged" => Some("staged"),
        "association-publication-after_rename" => Some("after_rename"),
        "association-publication-after_receipt_published" => Some("after_receipt_published"),
        "association-publication-linked" => Some("linked"),
        "association-publication-after_catalog_commit" => Some("after_catalog_commit"),
        _ => None,
    };
    assert!(
        publish_point.is_some()
            || matches!(
                mode.as_str(),
                "association-before-commit" | "association-after-commit"
            )
    );
    let before_commit = mode == "association-before-commit";
    reset_runtime();
    let confirmed = confirmed_plan();
    let requested = requested_job(&confirmed, "synthetic-real-association-lost-ACK");
    let items = engine_commit::commit_items_for_plan(&confirmed, &requested, false).unwrap();
    let preview_path = jobs::find_preview_dir(&confirmed.preview_ids[0])
        .unwrap()
        .unwrap();
    let original_lease = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    let report = json!({"job": requested, "previewId": confirmed.preview_ids[0],
        "originalLease": original_lease, "mode": mode, "pid": std::process::id()});
    let exit = move || {
        let temporary = signal.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&report).unwrap()).unwrap();
        std::fs::rename(temporary, signal).unwrap();
        std::process::exit(if publish_point.is_some() {
            77
        } else if before_commit {
            73
        } else {
            74
        });
    };
    if let Some(point) = publish_point {
        crate::session_migration::preview_lifecycle_tests::test_publish_point(point, exit);
    } else if before_commit {
        crate::session_migration::preview_lifecycle_tests::test_before_enqueue_commit(exit);
    } else {
        crate::session_migration::preview_lifecycle_tests::test_after_enqueue_commit(exit);
    }
    job_control::enqueue_confirmed_associations(requested, items).unwrap();
    if publish_point.is_some() {
        reset_runtime();
        job_control::run_pending().unwrap();
    }
    panic!("child must exit at its actual SQL COMMIT boundary");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_lost_ack() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let old = text(IMPORT, REVISION);
    let report = crate::session_migration::preview_lifecycle_tests::association_crash_report(&home);
    let requested: JobRecord = serde_json::from_value(report["job"].clone()).unwrap();
    let preview_id = report["previewId"].as_str().unwrap();
    let path = jobs::find_preview_dir(preview_id).unwrap().unwrap();
    assert!(job_control::preview_retention::pending());
    let accepted = job_control::enqueue_confirmed_associations(requested.clone(), vec![]).unwrap();
    assert_eq!(accepted.job_id, requested.job_id);
    assert_eq!(accepted.state, JobState::Queued);
    assert_eq!(text(IMPORT, REVISION), old);
    assert_eq!(jobs::cleanup_unreferenced_preview_dirs().unwrap(), 0);
    assert!(
        !job_control::preview_retention::pending(),
        "the original recovery owner must reconcile actual acceptance"
    );
    assert!(
        jobs::read_preview_lease(&path)
            .unwrap()
            .unwrap()
            .retained_by_job
    );
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), old);
    assert_eq!(
        job_control::enqueue_confirmed_associations(requested, vec![])
            .unwrap()
            .job_id,
        accepted.job_id
    );
    println!("ASSOCIATION_LOST_ACK_CHILD={report}");
}
