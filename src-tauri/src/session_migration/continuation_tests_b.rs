
    fn msg(id: &str, ordinal: u32, role: MessageRole, text: &str) -> ImportedEvent {
        ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        }
    }

    fn parent(from: &str, to: &str) -> crate::session_migration::types::EventRelation {
        crate::session_migration::types::EventRelation {
            kind: crate::session_migration::types::RelationKind::Parent,
            from: from.into(),
            to: to.into(),
            confidence: crate::session_migration::types::RelationConfidence::Explicit,
        }
    }

    #[test]
    fn default_selection_omits_system_reasoning_sidechain_and_images() {
        let mut assistant = msg("a1", 1, MessageRole::Assistant, "visible");
        if let ImportedEvent::Message { blocks, .. } = &mut assistant {
            blocks.push(ContentBlock::VisibleReasoning {
                text: "hidden-thought".into(),
            });
            blocks.push(ContentBlock::Asset {
                asset_id: "img-1".into(),
                alt: Some("shot.png".into()),
            });
        }
        let events = vec![
            msg("u1", 0, MessageRole::User, "hello"),
            assistant,
            msg("sys", 2, MessageRole::System, "source-system"),
            msg("side", 3, MessageRole::Assistant, "sidechain-secret"),
            msg("leaf", 4, MessageRole::User, "continue"),
        ];
        let relations = vec![
            parent("a1", "u1"),
            parent("sys", "a1"),
            parent("leaf", "sys"),
            crate::session_migration::types::EventRelation {
                kind: crate::session_migration::types::RelationKind::Subagent,
                from: "side".into(),
                to: "u1".into(),
                confidence: crate::session_migration::types::RelationConfidence::Explicit,
            },
            parent("side", "u1"),
        ];
        let sel = default_selection(&events, &relations, "leaf");
        assert!(!sel.fields.visible_reasoning);
        assert!(!sel.include_subagents);
        assert!(sel.asset_ids.is_empty());
        assert!(sel.event_ids.contains(&"u1".into()));
        assert!(sel.event_ids.contains(&"a1".into()));
        assert!(sel.event_ids.contains(&"leaf".into()));
        assert!(!sel.event_ids.iter().any(|id| id == "sys"));
        assert!(!sel.event_ids.iter().any(|id| id == "side"));
        let text = build_handoff_text_with_relations(&events, &relations, &sel, None).text;
        assert!(text.contains("hello"));
        assert!(text.contains("visible"));
        assert!(!text.contains("source-system"));
        assert!(!text.contains("sidechain-secret"));
        assert!(!text.contains("hidden-thought"));
        assert!(!sel.permits_event(&events[2], &relations));
        assert!(!sel.permits_event(&events[3], &relations));
    }

    #[test]
    fn sibling_leaf_selection_excludes_the_other_branch() {
        let events = vec![
            msg("u1", 0, MessageRole::User, "shared"),
            msg("b", 1, MessageRole::Assistant, "branch-b-only"),
            msg("c", 2, MessageRole::Assistant, "branch-c-only"),
        ];
        let relations = vec![parent("b", "u1"), parent("c", "u1")];
        let sel_c = default_selection(&events, &relations, "c");
        assert!(sel_c.event_ids.contains(&"c".into()));
        assert!(!sel_c.event_ids.iter().any(|id| id == "b"));
        let text = build_handoff_text_with_relations(&events, &relations, &sel_c, None).text;
        assert!(text.contains("shared"));
        assert!(text.contains("branch-c-only"));
        assert!(!text.contains("branch-b-only"));
    }

    #[test]
    fn sibling_file_change_facts_are_excluded_from_selected_leaf_handoff() {
        let events = vec![
            msg("u1", 0, MessageRole::User, "shared-root"),
            msg("b", 1, MessageRole::Assistant, "branch-b"),
            ImportedEvent::ToolCall {
                base: EventBase {
                    id: "tb".into(),
                    ordinal: 2,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                call_id: "call-b".into(),
                name: "apply_patch".into(),
                input: crate::session_migration::types::ToolInput::Json {
                    value: serde_json::json!({
                        "path": "sibling-b-only.rs",
                        "diff": "B-FILE-CHANGE"
                    }),
                },
                parent_message_id: Some("b".into()),
            },
            msg("c", 3, MessageRole::Assistant, "branch-c"),
            ImportedEvent::ToolCall {
                base: EventBase {
                    id: "tc".into(),
                    ordinal: 4,
                    spans: vec![],
                    timestamp: None,
                    time_origin: TimeOrigin::Unknown,
                },
                call_id: "call-c".into(),
                name: "apply_patch".into(),
                input: crate::session_migration::types::ToolInput::Json {
                    value: serde_json::json!({
                        "path": "leaf-c-only.rs",
                        "diff": "C-FILE-CHANGE"
                    }),
                },
                parent_message_id: Some("c".into()),
            },
        ];
        let relations = vec![
            parent("b", "u1"),
            parent("tb", "b"),
            parent("c", "u1"),
            parent("tc", "c"),
        ];
        let sel_c = default_selection(&events, &relations, "tc");
        assert!(sel_c.event_ids.contains(&"tc".into()));
        assert!(sel_c.event_ids.contains(&"c".into()));
        assert!(!sel_c.event_ids.iter().any(|id| id == "b" || id == "tb"));
        let text = build_handoff_text_with_relations(&events, &relations, &sel_c, None).text;
        assert!(text.contains("shared-root"));
        assert!(text.contains("C-FILE-CHANGE"));
        assert!(text.contains("leaf-c-only.rs"));
        assert!(
            !text.contains("B-FILE-CHANGE") && !text.contains("sibling-b-only.rs"),
            "sibling B file-change facts must stay out of leaf C handoff: {text}"
        );
    }

    #[test]
    fn excluding_secret_keeps_it_out_of_handoff_and_model_ids() {
        let events = vec![
            msg("u1", 0, MessageRole::User, "alpha"),
            msg("secret", 1, MessageRole::User, "synthetic-secret"),
        ];
        let sel = AllowedSelection {
            event_ids: vec!["u1".into()],
            ..AllowedSelection::default()
        };
        assert!(sel.permits_event(&events[0], &[]));
        assert!(!sel.permits_event(&events[1], &[]));
        let built = build_handoff_text_with_relations(&events, &[], &sel, None);
        assert!(built.text.contains("alpha"));
        assert!(!built.text.contains("synthetic-secret"));
        assert_eq!(built.fragment_event_ids, vec!["u1".to_string()]);
        assert_eq!(sel.model_event_ids(&events, &[]), vec!["u1".to_string()]);
    }

    #[test]
    fn changing_selection_changes_selection_hash() {
        let a = AllowedSelection {
            event_ids: vec!["u1".into(), "a1".into()],
            ..AllowedSelection::default()
        };
        let mut b = a.clone();
        b.event_ids.pop();
        assert_ne!(a.hash(), b.hash());
        let mut with_reasoning = a.clone();
        with_reasoning.fields.visible_reasoning = true;
        assert_ne!(a.hash(), with_reasoning.hash());
    }

    #[test]
    fn permits_event_cartesian_covers_event_branch_field_and_guessed_ids() {
        let user = msg("u1", 0, MessageRole::User, "alpha");
        let mut assistant = msg("a1", 1, MessageRole::Assistant, "beta");
        if let ImportedEvent::Message { blocks, .. } = &mut assistant {
            blocks.push(ContentBlock::VisibleReasoning {
                text: "think-secret".into(),
            });
        }
        let system = msg("sys", 2, MessageRole::System, "sys-secret");
        let side = msg("side", 3, MessageRole::Assistant, "side-secret");
        let guessed = msg("guessed", 4, MessageRole::User, "not-in-selection");
        let events = vec![user, assistant, system, side, guessed];
        let relations = vec![
            parent("a1", "u1"),
            crate::session_migration::types::EventRelation {
                kind: crate::session_migration::types::RelationKind::Subagent,
                from: "side".into(),
                to: "u1".into(),
                confidence: crate::session_migration::types::RelationConfidence::Explicit,
            },
        ];
        let sel = AllowedSelection {
            event_ids: vec!["u1".into(), "a1".into(), "sys".into(), "side".into()],
            ..AllowedSelection::default()
        };
        assert!(sel.permits_event(&events[0], &relations));
        assert!(sel.permits_event(&events[1], &relations));
        assert!(
            !sel.permits_event(&events[2], &relations),
            "system stays denied"
        );
        assert!(
            !sel.permits_event(&events[3], &relations),
            "sidechain denied until include_subagents"
        );
        assert!(
            !sel.permits_event(&events[4], &relations),
            "guessed ids are denied"
        );
        let mut with_sub = sel.clone();
        with_sub.include_subagents = true;
        assert!(with_sub.permits_event(&events[3], &relations));
        let text = build_handoff_text_with_relations(&events, &relations, &sel, None).text;
        assert!(text.contains("alpha") && text.contains("beta"));
        assert!(!text.contains("sys-secret"));
        assert!(!text.contains("side-secret"));
        assert!(!text.contains("think-secret"));
        assert!(!text.contains("not-in-selection"));
    }

    #[test]
    fn pinned_over_budget_keeps_constraint_and_flags_instead_of_dropping() {
        let pin = "MUST-KEEP-PIN ".repeat(400);
        let events = vec![
            msg("e0", 0, MessageRole::User, &pin),
            msg("e1", 1, MessageRole::User, "later-history"),
        ];
        let selection = AllowedSelection {
            event_ids: vec!["e0".into(), "e1".into()],
            pinned_event_ids: vec!["e0".into()],
            ..AllowedSelection::default()
        };
        let built = build_handoff_text(&events, &selection, Some(200));
        assert_eq!(built.coverage, "pinned-over-budget");
        assert!(built.text.contains("MUST-KEEP-PIN"));
        assert!(built.fragment_event_ids.contains(&"e0".to_string()));
    }

    #[test]
    fn truncated_fragment_is_a_subset_of_the_tool_readable_set() {
        let events: Vec<_> = (0..40)
            .map(|n| {
                msg(
                    &format!("e{n}"),
                    n,
                    MessageRole::User,
                    &format!("row-{n}-{}", "x".repeat(80)),
                )
            })
            .collect();
        let selection = AllowedSelection {
            event_ids: events.iter().map(|e| e.id().to_string()).collect(),
            ..AllowedSelection::default()
        };
        let built = build_handoff_text(&events, &selection, Some(4_000));
        let readable = selection.model_event_ids(&events, &[]);
        assert_eq!(readable.len(), 40);
        assert!(!built.fragment_event_ids.is_empty());
        assert!(built.fragment_event_ids.len() < readable.len());
        for id in &built.fragment_event_ids {
            assert!(readable.contains(id), "{id} in fragment must stay readable");
        }
        assert_eq!(built.coverage, "truncated-to-budget");
    }

    #[test]
    fn full_mode_send_rejects_self_tools_list_and_stays_prepared() {
        use crate::session_migration::capability::{
            ProbeEvidence, IMPORT_HISTORY_READ, IMPORT_HISTORY_SEARCH,
        };
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-send-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = "sess-probe-self";
        prepared_sidecar(sid);
        write_draft_text(sid, "fixed-handoff").unwrap();
        let listed = crate::session_migration::mcp_stdio::handle_method(
            &crate::session_migration::mcp_stdio::ImportHistoryGrant {
                token: "g".into(),
                app_session_id: sid.into(),
                import_id: "imp:x".into(),
                revision_id: "r".into(),
                branch_leaf: "l".into(),
                allowed_event_ids: vec!["e1".into()],
                allowed_selection_hash: "h".into(),
                selection: None,
            },
            "tools/list",
            &serde_json::json!({}),
        )
        .unwrap();
        let names: Vec<String> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        assert!(names.contains(&IMPORT_HISTORY_SEARCH.to_string()));
        assert!(names.contains(&IMPORT_HISTORY_READ.to_string()));
        record_advertised_tools(sid, names, ProbeEvidence::SelfToolsList).unwrap();
        let err = continuation_dispatch_handoff(sid, SessionMcpPolicy::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(err.message, "HISTORY_TOOLS_UNVERIFIED");
        assert_eq!(
            read_sidecar(sid).unwrap().state,
            DispatchState::Prepared,
            "blocked full send must not mark dispatching"
        );
        let err = apply_continuation_first_send(sid, "continue", SessionMcpPolicy::default())
            .unwrap_err();
        assert_eq!(err.message, "HISTORY_TOOLS_UNVERIFIED");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dispatch_does_not_panic_when_sidecar_vanishes_between_reads() {
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("cont-sidecar-race");
        let sid = "sess-sidecar-race";
        prepared_sidecar(sid);
        let mut side = read_sidecar(sid).unwrap();
        side.runtime_capability = "fragment-only".into();
        write_sidecar(sid, &side).unwrap();
        write_draft_text(sid, "fixed-handoff").unwrap();
        std::env::set_var("GROK_TEST_DROP_SIDECAR", "1");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            continuation_dispatch_handoff(sid, SessionMcpPolicy::default())
        }));
        std::env::remove_var("GROK_TEST_DROP_SIDECAR");
        assert!(
            result.is_ok(),
            "sidecar disappearing between reads must return a stable Result, not panic"
        );
        let outcome = result.unwrap();
        assert!(
            matches!(outcome, Ok(None) | Err(_)),
            "vanished sidecar must not dispatch, got {outcome:?}"
        );
        let _ = home;
    }

    #[test]
    fn fragment_mode_send_allowed_without_agent_tool_list() {
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-frag-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = "sess-probe-frag";
        prepared_sidecar(sid);
        let mut side = read_sidecar(sid).unwrap();
        side.runtime_capability = "fragment-only".into();
        write_sidecar(sid, &side).unwrap();
        write_draft_text(sid, "fixed-handoff").unwrap();
        let text = continuation_dispatch_handoff(sid, SessionMcpPolicy::default())
            .unwrap()
            .expect("fragment send");
        assert_eq!(text, "fixed-handoff");
        assert_eq!(read_sidecar(sid).unwrap().state, DispatchState::Prepared);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn meta_tools_update_unlocks_full_mode_send() {
        use crate::session_migration::capability::{
            extract_advertised_import_tools, ProbeEvidence, IMPORT_HISTORY_READ,
            IMPORT_HISTORY_SEARCH,
        };
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-agent-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = "sess-probe-agent";
        prepared_sidecar(sid);
        write_draft_text(sid, "fixed-handoff").unwrap();
        let update = serde_json::json!({
            "sessionId": "agent-session",
            "update": {
                "sessionUpdate": "tool_status",
                "_meta": {
                    "tools": [
                        {"name": format!("import-history__{IMPORT_HISTORY_SEARCH}")},
                        {"name": format!("import-history__{IMPORT_HISTORY_READ}")}
                    ]
                }
            }
        });
        record_advertised_tools(
            sid,
            extract_advertised_import_tools(&update),
            ProbeEvidence::AgentAdvertised,
        )
        .unwrap();
        let text = continuation_dispatch_handoff(sid, SessionMcpPolicy::default())
            .unwrap()
            .expect("full send");
        assert_eq!(text, "fixed-handoff");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ssh_policy_blocks_full_send_even_with_advertised_names() {
        use crate::session_migration::capability::{
            ProbeEvidence, IMPORT_HISTORY_READ, IMPORT_HISTORY_SEARCH,
        };
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-ssh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = "sess-probe-ssh";
        prepared_sidecar(sid);
        write_draft_text(sid, "fixed-handoff").unwrap();
        record_advertised_tools(
            sid,
            vec![IMPORT_HISTORY_SEARCH.into(), IMPORT_HISTORY_READ.into()],
            ProbeEvidence::AgentAdvertised,
        )
        .unwrap();
        let err = continuation_dispatch_handoff(
            sid,
            SessionMcpPolicy {
                ssh: true,
                ..SessionMcpPolicy::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.message, "HISTORY_TOOLS_REMOTE_DISABLED");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_journal_prepared_self_list_blocks_first_send_via_apply_gate() {
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let sid = uuid::Uuid::new_v4().to_string();
        prepared_sidecar(&sid);
        write_draft_text(&sid, "fixed-handoff").unwrap();
        let messages_dir = crate::paths::app_data_root().join("sessions").join(&sid);
        let journal = messages_dir.join("messages.json");
        assert!(
            !journal.exists()
                || std::fs::read_to_string(&journal)
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                || std::fs::read_to_string(&journal).unwrap_or_default() == "[]",
            "continue_create equivalent: empty native journal"
        );
        let err =
            apply_continuation_first_send(&sid, "hello from composer", SessionMcpPolicy::default())
                .expect_err("empty journal must not skip the probe");
        assert_eq!(err.code, ErrorCode::HistoryAccessUnavailable);
        assert_eq!(err.message, "HISTORY_TOOLS_UNVERIFIED");
        assert_eq!(read_sidecar(&sid).unwrap().state, DispatchState::Prepared);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ordinary_session_first_send_is_passthrough() {
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-ord-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = uuid::Uuid::new_v4().to_string();
        match apply_continuation_first_send(&sid, "hello", SessionMcpPolicy::default()).unwrap() {
            ContinuationFirstSend::Passthrough => {}
            ContinuationFirstSend::Handoff { .. } => {
                panic!("ordinary session must not attach handoff")
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn available_commands_update_does_not_unlock_first_send_after_record() {
        use crate::session_migration::capability::{
            extract_advertised_import_tools, ProbeEvidence,
        };
        use crate::session_migration::mcp_stdio::SessionMcpPolicy;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("probe-acmd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let sid = uuid::Uuid::new_v4().to_string();
        prepared_sidecar(&sid);
        write_draft_text(&sid, "fixed-handoff").unwrap();
        let blocked = apply_continuation_first_send(&sid, "hello", SessionMcpPolicy::default())
            .expect_err("no CLI advertisement yet");
        assert_eq!(blocked.message, "HISTORY_TOOLS_UNVERIFIED");
        let params = serde_json::json!({
            "update": {
                "sessionUpdate": "available_commands_update",
                "availableCommands": [
                    {"name": "import-history__import_history_search"},
                    {"name": "import-history__import_history_read"}
                ]
            }
        });
        let names = extract_advertised_import_tools(&params);
        assert!(names.is_empty(), "slash commands are not MCP tool evidence");
        record_advertised_tools(&sid, names, ProbeEvidence::AgentAdvertised).unwrap();
        let blocked = apply_continuation_first_send(&sid, "hello", SessionMcpPolicy::default())
            .expect_err("slash availableCommands must not unlock full-history handoff");
        assert_eq!(blocked.message, "HISTORY_TOOLS_NOT_EXPOSED");
        assert_eq!(read_sidecar(&sid).unwrap().state, DispatchState::Prepared);
        let _ = std::fs::remove_dir_all(&tmp);
    }
