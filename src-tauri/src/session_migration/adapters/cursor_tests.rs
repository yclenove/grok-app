    use rusqlite::params;

    #[test]
    fn missing_store_blob_is_partial() {
        let root = std::env::temp_dir().join(format!("cursor-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("store.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB);")
            .unwrap();
        std::fs::write(
            root.join("meta.json"),
            r#"{"name":"demo","rootBlobId":"missing-root"}"#,
        )
        .unwrap();
        drop(conn);
        let err = normalize_store(&db).unwrap_err();
        assert_eq!(err.code, ErrorCode::PartialHistory);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn aggregate_does_not_duplicate_same_native_id() {
        let a = SourceCandidate {
            candidate_id: "cursor-store:a".into(),
            provider: Provider::Cursor,
            native_session_id: "abc".into(),
            title: "A".into(),
            cwd: None,
            started_at: None,
            updated_at: None,
            locator: "store".into(),
            source_revision: "1".into(),
            archived: None,
            completeness: "store".into(),
        };
        let b = a.clone();
        let mut b = b;
        b.candidate_id = "cursor-transcript:a".into();
        b.completeness = "transcript".into();
        b.title = "B".into();
        let (out, issues) = aggregate_cursor_identities(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert!(issues.iter().any(|i| i.code == "source-conflict"));
    }

    #[test]
    fn composer_wal_body_is_read() {
        let root = std::env::temp_dir().join(format!("cursor-comp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        let composer = serde_json::json!({
            "composerId": "cid1",
            "name": "wal chat",
            "fullConversationHeadersOnly": [{"bubbleId": "b1"}, {"bubbleId": "b2"}]
        });
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["composerData:cid1", composer.to_string().as_bytes()],
            )
            .unwrap();
        let b1 = serde_json::json!({"type":"user","text":"hello wal"});
        let b2 = serde_json::json!({"type":"assistant","text":"from wal"});
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["bubbleId:cid1:b1", b1.to_string().as_bytes()],
            )
            .unwrap();
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["bubbleId:cid1:b2", b2.to_string().as_bytes()],
            )
            .unwrap();
        let out = normalize_composer(&db, Some("cid1")).expect("composer");
        drop(writer);
        let texts: Vec<_> = out
            .graph
            .events
            .iter()
            .filter_map(|e| match e {
                ImportedEvent::Message { blocks, .. } => {
                    blocks[0].plain_text().map(|s| s.to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["hello wal".to_string(), "from wal".into()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_or_locked_composer_is_not_zero_sessions() {
        let root = std::env::temp_dir().join(format!("cursor-busy-{}", uuid::Uuid::new_v4()));
        let ws = root.join("User").join("workspaceStorage").join("ws1");
        std::fs::create_dir_all(&ws).unwrap();
        let garbage = ws.join("state.vscdb");
        std::fs::write(&garbage, b"this is not a sqlite database").unwrap();
        let err = discover(&root).expect_err("garbage db must not look like zero sessions");
        assert!(
            matches!(
                err.code,
                ErrorCode::SnapshotUnavailable
                    | ErrorCode::SourceBusy
                    | ErrorCode::SchemaUnsupported
                    | ErrorCode::SourcePermissionDenied
            ),
            "unexpected code {:?}",
            err.code
        );

        let root2 = std::env::temp_dir().join(format!("cursor-lock-{}", uuid::Uuid::new_v4()));
        let ws2 = root2.join("User").join("workspaceStorage").join("ws2");
        std::fs::create_dir_all(&ws2).unwrap();
        let db = ws2.join("state.vscdb");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["composerData:locked-cid", b"{}"],
            )
            .unwrap();
        writer
            .execute_batch("PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE;")
            .unwrap();
        let listed = list_composer_from_db(&db, None);
        drop(writer);
        match listed {
            Err(e) => assert!(
                matches!(
                    e.code,
                    ErrorCode::SourceBusy | ErrorCode::SnapshotUnavailable
                ),
                "unexpected {:?}",
                e.code
            ),
            Ok(v) => {
                // Exclusive lock not visible to this platform's readonly open:
                // committed composer rows must still be listed, never an empty
                // "zero sessions" result that hides the live writer.
                assert!(
                    v.iter().any(|c| c.native_session_id == "locked-cid"),
                    "lock not visible must still see committed composer, not empty: {v:?}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn garbage_composer_does_not_drop_transcripts() {
        let root = std::env::temp_dir().join(format!("cursor-mix-{}", uuid::Uuid::new_v4()));
        let tx = root.join("projects").join("p1").join("agent-transcripts");
        std::fs::create_dir_all(&tx).unwrap();
        std::fs::write(
            tx.join("keep.jsonl"),
            r#"{"role":"user","message":{"content":[{"type":"text","text":"keep-me"}]}}
{"role":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}
"#,
        )
        .unwrap();
        let ws = root.join("User").join("workspaceStorage").join("ws-bad");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("state.vscdb"), b"this is not a sqlite database").unwrap();
        let listed = discover(&root).expect("transcripts must still list");
        assert!(
            listed.iter().any(|c| c.locator.contains("keep.jsonl")),
            "expected transcript, got {listed:?}"
        );
        let scan = discover_scan(&root);
        assert!(!scan.issues.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn transcript_string_blocks_tools_unknown_and_empty_cwd() {
        let tmp = std::env::temp_dir().join(format!("cursor-tx-{}.jsonl", uuid::Uuid::new_v4()));
        let raw = concat!(
            r#"{"id":"u1","role":"user","timestamp":"2026-01-01T00:00:00Z","content":"hello one"}"#,
            "\n",
            r#"{"id":"a1","role":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"content":[{"type":"text","text":"working"},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"a.txt"}},{"type":"mystery","payload":{}}]}}"#,
            "\n",
            r#"{"id":"r1","role":"tool","tool_call_id":"t1","content":"file ok"}"#,
            "\n",
            r#"{"id":"a2","role":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"content":[{"type":"text","text":"done"}]}}"#,
            "\n"
        );
        std::fs::write(&tmp, raw).unwrap();
        let out = normalize_transcript(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert!(out.graph.cwd.is_none());
        assert_eq!(out.outcome, ImportOutcome::Partial);
        let msgs: Vec<_> = out
            .graph
            .events
            .iter()
            .filter_map(|e| match e {
                ImportedEvent::Message {
                    role, blocks, base, ..
                } => Some((
                    *role,
                    blocks[0].plain_text().unwrap_or("").to_string(),
                    base.timestamp.clone(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].1, "hello one");
        assert_eq!(msgs[0].2.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(msgs[2].1, "done");
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::ToolCall { .. }))
                .count(),
            1
        );
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::ToolResult { .. }))
                .count(),
            1
        );
        assert!(out.graph.events.iter().any(|e| matches!(
            e,
            ImportedEvent::Unsupported { source_type, .. } if source_type == "mystery"
        )));
        assert_eq!(out.graph.events[0].id(), "cursor:u1");
    }

    #[test]
    fn same_named_transcripts_stay_two_candidates_and_subagents_are_listed() {
        let root = std::env::temp_dir().join(format!("cursor-names-{}", uuid::Uuid::new_v4()));
        let a = root.join("projects").join("p1").join("agent-transcripts");
        let b = root.join("projects").join("p2").join("agent-transcripts");
        let sub = a.join("subagents");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        let body = r#"{"role":"user","content":"same"}"#;
        std::fs::write(a.join("chat.jsonl"), body).unwrap();
        std::fs::write(b.join("chat.jsonl"), body).unwrap();
        std::fs::write(sub.join("child.jsonl"), body).unwrap();
        let listed = discover(&root).unwrap();
        let chats: Vec<_> = listed
            .iter()
            .filter(|c| c.locator.ends_with("chat.jsonl"))
            .collect();
        assert_eq!(chats.len(), 2);
        assert_ne!(chats[0].native_session_id, chats[1].native_session_id);
        assert!(listed.iter().any(|c| c.completeness == "subagent"));
        let empty = root
            .join("projects")
            .join("empty")
            .join("agent-transcripts");
        std::fs::create_dir_all(&empty).unwrap();
        let again = discover(&root).unwrap();
        assert_eq!(again.len(), listed.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn transcript_and_store_same_id_aggregate() {
        let transcript = SourceCandidate {
            candidate_id: "cursor-transcript:p1/abc.jsonl".into(),
            provider: Provider::Cursor,
            native_session_id: "p1/agent-transcripts/abc.jsonl".into(),
            title: "from file".into(),
            cwd: None,
            started_at: None,
            updated_at: None,
            locator: "p1/agent-transcripts/abc.jsonl".into(),
            source_revision: "t".into(),
            archived: None,
            completeness: "transcript".into(),
        };
        let store = SourceCandidate {
            candidate_id: "cursor-store:abc".into(),
            provider: Provider::Cursor,
            native_session_id: "abc".into(),
            title: "from store".into(),
            cwd: None,
            started_at: None,
            updated_at: None,
            locator: "store.db".into(),
            source_revision: "s".into(),
            archived: None,
            completeness: "store".into(),
        };
        let (out, issues) = aggregate_cursor_identities(vec![transcript, store]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].native_session_id, "abc");
        assert!(issues.iter().any(|i| i.code == "source-conflict"));
    }

    fn composer_layout(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("cursor-s16-{tag}-{}", uuid::Uuid::new_v4()));
        let ws = root.join("User").join("workspaceStorage").join("ws1");
        let global = root.join("User").join("globalStorage");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&global).unwrap();
        (root, ws.join("state.vscdb"), global.join("state.vscdb"))
    }

    fn write_item_table(db: &std::path::Path, rows: &[(&str, &[u8])]) {
        let conn = Connection::open(db).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT PRIMARY KEY, value BLOB);",
        )
        .unwrap();
        for (key, value) in rows {
            conn.execute(
                "INSERT OR REPLACE INTO ItemTable(key, value) VALUES (?1, ?2)",
                params![*key, *value],
            )
            .unwrap();
        }
        drop(conn);
    }

    fn composer_header(id: &str, bubbles: &[&str]) -> Vec<u8> {
        let headers: Vec<_> = bubbles
            .iter()
            .map(|b| serde_json::json!({"bubbleId": b}))
            .collect();
        serde_json::json!({
            "composerId": id,
            "name": id,
            "fullConversationHeadersOnly": headers
        })
        .to_string()
        .into_bytes()
    }

    fn bubble(kind: &str, text: &str) -> Vec<u8> {
        serde_json::json!({"type": kind, "text": text})
            .to_string()
            .into_bytes()
    }

    fn texts(out: &AdapterOutput) -> Vec<String> {
        out.graph
            .events
            .iter()
            .filter_map(|e| match e {
                ImportedEvent::Message { blocks, .. } => {
                    blocks[0].plain_text().map(|s| s.to_string())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dangling_workspace_bubble_is_filled_from_global_once() {
        let (root, ws, global) = composer_layout("fill");
        write_item_table(
            &ws,
            &[
                ("composerData:cid", &composer_header("cid", &["b1", "b2"])),
                ("bubbleId:cid:b1", &bubble("user", "from-workspace")),
            ],
        );
        write_item_table(
            &global,
            &[("bubbleId:cid:b2", &bubble("assistant", "from-global"))],
        );
        let out = normalize_composer(&ws, Some("cid")).unwrap();
        assert_eq!(
            texts(&out),
            vec!["from-workspace".to_string(), "from-global".into()]
        );
        assert_eq!(out.outcome, ImportOutcome::Complete);
        assert_eq!(out.evidence.artifacts.len(), 2);
        assert!(companion_composer_db(&ws).unwrap().ends_with("state.vscdb"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dangling_bubble_after_retry_is_partial_and_does_not_invent_text() {
        let (root, ws, global) = composer_layout("miss");
        write_item_table(
            &ws,
            &[
                ("composerData:cid", &composer_header("cid", &["b1", "b2"])),
                ("bubbleId:cid:b1", &bubble("user", "kept")),
            ],
        );
        write_item_table(&global, &[("unrelated", b"nope")]);
        let out = normalize_composer(&ws, Some("cid")).unwrap();
        assert_eq!(texts(&out), vec!["kept".to_string()]);
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(out.issues.iter().any(|i| i.code == "missing-bubble"));
        assert!(!texts(&out).iter().any(|t| t.contains("b2")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn busy_companion_db_retries_then_partial_without_invented_bubbles() {
        let (root, ws, global) = composer_layout("busy-comp");
        write_item_table(
            &ws,
            &[
                ("composerData:cid", &composer_header("cid", &["b1", "b2"])),
                ("bubbleId:cid:b1", &bubble("user", "workspace-kept")),
            ],
        );
        write_item_table(
            &global,
            &[("bubbleId:cid:b2", &bubble("assistant", "COMPANION-SECRET"))],
        );
        let mut journal = global.as_os_str().to_os_string();
        journal.push("-journal");
        std::fs::write(&journal, b"rollback-journal-busy").unwrap();
        let out = normalize_composer(&ws, Some("cid")).unwrap();
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert_ne!(out.outcome, ImportOutcome::Complete);
        assert!(out.issues.iter().any(|i| i.code == "missing-bubble"));
        assert_eq!(texts(&out), vec!["workspace-kept".to_string()]);
        assert!(
            !texts(&out).iter().any(|t| t.contains("COMPANION-SECRET")),
            "busy companion must not invent bubble text: {:?}",
            texts(&out)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn companion_does_not_override_primary_bubble_text() {
        let (root, ws, global) = composer_layout("d03");
        write_item_table(
            &ws,
            &[
                ("composerData:cid", &composer_header("cid", &["b1"])),
                ("bubbleId:cid:b1", &bubble("user", "workspace-text")),
            ],
        );
        write_item_table(
            &global,
            &[("bubbleId:cid:b1", &bubble("user", "global-newer-text"))],
        );
        let out = normalize_composer(&ws, Some("cid")).unwrap();
        assert_eq!(texts(&out), vec!["workspace-text".to_string()]);
        assert!(!texts(&out).iter().any(|t| t.contains("global-newer")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_table_is_schema_unsupported_not_empty_session() {
        let root = std::env::temp_dir().join(format!("cursor-s16-unk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE other (k TEXT);").unwrap();
        drop(conn);
        let err = normalize_composer(&db, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::SchemaUnsupported);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_keys_are_ignored_and_legacy_chatdata_reads() {
        let root = std::env::temp_dir().join(format!("cursor-s16-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let chat = serde_json::json!({
            "messages": [
                {"type": "user", "text": "old user"},
                {"type": "assistant", "text": "old assistant"}
            ]
        })
        .to_string()
        .into_bytes();
        write_item_table(
            &db,
            &[("noise:skip-me", b"ignore"), ("chatData:old1", &chat)],
        );
        let listed = list_composer_from_db(&db, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].completeness, "legacy-chat");
        let out = normalize_composer(&db, Some("old1")).unwrap();
        assert_eq!(
            texts(&out),
            vec!["old user".to_string(), "old assistant".into()]
        );
        assert_eq!(out.evidence.schema_fingerprint, "cursor-legacy-chat-v1");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn composer_wal_source_files_stay_byte_identical() {
        use crate::session_migration::sqlite_ro::open_readonly;
        let root = std::env::temp_dir().join(format!("cursor-s16-wal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["composerData:wal", composer_header("wal", &["b1"])],
            )
            .unwrap();
        writer
            .execute(
                "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                params!["bubbleId:wal:b1", bubble("user", "only-in-wal")],
            )
            .unwrap();
        let before = std::fs::read(&db).unwrap();
        let wal_path = {
            let mut p = db.as_os_str().to_os_string();
            p.push("-wal");
            std::path::PathBuf::from(p)
        };
        let wal_before = std::fs::read(&wal_path).unwrap();
        let out = normalize_composer(&db, Some("wal")).unwrap();
        assert_eq!(texts(&out), vec!["only-in-wal".to_string()]);
        assert_eq!(std::fs::read(&db).unwrap(), before);
        assert_eq!(std::fs::read(&wal_path).unwrap(), wal_before);
        drop(open_readonly(&db).unwrap());
        assert_eq!(std::fs::read(&db).unwrap(), before);
        drop(writer);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn write_store(dir: &std::path::Path, meta: &str, blobs: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("meta.json"), meta).unwrap();
        let db = dir.join("store.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB);")
            .unwrap();
        for (id, json) in blobs {
            conn.execute(
                "INSERT INTO blobs(id, data) VALUES (?1, ?2)",
                params![*id, json.as_bytes()],
            )
            .unwrap();
        }
        drop(conn);
    }

    #[test]
    fn store_tree_keeps_fork_and_selects_leaf() {
        let root = std::env::temp_dir().join(format!("cursor-s17-fork-{}", uuid::Uuid::new_v4()));
        write_store(
            &root,
            r#"{"name":"demo","rootBlobId":"root","leaf":"c"}"#,
            &[
                ("root", r#"{"id":"root","children":["a"]}"#),
                (
                    "a",
                    r#"{"id":"a","role":"user","text":"ask","children":["b","c"]}"#,
                ),
                ("b", r#"{"id":"b","role":"assistant","text":"branch-b"}"#),
                ("c", r#"{"id":"c","role":"assistant","text":"branch-c"}"#),
            ],
        );
        let out = normalize_store(&root.join("store.db")).unwrap();
        assert_eq!(out.outcome, ImportOutcome::Complete);
        assert_eq!(out.graph.active_leaf.as_deref(), Some("cursor:store:c"));
        let b = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "cursor:store:b",
        );
        let c = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "cursor:store:c",
        );
        assert!(b.iter().any(|e| e.searchable_text() == "branch-b"));
        assert!(
            !b.iter().any(|e| e.searchable_text() == "branch-c"),
            "selecting B must not include C"
        );
        assert!(c.iter().any(|e| e.searchable_text() == "branch-c"));
        assert!(!c.iter().any(|e| e.searchable_text() == "branch-b"));
        assert!(out
            .graph
            .relations
            .iter()
            .any(|r| r.kind == RelationKind::Fork));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn store_missing_child_blob_is_partial_with_kept_messages() {
        let root = std::env::temp_dir().join(format!("cursor-s17-miss-{}", uuid::Uuid::new_v4()));
        write_store(
            &root,
            r#"{"rootBlobId":"root"}"#,
            &[
                ("root", r#"{"id":"root","children":["a","gone"]}"#),
                ("a", r#"{"id":"a","role":"user","text":"kept"}"#),
            ],
        );
        let out = normalize_store(&root.join("store.db")).unwrap();
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(out.issues.iter().any(|i| i.code == "missing-blob"));
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn store_cycle_and_multi_parent_are_reported() {
        let root = std::env::temp_dir().join(format!("cursor-s17-cycle-{}", uuid::Uuid::new_v4()));
        write_store(
            &root,
            r#"{"rootBlobId":"a"}"#,
            &[
                (
                    "a",
                    r#"{"id":"a","role":"user","text":"one","children":["b"]}"#,
                ),
                (
                    "b",
                    r#"{"id":"b","role":"assistant","text":"two","children":["a"]}"#,
                ),
            ],
        );
        let out = normalize_store(&root.join("store.db")).unwrap();
        assert!(out.issues.iter().any(|i| i.code == "store-cycle"));
        let root2 = std::env::temp_dir().join(format!("cursor-s17-mp-{}", uuid::Uuid::new_v4()));
        write_store(
            &root2,
            r#"{"rootBlobId":"root"}"#,
            &[
                ("root", r#"{"id":"root","children":["a","b"]}"#),
                (
                    "a",
                    r#"{"id":"a","role":"user","text":"left","children":["c"]}"#,
                ),
                (
                    "b",
                    r#"{"id":"b","role":"user","text":"right","children":["c"]}"#,
                ),
                ("c", r#"{"id":"c","role":"assistant","text":"shared"}"#),
            ],
        );
        let out2 = normalize_store(&root2.join("store.db")).unwrap();
        let parents: Vec<_> = out2
            .graph
            .relations
            .iter()
            .filter(|r| r.kind == RelationKind::Parent && r.from == "cursor:store:c")
            .collect();
        assert!(
            parents.len() >= 2 || out2.issues.iter().any(|i| i.code == "multiple-parents"),
            "shared child must keep both parents or report them: {parents:?} {:?}",
            out2.issues
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn three_sources_same_id_aggregate_and_text_similarity_does_not_merge() {
        let store = SourceCandidate {
            candidate_id: "cursor-store:abc".into(),
            provider: Provider::Cursor,
            native_session_id: "abc".into(),
            title: "Store title".into(),
            cwd: Some("/repo".into()),
            started_at: None,
            updated_at: None,
            locator: "store.db".into(),
            source_revision: "s1".into(),
            archived: None,
            completeness: "store".into(),
        };
        let mut composer = store.clone();
        composer.candidate_id = "cursor-composer:abc".into();
        composer.completeness = "composer".into();
        composer.title = "Composer title".into();
        composer.source_revision = "c1".into();
        composer.cwd = None;
        composer.locator = "state.vscdb#composerData:abc".into();
        let mut transcript = store.clone();
        transcript.candidate_id = "cursor-transcript:abc.jsonl".into();
        transcript.completeness = "transcript".into();
        transcript.title = "Store title".into();
        transcript.source_revision = "t1".into();
        transcript.locator = "abc.jsonl".into();
        let (out, issues) = aggregate_cursor_identities(vec![store.clone(), composer, transcript]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].completeness, "store");
        assert_eq!(out[0].cwd.as_deref(), Some("/repo"));
        assert!(issues.iter().any(|i| i.code == "source-conflict"));
        let mut other = store.clone();
        other.native_session_id = "xyz".into();
        other.candidate_id = "cursor-store:xyz".into();
        other.title = "Store title".into();
        let (out2, _) = aggregate_cursor_identities(vec![store, other]);
        assert_eq!(out2.len(), 2, "same title must not merge different ids");
    }

    #[test]
    fn discover_scan_aggregates_store_and_transcript() {
        let root = std::env::temp_dir().join(format!("cursor-s17-disc-{}", uuid::Uuid::new_v4()));
        let store_dir = root.join(".cursor").join("chats").join("abc");
        write_store(
            &store_dir,
            r#"{"name":"demo","rootBlobId":"a"}"#,
            &[("a", r#"{"id":"a","role":"user","text":"hi"}"#)],
        );
        let tx = root.join("projects").join("p1").join("agent-transcripts");
        std::fs::create_dir_all(&tx).unwrap();
        std::fs::write(tx.join("abc.jsonl"), r#"{"role":"user","content":"hi"}"#).unwrap();
        let scan = discover_scan(&root);
        let abc: Vec<_> = scan
            .candidates
            .iter()
            .filter(|c| {
                c.native_session_id == "abc"
                    || Path::new(&c.locator).file_stem().and_then(|s| s.to_str()) == Some("abc")
            })
            .collect();
        assert_eq!(
            abc.len(),
            1,
            "store+transcript same id must be one candidate: {:?}",
            scan.candidates
        );
        let _ = std::fs::remove_dir_all(&root);
    }
