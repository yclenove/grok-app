//! Dispatch/refusal evidence, not a real accepted V3 producer positive control.
use super::*;
use crate::session_migration::{
    catalog, job_control, job_progress, jobs, test_home::IsolatedAppHome,
};
use rusqlite::Connection;
use uuid::{Uuid, Variant};

const SIGNED_JOB: &str = "abcdef12-1234-8567-8123-abcdef123456";
const LEGACY_JOB: &str = "abcdef12-1234-4567-8123-abcdef123456";

fn job(job_id: &str) -> JobRecord {
    JobRecord {
        job_id: job_id.into(),
        request_id: format!("synthetic-{job_id}"),
        plan_id: PLAN.into(),
        plan_hash: "a".repeat(64),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: CREATED.into(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    }
}

fn assert_closed<T>(result: Result<T, ImportError>) {
    let error = result.err().expect("new job must not enter legacy");
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(error.is_job_local());
}

fn import_tree() -> std::collections::BTreeMap<std::path::PathBuf, Option<String>> {
    let root = catalog::imports_dir();
    let mut pending = vec![root.clone()];
    let mut snapshot = std::collections::BTreeMap::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let is_dir = path.is_dir();
            let digest = if is_dir {
                pending.push(path.clone());
                None
            } else {
                Some(hash::sha256_bytes(&std::fs::read(&path).unwrap()))
            };
            snapshot.insert(path.strip_prefix(&root).unwrap().to_path_buf(), digest);
        }
    }
    snapshot
}

#[test]
fn publication_job_id_generation_is_random_canonical_and_protocol_bound() {
    let mut seen = std::collections::HashSet::new();
    for protocol in [
        PublicationProtocol::Legacy,
        PublicationProtocol::SignedOutcomeV1,
    ] {
        for _ in 0..128 {
            let generated = protocol.new_job_id();
            let parsed = Uuid::parse_str(&generated).unwrap();
            assert_eq!(parsed.to_string(), generated);
            assert_eq!(parsed.get_variant(), Variant::RFC4122);
            assert_eq!(
                parsed.get_version_num(),
                if protocol == PublicationProtocol::Legacy {
                    4
                } else {
                    8
                }
            );
            assert_eq!(
                PublicationProtocol::for_job_id(&generated).unwrap(),
                protocol
            );
            protocol.require_job_id(&generated).unwrap();
            assert!(seen.insert(generated));
        }
    }
    assert_eq!(seen.len(), 256);
}

#[test]
fn publication_job_id_dispatch_preserves_opaque_legacy_and_refuses_new_spellings() {
    for legacy in [
        "job-undo",
        "job-ref",
        LEGACY_JOB,
        &LEGACY_JOB.to_uppercase(),
    ] {
        assert_eq!(
            PublicationProtocol::for_job_id(legacy).unwrap(),
            PublicationProtocol::Legacy
        );
    }
    assert_eq!(
        PublicationProtocol::for_job_id(SIGNED_JOB).unwrap(),
        PublicationProtocol::SignedOutcomeV1
    );
    for alias in [
        SIGNED_JOB.to_uppercase(),
        SIGNED_JOB.replace('-', ""),
        format!("{{{SIGNED_JOB}}}"),
        format!("urn:uuid:{SIGNED_JOB}"),
    ] {
        let error = PublicationProtocol::for_job_id(&alias).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale);
        assert!(error.is_job_local());
    }
    for version in 0..=15 {
        if matches!(version, 4 | 8) {
            continue;
        }
        let id = format!("abcdef12-1234-{version:x}567-8123-abcdef123456");
        let error = PublicationProtocol::for_job_id(&id).unwrap_err();
        assert_eq!(
            error.code,
            if version > 8 {
                ErrorCode::ImportSchemaTooNew
            } else {
                ErrorCode::SchemaUnsupported
            }
        );
        assert!(error.is_job_local());
    }
    for variant in ['0', '7', 'c', 'f'] {
        let id = format!("abcdef12-1234-8567-{variant}123-abcdef123456");
        assert_eq!(
            PublicationProtocol::for_job_id(&id).unwrap_err().code,
            ErrorCode::SchemaUnsupported
        );
    }
}

#[test]
fn publication_job_id_plan_mismatch_cannot_supply_a_grant() {
    for (protocol, id) in [
        (PublicationProtocol::Legacy, SIGNED_JOB),
        (PublicationProtocol::SignedOutcomeV1, LEGACY_JOB),
        (PublicationProtocol::SignedOutcomeV1, "job-undo"),
    ] {
        let error = protocol.require_job_id(id).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale);
        assert!(error.is_job_local());
    }
    let legacy_plan = decode_record(&record(PublicationProtocol::Legacy, false)).unwrap();
    assert_eq!(
        engine::commit_items_for_plan(&legacy_plan, &job(SIGNED_JOB), false)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
}

