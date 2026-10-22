use super::*;

#[test]
fn empty_query_yields_none() {
    let msgs = [("user", "hello world")];
    assert!(match_messages("", msgs).is_none());
    assert!(match_messages("   ", msgs).is_none());
}

#[test]
fn case_insensitive_substring() {
    let msgs = [
        ("user", "Please fix the Doctor reset button"),
        ("assistant", "Sure, I will patch it."),
        ("system", "doctor ignore me"),
    ];
    let (n, snip) = match_messages("doctor", msgs).expect("hit");
    // system role ignored; only user matches
    assert_eq!(n, 1);
    assert!(snip.to_lowercase().contains("doctor"));
}

#[test]
fn counts_multiple_message_hits() {
    let msgs = [
        ("user", "alpha beta"),
        ("assistant", "reply without key"),
        ("user", "gamma beta again"),
        ("assistant", "BETA uppercase"),
    ];
    let (n, _) = match_messages("beta", msgs).expect("hit");
    assert_eq!(n, 3);
}

#[test]
fn snippet_truncates_with_ellipsis() {
    let long_prefix = "word ".repeat(40);
    let content = format!("{long_prefix}TARGET rest of the message body continues here");
    let lower = content.to_lowercase();
    let idx = lower.find("target").unwrap();
    let snip = make_snippet(&content, idx, "TARGET".len());
    assert!(snip.contains("TARGET"));
    assert!(snip.starts_with('…'));
    assert!(snip.chars().count() <= SNIPPET_MAX + 1);
}

#[test]
fn unicode_safe_snippet() {
    let content = "你好世界测试内容 TARGET 后面还有更多中文内容用于截断";
    let idx = content.find("TARGET").unwrap();
    let snip = make_snippet(content, idx, 6);
    assert!(snip.contains("TARGET"));
    assert!(!snip.is_empty());
}

#[test]
fn search_sessions_empty_query() {
    let result = search_sessions("", 10);
    assert!(result.hits.is_empty());
    assert_eq!(result.imported_history_error, None);
}

fn seed_native_body_hits(query: &str, count: usize) {
    for index in 0..count {
        let session =
            crate::store::create_session(None, Some(format!("native-search-{index}")), false)
                .unwrap();
        crate::store::replace_messages(
            &session.id,
            &[crate::store::ChatMessageStored {
                id: format!("message-{index}"),
                role: "user".into(),
                content: format!("{query} native body {index}"),
                thought: None,
                created_at: chrono::Utc::now(),
                is_error: false,
                attachments: None,
                marker: None,
            }],
        )
        .unwrap();
    }
}

#[test]
fn search_without_import_catalog_does_not_create_one() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-no-catalog");
    let catalog = crate::session_migration::catalog::catalog_path();
    assert!(!catalog.exists());

    let result = search_sessions("no-catalog-query", 10);

    assert!(result.hits.is_empty());
    assert_eq!(result.imported_history_error, None);
    assert!(
        !catalog.exists(),
        "a read-only global search must not initialize the import catalog"
    );
    let _ = home;
}

#[test]
fn imported_hit_survives_native_limit_saturation() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-fair-merge");
    let query = "fair-merge-needle";
    seed_native_body_hits(query, 20);
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let import_id = seed_imported_body(
        &conn,
        0,
        "2026-01-01T00:00:00Z",
        query,
        "body does not need to match",
        false,
    );
    drop(conn);

    let result = search_sessions(query, 20);

    assert_eq!(result.hits.len(), 20);
    assert!(
        result
            .hits
            .iter()
            .any(|hit| hit.id == import_id && hit.kind == "importedHistory"),
        "a full native result set must not starve imported history: {:?}",
        result.hits
    );
    assert_eq!(result.imported_history_error, None);
    let _ = home;
}

#[test]
fn imported_error_is_visible_when_native_limit_is_saturated() {
    use crate::session_migration::errors::ErrorCode;

    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-fair-error");
    let query = "fair-error-needle";
    seed_native_body_hits(query, 20);
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    conn.execute_batch("DROP TABLE body_search_chunks;")
        .unwrap();
    drop(conn);

    let result = search_sessions(query, 20);

    assert_eq!(result.hits.len(), 20);
    assert_eq!(
        result.imported_history_error,
        Some(ErrorCode::SchemaUnsupported),
        "native saturation must not suppress imported-history failures"
    );
    let _ = home;
}

