// Exercise the production command boundary without constructing a WebView or Agent.
#[test]
fn native_creation_notifies_only_after_the_index_journal_and_handoff_are_visible() {
    let home = IsolatedAppHome::acquire("continue-notify-published");
    let prepared = draft(&home);
    let mut notifications = Vec::new();
    let response = tauri::async_runtime::block_on(crate::commands::create_import_continuation(
        prepared.draft_id.clone(),
        prepared.draft_hash.clone(),
        "notify-published".into(),
        Some("Reviewed synthetic handoff".into()),
        |sid| {
            let rows = store::load_sessions_snapshot().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, sid);
            let side = continuation::read_sidecar(sid).unwrap();
            assert_eq!(side.state, DispatchState::Prepared);
            assert!(side.dispatch_id.is_none());
            assert!(side.agent_session_id.is_none());
            let dir = continuation::sidecar_path(sid)
                .parent()
                .unwrap()
                .to_path_buf();
            assert_eq!(
                std::fs::read_to_string(dir.join("messages.json")).unwrap(),
                "[]"
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("handoff_draft.txt")).unwrap(),
                "Reviewed synthetic handoff"
            );
            notifications.push(sid.to_owned());
        },
    ))
    .unwrap();
    assert_eq!(
        notifications,
        vec![response["appSessionId"].as_str().unwrap()]
    );
}

#[test]
fn native_creation_failure_never_notifies_or_invents_a_catalog_row() {
    let home = IsolatedAppHome::acquire("continue-notify-failure");
    let prepared = draft(&home);
    let mut notified = false;
    let response = tauri::async_runtime::block_on(crate::commands::create_import_continuation(
        prepared.draft_id,
        "unreviewed-hash".into(),
        "notify-failure".into(),
        None,
        |_| notified = true,
    ));
    assert!(response.is_err());
    assert!(!notified);
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
}

#[test]
fn native_creation_recovery_and_reply_retry_notify_one_existing_identity() {
    let home = IsolatedAppHome::acquire("continue-notify-retry");
    let prepared = draft(&home);
    FAIL_AT.with(|fail| *fail.borrow_mut() = Some("intent"));
    let first = create(&prepared, "notify-retry");
    FAIL_AT.with(|fail| *fail.borrow_mut() = None);
    assert!(first.is_err());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    let mut notifications = Vec::new();
    let changed_input =
        tauri::async_runtime::block_on(crate::commands::create_import_continuation(
            prepared.draft_id.clone(),
            prepared.draft_hash.clone(),
            "notify-retry".into(),
            Some("must not replace the durable intent".into()),
            |sid| notifications.push(sid.to_owned()),
        ));
    assert!(changed_input.unwrap_err().contains("PLAN_STALE"));
    assert!(notifications.is_empty());
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    let mut ids = Vec::new();
    for _ in 0..2 {
        let response = tauri::async_runtime::block_on(crate::commands::create_import_continuation(
            prepared.draft_id.clone(),
            prepared.draft_hash.clone(),
            "notify-retry".into(),
            Some("edited input".into()),
            |sid| {
                assert!(store::load_sessions_snapshot()
                    .unwrap()
                    .iter()
                    .any(|row| row.id == sid));
                notifications.push(sid.to_owned());
            },
        ))
        .unwrap();
        ids.push(response["appSessionId"].as_str().unwrap().to_owned());
    }
    assert_eq!(notifications, ids);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(store::load_sessions_snapshot().unwrap().len(), 1);
    let dir = continuation::sidecar_path(&ids[0])
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        std::fs::read_to_string(dir.join("handoff_draft.txt")).unwrap(),
        "edited input"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("messages.json")).unwrap(),
        "[]"
    );
}