#[test]
fn publication_job_id_missing_all_facts_refuses_before_body_or_legacy_schema_reads() {
    // Only a locator exists: no meta, input, plan or proof tables. Deliberately
    // invalid body UTF8 proves the reader never attempts to decode it.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE jobs(job_id TEXT PRIMARY KEY,json TEXT);")
        .unwrap();
    for id in [SIGNED_JOB, "abcdef12-1234-9567-8123-abcdef123456"] {
        conn.execute("INSERT INTO jobs VALUES(?1,CAST(X'ff' AS TEXT))", [id])
            .unwrap();
    }
    let before = conn.total_changes();
    {
        let tx = conn.transaction().unwrap();
        assert_closed(jobs::get_job(&tx, SIGNED_JOB));
        let future = jobs::get_job(&tx, "abcdef12-1234-9567-8123-abcdef123456").unwrap_err();
        assert_eq!(future.code, ErrorCode::ImportSchemaTooNew);
        assert!(future.is_job_local());
        // Not-found remains not-found, even for an unsupported locator.
        assert!(jobs::get_job(&tx, "abcdef12-1234-8567-8123-abcdef123457")
            .unwrap()
            .is_none());
        assert!(jobs::get_job(&tx, "abcdef12-1234-a567-8123-abcdef123457")
            .unwrap()
            .is_none());
    }
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn publication_job_id_unregistered_enqueue_cannot_mint_acceptance() {
    let _home = IsolatedAppHome::acquire("publication-job-enqueue");
    let conn = catalog::open_catalog().unwrap();
    let before = conn.total_changes();
    assert_closed(job_control::enqueue(job(SIGNED_JOB), vec![]));
    assert!(jobs::get_job(&conn, SIGNED_JOB).unwrap().is_none());
    let accepted: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM meta WHERE key LIKE 'publication_acceptance:%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(accepted, 0);
    assert_eq!(conn.total_changes(), before);
}

#[test]
fn publication_job_id_unaccepted_signed_read_state_cancel_retry_and_undo_remain_closed() {
    let _home = IsolatedAppHome::acquire("publication-job-dispatch");
    let conn = catalog::open_catalog().unwrap();
    let queued = job(SIGNED_JOB);
    jobs::save_job(&conn, &queued).unwrap();
    let before = conn.total_changes();
    let raw: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [SIGNED_JOB],
            |row| row.get(0),
        )
        .unwrap();
    assert_closed(jobs::get_job(&conn, SIGNED_JOB));
    assert_closed(jobs::find_job_by_request(&conn, &queued.request_id));
    assert_closed(engine::job_items_page(SIGNED_JOB, None, Some(1)));
    assert_closed(job_control::change_job(&conn, SIGNED_JOB, |_| {
        panic!("no state callback may run")
    }));
    assert_closed(job_progress::request_cancel(&conn, SIGNED_JOB));
    assert_closed(job_progress::cancel_unstarted_queued(&conn, SIGNED_JOB));
    assert_closed(job_control::retry(SIGNED_JOB));
    assert_closed(jobs::undo_job(&conn, SIGNED_JOB));
    let after: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [SIGNED_JOB],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, after);
    assert_eq!(conn.total_changes(), before);
    let rows: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM meta WHERE key LIKE 'job_entry_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        rows, 0,
        "no compatibility repair or signed fact is synthesized"
    );
}

#[test]
fn publication_job_id_legacy_entry_keeps_existing_authentication_and_not_found_semantics() {
    let _home = IsolatedAppHome::acquire("publication-job-legacy");
    let conn = catalog::open_catalog().unwrap();
    for id in [LEGACY_JOB, "job-undo"] {
        jobs::save_job(&conn, &job(id)).unwrap();
        assert_eq!(jobs::get_job(&conn, id).unwrap().unwrap().job_id, id);
    }
    assert!(jobs::get_job(&conn, "missing-job").unwrap().is_none());
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,'{}')",
        [format!("job_entry_scope:{LEGACY_JOB}")],
    )
    .unwrap();
    let before = conn.total_changes();
    let error = jobs::get_job(&conn, LEGACY_JOB).unwrap_err();
    assert_eq!(error.code, ErrorCode::HistoryAccessUnavailable);
    assert!(error.is_job_local());
    assert_eq!(conn.total_changes(), before);
    // UUIDv4 and a readable body never bypass a damaged acceptance scope.
    assert_eq!(
        jobs::get_job(&conn, "job-undo").unwrap().unwrap().job_id,
        "job-undo"
    );
}

#[test]
fn publication_job_id_publisher_refuses_before_preview_store_or_archive_io() {
    let _home = IsolatedAppHome::acquire("publication-job-publisher");
    let conn = catalog::open_catalog().unwrap();
    let adapter = crate::session_migration::adapters::generic::normalize_bytes(
        br#"[{"role":"user","content":"synthetic"}]"#,
        "synthetic.json",
    )
    .unwrap();
    let input = jobs::CommitItem {
        candidate_id: "synthetic-candidate".into(),
        installation_id: "synthetic-installation".into(),
        provider: "generic-file".into(),
        root_path: "synthetic-offline".into(),
        adapter,
        prepared: None,
        options: json!({}),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        selection_set: Default::default(),
        identity_association: None,
    };
    // Prepare an absent job against a real SQL schema, not a test-only crypto
    // constructor. It grants nothing, even against the complete catalog.
    let preparation = Connection::open_in_memory().unwrap();
    preparation
        .execute_batch("CREATE TABLE jobs(job_id TEXT PRIMARY KEY)")
        .unwrap();
    let crypto =
        crate::session_migration::publication_crypto::JobCrypto::prepare(&preparation, SIGNED_JOB)
            .unwrap();
    let before = import_tree();
    let changes = conn.total_changes();
    assert_closed(jobs::publish_item(&conn, SIGNED_JOB, &input, &|| {
        panic!("no cancel callback")
    }));
    assert_closed(jobs::publish_for_worker(
        &conn,
        SIGNED_JOB,
        0,
        &input,
        &|| panic!("no worker callback"),
        &crypto,
    ));
    assert_eq!(import_tree(), before);
    assert_eq!(conn.total_changes(), changes);
}