#[test]
fn imported_catalog_open_error_is_returned_as_code_only() {
    use crate::session_migration::errors::ErrorCode;

    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-schema");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    conn.execute_batch("DROP TABLE body_search_chunks;")
        .unwrap();
    drop(conn);

    let result = search_sessions("needle", 10);
    assert!(result.hits.is_empty());
    assert_eq!(
        result.imported_history_error,
        Some(ErrorCode::SchemaUnsupported)
    );
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["importedHistoryError"], "SCHEMA_UNSUPPORTED");
    assert!(json.get("message").is_none());
    assert!(!json
        .to_string()
        .contains(&home.path.to_string_lossy().to_string()));
}

#[test]
fn imported_catalog_hit_is_imported_history_not_native_session() {
    let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("u06-search");
    use crate::session_migration::catalog::open_catalog;
    use crate::session_migration::jobs::{publish_item, CommitItem};
    use crate::session_migration::types::{
        AdapterOutput, ContentBlock, EventBase, ImportOutcome, ImportedEvent, Inventory,
        MessageRole, NormalizedGraph, Provider as Prov, SourceEvidence, TimeOrigin,
    };
    let conn = open_catalog().unwrap();
    let adapter = AdapterOutput {
        evidence: SourceEvidence {
            provider: Prov::GenericFile,
            source_version: None,
            schema_fingerprint: "t".into(),
            adapter_version: "1.0.0".into(),
            platform: "test".into(),
            artifacts: vec![],
        },
        graph: NormalizedGraph {
            events: vec![ImportedEvent::Message {
                base: EventBase {
                    id: "u1".into(),
                    ordinal: 0,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: MessageRole::User,
                blocks: vec![ContentBlock::text("unique-imported-keep-compat")],
                complete: true,
            }],
            relations: vec![],
            active_leaf: Some("u1".into()),
            native_session_id: "native-src".into(),
            title: Some("Imported keep".into()),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues: vec![],
        assets: vec![],
        outcome: ImportOutcome::Complete,
    };
    let published = publish_item(
        &conn,
        "job-u06",
        &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
            candidate_id: "c-u06".into(),
            installation_id: "inst-u06".into(),
            provider: "generic-file".into(),
            root_path: "/tmp".into(),
            adapter,
            prepared: None,
            options: serde_json::json!({}),
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            identity_association: None,
            selection_set: crate::session_migration::selection::SelectionSet::default(),
        }),
        &|| false,
    )
    .unwrap();
    let import_id = published.import_id.clone().unwrap();
    let hits = search_sessions("unique-imported-keep-compat", 10).hits;
    assert!(
        hits.iter()
            .any(|h| h.id == import_id && h.kind == "importedHistory"),
        "global search must identify importedHistory, not a native session: {hits:?}"
    );
    assert!(hits.iter().all(|h| h.id != "native-src"));
    let err = crate::session_migration::reject_imported_history_id(Some(&import_id)).unwrap_err();
    assert!(
        err.starts_with("IMPORTED_HISTORY_READ_ONLY"),
        "imp: must not be feedable to native session APIs: {err}"
    );
}

#[test]
fn imported_catalog_fixture_restores_inherited_home_child() {
    let Some(root) = std::env::var_os("GROK_SEARCH_ENV_RESTORE_CHILD") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("grok-search-env-restore-"));
    assert_eq!(
        std::env::var_os("GROK_APP_HOME"),
        Some(root.clone().into_os_string())
    );
    imported_catalog_hit_is_imported_history_not_native_session();
    assert_eq!(
        std::env::var_os("GROK_APP_HOME"),
        Some(root.clone().into_os_string()),
        "the completed fixture must restore the inherited sandbox, not remove it"
    );
    assert_eq!(
        std::fs::read(root.join("sentinel")).unwrap(),
        b"unchanged-parent-home"
    );
}

#[test]
fn imported_catalog_fixture_restores_inherited_home_in_fresh_process() {
    let root =
        std::env::temp_dir().join(format!("grok-search-env-restore-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("sentinel"), b"unchanged-parent-home").unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "session_content_search::tests::imported_catalog_fixture_restores_inherited_home_child",
            "--test-threads=1",
        ])
        .env("GROK_SEARCH_ENV_RESTORE_CHILD", &root)
        .env("GROK_APP_HOME", &root)
        .env("GROK_HOME", root.join("agent-home"))
        .env("GROK_APP_ACP", "mock")
        .env("GROK_DISABLE_DEFAULT_ROOTS", "1")
        .output()
        .unwrap();
    let files = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let sentinel = std::fs::read(root.join("sentinel")).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        result.status.success(),
        "child fixture environment contract failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sentinel, b"unchanged-parent-home");
    assert_eq!(files, vec![std::ffi::OsString::from("sentinel")]);
}

