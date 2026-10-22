
    fn selection(events: &[&str], assets: &[&str]) -> AllowedSelection {
        AllowedSelection {
            event_ids: events.iter().map(|value| (*value).to_string()).collect(),
            asset_ids: assets.iter().map(|value| (*value).to_string()).collect(),
            ..AllowedSelection::default()
        }
    }

    #[test]
    fn selection_hash_is_canonical_but_semantic_changes_are_visible() {
        let a = selection(&["e2", "e1"], &["asset-b", "asset-a"]);
        let mut reordered = selection(&["e1", "e2"], &["asset-a", "asset-b"]);
        assert_eq!(a.hash(), reordered.hash(), "set order is not semantic");

        reordered.fields.tool_call_input = false;
        assert_ne!(a.hash(), reordered.hash(), "field grants are semantic");
        let mut asset_changed = a.clone();
        asset_changed.asset_ids.pop();
        assert_ne!(a.hash(), asset_changed.hash(), "asset grants are semantic");
        assert_ne!(a.hash_for_leaf("leaf-a"), a.hash_for_leaf("leaf-b"));
    }

    #[test]
    fn malformed_selection_is_rejected_instead_of_normalized_silently() {
        let duplicate = selection(&["e1", "e1"], &[]);
        assert!(duplicate.validate_shape().is_err());

        let mut bad_pin = selection(&["e1"], &[]);
        bad_pin.pinned_event_ids.push("not-selected".into());
        assert!(bad_pin.validate_shape().is_err());

        let mut old = selection(&["e1"], &[]);
        old.version = 1;
        assert!(old.validate_shape().is_err());
    }

    #[test]
    fn selection_set_hash_is_independent_per_preview_and_order_stable() {
        let one = PreviewSelection {
            preview_id: "preview-a".into(),
            leaf_id: "leaf-a".into(),
            selection: selection(&["a1"], &[]),
        };
        let two = PreviewSelection {
            preview_id: "preview-b".into(),
            leaf_id: "leaf-b".into(),
            selection: selection(&["b1"], &[]),
        };
        let a = SelectionSet {
            entries: vec![one.clone(), two.clone()],
            ..SelectionSet::default()
        };
        let b = SelectionSet {
            entries: vec![two, one],
            ..SelectionSet::default()
        };
        assert_eq!(a.hash(), b.hash());

        let mut changed = a.clone();
        changed.entries[1].selection.event_ids = vec!["b2".into()];
        assert_ne!(a.hash(), changed.hash());
    }

    const GOLDEN_V2: &str = include_str!("../../../src/lib/sessionImport/selectionV2.golden.json");

    #[test]
    fn v2_golden_json_roundtrips_and_canonicalizes() {
        let value: serde_json::Value = serde_json::from_str(GOLDEN_V2).unwrap();
        let stored = parse_stored_selection(&value).unwrap();
        assert!(!stored.requires_reprepare);
        assert_eq!(stored.selection.version, 2);
        assert_eq!(stored.selection.event_ids, vec!["e1", "e2"]);
        assert_eq!(stored.selection.pinned_event_ids, vec!["e1"]);
        assert_eq!(stored.selection.asset_ids, vec!["asset-a"]);
        assert!(!stored.selection.fields.tool_call_input);
        assert!(!stored.selection.fields.visible_reasoning);
        let expected = crate::session_migration::hash::canonical_json(&serde_json::json!({
            "assetIds": ["asset-a"],
            "eventIds": ["e1", "e2"],
            "fields": {
                "compactionSummary": true,
                "messageText": true,
                "toolCallInput": false,
                "toolCallName": true,
                "toolResultContent": true,
                "visibleReasoning": false
            },
            "includeSubagents": false,
            "pinnedEventIds": ["e1"],
            "version": 2
        }));
        assert_eq!(stored.selection.canonical_json_string(), expected);
    }

    #[test]
    fn v1_draft_is_minimal_and_requires_reprepare() {
        let raw = serde_json::json!({
            "eventIds": ["e1", "e2"],
            "pinnedEventIds": ["e1"],
            "includeReasoning": true,
            "includeSubagents": true,
            "includeImages": true
        });
        let stored = parse_stored_selection(&raw).unwrap();
        assert!(stored.requires_reprepare);
        assert_eq!(stored.selection.version, 2);
        assert!(stored.selection.fields.visible_reasoning);
        assert!(stored.selection.include_subagents);
        assert!(
            stored.selection.asset_ids.is_empty(),
            "v1 includeImages must not invent an asset allowlist"
        );
        let v1_versioned = serde_json::json!({
            "version": 1,
            "eventIds": ["e1"],
            "includeReasoning": false,
            "includeSubagents": false,
            "includeImages": false
        });
        let stored = parse_stored_selection(&v1_versioned).unwrap();
        assert!(stored.requires_reprepare);
        assert!(!stored.selection.fields.visible_reasoning);
    }

    #[test]
    fn too_new_selection_is_rejected() {
        let raw = serde_json::json!({"version": 3, "eventIds": ["e1"]});
        let err = parse_stored_selection(&raw).unwrap_err();
        assert_eq!(err.code, ErrorCode::ImportSchemaTooNew);
    }

    #[test]
    fn live_v2_missing_fields_and_unknown_fields_are_rejected() {
        let raw = serde_json::json!({"eventIds": ["e1"]});
        assert!(parse_stored_selection(&raw).is_err());

        let mut full: serde_json::Value = serde_json::from_str(GOLDEN_V2).unwrap();
        full.as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        assert!(parse_stored_selection(&full).is_err());
    }

    #[test]
    fn live_selection_set_rejects_zero_two_duplicate_and_unknown_before_canonicalizing() {
        let one = PreviewSelection {
            preview_id: "preview-a".into(),
            leaf_id: "leaf-a".into(),
            selection: selection(&["e1"], &[]),
        };
        let empty = serde_json::json!({"version": 2, "entries": []});
        assert!(parse_live_selection_set(&empty).is_err());

        let mut second = one.clone();
        second.preview_id = "preview-b".into();
        let two = SelectionSet {
            version: SELECTION_SCHEMA_VERSION,
            entries: vec![one.clone(), second],
        };
        assert!(two.single_entry().is_err());

        let mut duplicate = one.clone();
        duplicate.selection.event_ids = vec!["e1".into(), "e1".into()];
        let raw = serde_json::to_value(SelectionSet::single(duplicate)).unwrap();
        assert!(parse_live_selection_set(&raw).is_err());

        let mut unknown = serde_json::to_value(SelectionSet::single(one)).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::json!(1));
        assert!(parse_live_selection_set(&unknown).is_err());
    }

    fn msg(id: &str, ordinal: u32, role: MessageRole) -> ImportedEvent {
        ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role,
            blocks: vec![ContentBlock::text(id)],
            complete: true,
        }
    }

    #[test]
    fn unknown_and_out_of_branch_ids_are_rejected() {
        let events = vec![
            msg("root", 0, MessageRole::User),
            msg("leaf-a", 1, MessageRole::Assistant),
            msg("leaf-b", 2, MessageRole::Assistant),
        ];
        let relations = vec![
            EventRelation {
                kind: RelationKind::Parent,
                from: "leaf-a".into(),
                to: "root".into(),
                confidence: RelationConfidence::Explicit,
            },
            EventRelation {
                kind: RelationKind::Parent,
                from: "leaf-b".into(),
                to: "root".into(),
                confidence: RelationConfidence::Explicit,
            },
        ];
        let missing = AllowedSelection::for_events(vec!["nope".into()]);
        assert!(missing
            .validate_for_branch(&events, &relations, "leaf-a")
            .is_err());
        let sibling = AllowedSelection::for_events(vec!["root".into(), "leaf-b".into()]);
        assert!(sibling
            .validate_for_branch(&events, &relations, "leaf-a")
            .is_err());
        let ok = AllowedSelection::for_events(vec!["root".into(), "leaf-a".into()]);
        assert!(ok
            .validate_for_branch(&events, &relations, "leaf-a")
            .is_ok());
    }

    #[test]
    fn model_projection_uses_one_evaluator_for_fields_and_never_leaks_denied() {
        let user = msg("u1", 0, MessageRole::User);
        let mut assistant = msg("a1", 1, MessageRole::Assistant);
        if let ImportedEvent::Message { blocks, .. } = &mut assistant {
            blocks.push(ContentBlock::VisibleReasoning {
                text: "think-secret".into(),
            });
            blocks.push(ContentBlock::Asset {
                asset_id: "pic".into(),
                alt: Some("asset-secret".into()),
            });
        }
        let system = msg("sys", 2, MessageRole::System);
        let tool = ImportedEvent::ToolCall {
            base: EventBase {
                id: "t1".into(),
                ordinal: 3,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            call_id: "c1".into(),
            name: "apply_patch".into(),
            input: ToolInput::Json {
                value: serde_json::json!({"path": "secret.diff"}),
            },
            parent_message_id: None,
        };
        let result = ImportedEvent::ToolResult {
            base: EventBase {
                id: "r1".into(),
                ordinal: 4,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            call_id: "c1".into(),
            blocks: vec![ContentBlock::text("tool-secret")],
            outcome: ToolOutcome::Success,
        };
        let compact = ImportedEvent::Compaction {
            base: EventBase {
                id: "k1".into(),
                ordinal: 5,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            summary: vec![ContentBlock::text("compact-secret")],
            covered_event_ids: None,
            coverage_known: false,
        };
        let meta = ImportedEvent::Metadata {
            base: EventBase {
                id: "m1".into(),
                ordinal: 6,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            source_type: "meta-secret".into(),
            payload: None,
        };
        let events = [user, assistant, system, tool, result, compact, meta];
        let mut sel = AllowedSelection::for_events(vec![
            "u1".into(),
            "a1".into(),
            "sys".into(),
            "t1".into(),
            "r1".into(),
            "k1".into(),
            "m1".into(),
        ]);
        sel.fields.tool_call_input = false;
        sel.fields.tool_result_content = false;
        sel.fields.compaction_summary = false;
        assert!(sel.permits_event(&events[0], &[]));
        assert!(!sel.permits_event(&events[2], &[]));
        assert!(!sel.permits_event(&events[6], &[]));
        assert!(!sel.permits_block(
            &events[1],
            &ContentBlock::VisibleReasoning { text: "x".into() }
        ));
        assert!(!sel.permits_asset("pic"));
        assert!(sel.permits_tool_field(ToolField::Name));
        assert!(!sel.permits_tool_field(ToolField::Input));
        let joined: String = events
            .iter()
            .filter_map(|event| sel.project_event(event, &[]))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("u1") && joined.contains("a1"));
        assert!(joined.contains("apply_patch"));
        assert!(!joined.contains("think-secret"));
        assert!(!joined.contains("asset-secret"));
        assert!(!joined.contains("secret.diff"));
        assert!(!joined.contains("tool-secret"));
        assert!(!joined.contains("compact-secret"));
        assert!(!joined.contains("meta-secret"));
        assert!(!joined.contains("sys"));
        sel.fields.visible_reasoning = true;
        sel.asset_ids = vec!["pic".into()];
        sel.fields.tool_call_input = true;
        let opened = sel.project_event(&events[1], &[]).unwrap();
        assert!(opened.contains("think-secret"));
        assert!(opened.contains("asset-secret"));
        assert!(sel
            .project_event(&events[3], &[])
            .unwrap()
            .contains("secret.diff"));
    }
