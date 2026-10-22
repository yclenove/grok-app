// The JSON writer owns publication and consumption of its creation draft.
fn cleanup_direct(prepared: &HandoffDraft, request: &str) -> Result<String, ImportError> {
    run(
        Some(prepared),
        &prepared.draft_id,
        &prepared.draft_hash,
        request,
        Some("edited input".into()),
        Some(ContinuationSidecar {
            import_id: prepared.import_id.clone(),
            revision_id: prepared.revision_id.clone(),
            branch_leaf: prepared.leaf.clone(),
            allowed_selection_hash: prepared.selection.hash(),
            handoff_hash: prepared.draft_hash.clone(),
            request_id: request.into(),
            grant_token: uuid::Uuid::new_v4().to_string(),
            runtime_capability: prepared.runtime_capability.clone(),
            ..Default::default()
        }),
    )
}

fn cleanup_files(sid: &str) -> Vec<Vec<u8>> {
    [
        "messages.json",
        "handoff_draft.txt",
        "import_continuation.json",
    ]
    .iter()
    .map(|name| std::fs::read(continuation::sidecar_path(sid).with_file_name(name)).unwrap())
    .collect()
}

#[test]
fn cleanup_is_consumed_before_creation_releases_the_native_writer() {
    let home = IsolatedAppHome::acquire("continue-cleanup-boundary");
    let prepared = draft(&home);
    let sid = cleanup_direct(&prepared, "cleanup-boundary").unwrap();
    let conn = catalog::open_catalog().unwrap();
    assert!(catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .is_none());
    assert!(
        intents::load(&conn, "cleanup-boundary")
            .unwrap()
            .unwrap()
            .published
    );
    let files = cleanup_files(&sid);
    assert_eq!(cleanup_direct(&prepared, "cleanup-boundary").unwrap(), sid);
    assert_eq!(cleanup_files(&sid), files);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    assert_eq!(
        continuation::read_sidecar(&sid).unwrap().state,
        DispatchState::Prepared
    );
}

#[test]
fn cleanup_abort_rolls_back_publication_receipt_and_retry_keeps_complete_native_files() {
    let home = IsolatedAppHome::acquire("continue-cleanup-atomic");
    let prepared = draft(&home);
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_draft_cleanup BEFORE DELETE ON drafts
        BEGIN SELECT RAISE(ABORT, 'synthetic cleanup failure'); END;",
    )
    .unwrap();
    let failed = create(&prepared, "cleanup-atomic").unwrap_err();
    assert!(failed.message.contains("synthetic cleanup failure"));
    let intent = intents::load(&conn, "cleanup-atomic").unwrap().unwrap();
    assert!(
        !intent.published,
        "the receipt and draft must commit together"
    );
    assert!(catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .is_some());
    let sid = intent.meta.id;
    assert_eq!(store::load_sessions_snapshot().unwrap()[0].id, sid);
    let files = cleanup_files(&sid);
    conn.execute_batch("DROP TRIGGER fail_draft_cleanup")
        .unwrap();
    assert_eq!(create(&prepared, "cleanup-atomic").unwrap(), sid);
    assert!(
        intents::load(&conn, "cleanup-atomic")
            .unwrap()
            .unwrap()
            .published
    );
    assert!(catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .is_none());
    assert_eq!(cleanup_files(&sid), files);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
}

fn cleanup_leftover(legacy: bool) {
    let home = IsolatedAppHome::acquire("continue-cleanup-leftover");
    let prepared = draft(&home);
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch("CREATE TEMP TABLE original_draft AS SELECT * FROM drafts")
        .unwrap();
    let sid = create(&prepared, "cleanup-leftover").unwrap();
    let files = cleanup_files(&sid);
    conn.execute_batch("INSERT INTO drafts SELECT * FROM original_draft")
        .unwrap();
    if legacy {
        conn.execute(
            "DELETE FROM continuation_intents WHERE request_id=?1",
            ["cleanup-leftover"],
        )
        .unwrap();
    }
    assert_eq!(cleanup_direct(&prepared, "cleanup-leftover").unwrap(), sid);
    assert!(catalog::load_draft_json(&conn, &prepared.draft_id)
        .unwrap()
        .is_none());
    assert_eq!(cleanup_files(&sid), files);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
}

#[test]
fn cleanup_published_receipt_consumes_leftover_draft_before_unlock() {
    cleanup_leftover(false);
}

#[test]
fn cleanup_legacy_receipt_consumes_leftover_draft_before_unlock() {
    cleanup_leftover(true);
}