fn seed_imported_body(
    conn: &rusqlite::Connection,
    index: usize,
    created_at: &str,
    title: &str,
    body: &str,
    with_index: bool,
) -> String {
    use crate::session_migration::catalog::{
        insert_revision, revision_dir, upsert_entry, CatalogEntry,
    };
    use crate::session_migration::search::build_index;
    use crate::session_migration::types::{
        ContentBlock, EventBase, ImportedEvent, MessageRole, TimeOrigin,
    };
    let import_id = format!("imp:{}", uuid::Uuid::new_v4());
    let revision_id = uuid::Uuid::new_v4().to_string();
    upsert_entry(
        conn,
        &CatalogEntry {
            import_id: import_id.clone(),
            installation_id: "seed-inst".into(),
            provider: "generic-file".into(),
            native_session_id: format!("native-{index}"),
            title: title.into(),
            alias: None,
            hidden: false,
            current_revision: Some(revision_id.clone()),
            created_at: created_at.into(),
        },
    )
    .unwrap();
    let dir = revision_dir(&import_id, &revision_id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    insert_revision(
        conn,
        &revision_id,
        &import_id,
        "snap",
        "sem",
        "complete",
        "{}",
        &dir.to_string_lossy(),
    )
    .unwrap();
    if with_index {
        let event = ImportedEvent::Message {
            base: EventBase {
                id: format!("e-{index}"),
                ordinal: 0,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![ContentBlock::text(body)],
            complete: true,
        };
        build_index(&dir, &[event]).unwrap();
    }
    import_id
}

#[test]
fn imported_body_query_error_preserves_valid_title_hits() {
    use crate::session_migration::errors::ErrorCode;

    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-partial");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let import_id = seed_imported_body(
        &conn,
        0,
        "2020-01-01T00:00:00Z",
        "unique-partial-title",
        "body does not matter",
        false,
    );
    conn.execute_batch("DROP TABLE body_search_chunks;")
        .unwrap();

    let (hits, error) = search_imported_history_with_conn(&conn, "unique-partial-title", 10);
    assert!(hits.iter().any(|hit| hit.id == import_id));
    assert_eq!(error, Some(ErrorCode::SnapshotUnavailable));
    let _ = home;
}

#[test]
fn imported_body_search_hits_the_51st_catalog_entry() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-51");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let mut target = String::new();
    for i in 0..51 {
        let created = format!("2020-01-01T00:00:{i:02}Z");
        let body = if i == 0 {
            "unique-body-needle-51"
        } else {
            "unrelated-prefix-body"
        };
        let id = seed_imported_body(&conn, i, &created, &format!("title-{i}"), body, true);
        if i == 0 {
            target = id;
        }
    }
    let hits = search_sessions("unique-body-needle-51", 10).hits;
    assert!(
        hits.iter()
            .any(|h| h.id == target && h.kind == "importedHistory"),
        "body match on the oldest of 51 catalog entries must not be dropped: {hits:?}"
    );
    let _ = home;
}

#[test]
fn imported_body_search_hits_the_1000th_catalog_entry() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-1000");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let mut target = String::new();
    for i in 0..1000 {
        let created = format!(
            "2019-01-01T{:02}:{:02}:{:02}Z",
            i / 3600,
            (i / 60) % 60,
            i % 60
        );
        let body = if i == 0 {
            "unique-body-needle-1000"
        } else {
            "unrelated-prefix-body"
        };
        let id = seed_imported_body(&conn, i, &created, &format!("t-{i}"), body, i == 0);
        if i == 0 {
            target = id;
        }
    }
    let hits = search_sessions("unique-body-needle-1000", 10).hits;
    assert!(
        hits.iter()
            .any(|h| h.id == target && h.kind == "importedHistory"),
        "body match on the 1000th catalog entry must be reachable: {hits:?}"
    );
    let _ = home;
}

