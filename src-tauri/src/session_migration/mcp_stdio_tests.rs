    use crate::session_migration::search::build_index;
    use crate::session_migration::types::{
        ContentBlock, EventBase, ImportedEvent, MessageRole, TimeOrigin,
    };

    fn mcp_msg(id: &str, ordinal: u32, text: &str) -> ImportedEvent {
        ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        }
    }

    fn publish_grant(
        events: Vec<ImportedEvent>,
        leaf: &str,
        selection: AllowedSelection,
        native: &str,
        installation: &str,
    ) -> ImportHistoryGrant {
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::selection::{PreviewSelection, SelectionSet};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        let suffix = uuid::Uuid::new_v4().to_string();
        let item = crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
            candidate_id: format!("c-mcp-{suffix}"),
            installation_id: installation.into(),
            provider: "generic-file".into(),
            root_path: "/tmp".into(),
            adapter: AdapterOutput {
                evidence: SourceEvidence {
                    provider: Prov::GenericFile,
                    source_version: None,
                    schema_fingerprint: "t".into(),
                    adapter_version: "1.0.0".into(),
                    platform: "test".into(),
                    artifacts: vec![],
                },
                graph: crate::session_migration::types::NormalizedGraph {
                    events,
                    relations: vec![],
                    active_leaf: Some(leaf.into()),
                    native_session_id: native.into(),
                    title: Some(native.into()),
                    cwd: None,
                },
                inventory: Inventory::default(),
                issues: vec![],
                assets: vec![],
                outcome: ImportOutcome::Complete,
            },
            prepared: None,
            options: serde_json::json!({}),
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            identity_association: None,
            selection_set: SelectionSet::single(PreviewSelection {
                preview_id: uuid::Uuid::new_v4().to_string(),
                leaf_id: leaf.into(),
                selection,
            }),
        });
        let published =
            publish_item(&conn, &format!("job-mcp-{suffix}"), &item, &|| false).unwrap();
        let import_id = published.import_id.unwrap();
        let rev = published.revision_id.unwrap();
        let draft = crate::session_migration::engine::continue_prepare(
            &import_id,
            Some(&rev),
            Some(leaf),
            None,
            None,
        )
        .unwrap();
        let sid = crate::session_migration::engine::continue_create(
            &draft.draft_id,
            &draft.draft_hash,
            &format!("req-mcp-{suffix}"),
            None,
        )
        .unwrap();
        crate::session_migration::engine::grant_with_selection(&sid).expect("published grant")
    }

    #[test]
    fn stdio_uses_bounded_newline_json_messages() {
        let mut input = std::io::Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n{\"id\":2,\"method\":\"tools/list\"}\n");
        assert_eq!(read_rpc(&mut input).unwrap().unwrap()["id"], 1);
        assert_eq!(read_rpc(&mut input).unwrap().unwrap()["id"], 2);
        assert!(read_rpc(&mut input).unwrap().is_none());
        let mut output = Vec::new();
        write_rpc(
            &mut output,
            &json!({"id": 1, "result": {"text": "中文\n文本"}}),
        )
        .unwrap();
        assert_eq!(output.iter().filter(|&&b| b == b'\n').count(), 1);
        assert!(serde_json::from_slice::<Value>(&output).is_ok());
        assert!(read_rpc(&mut std::io::Cursor::new(vec![b'x'; 1024 * 1024 + 2])).is_err());
        assert!(
            read_rpc(&mut std::io::Cursor::new(b"{not-json\n")).is_err(),
            "malformed JSON must not be treated as a method"
        );
        let init = handle_method(
            &ImportHistoryGrant {
                token: "t".into(),
                app_session_id: "s".into(),
                import_id: "imp:x".into(),
                revision_id: "r".into(),
                branch_leaf: "e1".into(),
                allowed_event_ids: vec![],
                allowed_selection_hash: String::new(),
                selection: None,
            },
            "initialize",
            &json!({}),
        )
        .unwrap();
        assert_eq!(init["serverInfo"]["name"], "import-history");
        let listed = handle_method(
            &ImportHistoryGrant {
                token: "t".into(),
                app_session_id: "s".into(),
                import_id: "imp:x".into(),
                revision_id: "r".into(),
                branch_leaf: "e1".into(),
                allowed_event_ids: vec![],
                allowed_selection_hash: String::new(),
                selection: None,
            },
            "tools/list",
            &json!({}),
        )
        .unwrap();
        let names: Vec<_> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"import_history_search"));
        assert!(names.contains(&"import_history_read"));
    }

    #[test]
    fn excluded_event_is_not_searchable() {
        let dir = std::env::temp_dir().join(format!("mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let events = vec![
            ImportedEvent::Message {
                base: EventBase {
                    id: "keep".into(),
                    ordinal: 0,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: MessageRole::User,
                blocks: vec![ContentBlock::text("alpha beta")],
                complete: true,
            },
            ImportedEvent::Message {
                base: EventBase {
                    id: "secret".into(),
                    ordinal: 1,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                role: MessageRole::User,
                blocks: vec![ContentBlock::text("synthetic-secret")],
                complete: true,
            },
        ];
        build_index(&dir, &events).unwrap();
        let grant = ImportHistoryGrant {
            token: "tok".into(),
            app_session_id: "s".into(),
            import_id: "imp:x".into(),
            revision_id: dir.file_name().unwrap().to_string_lossy().into_owned(),
            branch_leaf: "keep".into(),
            allowed_event_ids: vec!["keep".into()],
            allowed_selection_hash: AllowedSelection {
                event_ids: vec!["keep".into()],
                ..AllowedSelection::default()
            }
            .hash(),
            selection: Some(AllowedSelection::for_events(vec!["keep".into()])),
        };
        // Direct search with allow-set (same function the tool uses).
        let allow = grant.allowed_set();
        let (hits, _) = search(&dir, "synthetic-secret", None, Some(10), Some(&allow)).unwrap();
        assert!(hits.is_empty());
        let (hits, _) = search(&dir, "alpha", None, Some(10), Some(&allow)).unwrap();
        assert_eq!(hits.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grant_for_session_b_cannot_read_session_a_events() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-iso");
        let grant_a = publish_grant(
            vec![mcp_msg("a-only", 0, "session-a-token")],
            "a-only",
            AllowedSelection::for_events(vec!["a-only".into()]),
            "n-mcp-a",
            "inst-mcp-a",
        );
        let grant_b = publish_grant(
            vec![mcp_msg("b-only", 0, "session-b-token")],
            "b-only",
            AllowedSelection::for_events(vec!["b-only".into()]),
            "n-mcp-b",
            "inst-mcp-b",
        );
        let out = call_tool(
            &grant_b,
            "search",
            &json!({"query": "session-a-token", "appSessionId": grant_b.app_session_id}),
        )
        .unwrap();
        let text = out.to_string();
        assert!(!text.contains("session-a-token"));
        let mismatch = call_tool(
            &grant_a,
            "search",
            &json!({"query": "session-a-token", "appSessionId": grant_b.app_session_id}),
        )
        .unwrap_err();
        assert!(mismatch.contains("HISTORY_ACCESS_UNAVAILABLE"));
        let entry = mcp_server_entry(&grant_b).unwrap();
        assert!(entry["env"].is_array());
        assert!(entry["env"][0]["value"].as_str().unwrap().len() < 1024);
        let mut revoked =
            super::super::continuation::read_sidecar(&grant_b.app_session_id).unwrap();
        revoked.grant_token = "rotated".into();
        super::super::continuation::write_sidecar(&grant_b.app_session_id, &revoked).unwrap();
        assert!(call_tool(&grant_b, "search", &json!({"query": "session-b-token"})).is_err());
    }

    #[test]
    fn hidden_entry_and_deleted_sidecar_revoke_tool_access() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-rev");
        let grant = publish_grant(
            vec![mcp_msg("keep", 0, "visible-after-import")],
            "keep",
            AllowedSelection::for_events(vec!["keep".into()]),
            "n-mcp-hide",
            "inst-mcp-hide",
        );
        let ok = call_tool(&grant, "search", &json!({"query": "visible-after-import"})).unwrap();
        assert!(ok.to_string().contains("visible-after-import"));
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        crate::session_migration::catalog::hide_entry(&conn, &grant.import_id).unwrap();
        let hidden =
            call_tool(&grant, "search", &json!({"query": "visible-after-import"})).unwrap_err();
        assert!(hidden.contains("HISTORY_ACCESS_UNAVAILABLE"));
        conn.execute(
            "UPDATE entries SET hidden=0 WHERE import_id=?1",
            rusqlite::params![grant.import_id],
        )
        .unwrap();
        std::fs::remove_file(crate::session_migration::continuation::sidecar_path(
            &grant.app_session_id,
        ))
        .unwrap();
        let gone =
            call_tool(&grant, "search", &json!({"query": "visible-after-import"})).unwrap_err();
        assert!(gone.contains("HISTORY_ACCESS_UNAVAILABLE"));
    }

    #[test]
    fn grant_bound_to_revision_a_cannot_search_revision_b() {
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::selection::{PreviewSelection, SelectionSet};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-xrev");
        let grant = publish_grant(
            vec![mcp_msg("keep", 0, "rev-a-visible")],
            "keep",
            AllowedSelection::for_events(vec!["keep".into()]),
            "n-mcp-xrev",
            "inst-mcp-xrev",
        );
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        let item = crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
            candidate_id: "c-mcp-xrev-b".into(),
            installation_id: "inst-mcp-xrev".into(),
            provider: "generic-file".into(),
            root_path: "/tmp".into(),
            adapter: AdapterOutput {
                evidence: SourceEvidence {
                    provider: Prov::GenericFile,
                    source_version: None,
                    schema_fingerprint: "t".into(),
                    adapter_version: "1.0.0".into(),
                    platform: "test".into(),
                    artifacts: vec![],
                },
                graph: crate::session_migration::types::NormalizedGraph {
                    events: vec![mcp_msg("secret", 0, "rev-b-only-secret")],
                    relations: vec![],
                    active_leaf: Some("secret".into()),
                    native_session_id: "n-mcp-xrev".into(),
                    title: Some("n-mcp-xrev".into()),
                    cwd: None,
                },
                inventory: Inventory::default(),
                issues: vec![],
                assets: vec![],
                outcome: ImportOutcome::Complete,
            },
            prepared: None,
            options: serde_json::json!({}),
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            identity_association: None,
            selection_set: SelectionSet::single(PreviewSelection {
                preview_id: uuid::Uuid::new_v4().to_string(),
                leaf_id: "secret".into(),
                selection: AllowedSelection::for_events(vec!["secret".into()]),
            }),
        });
        let second = publish_item(&conn, "job-mcp-xrev-b", &item, &|| false).unwrap();
        assert_eq!(second.import_id.as_deref(), Some(grant.import_id.as_str()));
        assert_ne!(
            second.revision_id.as_deref(),
            Some(grant.revision_id.as_str())
        );
        let a = call_tool(&grant, "search", &json!({"query": "rev-a-visible"})).unwrap();
        assert!(a.to_string().contains("rev-a-visible"));
        let b = call_tool(&grant, "search", &json!({"query": "rev-b-only-secret"})).unwrap();
        assert!(
            !b.to_string().contains("rev-b-only-secret"),
            "frozen revision A must not leak revision B: {b}"
        );
    }

    #[test]
    fn read_of_excluded_or_path_like_citation_is_denied() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-read");
        let grant = publish_grant(
            vec![
                mcp_msg("keep", 0, "allowed-body"),
                mcp_msg("secret", 1, "excluded-body"),
            ],
            "keep",
            AllowedSelection::for_events(vec!["keep".into()]),
            "n-mcp-read",
            "inst-mcp-read",
        );
        let ok = call_tool(&grant, "read", &json!({"citation": "keep"})).unwrap();
        assert!(ok.to_string().contains("allowed-body"));
        let denied = call_tool(&grant, "read", &json!({"citation": "secret"})).unwrap_err();
        assert!(
            denied.contains("HISTORY_ACCESS_UNAVAILABLE") || denied.contains("not"),
            "{denied}"
        );
        assert!(!denied.contains("excluded-body"));
        let pathish = call_tool(&grant, "read", &json!({"citation": "../secret"})).unwrap_err();
        assert!(pathish.contains("HISTORY_ACCESS_UNAVAILABLE"));
    }

    #[test]
    fn search_and_read_keep_imported_tokens_literal_and_reject_execute_tools() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-c05");
        let other = "11111111-1111-4111-8111-111111111111";
        let keep_body = format!("[[chat:{other}]] `rm -rf /` $(whoami) SELECT * FROM entries");
        let grant = publish_grant(
            vec![
                mcp_msg("keep", 0, &keep_body),
                mcp_msg("secret", 1, "OTHER-SESSION-SECRET"),
            ],
            "keep",
            AllowedSelection::for_events(vec!["keep".into()]),
            "n-mcp-c05",
            "inst-mcp-c05",
        );
        let listed = handle_method(&grant, "tools/list", &json!({})).unwrap();
        let names: Vec<_> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "import_history_search",
                "import_history_read",
                "import_history_asset_read"
            ],
            "import MCP must not advertise execute/shell tools: {names:?}"
        );
        for forbidden in ["bash", "shell", "execute", "run"] {
            let err = call_tool(&grant, forbidden, &json!({"command": "rm -rf /"})).unwrap_err();
            assert!(
                err.contains("unknown tool"),
                "{forbidden} must not execute: {err}"
            );
            assert!(!err.contains("OTHER-SESSION-SECRET"));
        }
        let hit = call_tool(
            &grant,
            "search",
            &json!({"query": format!("[[chat:{other}]]")}),
        )
        .unwrap()
        .to_string();
        assert!(hit.contains("[[chat:"), "{hit}");
        assert!(
            !hit.contains("OTHER-SESSION-SECRET"),
            "search must not fetch the other session: {hit}"
        );
        let sqlish = call_tool(
            &grant,
            "search",
            &json!({"query": "'; DROP TABLE events; --"}),
        )
        .unwrap()
        .to_string();
        assert!(
            !sqlish.contains("OTHER-SESSION-SECRET"),
            "SQL-like query must stay literal: {sqlish}"
        );
        let still = call_tool(&grant, "search", &json!({"query": "$(whoami)"})).unwrap();
        assert!(
            still.to_string().contains("$(whoami)"),
            "index must survive a SQL-like query: {still}"
        );
        let body = call_tool(&grant, "read", &json!({"citation": "keep"}))
            .unwrap()
            .to_string();
        assert!(body.contains(&keep_body), "{body}");
        assert!(!body.contains("OTHER-SESSION-SECRET"));
        let as_citation = call_tool(
            &grant,
            "read",
            &json!({"citation": format!("[[chat:{other}]]")}),
        )
        .unwrap_err();
        assert!(
            as_citation.contains("HISTORY_ACCESS_UNAVAILABLE") || as_citation.contains("not"),
            "{as_citation}"
        );
        assert!(!as_citation.contains("OTHER-SESSION-SECRET"));
    }

    fn grant(session: &str, token: &str) -> ImportHistoryGrant {
        ImportHistoryGrant {
            token: token.into(),
            app_session_id: session.into(),
            import_id: "imp:x".into(),
            revision_id: "rev".into(),
            branch_leaf: "e1".into(),
            allowed_event_ids: vec!["e1".into()],
            allowed_selection_hash: AllowedSelection {
                event_ids: vec!["e1".into()],
                ..AllowedSelection::default()
            }
            .hash(),
            selection: Some(AllowedSelection::for_events(vec!["e1".into()])),
        }
    }

    #[test]
    fn asset_read_allows_selected_stored_blob_and_denies_path_guess_and_unselected() {
        use crate::session_migration::jobs::{
            prepare_revision, publish_item, write_preview_lease, CommitItem, PreviewLease,
        };
        use crate::session_migration::selection::{PreviewSelection, SelectionSet};
        use crate::session_migration::types::{
            AdapterOutput, AssetRecord, AssetState, ImportOutcome, Inventory, Provider as Prov,
            SourceEvidence, SourceSpan,
        };
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("mcp-asset");
        let source = home.path.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("ok.bin"), b"allowed-blob-bytes").unwrap();
        std::fs::write(source.join("no.bin"), b"denied-blob-bytes").unwrap();
        let span = SourceSpan {
            artifact_id: "a".into(),
            record: "0".into(),
            block: None,
        };
        let mut keep = mcp_msg("keep", 0, "ok");
        if let ImportedEvent::Message { blocks, .. } = &mut keep {
            blocks.push(ContentBlock::Asset {
                asset_id: "asset-ok".into(),
                alt: Some("ok".into()),
            });
            blocks.push(ContentBlock::Asset {
                asset_id: "asset-no".into(),
                alt: Some("no".into()),
            });
        }
        let mut adapter = AdapterOutput {
            evidence: SourceEvidence {
                provider: Prov::GenericFile,
                source_version: None,
                schema_fingerprint: "t".into(),
                adapter_version: "1.0.0".into(),
                platform: "test".into(),
                artifacts: vec![],
            },
            graph: crate::session_migration::types::NormalizedGraph {
                events: vec![keep],
                relations: vec![],
                active_leaf: Some("keep".into()),
                native_session_id: "n-mcp-asset".into(),
                title: Some("n-mcp-asset".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![
                AssetRecord {
                    asset_id: "asset-ok".into(),
                    digest: None,
                    mime: Some("image/png".into()),
                    size: 0,
                    state: AssetState::Unresolved,
                    source_span: span.clone(),
                    source_ref: Some("path:ok.bin".into()),
                },
                AssetRecord {
                    asset_id: "asset-no".into(),
                    digest: None,
                    mime: Some("image/png".into()),
                    size: 0,
                    state: AssetState::Unresolved,
                    source_span: span,
                    source_ref: Some("path:no.bin".into()),
                },
            ],
            outcome: ImportOutcome::Complete,
        };
        let options = serde_json::json!({"rawRetention":"normalized-only"});
        let preview_id = uuid::Uuid::new_v4().to_string();
        let prepared =
            prepare_revision(&mut adapter, &options, Some(&source), Some(&preview_id)).unwrap();
        write_preview_lease(
            &prepared.path,
            &PreviewLease {
                source_member: None,
                preview_id: preview_id.clone(),
                candidate_id: "c-mcp-asset".into(),
                source_revision: "test-source-revision".into(),
                prepare_batch_id: None,
                source_id: "asset-source".into(),
                root_path: source.to_string_lossy().into_owned(),
                legacy_id: "asset-legacy".into(),
                snapshot_digest: prepared.digest.clone(),
                expires_unix: chrono::Utc::now().timestamp() + 3600,
                retained_by_job: false,
                file_hashes: prepared.file_hashes.clone(),
            },
        )
        .unwrap();
        let mut selection = AllowedSelection::for_events(vec!["keep".into()]);
        selection.asset_ids = vec!["asset-ok".into()];
        let item = CommitItem {
            candidate_id: "c-mcp-asset".into(),
            installation_id: "inst-mcp-asset".into(),
            provider: "generic-file".into(),
            root_path: source.to_string_lossy().into_owned(),
            adapter,
            prepared: Some(prepared),
            options,
            mapping_project_id: None,
            explicit_reimport: false,
            create_project: false,
            import_unassigned_anyway: false,
            explicit_target: None,
            identity_association: None,
            selection_set: SelectionSet::single(PreviewSelection {
                preview_id,
                leaf_id: "keep".into(),
                selection: selection.clone(),
            }),
        };
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        let published = publish_item(&conn, "job-mcp-asset", &item, &|| false).unwrap();
        let import_id = published.import_id.unwrap();
        let rev = published.revision_id.unwrap();
        let draft = crate::session_migration::engine::continue_prepare(
            &import_id,
            Some(&rev),
            Some("keep"),
            None,
            None,
        )
        .unwrap();
        let sid = crate::session_migration::engine::continue_create(
            &draft.draft_id,
            &draft.draft_hash,
            "req-mcp-asset",
            None,
        )
        .unwrap();
        let grant = crate::session_migration::engine::grant_with_selection(&sid).expect("grant");
        let ok = call_tool(
            &grant,
            "import_history_asset_read",
            &json!({"assetId": "asset-ok"}),
        )
        .unwrap()
        .to_string();
        assert!(
            ok.contains("allowed-blob") || ok.contains("asset-ok"),
            "{ok}"
        );
        assert!(!ok.contains("denied-blob-bytes"));
        let unselected = call_tool(
            &grant,
            "import_history_asset_read",
            &json!({"assetId": "asset-no"}),
        )
        .unwrap_err();
        assert!(
            unselected.contains("HISTORY_ACCESS_UNAVAILABLE"),
            "{unselected}"
        );
        let guessed = call_tool(
            &grant,
            "import_history_asset_read",
            &json!({"assetId": "missing-id"}),
        )
        .unwrap_err();
        assert!(guessed.contains("HISTORY_ACCESS_UNAVAILABLE"), "{guessed}");
        let path = call_tool(
            &grant,
            "import_history_asset_read",
            &json!({"assetId": "asset-ok", "path": "C:\\\\secret.png"}),
        )
        .unwrap_err();
        assert!(path.contains("HISTORY_ACCESS_UNAVAILABLE"), "{path}");
        let traverse = call_tool(
            &grant,
            "import_history_asset_read",
            &json!({"assetId": "..\\\\asset-ok"}),
        )
        .unwrap_err();
        assert!(
            traverse.contains("HISTORY_ACCESS_UNAVAILABLE"),
            "{traverse}"
        );
    }

    fn import_names(servers: &Value) -> Vec<String> {
        servers
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("name")?.as_str().map(str::to_string))
            .collect()
    }

    fn grant_session_id(servers: &Value) -> Option<String> {
        let entry = servers
            .as_array()?
            .iter()
            .find(|v| v.get("name").and_then(|n| n.as_str()) == Some("import-history"))?;
        let raw = entry["env"][0]["value"].as_str()?;
        let v: Value = serde_json::from_str(raw).ok()?;
        v.get("appSessionId")?.as_str().map(str::to_string)
    }

    #[test]
    fn process_reuse_rebinds_grant_a_to_b_to_a_and_clears_on_release() {
        let user = json!([{ "name": "user-mcp", "command": "echo" }]);
        let local = SessionMcpPolicy::default();
        let a = grant("session-a", "tok-a");
        let b = grant("session-b", "tok-b");
        let servers_a = compose_session_mcp_servers(user.clone(), Some(&a), local);
        assert!(import_names(&servers_a).contains(&"user-mcp".into()));
        assert!(import_names(&servers_a).contains(&"import-history".into()));
        assert_eq!(grant_session_id(&servers_a).as_deref(), Some("session-a"));
        let servers_b = compose_session_mcp_servers(servers_a, Some(&b), local);
        assert_eq!(
            import_names(&servers_b)
                .iter()
                .filter(|n| *n == "import-history")
                .count(),
            1
        );
        assert_eq!(grant_session_id(&servers_b).as_deref(), Some("session-b"));
        let servers_a2 = compose_session_mcp_servers(servers_b, Some(&a), local);
        assert_eq!(grant_session_id(&servers_a2).as_deref(), Some("session-a"));
        let released = compose_session_mcp_servers(servers_a2, None, local);
        assert!(!import_names(&released).contains(&"import-history".into()));
        assert!(import_names(&released).contains(&"user-mcp".into()));
    }

    #[test]
    fn ordinary_session_and_remote_transports_never_inject_local_binary() {
        let user = json!([{ "name": "user-mcp", "command": "echo" }]);
        let g = grant("session-a", "tok-a");
        let ordinary = compose_session_mcp_servers(user.clone(), None, SessionMcpPolicy::default());
        assert_eq!(import_names(&ordinary), vec!["user-mcp".to_string()]);
        let ssh = compose_session_mcp_servers(
            user.clone(),
            Some(&g),
            SessionMcpPolicy {
                ssh: true,
                ..SessionMcpPolicy::default()
            },
        );
        assert!(!import_names(&ssh).contains(&"import-history".into()));
        let wsl = compose_session_mcp_servers(
            user.clone(),
            Some(&g),
            SessionMcpPolicy {
                wsl: true,
                ..SessionMcpPolicy::default()
            },
        );
        assert!(!import_names(&wsl).contains(&"import-history".into()));
        let leftover =
            compose_session_mcp_servers(user.clone(), Some(&g), SessionMcpPolicy::default());
        assert!(import_names(&leftover).contains(&"import-history".into()));
        let aux = compose_session_mcp_servers(
            leftover.clone(),
            Some(&g),
            SessionMcpPolicy {
                official_aux: true,
                ..SessionMcpPolicy::default()
            },
        );
        assert!(!import_names(&aux).contains(&"import-history".into()));
        let tcp = compose_session_mcp_servers(
            leftover.clone(),
            Some(&g),
            SessionMcpPolicy {
                tcp: true,
                ..SessionMcpPolicy::default()
            },
        );
        assert!(!import_names(&tcp).contains(&"import-history".into()));
        let ssh_from_leftover = compose_session_mcp_servers(
            leftover,
            Some(&g),
            SessionMcpPolicy {
                ssh: true,
                ..SessionMcpPolicy::default()
            },
        );
        assert!(!import_names(&ssh_from_leftover).contains(&"import-history".into()));
        let entry = mcp_server_entry(&g).unwrap();
        assert!(
            entry["command"].as_str().unwrap().contains("grok")
                || std::path::Path::new(entry["command"].as_str().unwrap()).is_absolute()
        );
        assert!(
            !ssh.to_string().contains(entry["command"].as_str().unwrap()),
            "SSH inject must not include the local binary path"
        );
        let env_names: Vec<&str> = entry["env"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert_eq!(env_names, vec![GRANT_ENV, "GROK_APP_HOME"]);
        assert_eq!(
            entry["env"][1]["value"].as_str().unwrap(),
            crate::paths::app_data_root().to_string_lossy()
        );
        assert!(
            !env_names.iter().any(|n| *n == "GROK_HOME" || *n == "HOME"),
            "import MCP must not rewrite the shared CLI home"
        );
    }

    #[test]
    fn new_load_and_fork_opens_rebind_the_same_current_grant() {
        let user = json!([{ "name": "user-mcp", "command": "echo" }]);
        let a = grant("session-a", "tok-a");
        let local = SessionMcpPolicy::default();
        let mut servers = user;
        for _ in 0..3 {
            servers = compose_session_mcp_servers(servers, Some(&a), local);
            assert_eq!(
                import_names(&servers)
                    .iter()
                    .filter(|n| *n == "import-history")
                    .count(),
                1
            );
            assert_eq!(grant_session_id(&servers).as_deref(), Some("session-a"));
        }
    }

    #[test]
    fn concurrent_compose_does_not_cross_bind_grants() {
        let user = json!([{ "name": "user-mcp", "command": "echo" }]);
        let a = grant("session-a", "tok-a");
        let b = grant("session-b", "tok-b");
        let local = SessionMcpPolicy::default();
        std::thread::scope(|s| {
            let ha = s.spawn(|| compose_session_mcp_servers(user.clone(), Some(&a), local));
            let hb = s.spawn(|| compose_session_mcp_servers(user.clone(), Some(&b), local));
            let sa = ha.join().unwrap();
            let sb = hb.join().unwrap();
            assert_eq!(grant_session_id(&sa).as_deref(), Some("session-a"));
            assert_eq!(grant_session_id(&sb).as_deref(), Some("session-b"));
        });
    }
