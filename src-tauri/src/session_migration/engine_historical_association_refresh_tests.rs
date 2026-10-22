//! G3: public Undo followed by genuine changed-body ordinary refresh.
//! Only a new disposable source copy is edited; seed/archive authority is never
//! manufactured through SQL or by rewriting an accepted receipt.
use super::*;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_undo_changed_body_refresh_keeps_canonical_entry() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let (_, association) = publish("synthetic-association-G3-before-refresh");
    let associated_revision = association.items[0].revision_id.as_deref().unwrap();
    let canonical = get_entry(&conn, IMPORT).unwrap().unwrap();
    let proof = proof_bytes(&conn);
    let old_hashes = immutable_hashes(REVISION);
    let association_hashes = immutable_hashes(associated_revision);
    let before_refresh_receipts = receipts(&conn);
    let old_text = text(IMPORT, REVISION);
    let undone = engine::undo_import_job(&association.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert_eq!(
        get_entry(&conn, IMPORT)
            .unwrap()
            .unwrap()
            .current_revision
            .as_deref(),
        Some(REVISION)
    );

    // Explicitly relocate the isolated catalog's registered PRIMARY through its
    // normal domain API. The relative transcript locator and installation stay
    // the same; an associated root would intentionally have a different scope.
    let fixture = explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE");
    let source_root = home.path.join("G3 changed source");
    std::fs::create_dir(&source_root).unwrap();
    let copied_source = source_root.join(fixture.file_name().unwrap());
    std::fs::copy(&fixture, &copied_source).unwrap();
    require_hash(&copied_source, FIXTURE_HASH);
    let changed = std::fs::read_to_string(&copied_source)
        .unwrap()
        .replace("hello one", "G3_CHANGED_BODY_新正文");
    assert!(changed.contains("G3_CHANGED_BODY_新正文") && !changed.contains("hello one"));
    std::fs::write(&copied_source, changed).unwrap();
    let changed_hash = hash::sha256_file(&copied_source).unwrap();
    assert_ne!(changed_hash, FIXTURE_HASH);
    let source = sources::get(&conn, INSTALLATION).unwrap();
    let relocated = sources::register(
        &conn,
        source.provider,
        &source_root,
        Some(INSTALLATION),
        &source.profile,
    )
    .unwrap();
    assert_eq!(relocated.source_id, INSTALLATION);
    assert!(sources::registered_paths(&conn, &relocated)
        .unwrap()
        .iter()
        .any(|root| sources::same_location(root, &source.root_path)));
    reset_runtime();
    let started = crate::session_migration::scan::begin(vec![ScanSourcePath {
        provider: "cursor".into(),
        path: relocated.root_path.clone(),
        source_id: Some(INSTALLATION.into()),
        location_id: None,
        host: Some("local".into()),
        profile: relocated.profile,
    }])
    .unwrap();
    crate::session_migration::scan::run(&started.scan_id).unwrap();
    let scanned = scan_pump(&started.scan_id, started.epoch).unwrap();
    assert!(scanned.complete && scanned.issues.is_empty());
    assert_eq!(scanned.candidates.len(), 1);
    let candidate = &scanned.candidates[0];
    assert_eq!(candidate.native_session_id, canonical.native_session_id);
    let (preview_id, details) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert_eq!(details.event_count, 2);
    let ordinary = plan(vec![preview_id], vec![]).unwrap();
    assert!(ordinary.identity_associations.is_empty());
    let queued = engine::commit(
        &ordinary.plan_id,
        &ordinary.plan_hash,
        "synthetic-association-G3-ordinary-refresh",
    )
    .unwrap();
    let raw: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let input: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(input["version"], 2);
    assert!(input
        .get("identityAssociation")
        .is_none_or(serde_json::Value::is_null));
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let refreshed = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(refreshed.state, JobState::Completed);
    assert_eq!(refreshed.items.len(), 1);
    assert_eq!(refreshed.items[0].phase, "committed");
    assert_ne!(refreshed.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(refreshed.items[0].import_id.as_deref(), Some(IMPORT));
    let revision = refreshed.items[0].revision_id.as_deref().unwrap();
    assert_ne!(revision, REVISION);
    assert_ne!(revision, associated_revision);
    let receipt = jobs::latest_receipt(&conn, &refreshed.job_id, &refreshed.items[0].candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.phase, "committed");
    assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
    assert!(!receipt.created_entry);
    let refreshed_hashes = immutable_hashes(revision);
    let all_receipts = receipts(&conn);
    for previous in &before_refresh_receipts {
        assert!(all_receipts.contains(previous));
    }
    let check_preserved = || {
        let entry = get_entry(&conn, IMPORT).unwrap().unwrap();
        assert_eq!(entry.current_revision.as_deref(), Some(revision));
        assert!(!entry.hidden);
        assert_eq!(entry.native_session_id, canonical.native_session_id);
        assert_eq!(entry.installation_id, canonical.installation_id);
        assert_eq!(list_entries(&conn).unwrap().len(), 1);
        let count: u64 = conn
            .query_row(
                "SELECT count(*) FROM revisions WHERE import_id=?1",
                [IMPORT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
        let (events, cursor, total) =
            engine::events_page(IMPORT, Some(revision), None, Some(20), None).unwrap();
        assert!(cursor.is_none());
        assert_eq!(total, 2);
        assert_eq!(events.len(), 2);
        let encoded = serde_json::to_string(&events).unwrap();
        assert!(encoded.contains("G3_CHANGED_BODY_新正文"));
        assert!(!encoded.contains("hello one"));
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(text(IMPORT, associated_revision), old_text);
        assert_eq!(immutable_hashes(REVISION), old_hashes);
        assert_eq!(immutable_hashes(associated_revision), association_hashes);
        assert_eq!(immutable_hashes(revision), refreshed_hashes);
        assert_eq!(receipts(&conn), all_receipts);
        assert_eq!(proof_bytes(&conn), proof);
        assert_eq!(hash::sha256_file(&copied_source).unwrap(), changed_hash);
    };
    check_preserved();
    reset_runtime();
    check_preserved();
    // A completed association Undo cannot rewind a later ordinary revision.
    let repeated = engine::undo_import_job(&association.job_id).unwrap();
    assert_eq!(repeated.items[0].phase, "undone");
    check_preserved();
    // The original still-committed creator must explicitly keep the later
    // revision, not hide the canonical entry or reset it to the old archive.
    let original = engine::undo_import_job(OLD_JOB).unwrap();
    assert_eq!(original.items[0].phase, "conflict");
    assert_eq!(
        original.items[0].error.as_deref(),
        Some("later revision kept")
    );
    check_preserved();
    assert_eq!(
        engine::commit(
            &ordinary.plan_id,
            &ordinary.plan_hash,
            "synthetic-association-G3-ordinary-refresh"
        )
        .unwrap()
        .job_id,
        refreshed.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    check_preserved();
    require_seed(&explicit_path("GROK_IMPORT_HISTORICAL_SEED"), &fixture);
    println!("ASSOCIATION_UNDO_CHANGED_REFRESH={{\"canonicalEntries\":1,\"revisions\":3,\"changedBody\":true,\"ordinaryInputVersion\":2,\"oldArchivesReadable\":true,\"laterRevisionKept\":true,\"retryNoRepublish\":true}}");
}