#[test]
fn imported_body_search_hits_the_10000th_catalog_entry() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-10000");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let mut target = String::new();
    for i in 0..10000 {
        let created = format!(
            "2018-01-01T{:02}:{:02}:{:02}Z",
            (i / 3600) % 24,
            (i / 60) % 60,
            i % 60
        );
        let body = if i == 0 {
            "unique-body-needle-10000"
        } else {
            "unrelated-prefix-body"
        };
        let id = seed_imported_body(&conn, i, &created, &format!("t-{i}"), body, i == 0);
        if i == 0 {
            target = id;
        }
    }
    let hits = search_sessions("unique-body-needle-10000", 10).hits;
    assert!(
        hits.iter()
            .any(|h| h.id == target && h.kind == "importedHistory"),
        "body match on the 10000th catalog entry must be reachable: {hits:?}"
    );
    let _ = home;
}

#[test]
fn imported_body_search_does_not_treat_index_errors_as_empty() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-err");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let id = seed_imported_body(
        &conn,
        0,
        "2020-01-01T00:00:00Z",
        "title-err",
        "unique-body-needle-err",
        true,
    );
    let rev = crate::session_migration::catalog::get_entry(&conn, &id)
        .unwrap()
        .unwrap()
        .current_revision
        .unwrap();
    let dir = crate::session_migration::catalog::revision_dir(&id, &rev).unwrap();
    std::fs::write(dir.join("event-index.sqlite"), b"not-a-sqlite").unwrap();
    let hits = search_sessions("unique-body-needle-err", 10).hits;
    assert!(
        hits.iter().any(|h| h.id == id),
        "a corrupt event-index must not look like a successful empty search: {hits:?}"
    );
    let _ = home;
}

#[test]
fn imported_body_search_matches_utf8_text_across_chunk_boundary() {
    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-chunk-utf8");
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let needle = "火箭🚀关键字";
    let body = format!(
        "{}{}",
        "x".repeat(crate::session_migration::catalog::BODY_SEARCH_CHUNK_BYTES.saturating_sub(4)),
        needle
    );
    let import_id = seed_imported_body(
        &conn,
        0,
        "2020-01-01T00:00:00Z",
        "chunk-boundary",
        &body,
        true,
    );
    let revision_id = crate::session_migration::catalog::get_entry(&conn, &import_id)
        .unwrap()
        .unwrap()
        .current_revision
        .unwrap();
    let (count, max_bytes): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), MAX(length(CAST(text AS BLOB)))
             FROM body_search_chunks WHERE import_id=?1 AND revision_id=?2",
            rusqlite::params![import_id, revision_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(count >= 2, "fixture must cross a projection boundary");
    assert!(
        max_bytes <= crate::session_migration::catalog::BODY_SEARCH_CHUNK_BYTES as i64,
        "projection chunks must stay byte-bounded: {max_bytes}"
    );
    let hits = search_sessions(needle, 10).hits;
    assert!(
        hits.iter().any(|hit| hit.id == import_id),
        "UTF-8 text split at a chunk edge must remain searchable: {hits:?}"
    );
    let _ = home;
}

#[test]
fn committed_preview_indexes_body_beyond_the_first_64k() {
    use crate::session_migration::engine::{commit, job_get, plan, preview, propose_mappings};
    use crate::session_migration::types::Provider;

    let home = crate::session_migration::test_home::IsolatedAppHome::acquire("gsearch-real-flow");
    let source = home.path.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let path = source.join("late.json");
    let needle = "late-real-flow-needle";
    let payload = serde_json::json!([
        {"role": "user", "content": "x".repeat(70 * 1024)},
        {"role": "assistant", "content": needle}
    ]);
    std::fs::write(&path, serde_json::to_vec(&payload).unwrap()).unwrap();

    let scan = crate::session_migration::scan::scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into_owned(),
    )])
    .unwrap();
    let candidate = scan
        .candidates
        .iter()
        .find(|candidate| candidate.locator.ends_with("late.json"))
        .expect("generic fixture candidate");
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();
    let plan = plan(vec![preview_id], mappings).unwrap();
    let job = commit(&plan.plan_id, &plan.plan_hash, "gsearch-real-flow-request").unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let completed = job_get(&job.job_id).unwrap();
    let import_id = completed.items[0]
        .import_id
        .as_deref()
        .expect("published import");

    let hits = search_sessions(needle, 10).hits;
    assert!(
        hits.iter().any(|hit| hit.id == import_id),
        "the shipped preview/commit path must index the complete imported body: {hits:?}"
    );
    let _ = home;
}
