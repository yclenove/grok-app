
    fn rec(i: u32, json: &str) -> JsonlRecord {
        JsonlRecord {
            index: i,
            byte_start: 0,
            byte_end: 0,
            value: serde_json::from_str(json).unwrap(),
        }
    }

    #[test]
    fn stream_and_final_collapse_but_real_repeat_remains() {
        let records = vec![
            rec(
                0,
                r#"{"type":"session_meta","payload":{"id":"g4","cwd":"C:\\r"}}"#,
            ),
            rec(
                1,
                r#"{"type":"event_msg","payload":{"type":"agent_message_delta","delta":"same"}}"#,
            ),
            rec(
                2,
                r#"{"type":"event_msg","payload":{"type":"agent_message","message":"same"}}"#,
            ),
            rec(
                3,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"same"}]}}"#,
            ),
            rec(
                4,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"again"}]}}"#,
            ),
            rec(
                5,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"same"}]}}"#,
            ),
        ];
        let prefix = crate::session_migration::jsonl::JsonlPrefix {
            records: records.clone(),
            complete_prefix_sha256: "x".into(),
            file_sha256: "x".into(),
            byte_length: 1,
            trailing_incomplete: false,
            issues: vec![],
        };
        let tmp = std::env::temp_dir().join("codex-g4.jsonl");
        std::fs::write(&tmp, "").unwrap();
        let out = normalize_records(records.into_iter().map(Ok), &tmp, &prefix).unwrap();
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
        assert_eq!(
            texts,
            vec!["same".to_string(), "again".into(), "same".into()]
        );
        assert_eq!(out.graph.events[0].base().spans.len(), 2);
    }

    #[test]
    fn real_repeated_sends_and_replies_are_preserved_even_without_intervening_role() {
        let rows = [
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"继续"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"继续"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"继续"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","id":"a","content":[{"type":"output_text","text":"完成"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","id":"b","content":[{"type":"output_text","text":"完成"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"完成"}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
        let messages: Vec<_> = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::Message { .. }))
            .collect();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].searchable_text(), "继续");
        assert_eq!(messages[1].searchable_text(), "继续");
        assert_eq!(messages[1].base().spans.len(), 2);
        assert_eq!(messages[2].id(), "codex:a");
        assert_eq!(messages[3].id(), "codex:b");
    }

    #[test]
    fn actual_rollout_rollback_keeps_old_branch_but_selects_surviving_history() {
        let rows = [
            r#"{"type":"response_item","payload":{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":"first"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"a1","role":"assistant","content":[{"type":"output_text","text":"kept"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"u2","role":"user","content":[{"type":"input_text","text":"second"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"a2","role":"assistant","content":[{"type":"output_text","text":"discarded branch"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"thread_rolled_back","num_turns":1}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"u3","role":"user","content":[{"type":"input_text","text":"new direction"}]}}"#,
            r#"{"type":"compacted","payload":{"message":"Keep the first constraint"}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
        assert_eq!(out.graph.events.len(), 7);
        let chain = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            out.graph.active_leaf.as_deref().unwrap(),
        );
        assert_eq!(
            chain
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .map(|e| e.id())
                .collect::<Vec<_>>(),
            vec!["codex:u1", "codex:a1", "codex:u3"]
        );
        assert_eq!(
            chain.last().unwrap().searchable_text(),
            "Keep the first constraint"
        );
        let old = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "codex:a2",
        );
        assert_eq!(old.len(), 4);
        assert!(
            crate::session_migration::graph::validate(&out.graph.events, &out.graph.relations)
                .issues
                .is_empty()
        );
    }

    #[test]
    fn replacement_history_covers_old_branch_without_splicing_timelines() {
        let rows = [
            r#"{"type":"response_item","payload":{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":"first"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"a1","role":"assistant","content":[{"type":"output_text","text":"long history"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"u2","role":"user","content":[{"type":"input_text","text":"second"}]}}"#,
            r#"{"type":"compacted","payload":{"message":"Keep the first constraint","replacement_history":[{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":"first"}]},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Keep the first constraint"}]}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"u3","role":"user","content":[{"type":"input_text","text":"after compact"}]}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
        let compact = out
            .graph
            .events
            .iter()
            .find(|e| matches!(e, ImportedEvent::Compaction { .. }))
            .unwrap();
        match compact {
            ImportedEvent::Compaction {
                coverage_known,
                covered_event_ids,
                summary,
                ..
            } => {
                assert!(*coverage_known);
                let covered = covered_event_ids.as_ref().unwrap();
                assert!(covered.iter().any(|id| id == "codex:u1"));
                assert!(covered.iter().any(|id| id == "codex:u2"));
                assert_eq!(summary[0].plain_text(), Some("Keep the first constraint"));
            }
            _ => panic!("compaction"),
        }
        let live = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            out.graph.active_leaf.as_deref().unwrap(),
        );
        let live_ids: Vec<_> = live.iter().map(|e| e.id()).collect();
        assert!(live_ids.contains(&"codex:u3"));
        assert!(
            !live_ids.contains(&"codex:u2"),
            "post-compact live chain must not splice the covered user turn: {live_ids:?}"
        );
        let old = crate::session_migration::graph::lineage(
            &out.graph.events,
            &out.graph.relations,
            "codex:u2",
        );
        assert!(old.iter().any(|e| e.id() == "codex:u2"));
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| e.searchable_text() == "first")
                .count(),
            1,
            "surviving replacement items must not duplicate existing events"
        );
    }

    #[test]
    fn four_messages_keep_source_time_and_ordinal_when_clocks_go_backwards() {
        let rows = [
            r#"{"timestamp":"2026-01-01T00:00:04Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"u1"}]}}"#,
            r#"{"timestamp":"2026-01-01T00:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"a1"}]}}"#,
            r#"{"timestamp":"2026-01-01T00:00:10Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"u2"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"a2"}]}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
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
                    base.ordinal,
                    base.timestamp.clone(),
                    base.time_origin,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].0, MessageRole::User);
        assert_eq!(msgs[1].0, MessageRole::Assistant);
        assert_eq!(msgs[2].0, MessageRole::User);
        assert_eq!(msgs[3].0, MessageRole::Assistant);
        assert_eq!(
            msgs.iter().map(|m| m.2).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(msgs[0].3.as_deref(), Some("2026-01-01T00:00:04Z"));
        assert_eq!(msgs[1].3.as_deref(), Some("2026-01-01T00:00:03Z"));
        assert_eq!(msgs[3].3, None);
        assert_eq!(msgs[3].4, TimeOrigin::Unknown);
    }

    #[test]
    fn encrypted_reasoning_is_unsupported_and_not_decoded() {
        let rows = [
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"why"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","encrypted_content":"SECRET-BLOB"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(out.graph.events.iter().any(|e| matches!(
            e,
            ImportedEvent::Unsupported {
                source_type,
                payload: None,
                ..
            } if source_type == "reasoning"
        )));
        let dump = serde_json::to_string(&out.graph).unwrap();
        assert!(
            !dump.contains("SECRET-BLOB"),
            "encrypted reasoning must not be copied into the graph: {dump}"
        );
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn broken_middle_record_is_partial_and_keeps_later_turns() {
        let tmp = std::env::temp_dir().join(format!("codex-f06-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(
            &tmp,
            concat!(
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"one"}]}}"#,
                "\n{not json}\n",
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"two"}]}}"#,
                "\n"
            ),
        )
        .unwrap();
        let out = normalize_file(&tmp).unwrap();
        assert_eq!(out.outcome, ImportOutcome::Partial);
        assert!(
            out.issues.iter().any(|i| i.code == "broken-record"),
            "{:?}",
            out.issues
        );
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
        assert_eq!(texts, vec!["one".to_string(), "two".into()]);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn trailing_half_line_fixes_prefix_until_append_completes() {
        let tmp = std::env::temp_dir().join(format!("codex-f07-{}.jsonl", uuid::Uuid::new_v4()));
        let user = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}"#;
        let assistant_partial = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"par"#;
        let assistant = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"part"}]}}"#;
        let next = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"next"}]}}"#;
        std::fs::write(&tmp, format!("{user}\n{assistant_partial}")).unwrap();
        let partial = normalize_file(&tmp).unwrap();
        assert_eq!(partial.outcome, ImportOutcome::Partial);
        assert!(partial
            .issues
            .iter()
            .any(|i| i.code == "trailing-incomplete"));
        assert_eq!(
            partial
                .graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            1
        );
        let prefix = partial.evidence.artifacts[0]
            .complete_prefix_sha256
            .clone()
            .unwrap();
        std::fs::write(&tmp, format!("{user}\n{assistant}\n{next}\n")).unwrap();
        let complete = normalize_file(&tmp).unwrap();
        assert_ne!(
            complete.evidence.artifacts[0]
                .complete_prefix_sha256
                .as_deref(),
            Some(prefix.as_str())
        );
        assert_eq!(
            complete
                .graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            3
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn metadata_only_projection_is_unsupported_not_an_empty_session() {
        let tmp = std::env::temp_dir().join(format!("codex-meta-{}.db", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"not a transcript").unwrap();
        let out = metadata_only_db(&tmp);
        assert_eq!(out.outcome, ImportOutcome::Unsupported);
        assert!(out.graph.events.is_empty());
        assert!(out.issues.iter().any(|i| i.code == "metadata-only"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fork_metadata_does_not_import_another_file() {
        let rows = [
            r#"{"type":"response_item","payload":{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":"root"}]}}"#,
            r#"{"type":"fork","payload":{"parent_thread_id":"other-session"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","id":"u2","role":"user","content":[{"type":"input_text","text":"forked"}]}}"#,
        ];
        let out = normalize_records(
            rows.iter().enumerate().map(|(i, s)| Ok(rec(i as u32, s))),
            Path::new("synthetic.jsonl"),
            &JsonlPrefix::default(),
        )
        .unwrap();
        assert!(out.graph.events.iter().any(|e| matches!(
            e,
            ImportedEvent::Metadata {
                source_type,
                ..
            } if source_type == "fork"
        )));
        assert_eq!(
            out.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            2
        );
        assert!(out
            .graph
            .relations
            .iter()
            .any(|r| r.kind == RelationKind::Fork));
    }
