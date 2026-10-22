    #[test]
    fn changing_selection_invalidates_plan_hash() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("plan-sel-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), r#"[{"role":"user","content":"alpha"}]"#).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
        let a = details.default_selection;
        assert!(!a.event_ids.is_empty());
        let mut b = a.clone();
        b.event_ids.clear();
        let plan_a = plan_with_selection(vec![pid.clone()], maps.clone(), Some(&a)).unwrap();
        let plan_b = plan_with_selection(vec![pid.clone()], maps.clone(), Some(&b)).unwrap();
        assert_ne!(plan_a.plan_hash, plan_b.plan_hash);
        assert_ne!(plan_a.selection_hash, plan_b.selection_hash);
        assert_eq!(plan_a.selection_set.entries.len(), 1);
        assert_eq!(
            plan_a.selection_set.entries[0].selection.event_ids,
            a.event_ids
        );
        let mut invalid = a;
        invalid.event_ids.push("event-not-in-preview".into());
        let err = plan_with_selection(vec![pid], maps, Some(&invalid)).unwrap_err();
        assert_eq!(err.code, ErrorCode::HistoryAccessUnavailable);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn changing_selection_changes_draft_hash_and_omits_secret() {
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("draft-sel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        let adapter = AdapterOutput {
            evidence: SourceEvidence {
                provider: Prov::GenericFile,
                source_version: None,
                schema_fingerprint: "t".into(),
                adapter_version: "1.0.0".into(),
                platform: "test".into(),
                artifacts: vec![],
            },
            graph: super::super::types::NormalizedGraph {
                events: vec![
                    msg("u1", 0, super::super::types::MessageRole::User, "alpha"),
                    msg(
                        "secret",
                        1,
                        super::super::types::MessageRole::User,
                        "synthetic-secret",
                    ),
                ],
                relations: vec![parent("secret", "u1")],
                active_leaf: Some("secret".into()),
                native_session_id: "n-sel".into(),
                title: Some("n-sel".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        };
        let published = publish_item(
            &conn,
            "job-sel",
            &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: "c-sel".into(),
                installation_id: "inst-sel".into(),
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
                selection_set: SelectionSet::default(),
            }),
            &|| false,
        )
        .unwrap();
        let import_id = published.import_id.unwrap();
        let rev = published.revision_id.unwrap();
        let keep = crate::session_migration::continuation::AllowedSelection {
            event_ids: vec!["u1".into()],
            ..Default::default()
        };
        let all = crate::session_migration::continuation::AllowedSelection {
            event_ids: vec!["u1".into(), "secret".into()],
            ..Default::default()
        };
        let published = continue_prepare_with_selection(
            &import_id,
            Some(&rev),
            Some("secret"),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            published.text.contains("synthetic-secret"),
            "published default selection includes the selected leaf user events"
        );
        let narrowed = continue_prepare_with_selection(
            &import_id, Some(&rev), Some("secret"), None, None, Some(keep.clone()),
        ).unwrap();
        assert_eq!(narrowed.selection.hash(), keep.hash());
        assert_ne!(narrowed.draft_hash, published.draft_hash);
        assert!(!narrowed.text.contains("synthetic-secret"));
        assert!(continue_restore(&published.draft_id, &published.draft_hash).is_err());
        let model = export_for_model(&import_id, Some(&rev), Some("secret"), &keep).unwrap();
        assert!(model.contains("alpha"));
        assert!(!model.contains("synthetic-secret"));
        let mut outside = keep.clone();
        outside.event_ids.push("not-published".into());
        assert!(continue_prepare_with_selection(
            &import_id, Some(&rev), Some("secret"), None, None, Some(outside.clone()),
        ).is_err());
        assert!(export_for_model(&import_id, Some(&rev), Some("secret"), &outside).is_err());
        let same = continue_prepare_with_selection(
            &import_id,
            Some(&rev),
            Some("secret"),
            None,
            None,
            Some(all),
        )
        .unwrap();
        assert_eq!(same.selection.hash(), published.selection.hash());
        let dir = crate::session_migration::catalog::revision_dir(&import_id, &rev).unwrap();
        let allow: std::collections::HashSet<_> = keep.event_ids.iter().cloned().collect();
        let (hits, _) = crate::session_migration::search::search(
            &dir,
            "synthetic-secret",
            None,
            Some(10),
            Some(&allow),
        )
        .unwrap();
        assert!(
            hits.is_empty(),
            "model search must honor the same allow set: {hits:?}"
        );
        let (hits, _) = crate::session_migration::search::search(
            &dir,
            "synthetic-secret",
            None,
            Some(10),
            None,
        )
        .unwrap();
        assert_eq!(
            hits.len(),
            1,
            "local UI search can still see the source event"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn msg(
        id: &str,
        ordinal: u32,
        role: super::super::types::MessageRole,
        text: &str,
    ) -> super::super::types::ImportedEvent {
        super::super::types::ImportedEvent::Message {
            base: super::super::types::EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: super::super::types::TimeOrigin::Unknown,
            },
            role,
            blocks: vec![super::super::types::ContentBlock::text(text)],
            complete: true,
        }
    }

    fn parent(from: &str, to: &str) -> super::super::types::EventRelation {
        super::super::types::EventRelation {
            kind: super::super::types::RelationKind::Parent,
            from: from.into(),
            to: to.into(),
            confidence: super::super::types::RelationConfidence::Explicit,
        }
    }

    #[test]
    fn export_markdown_keeps_selected_leaf_and_drops_sibling_and_system() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
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
            graph: super::super::types::NormalizedGraph {
                events: vec![
                    msg(
                        "u1",
                        0,
                        super::super::types::MessageRole::User,
                        "shared-root",
                    ),
                    msg(
                        "a1",
                        1,
                        super::super::types::MessageRole::Assistant,
                        "after-root",
                    ),
                    msg(
                        "sys",
                        2,
                        super::super::types::MessageRole::System,
                        "secret-system",
                    ),
                    msg(
                        "u2",
                        3,
                        super::super::types::MessageRole::User,
                        "sibling-only",
                    ),
                    msg(
                        "a2",
                        4,
                        super::super::types::MessageRole::Assistant,
                        "sibling-answer",
                    ),
                    msg("u3", 5, super::super::types::MessageRole::User, "leaf-only"),
                    msg(
                        "a3",
                        6,
                        super::super::types::MessageRole::Assistant,
                        "leaf-answer",
                    ),
                ],
                relations: vec![
                    parent("a1", "u1"),
                    parent("sys", "a1"),
                    parent("u2", "a1"),
                    parent("a2", "u2"),
                    parent("u3", "sys"),
                    parent("a3", "u3"),
                ],
                active_leaf: Some("a3".into()),
                native_session_id: "n-fork".into(),
                title: Some("n-fork".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        };
        let published = publish_item(
            &conn,
            "job-export",
            &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: "c-export".into(),
                installation_id: "inst-export".into(),
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
                selection_set: SelectionSet::default(),
            }),
            &|| false,
        )
        .unwrap();
        let import_id = published.import_id.unwrap();
        let md = export_markdown(&import_id, None, Some("a3")).unwrap();
        assert!(md.contains("shared-root"));
        assert!(md.contains("leaf-only"));
        assert!(md.contains("leaf-answer"));
        assert!(
            !md.contains("sibling-only") && !md.contains("sibling-answer"),
            "sibling branch must stay out of the selected export: {md}"
        );
        assert!(
            !md.contains("secret-system"),
            "system text must not be exported: {md}"
        );
        assert!(
            md.contains(&import_id),
            "export may document the import id as markdown, not a native session handle: {md}"
        );
        let native_err =
            crate::session_migration::reject_imported_history_id(Some(&import_id)).unwrap_err();
        assert!(
            native_err.starts_with("IMPORTED_HISTORY_READ_ONLY"),
            "exported imp: must not be accepted by native session APIs: {native_err}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn continue_create_same_request_repairs_missing_sidecar() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::continuation::{read_sidecar, sidecar_path};
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("c10-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
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
            graph: super::super::types::NormalizedGraph {
                events: vec![msg(
                    "e1",
                    0,
                    super::super::types::MessageRole::User,
                    "continue-me",
                )],
                relations: vec![],
                active_leaf: Some("e1".into()),
                native_session_id: "n-c10".into(),
                title: Some("n-c10".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        };
        let published = publish_item(
            &conn,
            "job-c10",
            &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: "c-c10".into(),
                installation_id: "inst-c10".into(),
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
                selection_set: SelectionSet::default(),
            }),
            &|| false,
        )
        .unwrap();
        let import_id = published.import_id.unwrap();
        let rev = published.revision_id.unwrap();
        let draft = continue_prepare(&import_id, Some(&rev), None, None, None).unwrap();
        let req = "req-c10-same";
        let first = continue_create(&draft.draft_id, &draft.draft_hash, req, None).unwrap();
        std::fs::remove_file(sidecar_path(&first)).unwrap();
        assert!(read_sidecar(&first).is_none());
        let second = continue_create(&draft.draft_id, &draft.draft_hash, req, None).unwrap();
        assert_eq!(second, first);
        assert!(
            read_sidecar(&first).is_some(),
            "retry must restore the sidecar on the same session"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn continue_create_stays_prepared_without_dispatch() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::continuation::{read_sidecar, DispatchState};
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("c02-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
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
            graph: super::super::types::NormalizedGraph {
                events: vec![msg(
                    "e1",
                    0,
                    super::super::types::MessageRole::User,
                    "draft-only",
                )],
                relations: vec![],
                active_leaf: Some("e1".into()),
                native_session_id: "n-c02".into(),
                title: Some("n-c02".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        };
        let published = publish_item(
            &conn,
            "job-c02",
            &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: "c-c02".into(),
                installation_id: "inst-c02".into(),
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
                selection_set: SelectionSet::default(),
            }),
            &|| false,
        )
        .unwrap();
        let import_id = published.import_id.unwrap();
        let rev = published.revision_id.unwrap();
        let draft = continue_prepare(&import_id, Some(&rev), None, None, None).unwrap();
        let sid = continue_create(&draft.draft_id, &draft.draft_hash, "req-c02", None).unwrap();
        let side = read_sidecar(&sid).expect("sidecar");
        assert_eq!(side.state, DispatchState::Prepared);
        assert!(side.dispatch_id.is_none());
        assert!(side.prompt_hash.is_none());
        assert!(side.agent_session_id.is_none());
        assert!(!draft.readable_event_ids.is_empty());
        assert!(!draft.fragment_event_ids.is_empty());
        for id in &draft.fragment_event_ids {
            assert!(
                draft.readable_event_ids.contains(id),
                "first-send fragment must stay inside the tool-readable set"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn grant_with_selection_none_for_ordinary_session_and_isolates_sidecars() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::continuation::sidecar_path;
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::test_home::IsolatedAppHome;
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _home = IsolatedAppHome::acquire("grant");
        let ordinary = uuid::Uuid::new_v4().to_string();
        assert!(
            grant_with_selection(&ordinary).is_none(),
            "ordinary sessions must not receive an import-history grant"
        );

        fn publish_one(label: &str) -> (String, String) {
            let conn = open_catalog().unwrap();
            let item = crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: format!("c-grant-{label}"),
                installation_id: format!("inst-grant-{label}"),
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
                        events: vec![msg(
                            "e1",
                            0,
                            crate::session_migration::types::MessageRole::User,
                            &format!("grant-body-{label}"),
                        )],
                        relations: vec![],
                        active_leaf: Some("e1".into()),
                        native_session_id: format!("n-grant-{label}"),
                        title: Some(format!("grant-{label}")),
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
                selection_set: SelectionSet::default(),
            });
            let published =
                publish_item(&conn, &format!("job-grant-{label}"), &item, &|| false).unwrap();
            (published.import_id.unwrap(), published.revision_id.unwrap())
        }

        let (import_a, rev_a) = publish_one("a");
        let (import_b, rev_b) = publish_one("b");
        let draft_a = continue_prepare(&import_a, Some(&rev_a), Some("e1"), None, None).unwrap();
        let draft_b = continue_prepare(&import_b, Some(&rev_b), Some("e1"), None, None).unwrap();
        let sid_a =
            continue_create(&draft_a.draft_id, &draft_a.draft_hash, "req-grant-a", None).unwrap();
        let sid_b =
            continue_create(&draft_b.draft_id, &draft_b.draft_hash, "req-grant-b", None).unwrap();
        let ga = grant_with_selection(&sid_a).expect("continuation A");
        let gb = grant_with_selection(&sid_b).expect("continuation B");
        assert_eq!(ga.app_session_id, sid_a);
        assert_eq!(ga.import_id, import_a);
        assert_eq!(ga.revision_id, rev_a);
        assert_eq!(gb.app_session_id, sid_b);
        assert_eq!(gb.import_id, import_b);
        assert_eq!(gb.revision_id, rev_b);
        assert_ne!(ga.token, gb.token);
        assert_ne!(ga.import_id, gb.import_id);
        std::fs::remove_file(sidecar_path(&sid_a)).unwrap();
        assert!(
            grant_with_selection(&sid_a).is_none(),
            "releasing the sidecar must clear the grant"
        );
        assert!(grant_with_selection(&sid_b).is_some());
    }

    #[test]
    fn continue_stays_on_original_revision_after_later_refresh() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::continuation::{load_events, read_sidecar};
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            AdapterOutput, ImportOutcome, Inventory, Provider as Prov, SourceEvidence,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("d02-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let conn = open_catalog().unwrap();
        let native = "n-d02";
        let item_a = crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
            candidate_id: "c-d02-a".into(),
            installation_id: "inst-d02".into(),
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
                graph: super::super::types::NormalizedGraph {
                    events: vec![msg(
                        "e1",
                        0,
                        super::super::types::MessageRole::User,
                        "orig-rev-body",
                    )],
                    relations: vec![],
                    active_leaf: Some("e1".into()),
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
            selection_set: SelectionSet::default(),
        });
        let first = publish_item(&conn, "job-d02-a", &item_a, &|| false).unwrap();
        let import_id = first.import_id.clone().unwrap();
        let rev_a = first.revision_id.clone().unwrap();
        let draft = continue_prepare(&import_id, Some(&rev_a), None, None, None).unwrap();
        let sid = continue_create(&draft.draft_id, &draft.draft_hash, "req-d02", None).unwrap();
        let side = read_sidecar(&sid).expect("sidecar");
        assert_eq!(side.revision_id, rev_a);
        assert_eq!(side.import_id, import_id);
        let mut item_b = item_a.clone();
        item_b.candidate_id = "c-d02-b".into();
        item_b.adapter.graph.events = vec![msg(
            "e1",
            0,
            super::super::types::MessageRole::User,
            "refresh-rev-body",
        )];
        item_b = crate::session_migration::jobs::prepare_test_commit_item(item_b);
        let second = publish_item(&conn, "job-d02-b", &item_b, &|| false).unwrap();
        let rev_b = second.revision_id.clone().unwrap();
        assert_eq!(second.import_id.as_deref(), Some(import_id.as_str()));
        assert_ne!(rev_a, rev_b);
        let entry = crate::session_migration::catalog::get_entry(&conn, &import_id)
            .unwrap()
            .unwrap();
        assert_eq!(entry.current_revision.as_deref(), Some(rev_b.as_str()));
        let side_after = read_sidecar(&sid).expect("sidecar after refresh");
        assert_eq!(
            side_after.revision_id, rev_a,
            "continuation must stay pinned to the original revision"
        );
        let grant = grant_for_session(&sid).expect("grant");
        assert_eq!(grant.revision_id, rev_a);
        let dump =
            serde_json::to_string(&load_events(&import_id, &side_after.revision_id).unwrap())
                .unwrap();
        assert!(dump.contains("orig-rev-body"));
        assert!(
            !dump.contains("refresh-rev-body"),
            "reading the bound revision must not splice the new snapshot: {dump}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_exposes_leaves_relations_and_default_selection_metadata() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s61-meta-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("projects").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("s.jsonl"),
            concat!(
                r#"{"type":"user","uuid":"u1","sessionId":"s","message":{"content":"root"}}"#,
                "\n",
                r#"{"type":"assistant","uuid":"side","parentUuid":"u1","isSidechain":true,"sessionId":"s","message":{"content":"secret-side"}}"#,
                "\n",
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"s","message":{"content":"main-answer"}}"#,
                "\n",
                r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","sessionId":"s","message":{"content":"fork-answer"}}"#,
                "\n"
            ),
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::ClaudeCode, tmp.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        assert!(
            details.leaves.len() >= 2,
            "forked graph must expose leaves: {:?}",
            details.leaves
        );
        assert!(details.relation_summary.subagent_count >= 1);
        assert!(details.relation_summary.parent_count >= 1);
        assert!(!details.default_selection.event_ids.is_empty());
        assert!(details
            .events
            .iter()
            .any(|e| e.selectable && e.default_selected && e.citation == e.event_id));
        let side = details
            .events
            .iter()
            .find(|e| e.text.contains("secret-side"));
        if let Some(side) = side {
            assert!(side.subagent);
            assert!(!side.default_selected);
        }
        let main_leaf = details
            .leaves
            .iter()
            .find(|id| id.contains("a1") || id.contains("a2"))
            .cloned()
            .or_else(|| details.active_leaf.clone())
            .unwrap();
        let sibling = details
            .leaves
            .iter()
            .find(|id| *id != &main_leaf)
            .cloned()
            .expect("sibling leaf");
        let (page, cursor) = preview_events_page(&pid, None, Some(1), Some(&main_leaf)).unwrap();
        assert_eq!(page.len(), 1);
        let cursor = cursor.expect("more events on main leaf");
        let err = preview_events_page(&pid, Some(&cursor), Some(1), Some(&sibling)).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        expire_preview(&pid);
        assert_eq!(
            preview_events_page(&pid, None, Some(1), None)
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_first_page_is_bounded_for_ten_thousand_events() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s61-10k-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = String::from("[");
        for i in 0..10_000 {
            if i > 0 {
                body.push(',');
            }
            body.push_str(&format!(r#"{{"role":"user","content":"e{i}"}}"#));
        }
        body.push(']');
        std::fs::write(dir.join("chat.json"), body).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        assert_eq!(details.event_count, 10_000);
        assert!(details.events.len() <= super::super::budget::READ_PAGE_EVENTS);
        assert!(details.next_cursor.is_some());
        let (page, next) = preview_events_page(&pid, None, Some(50), None).unwrap();
        assert_eq!(page.len(), 50);
        assert!(next.is_some());
        let late = preview_events_page(&pid, next.as_deref(), Some(50), None).unwrap();
        assert_eq!(late.0.len(), 50);
        assert_ne!(page[0].event_id, late.0[0].event_id);
        let _ = std::fs::remove_dir_all(&tmp);
    }
