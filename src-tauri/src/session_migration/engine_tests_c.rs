    #[test]
    fn preview_reports_codex_fork_compaction_and_rollback() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s61-codex-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("codex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rollout.jsonl"),
            concat!(
                r#"{"type":"session_meta","payload":{"id":"s1","cwd":"C:\\r"}}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":"root"}]}}"#,
                "\n",
                r#"{"type":"compacted","payload":{"message":"compacted-summary"}}"#,
                "\n",
                r#"{"type":"rollback","payload":{}}"#,
                "\n",
                r#"{"type":"fork","payload":{"parent_thread_id":"other-session"}}"#,
                "\n",
                r#"{"type":"response_item","payload":{"type":"message","id":"u2","role":"user","content":[{"type":"input_text","text":"after-fork"}]}}"#,
                "\n"
            ),
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::Codex, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (_, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        assert!(
            details.relation_summary.fork_count
                + details.relation_summary.compaction_count
                + details.relation_summary.rollback_count
                >= 1,
            "codex relations missing: {:?}",
            details.relation_summary
        );
        assert!(details.active_leaf.is_some());
        assert!(!details.leaves.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn two_previews_keep_independent_selections_and_stale_plan_on_change() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s63-two-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.json"),
            r#"[{"role":"user","content":"alpha-keep"},{"role":"user","content":"alpha-secret"}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("b.json"),
            r#"[{"role":"user","content":"beta-keep"},{"role":"user","content":"beta-secret"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        assert!(scan.candidates.len() >= 2);
        let (pid_a, det_a) = preview(
            &scan.candidates[0].candidate_id,
            Some(&scan.candidates[0].source_revision),
        )
        .unwrap();
        let (pid_b, det_b) = preview(
            &scan.candidates[1].candidate_id,
            Some(&scan.candidates[1].source_revision),
        )
        .unwrap();
        let keep_a = det_a
            .events
            .iter()
            .find(|e| e.text.contains("keep"))
            .unwrap()
            .event_id
            .clone();
        let keep_b = det_b
            .events
            .iter()
            .find(|e| e.text.contains("keep"))
            .unwrap()
            .event_id
            .clone();
        let sel_a = AllowedSelection::for_events(vec![keep_a.clone()]);
        let sel_b = AllowedSelection::for_events(vec![keep_b.clone()]);
        let plan = plan_with_selections(
            vec![pid_a.clone(), pid_b.clone()],
            vec![],
            Some(vec![
                PreviewSelection {
                    preview_id: pid_a.clone(),
                    leaf_id: det_a.active_leaf.clone().unwrap_or_default(),
                    selection: sel_a.clone(),
                },
                PreviewSelection {
                    preview_id: pid_b.clone(),
                    leaf_id: det_b.active_leaf.clone().unwrap_or_default(),
                    selection: sel_b.clone(),
                },
            ]),
        )
        .unwrap();
        assert_eq!(plan.selection_set.entries.len(), 2);
        assert_ne!(
            plan.selection_set.entries[0].preview_id,
            plan.selection_set.entries[1].preview_id
        );
        let secret_a = det_a
            .events
            .iter()
            .find(|e| e.text.contains("secret"))
            .unwrap()
            .event_id
            .clone();
        let mut leaked = sel_a.clone();
        leaked.event_ids.push(secret_a.clone());
        let stale = plan_with_selections(
            vec![pid_a.clone(), pid_b.clone()],
            vec![],
            Some(vec![
                PreviewSelection {
                    preview_id: pid_a.clone(),
                    leaf_id: det_a.active_leaf.clone().unwrap_or_default(),
                    selection: leaked,
                },
                PreviewSelection {
                    preview_id: pid_b.clone(),
                    leaf_id: det_b.active_leaf.clone().unwrap_or_default(),
                    selection: sel_b.clone(),
                },
            ]),
        )
        .unwrap();
        assert_ne!(stale.plan_hash, plan.plan_hash);
        let job = commit(&plan.plan_id, &plan.plan_hash, "s66-keep").unwrap();
        super::super::job_control::run_pending().unwrap();
        let done = job_get(&job.job_id).unwrap();
        let import_a = done
            .items
            .iter()
            .filter_map(|item| item.import_id.clone())
            .find(|id| {
                export_markdown(id, None, None)
                    .unwrap()
                    .contains("alpha-keep")
            })
            .expect("imported keep session");
        crate::session_migration::continuation::take_load_events_calls();
        let md = export_markdown(&import_a, None, det_a.active_leaf.as_deref()).unwrap();
        assert!(md.contains("alpha-keep"));
        let model =
            export_for_model(&import_a, None, det_a.active_leaf.as_deref(), &sel_a).unwrap();
        assert!(!model.contains("alpha-secret"));
        assert_eq!(
            crate::session_migration::continuation::take_load_events_calls(),
            0,
            "export must not materialize the full archive"
        );
        let draft = continue_prepare_with_selection(
            &import_a,
            None,
            det_a.active_leaf.as_deref(),
            None,
            None,
            Some(sel_a.clone()),
        )
        .unwrap();
        assert_eq!(
            crate::session_migration::continuation::take_load_events_calls(),
            0,
            "continue_prepare must not materialize the full archive"
        );
        let sid = continue_create(&draft.draft_id, &draft.draft_hash, "s66-cont", None).unwrap();
        let grant = grant_for_session(&sid).expect("grant");
        let hit = crate::session_migration::mcp_stdio::call_tool(
            &grant,
            "import_history_search",
            &serde_json::json!({"query": "alpha-secret"}),
        )
        .unwrap()
        .to_string();
        assert!(
            !hit.contains("alpha-secret"),
            "MCP search must deny excluded secret: {hit}"
        );
        let denied = crate::session_migration::mcp_stdio::call_tool(
            &grant,
            "import_history_read",
            &serde_json::json!({"citation": secret_a}),
        )
        .unwrap_err();
        assert!(
            denied.contains("HISTORY_ACCESS_UNAVAILABLE") || denied.contains("outside"),
            "{denied}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn catalog_and_job_items_are_sql_paged() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s69-page-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let conn = open_catalog().unwrap();
        for i in 0..3 {
            crate::session_migration::catalog::upsert_entry(
                &conn,
                &crate::session_migration::catalog::CatalogEntry {
                    import_id: format!("imp:00000000-0000-0000-0000-00000000000{i}"),
                    installation_id: format!("inst-{i}"),
                    provider: "generic-file".into(),
                    native_session_id: format!("n{i}"),
                    title: format!("t{i}"),
                    alias: None,
                    hidden: false,
                    current_revision: None,
                    created_at: format!("2026-01-0{}T00:00:00Z", i + 1),
                },
            )
            .unwrap();
        }
        let (page, next, total) = list_imported_page(None, Some(2)).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(total, 3);
        assert!(next.is_some());
        let (page2, next2, _) = list_imported_page(next.as_deref(), Some(2)).unwrap();
        assert_eq!(page2.len(), 1);
        assert!(next2.is_none());
        assert_ne!(page[0].import_id, page2[0].import_id);
        let job = crate::session_migration::jobs::JobRecord {
            job_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            plan_hash: "h".into(),
            plan_id: "p".into(),
            state: crate::session_migration::jobs::JobState::Queued,
            state_version: 1,
            items: (0..5)
                .map(|i| crate::session_migration::jobs::JobItem {
                    candidate_id: format!("c{i}"),
                    title: None,
                    phase: "queued".into(),
                    import_id: None,
                    revision_id: None,
                    outcome: None,
                    error: None,
                    mapping_project_id: None,
                })
                .collect(),
            created_at: chrono::Utc::now().to_rfc3339(),
            item_count: 0,
            next_item_cursor: None,
            progress: None,
        };
        crate::session_migration::jobs::save_job(&conn, &job).unwrap();
        crate::session_migration::jobs::take_job_item_deserializes();
        let first = job_items_page(&job.job_id, None, Some(2)).unwrap();
        assert_eq!(first.item_count, 5);
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.items[0].candidate_id, "c0");
        let parsed = crate::session_migration::jobs::take_job_item_deserializes();
        assert!(
            parsed <= 3,
            "job_items_page must not deserialize the full items array, parsed {parsed}"
        );
        let second =
            job_items_page(&job.job_id, first.next_item_cursor.as_deref(), Some(2)).unwrap();
        assert_eq!(second.items.len(), 2);
        assert_eq!(second.items[0].candidate_id, "c2");
        let parsed2 = crate::session_migration::jobs::take_job_item_deserializes();
        assert!(
            parsed2 <= 3,
            "second page must not parse remaining items from the start, parsed {parsed2}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn events_page_leaf_and_second_prepare_drops_draft() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s64-leaf-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("projects").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("s.jsonl"),
            concat!(
                r#"{"type":"user","uuid":"u1","sessionId":"s","message":{"content":"root"}}"#,
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
            "forked claude fixture must expose leaves: {:?}",
            details.leaves
        );
        let leaf_a = details.leaves[0].clone();
        let leaf_b = details.leaves[1].clone();
        let plan = plan(vec![pid], vec![]).unwrap();
        let job = commit(&plan.plan_id, &plan.plan_hash, "s64-leaf").unwrap();
        super::super::job_control::run_pending().unwrap();
        let import_id = job_get(&job.job_id).unwrap().items[0]
            .import_id
            .clone()
            .unwrap();
        let (page_a, _, _) = events_page(&import_id, None, None, Some(50), Some(&leaf_a)).unwrap();
        let (page_b, _, _) = events_page(&import_id, None, None, Some(50), Some(&leaf_b)).unwrap();
        let dump_a = page_a
            .iter()
            .map(|event| {
                serde_json::from_value::<crate::session_migration::types::ImportedEvent>(
                    event.clone(),
                )
                    .unwrap()
                    .searchable_text()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let dump_b = page_b
            .iter()
            .map(|event| {
                serde_json::from_value::<crate::session_migration::types::ImportedEvent>(
                    event.clone(),
                )
                    .unwrap()
                    .searchable_text()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            dump_a, dump_b,
            "sibling leaves must page different lineages"
        );
        assert!(
            dump_a.contains("main-answer") ^ dump_b.contains("main-answer"),
            "main-answer must belong to one leaf: {dump_a:?} vs {dump_b:?}"
        );
        assert!(
            dump_a.contains("fork-answer") ^ dump_b.contains("fork-answer"),
            "fork-answer must belong to the other leaf: {dump_a:?} vs {dump_b:?}"
        );
        crate::session_migration::continuation::take_load_events_calls();
        let first =
            continue_prepare_with_selection(&import_id, None, None, None, None, None).unwrap();
        let second =
            continue_prepare_with_selection(&import_id, None, Some(&first.leaf), None, None, None)
                .unwrap();
        assert_ne!(first.draft_id, second.draft_id);
        assert_eq!(
            crate::session_migration::continuation::take_load_events_calls(),
            0
        );
        let err =
            continue_create(&first.draft_id, &first.draft_hash, "old-draft", None).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn durable_generic_plan() -> (std::path::PathBuf, PlanState) {
        let tmp = std::env::temp_dir().join(format!("dplan-{}", Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), r#"[{"role":"user","content":"alpha"}]"#).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
        let plan = plan(vec![pid], maps).unwrap();
        (tmp, plan)
    }

    #[test]
    fn default_plan_uses_index_metadata_without_loading_event_bodies() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire(
            "default-plan-metadata",
        );
        let files = home.path.join("files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(
            files.join("metadata.json"),
            r#"[{"role":"user","content":"alpha"},{"role":"assistant","content":"beta"}]"#,
        )
        .unwrap();
        let scan = scan_start(vec![(
            Provider::GenericFile,
            files.to_string_lossy().into(),
        )])
        .unwrap();
        let candidate = scan.candidates.first().unwrap();
        let (preview_id, _) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        let mappings = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();

        let _ = crate::session_migration::search::take_event_body_reads();
        let planned = plan(vec![preview_id], mappings).unwrap();

        assert_eq!(
            crate::session_migration::search::take_event_body_reads(),
            0,
            "default plan must derive its selection from index metadata"
        );
        assert_eq!(planned.selection_set.entries[0].selection.event_ids.len(), 2);
    }

    #[test]
    fn commit_loads_durable_plan_after_runtime_is_dropped() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        reset_runtime();
        let job = commit(&plan.plan_id, &plan.plan_hash, "durable-ok").unwrap();
        assert_eq!(job.plan_id, plan.plan_id);
        assert_eq!(job.plan_hash, plan.plan_hash);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_fails_closed_when_durable_plan_is_missing() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        let conn = open_catalog().unwrap();
        conn.execute("DELETE FROM plans WHERE plan_id = ?1", [&plan.plan_id])
            .unwrap();
        reset_runtime();
        let err = commit(&plan.plan_id, &plan.plan_hash, "durable-missing").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(err.message.contains("plan not found"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_fails_closed_on_plan_hash_mismatch() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        reset_runtime();
        let err = commit(&plan.plan_id, "deadbeef", "durable-hash").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(err.message.contains("plan hash mismatch"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_fails_closed_when_selection_mapping_or_options_change() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        let conn = open_catalog().unwrap();
        let (_, json) = load_plan_json(&conn, &plan.plan_id).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();

        let mut selection_tamper = value.clone();
        selection_tamper["selectionSet"]["entries"][0]["selection"]["eventIds"] =
            serde_json::json!(["not-an-event"]);
        super::super::catalog::replace_plan_json(
            &conn,
            &plan.plan_id,
            &serde_json::to_string(&selection_tamper).unwrap(),
        )
        .unwrap();
        reset_runtime();
        let err = commit(&plan.plan_id, &plan.plan_hash, "durable-sel").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);

        super::super::catalog::replace_plan_json(&conn, &plan.plan_id, &json).unwrap();
        let mut mapping_tamper = value.clone();
        mapping_tamper["mappings"] = serde_json::json!([{
            "rootId": "forged-root",
            "unassigned": true,
            "userChoiceVersion": 1
        }]);
        super::super::catalog::replace_plan_json(
            &conn,
            &plan.plan_id,
            &serde_json::to_string(&mapping_tamper).unwrap(),
        )
        .unwrap();
        let err = commit(&plan.plan_id, &plan.plan_hash, "durable-map").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);

        super::super::catalog::replace_plan_json(&conn, &plan.plan_id, &json).unwrap();
        value["options"] = serde_json::json!({"includeSecrets": true});
        super::super::catalog::replace_plan_json(
            &conn,
            &plan.plan_id,
            &serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
        let err = commit(&plan.plan_id, &plan.plan_hash, "durable-opt").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(err.message.contains("modified") || err.message.contains("mismatch"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_rejects_same_request_id_on_a_different_plan() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, first_plan) = durable_generic_plan();
        reset_runtime();
        let first = commit(&first_plan.plan_id, &first_plan.plan_hash, "shared-req").unwrap();
        let dir = tmp.join("files");
        std::fs::write(dir.join("b.json"), r#"[{"role":"user","content":"beta"}]"#).unwrap();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = scan
            .candidates
            .iter()
            .find(|c| c.locator.contains("b.json"))
            .expect("second session");
        let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
        let second_plan = plan(vec![pid], maps).unwrap();
        assert_ne!(second_plan.plan_id, first_plan.plan_id);
        reset_runtime();
        let err = commit(&second_plan.plan_id, &second_plan.plan_hash, "shared-req").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(err.message.contains("different plan"));
        let again = commit(&first_plan.plan_id, &first_plan.plan_hash, "shared-req").unwrap();
        assert_eq!(again.job_id, first.job_id);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_durable_plan_rejects_expired_record() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        let conn = open_catalog().unwrap();
        let (_, json) = load_plan_json(&conn, &plan.plan_id).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let expiry = chrono::DateTime::parse_from_rfc3339(value["expiry"].as_str().unwrap()).unwrap();
        let _clock = crate::session_migration::clock::Fixed::at(
            expiry.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1),
        );
        reset_runtime();
        let err = commit(&plan.plan_id, &plan.plan_hash, "expired-plan").unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(err.message.contains("expired"));
        assert_eq!(load_plan_json(&conn, &plan.plan_id).unwrap().1, json);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn recovery_restores_plan_after_runtime_drop_and_rejects_tamper() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let (tmp, plan) = durable_generic_plan();
        reset_runtime();
        let restored = recovery().unwrap();
        assert_eq!(
            restored.plan.as_ref().map(|p| p.plan_id.as_str()),
            Some(plan.plan_id.as_str())
        );
        let restored_json = serde_json::to_value(&restored).unwrap();
        let restored_plan = restored_json["plan"].as_object().unwrap();
        assert_eq!(restored_plan.len(), 4);
        assert!(restored_plan.contains_key("planId"));
        assert!(restored_plan.contains_key("planHash"));
        assert_eq!(restored_plan["previewCount"], serde_json::json!(1));
        assert_eq!(restored_plan["associationTargets"], serde_json::json!([]));
        assert!(!restored_plan.contains_key("previewIds"));
        assert!(!restored_plan.contains_key("mappings"));
        assert!(!restored_plan.contains_key("selectionSet"));
        assert!(!restored.expired);
        let conn = open_catalog().unwrap();
        let (_, json) = load_plan_json(&conn, &plan.plan_id).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["options"] = serde_json::json!({"includeSecrets": true});
        super::super::catalog::replace_plan_json(
            &conn,
            &plan.plan_id,
            &serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
        reset_runtime();
        let tampered = recovery().unwrap();
        assert!(tampered.plan.is_none());
        assert!(tampered.failed.is_some());
        assert!(commit(&plan.plan_id, &plan.plan_hash, "rec-tampered").is_err());

        // Only an unsubmitted plan is validated for recovery. Once committed,
        // the original job's independently validated durable inputs own recovery.
        super::super::catalog::replace_plan_json(&conn, &plan.plan_id, &json).unwrap();
        let job = commit(&plan.plan_id, &plan.plan_hash, "rec-ok").unwrap();
        assert_eq!(job.plan_id, plan.plan_id);
        super::super::catalog::replace_plan_json(
            &conn,
            &plan.plan_id,
            &serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
        reset_runtime();
        let submitted = recovery().unwrap();
        assert_eq!(submitted.job.unwrap().job_id, job.job_id);
        assert!(submitted.plan.is_none());
        assert!(submitted.failed.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn catalog_filter_is_bound_to_cursor_scope() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("cat-filter-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let conn = open_catalog().unwrap();
        for i in 0..8 {
            super::super::catalog::upsert_entry(
                &conn,
                &CatalogEntry {
                    import_id: format!("imp:{}", uuid::Uuid::from_u128(0x2000 + i as u128)),
                    installation_id: format!("inst-{i}"),
                    provider: if i % 2 == 0 {
                        "codex".into()
                    } else {
                        "cursor".into()
                    },
                    native_session_id: format!("n{i}"),
                    title: if i % 2 == 0 {
                        format!("alpha-{i}")
                    } else {
                        format!("beta-{i}")
                    },
                    alias: None,
                    hidden: false,
                    current_revision: None,
                    created_at: format!("2026-01-01T00:00:{i:02}Z"),
                },
            )
            .unwrap();
        }
        let (codex, next, total) =
            list_imported_page_filtered(None, Some(3), Some("codex"), None, Some("newest"))
                .unwrap();
        assert_eq!(total, 4);
        assert_eq!(codex.len(), 3);
        assert!(codex.iter().all(|e| e.provider == "codex"));
        let next = next.expect("filtered catalog page");
        let err =
            list_imported_page_filtered(Some(&next), Some(3), Some("cursor"), None, Some("newest"))
                .unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        let (page2, next2, _) =
            list_imported_page_filtered(Some(&next), Some(3), Some("codex"), None, Some("newest"))
                .unwrap();
        assert_eq!(page2.len(), 1);
        assert!(next2.is_none());
        let (q, _, qtotal) =
            list_imported_page_filtered(None, Some(10), None, Some("beta"), Some("newest"))
                .unwrap();
        assert_eq!(qtotal, 4);
        assert!(q.iter().all(|e| e.title.contains("beta")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn encode_preview_cursor_does_not_abort_when_secret_cannot_be_read() {
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire("prev-cur");
        let secret = home.path.join("imports").join(".cursor-hmac-secret");
        let _ = std::fs::remove_file(&secret);
        std::fs::create_dir_all(&secret).unwrap();
        let result = encode_preview_cursor("preview-id", "leaf-a", 0);
        assert!(
            result.is_err(),
            "preview page cursor encode must return a stable error instead of aborting, got {result:?}"
        );
    }
