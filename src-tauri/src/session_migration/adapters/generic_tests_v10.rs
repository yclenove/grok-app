#[test]
fn prepared_parser_rejects_duplicate_event_ids_and_cleans_staging() {
    let home =
        crate::session_migration::test_home::IsolatedAppHome::acquire("generic-duplicate-id");
    let options = serde_json::json!({"rawRetention":"normalized-only"});
    for (name, raw) in [
        (
            "duplicate-array.json",
            r#"[{"id":"same","role":"user","content":"first"},{"id":"same","role":"assistant","content":"second"}]"#,
        ),
        (
            "duplicate-envelope.json",
            r#"{"messages":[{"id":"same","role":"user","content":"first"},{"id":"same","role":"assistant","content":"second"}]}"#,
        ),
    ] {
        let source = home.path.join(name);
        std::fs::write(&source, raw).unwrap();
        let error = prepare_file_in_root(&source, None, name, &options, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::FormatUnsupported, "{name}");
        assert_eq!(
            error.message, "prepared revision contains duplicate event identity",
            "{name}"
        );
        let previews = crate::session_migration::catalog::staging_dir().join("previews");
        assert!(
            !previews.exists() || std::fs::read_dir(previews).unwrap().next().is_none(),
            "{name} must not leave a prepared preview"
        );
    }
}

#[test]
fn relative_locator_keeps_same_named_files_distinct() {
    let tmp = std::env::temp_dir().join(format!("generic-rel-{}", uuid::Uuid::new_v4()));
    let a = tmp.join("a");
    let b = tmp.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let content = r#"[{"role":"user","content":"same"}]"#;
    std::fs::write(a.join("chat.json"), content).unwrap();
    std::fs::write(b.join("chat.json"), content).unwrap();
    let ca = discovery::file_candidate(Provider::GenericFile, &a.join("chat.json"), &tmp).unwrap();
    let cb = discovery::file_candidate(Provider::GenericFile, &b.join("chat.json"), &tmp).unwrap();
    assert_ne!(ca.native_session_id, cb.native_session_id);
    let na = normalize_file_in_root(&a.join("chat.json"), Some(&tmp)).unwrap();
    let nb = normalize_file_in_root(&b.join("chat.json"), Some(&tmp)).unwrap();
    assert_eq!(na.graph.native_session_id, ca.native_session_id);
    assert_eq!(nb.graph.native_session_id, cb.native_session_id);
    assert_ne!(na.graph.native_session_id, nb.graph.native_session_id);
    let file = a.join("chat.json");
    let single = discover(&file).unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].native_session_id, "chat.json");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn identical_text_different_ids_stay_two_sessions() {
    let tmp = std::env::temp_dir().join(format!("generic-d05-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let content = r#"[{"role":"user","content":"same text"}]"#;
    std::fs::write(tmp.join("one.json"), content).unwrap();
    std::fs::write(tmp.join("two.json"), content).unwrap();
    let found = discover(&tmp).unwrap();
    assert_eq!(found.len(), 2);
    let ids: std::collections::BTreeSet<_> =
        found.iter().map(|candidate| candidate.native_session_id.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(found[0].source_revision, found[1].source_revision);
    let _ = std::fs::remove_dir_all(&tmp);
}
