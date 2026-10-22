
    #[test]
    fn scan_empty_roots_does_not_panic() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![(
            Provider::Codex,
            tmp.join("missing").to_string_lossy().into(),
        )])
        .unwrap();
        assert!(scan.complete);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn select_current_stays_on_frozen_scan_snapshot() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("snap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), r#"[{"role":"user","content":"one"}]"#).unwrap();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let frozen = scan_select_current(&scan.scan_id, scan.epoch).unwrap();
        assert_eq!(frozen.len(), 1);
        scan_append_discoveries(
            &scan.scan_id,
            vec![SourceCandidate {
                candidate_id: "later-extra".into(),
                provider: Provider::GenericFile,
                native_session_id: "later".into(),
                title: "later".into(),
                cwd: None,
                started_at: None,
                updated_at: None,
                locator: "later".into(),
                source_revision: "x".into(),
                archived: None,
                completeness: "body".into(),
            }],
        )
        .unwrap();
        let still = scan_select_current(&scan.scan_id, scan.epoch).unwrap();
        assert_eq!(still, frozen);
        assert!(!still.iter().any(|id| id == "later-extra"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn continue_create_persists_caller_edited_text() {
        use crate::session_migration::catalog::open_catalog;
        use crate::session_migration::jobs::{publish_item, CommitItem};
        use crate::session_migration::types::{
            ContentBlock, EventBase, ImportOutcome, ImportedEvent, Inventory, MessageRole,
            NormalizedGraph, Provider as Prov, SourceEvidence, TimeOrigin,
        };
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("hand-{}", uuid::Uuid::new_v4()));
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
            graph: NormalizedGraph {
                events: vec![ImportedEvent::Message {
                    base: EventBase {
                        id: "e1".into(),
                        ordinal: 0,
                        spans: vec![],
                        timestamp: None,
                        time_origin: TimeOrigin::Unknown,
                    },
                    role: MessageRole::User,
                    blocks: vec![ContentBlock::text("original-alpha")],
                    complete: true,
                }],
                relations: vec![],
                active_leaf: Some("e1".into()),
                native_session_id: "n-edit".into(),
                title: Some("n-edit".into()),
                cwd: None,
            },
            inventory: Inventory::default(),
            issues: vec![],
            assets: vec![],
            outcome: ImportOutcome::Complete,
        };
        let published = publish_item(
            &conn,
            "job-edit",
            &crate::session_migration::jobs::prepare_test_commit_item(CommitItem {
                candidate_id: "c-edit".into(),
                installation_id: "inst-edit".into(),
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
        assert!(draft.text.contains("original-alpha"));
        let sid = continue_create(
            &draft.draft_id,
            &draft.draft_hash,
            "req-edit",
            Some("user-edited-handoff".into()),
        )
        .unwrap();
        let stored = std::fs::read_to_string(
            crate::paths::app_data_root()
                .join("sessions")
                .join(&sid)
                .join("handoff_draft.txt"),
        )
        .unwrap();
        assert_eq!(stored, "user-edited-handoff");
        assert!(!stored.contains("original-alpha"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn write_mapping_fixture(dir: &Path) {
        for id in ["a", "b"] {
            let meta = serde_json::json!({"type":"session_meta", "payload":{"id":id,"cwd":format!(r"C:\repo-{id}")}});
            let event = serde_json::json!({"type":"response_item","payload":{"type":"message","id":"u1","role":"user","content":[{"type":"input_text","text":id}]}});
            std::fs::write(dir.join(format!("{id}.jsonl")), format!("{meta}\n{event}\n")).unwrap();
        }
    }

    fn mapping_projects(dir: &Path) -> [String; 2] {
        ["a", "b"].map(|id| {
            let target = dir.join(format!("target-{id}"));
            std::fs::create_dir(&target).unwrap();
            crate::store::add_project(target.to_string_lossy().into_owned(), false).unwrap().id
        })
    }

    #[test]
    fn scan_keeps_healthy_candidates_when_one_source_is_busy() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("mixscan-{}", uuid::Uuid::new_v4()));
        let tx = tmp.join("projects").join("p1").join("agent-transcripts");
        std::fs::create_dir_all(&tx).unwrap();
        std::fs::write(
            tx.join("keep.jsonl"),
            r#"{"role":"user","message":{"content":[{"type":"text","text":"keep-me"}]}}
{"role":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}
"#,
        )
        .unwrap();
        let ws = tmp.join("User").join("workspaceStorage").join("ws-bad");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("state.vscdb"), b"this is not a sqlite database").unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![(Provider::Cursor, tmp.to_string_lossy().into())]).unwrap();
        assert!(
            scan.candidates
                .iter()
                .any(|c| c.locator.contains("keep.jsonl")),
            "transcript must survive a bad composer db: {:?}",
            scan.candidates
        );
        assert!(
            scan.issues.iter().any(|i| i.code == "SNAPSHOT_UNAVAILABLE"
                || i.code == "SOURCE_BUSY"
                || i.code == "SCHEMA_UNSUPPORTED"),
            "busy/unreadable composer must be an issue, not silence: {:?}",
            scan.issues
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_page_does_not_repeat_ids_across_cursors() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("page-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.json", "b.json", "c.json"] {
            std::fs::write(dir.join(name), r#"[{"role":"user","content":"n"}]"#).unwrap();
        }
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        assert_eq!(scan.candidates.len(), 3);
        let (page1, next, _, _) = scan_page(&scan.scan_id, scan.epoch, None, Some(2)).unwrap();
        assert_eq!(page1.len(), 2);
        let next = next.expect("need a second page");
        let (page2, next2, _, _) =
            scan_page(&scan.scan_id, scan.epoch, Some(&next), Some(2)).unwrap();
        assert_eq!(page2.len(), 1);
        assert!(next2.is_none());
        let ids1: Vec<_> = page1.iter().map(|c| c.candidate_id.clone()).collect();
        let ids2: Vec<_> = page2.iter().map(|c| c.candidate_id.clone()).collect();
        assert!(ids1.iter().all(|id| !ids2.contains(id)));
        assert!(
            next.parse::<u32>().is_err(),
            "scan cursor must not be a bare number"
        );
        assert!(scan_page(&scan.scan_id, scan.epoch, Some("2"), Some(2)).is_err());
        let epoch_key = scan.epoch.to_string();
        let other = crate::session_migration::cursor::encode_bound_cursor(
            crate::session_migration::cursor::CursorScope {
                owner: "other-scan",
                snapshot: &epoch_key,
                leaf: "",
                filter: "candidates",
            },
            0,
            None,
        )
        .unwrap();
        assert!(scan_page(&scan.scan_id, scan.epoch, Some(&other), Some(2)).is_err());
        let stale = crate::session_migration::cursor::tamper_cursor_field(
            &next,
            "exp",
            serde_json::json!(1),
        );
        assert!(scan_page(&scan.scan_id, scan.epoch, Some(&stale), Some(2)).is_err());
        let tampered = crate::session_migration::cursor::tamper_cursor_field(
            &next,
            "snapshot",
            serde_json::json!("0"),
        );
        assert!(scan_page(&scan.scan_id, scan.epoch, Some(&tampered), Some(2)).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn commit_applies_per_root_mapping_not_first_only() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("map-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        write_mapping_fixture(&dir);
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let projects = mapping_projects(&tmp);
        let scan = scan_start(vec![(Provider::Codex, dir.to_string_lossy().into())]).unwrap();
        let mut preview_ids = Vec::new();
        for c in &scan.candidates {
            let (pid, _) = preview(&c.candidate_id, Some(&c.source_revision)).unwrap();
            preview_ids.push(pid);
        }
        assert_eq!(preview_ids.len(), 2);
        let mut maps = propose_mappings(&preview_ids, None).unwrap();
        assert_eq!(maps.len(), 2);
        for m in &mut maps {
            m.unassigned = false;
            assert!(m.explicit_target.is_none(), "cwd is only a suggestion");
            if m.suggested_target.as_deref() == Some(r"C:\repo-a") {
                m.target_project_id = Some(projects[0].clone());
            } else {
                m.target_project_id = Some(projects[1].clone());
            }
        }
        let plan = plan(preview_ids, maps).unwrap();
        let job = commit(&plan.plan_id, &plan.plan_hash, "req-map").unwrap();
        super::super::job_control::run_pending().unwrap();
        let job = job_get(&job.job_id).unwrap();
        assert!(job.items.iter().all(|item| item.outcome.as_deref() == Some("complete")), "{job:?}");
        let by_cand: std::collections::HashMap<_, _> = job
            .items
            .iter()
            .map(|i| (i.candidate_id.clone(), i.mapping_project_id.clone()))
            .collect();
        let ca = &scan.candidates[0].candidate_id;
        let cb = &scan.candidates[1].candidate_id;
        assert_eq!(by_cand.get(ca).and_then(|x| x.as_deref()), Some(projects[0].as_str()));
        assert_eq!(by_cand.get(cb).and_then(|x| x.as_deref()), Some(projects[1].as_str()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_uses_new_scan_and_preserves_old_selection() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("refresh-{}", Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        std::fs::write(dir.join("a.json"), r#"[{"role":"user","content":"one"}]"#).unwrap();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let frozen = scan_select_current(&scan.scan_id, scan.epoch).unwrap();
        std::fs::write(dir.join("b.json"), r#"[{"role":"user","content":"two"}]"#).unwrap();
        let fresh = scan_refresh(&scan.scan_id, scan.epoch).unwrap();
        assert_ne!(scan.scan_id, fresh.scan_id);
        assert_eq!(
            frozen,
            scan_select_current(&scan.scan_id, scan.epoch).unwrap()
        );
        assert_eq!(
            scan_select_current(&fresh.scan_id, fresh.epoch)
                .unwrap()
                .len(),
            2
        );
        assert!(fresh.extra_candidates.is_empty());
        let (_, cursor, _, _) = scan_page(&scan.scan_id, scan.epoch, None, Some(1)).unwrap();
        if let Some(cursor) = cursor {
            assert!(scan_page(&fresh.scan_id, fresh.epoch, Some(&cursor), None).is_err());
        }
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn retry_failed_does_not_duplicate_committed_identity() {
        use crate::session_migration::jobs::{get_job, save_job};
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("retry-{}", uuid::Uuid::new_v4()));
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
        // Historical split result/ACK replay; signed ACK tampering is tested
        // separately and must never become a successful signed recovery.
        let job = enqueue_legacy_plan(&plan, "req-retry");
        super::super::job_control::run_pending().unwrap();
        let job = job_get(&job.job_id).unwrap();
        let import_id = job.items[0]
            .import_id
            .clone()
            .expect("first commit must create a catalog row");
        let before = list_imported().unwrap();
        let visible: Vec<_> = before
            .iter()
            .filter(|e| e.import_id == import_id && !e.hidden)
            .collect();
        assert_eq!(visible.len(), 1);
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        let mut stored = get_job(&conn, &job.job_id).unwrap().unwrap();
        stored.items[0].outcome = Some("failed".into());
        stored.items[0].phase = "failed".into();
        stored.items[0].error = Some("simulated".into());
        save_job(&conn, &stored).unwrap();
        retry_failed(&job.job_id).unwrap();
        super::super::job_control::run_pending().unwrap();
        let retried = job_get(&job.job_id).unwrap();
        assert_eq!(retried.items[0].outcome.as_deref(), Some("complete"));
        assert_eq!(retried.items[0].revision_id, job.items[0].revision_id);
        let after = list_imported().unwrap();
        let visible_after: Vec<_> = after
            .iter()
            .filter(|e| !e.hidden && e.import_id == import_id)
            .collect();
        assert_eq!(visible_after.len(), 1);
        let same_identity: Vec<_> = after
            .iter()
            .filter(|e| !e.hidden && e.native_session_id == visible[0].native_session_id)
            .collect();
        assert_eq!(same_identity.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unassigned_root_does_not_inherit_first_project() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("unmap-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        write_mapping_fixture(&dir);
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let projects = mapping_projects(&tmp);
        let scan = scan_start(vec![(Provider::Codex, dir.to_string_lossy().into())]).unwrap();
        let mut preview_ids = Vec::new();
        for c in &scan.candidates {
            let (pid, _) = preview(&c.candidate_id, Some(&c.source_revision)).unwrap();
            preview_ids.push(pid);
        }
        let mut maps = propose_mappings(&preview_ids, None).unwrap();
        for m in &mut maps {
            assert!(m.explicit_target.is_none(), "cwd is only a suggestion");
            if m.suggested_target.as_deref() == Some(r"C:\repo-a") {
                m.target_project_id = Some(projects[0].clone());
                m.unassigned = false;
            } else {
                m.target_project_id = None;
                m.unassigned = true;
            }
        }
        let plan = plan(preview_ids, maps).unwrap();
        let job = commit(&plan.plan_id, &plan.plan_hash, "req-unmap").unwrap();
        super::super::job_control::run_pending().unwrap();
        let job = job_get(&job.job_id).unwrap();
        assert!(job.items.iter().all(|item| item.outcome.as_deref() == Some("complete")), "{job:?}");
        let by_cand: std::collections::HashMap<_, _> = job
            .items
            .iter()
            .map(|i| (i.candidate_id.clone(), i.mapping_project_id.clone()))
            .collect();
        let ca = &scan.candidates[0].candidate_id;
        let cb = &scan.candidates[1].candidate_id;
        assert_eq!(by_cand.get(ca).and_then(|x| x.as_deref()), Some(projects[0].as_str()));
        assert_eq!(by_cand.get(cb).and_then(|x| x.as_deref()), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_start_explicit_path_does_not_substitute_default_roots() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("scan-explicit-{}", uuid::Uuid::new_v4()));
        let wanted = tmp.join("wanted");
        let other = tmp.join("other");
        std::fs::create_dir_all(&wanted).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(
            wanted.join("keep-me.json"),
            r#"[{"role":"user","content":"keep"}]"#,
        )
        .unwrap();
        std::fs::write(
            other.join("skip-me.json"),
            r#"[{"role":"user","content":"skip"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![(
            Provider::GenericFile,
            wanted.to_string_lossy().into(),
        )])
        .unwrap();
        assert!(
            scan.candidates
                .iter()
                .any(|c| c.locator.contains("keep-me.json")),
            "explicit dir must list its files: {:?}",
            scan.candidates
        );
        assert!(
            scan.candidates.iter().all(|c| c
                .locator
                .contains(&wanted.to_string_lossy().into_owned())
                && !c.locator.contains("skip-me.json")),
            "must not scan sibling or default roots: {:?}",
            scan.candidates
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn events_page_is_bounded_and_past_end_is_not_empty_success() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("page-ev-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chat.json"),
            r#"[{"role":"user","content":"one"},{"role":"assistant","content":"two"},{"role":"user","content":"three"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
        let plan = plan(vec![pid], maps).unwrap();
        let job = commit(&plan.plan_id, &plan.plan_hash, "req-page").unwrap();
        super::super::job_control::run_pending().unwrap();
        let job = job_get(&job.job_id).unwrap();
        let import_id = job.items[0].import_id.clone().unwrap();
        let (page, next, _) = events_page(&import_id, None, None, Some(2), None).unwrap();
        assert_eq!(page.len(), 2, "first page must honor limit when more exist");
        let next = next.expect("more events remain");
        let (rest, next2, _) = events_page(&import_id, None, Some(&next), Some(2), None).unwrap();
        assert_eq!(rest.len(), 1);
        assert!(next2.is_none());
        let err = events_page(&import_id, None, Some("99"), Some(2), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert!(
            err.message.contains("cursor"),
            "past-end must not look like empty history: {}",
            err.message
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_cursor_composer_keeps_hash_in_filesystem_path() {
        use rusqlite::params;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("hash-loc-{}", uuid::Uuid::new_v4()));
        let ws = tmp.join("User").join("workspaceStorage").join("ws#room");
        std::fs::create_dir_all(&ws).unwrap();
        let db = ws.join("state.vscdb");
        let writer = crate::session_migration::sqlite_open::open(&db).unwrap();
        writer
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .unwrap();
        for (id, name, text) in [
            ("first-cid", "first", "alpha-first-only"),
            ("keep-cid", "keep", "beta-keep-only"),
        ] {
            let composer = serde_json::json!({
                "composerId": id,
                "name": name,
                "fullConversationHeadersOnly": [{"bubbleId": "b1"}]
            });
            writer
                .execute(
                    "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                    params![
                        format!("composerData:{id}"),
                        composer.to_string().as_bytes()
                    ],
                )
                .unwrap();
            let bubble = serde_json::json!({"type":"user","text": text});
            writer
                .execute(
                    "INSERT INTO ItemTable(key, value) VALUES (?1, ?2)",
                    params![format!("bubbleId:{id}:b1"), bubble.to_string().as_bytes()],
                )
                .unwrap();
        }
        drop(writer);
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::Cursor, tmp.to_string_lossy().into())]).unwrap();
        let cand = scan
            .candidates
            .iter()
            .find(|c| c.native_session_id == "cursor-identity-v1:composer:keep-cid")
            .expect("keep composer listed");
        assert!(
            cand.locator.contains("ws#room") && cand.locator.contains("#composerData:keep-cid"),
            "locator must keep filesystem hash and composer fragment: {}",
            cand.locator
        );
        let (_pid, details) =
            preview(&cand.candidate_id, Some(&cand.source_revision)).expect("preview");
        let texts: Vec<_> = details.events.iter().map(|e| e.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("beta-keep-only")),
            "must read the hashed-path composer, got {texts:?}"
        );
        assert!(
            texts.iter().all(|t| !t.contains("alpha-first-only")),
            "must not fall back to the other composer, got {texts:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_page_and_select_reject_stale_epoch() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("epoch-{}", uuid::Uuid::new_v4()));
        let a_dir = tmp.join("a");
        let b_dir = tmp.join("b");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        std::fs::write(a_dir.join("a.json"), r#"[{"role":"user","content":"aaa"}]"#).unwrap();
        std::fs::write(b_dir.join("b.json"), r#"[{"role":"user","content":"bbb"}]"#).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan_a = scan_start(vec![(
            Provider::GenericFile,
            a_dir.to_string_lossy().into(),
        )])
        .unwrap();
        let scan_b = scan_start(vec![(
            Provider::GenericFile,
            b_dir.to_string_lossy().into(),
        )])
        .unwrap();
        let a_ids = scan_select_current(&scan_a.scan_id, scan_a.epoch).unwrap();
        let b_ids = scan_select_current(&scan_b.scan_id, scan_b.epoch).unwrap();
        assert!(!a_ids.is_empty(), "scan a must have candidates");
        assert!(!b_ids.is_empty(), "scan b must have candidates");
        assert!(
            a_ids.iter().all(|id| !b_ids.contains(id)),
            "two scans must not share candidate ids: {a_ids:?} vs {b_ids:?}"
        );
        let stale = scan_a.epoch.wrapping_add(1);
        let page_err = scan_page(&scan_a.scan_id, stale, None, None).unwrap_err();
        assert_eq!(page_err.code, ErrorCode::PlanStale);
        let select_err = scan_select_current(&scan_a.scan_id, stale).unwrap_err();
        assert_eq!(select_err.code, ErrorCode::PlanStale);
        let cross =
            scan_page(&scan_a.scan_id, scan_b.epoch.wrapping_add(7), None, None).unwrap_err();
        assert_eq!(cross.code, ErrorCode::PlanStale);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_reads_new_candidate_without_expanding_old_selection() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("prev-extra-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.json"),
            r#"[{"role":"user","content":"frozen-only"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let frozen = scan_select_current(&scan.scan_id, scan.epoch).unwrap();
        std::fs::write(
            dir.join("later.json"),
            r#"[{"role":"user","content":"extra-keep-only"}]"#,
        )
        .unwrap();
        let refreshed = scan_refresh(&scan.scan_id, scan.epoch).unwrap();
        let extra = refreshed
            .candidates
            .iter()
            .find(|c| c.locator.contains("later.json"))
            .expect("new scan must list the newly created file");
        assert_eq!(
            frozen,
            scan_select_current(&scan.scan_id, scan.epoch).unwrap()
        );
        assert!(
            !frozen.iter().any(|id| id == &extra.candidate_id),
            "extra must stay out of select-current"
        );
        let (_pid, details) =
            preview(&extra.candidate_id, Some(&extra.source_revision)).expect("preview extra");
        let texts: Vec<_> = details.events.iter().map(|e| e.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("extra-keep-only")),
            "must preview the extra candidate, got {texts:?}"
        );
        assert!(
            texts.iter().all(|t| !t.contains("frozen-only")),
            "must not fall back to the frozen snapshot, got {texts:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_lists_later_roots_in_pages_without_caching_the_full_list() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("scan-pages-{}", Uuid::new_v4()));
        let first = tmp.join("first");
        let later = tmp.join("later");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&later).unwrap();
        for i in 0..50 {
            std::fs::write(
                first.join(format!("{i}.json")),
                r#"[{"role":"user","content":"first"}]"#,
            )
            .unwrap();
        }
        std::fs::write(
            later.join("later.json"),
            r#"[{"role":"user","content":"later-root"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![
            (Provider::GenericFile, first.to_string_lossy().into()),
            (Provider::GenericFile, later.to_string_lossy().into()),
        ])
        .unwrap();
        assert!(scan.complete);
        assert_eq!(scan.candidates.len(), 50);
        assert_eq!(scan.discovered_count, 51);
        let (_, next, _, _) = scan_page(&scan.scan_id, scan.epoch, None, Some(50)).unwrap();
        let (last, end, _, _) =
            scan_page(&scan.scan_id, scan.epoch, next.as_deref(), Some(50)).unwrap();
        assert_eq!(last.len(), 1);
        assert!(end.is_none());
        let (_, details) = preview(&last[0].candidate_id, Some(&last[0].source_revision)).unwrap();
        assert_eq!(details.events[0].text, "later-root");
        assert_eq!(
            scan_select_current(&scan.scan_id, scan.epoch)
                .unwrap()
                .len(),
            51
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn scan_file_as_codex_root_is_issue_not_empty_success() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("notdir-{}", uuid::Uuid::new_v4()));
        let good = tmp.join("files");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("ok.json"),
            r#"[{"role":"user","content":"keep-me"}]"#,
        )
        .unwrap();
        let fake = tmp.join("codex-home.jsonl");
        std::fs::write(&fake, b"not a directory").unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let scan = scan_start(vec![
            (Provider::GenericFile, good.to_string_lossy().into()),
            (Provider::Codex, fake.to_string_lossy().into()),
        ])
        .unwrap();
        assert!(
            scan.candidates
                .iter()
                .any(|c| c.locator.contains("ok.json")),
            "healthy generic must remain: {:?}",
            scan.candidates
        );
        assert!(
            scan.issues.iter().any(|i| {
                (i.code == "FORMAT_UNSUPPORTED" || i.code == "SOURCE_PERMISSION_DENIED")
                    && i.path.contains("codex-home.jsonl")
            }),
            "file-as-codex-root must be an issue, not zero sessions: {:?}",
            scan.issues
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_rejects_rewritten_source_without_relying_on_mtime() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("s04-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.json");
        std::fs::write(&path, r#"[{"role":"user","content":"before-rewrite"}]"#).unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = scan
            .candidates
            .iter()
            .find(|c| c.locator.contains("chat.json"))
            .cloned()
            .expect("candidate");
        let listed = cand.source_revision.clone();
        preview(&cand.candidate_id, Some(&listed)).expect("first preview");
        std::fs::write(&path, r#"[{"role":"user","content":"after-rewrite"}]"#).unwrap();
        let err = preview(&cand.candidate_id, Some(&listed)).unwrap_err();
        assert_eq!(err.code, ErrorCode::SourceChanged);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_events_page_reads_prepared_index_not_full_graph() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("prev-page-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chat.json"),
            r#"[{"role":"user","content":"page-one"},{"role":"assistant","content":"page-two"},{"role":"user","content":"page-three"}]"#,
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let (page1, next) = preview_events_page(&pid, None, Some(2), None).unwrap();
        assert_eq!(page1.len(), 2);
        let next = next.expect("more preview events remain");
        let (page2, next2) = preview_events_page(&pid, Some(&next), Some(2), None).unwrap();
        assert_eq!(page2.len(), 1);
        assert!(next2.is_none());
        let texts: Vec<_> = page1
            .iter()
            .chain(page2.iter())
            .map(|e| e.text.as_str())
            .collect();
        assert!(texts.iter().any(|t| t.contains("page-one")));
        assert!(texts.iter().any(|t| t.contains("page-three")));
        let err = preview_events_page(&pid, Some("99"), Some(2), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn preview_truncates_oversized_block_and_keeps_a_cursor() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("prev-big-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        let huge = "Z".repeat(2000);
        std::fs::write(
            dir.join("chat.json"),
            format!(r#"[{{"role":"user","content":"{huge}"}}]"#),
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::GenericFile, dir.to_string_lossy().into())]).unwrap();
        let cand = &scan.candidates[0];
        let (pid, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        assert!(
            details.events.iter().any(|e| e.truncated),
            "oversized preview text must be truncated: {:?}",
            details.events
        );
        assert!(details
            .events
            .iter()
            .all(|e| e.text.chars().count() <= 1024));
        let (page, _) = preview_events_page(&pid, None, Some(1), None).unwrap();
        assert_eq!(page.len(), 1);
        assert!(page[0].truncated);
        assert!(!page[0].text.contains(&huge));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn same_import_surfaces_unknown_block_missing_asset_and_oversized_as_partial() {
        use crate::session_migration::types::ImportOutcome;
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let tmp = std::env::temp_dir().join(format!("f08-{}", uuid::Uuid::new_v4()));
        let dir = tmp.join("projects").join("p");
        std::fs::create_dir_all(&dir).unwrap();
        let huge = "Z".repeat(2000);
        let assistant = format!(
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"s","message":{{"content":[{{"type":"text","text":"{huge}"}}]}}}}"#
        );
        std::fs::write(
            dir.join("s.jsonl"),
            format!(
                "{}\n{}\n",
                r#"{"type":"user","uuid":"u1","sessionId":"s","message":{"content":[{"type":"text","text":"hello"},{"type":"image","source":{"path":"gone.png","media_type":"image/png"}},{"type":"mystery_widget"}]}}"#,
                assistant
            ),
        )
        .unwrap();
        std::env::set_var("GROK_APP_HOME", &tmp);
        let _ = crate::paths::ensure_app_dirs();
        let scan = scan_start(vec![(Provider::ClaudeCode, tmp.to_string_lossy().into())]).unwrap();
        let cand = scan
            .candidates
            .first()
            .expect("claude jsonl must be a candidate");
        let (_, details) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        assert_eq!(details.outcome, ImportOutcome::Partial);
        assert_ne!(details.outcome, ImportOutcome::Complete);
        assert!(
            details.issues.iter().any(|i| i.code == "unknown-block"),
            "unknown block must be a distinct issue: {:?}",
            details.issues
        );
        assert!(
            details.issues.iter().any(|i| i.code == "asset-missing"),
            "missing image must be a distinct issue: {:?}",
            details.issues
        );
        assert!(
            details.events.iter().any(|e| e.truncated),
            "oversized output must be truncated in preview: {:?}",
            details
                .events
                .iter()
                .map(|e| e.text.len())
                .collect::<Vec<_>>()
        );
        assert!(
            details.inventory.unsupported.messages
                + details.inventory.unsupported.tool_calls
                + details.inventory.unsupported.tool_results
                > 0
        );
        assert!(details.inventory.missing.attachments >= 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    crate::session_migration::test_home::fixture_home_contract_tests!(
        scan_empty_roots_does_not_panic,
        select_current_stays_on_frozen_scan_snapshot,
        continue_create_persists_caller_edited_text,
        scan_keeps_healthy_candidates_when_one_source_is_busy,
        scan_page_does_not_repeat_ids_across_cursors,
        commit_applies_per_root_mapping_not_first_only,
        refresh_uses_new_scan_and_preserves_old_selection,
        retry_failed_does_not_duplicate_committed_identity,
        unassigned_root_does_not_inherit_first_project,
        scan_start_explicit_path_does_not_substitute_default_roots,
        events_page_is_bounded_and_past_end_is_not_empty_success,
        preview_cursor_composer_keeps_hash_in_filesystem_path,
        scan_page_and_select_reject_stale_epoch,
        preview_reads_new_candidate_without_expanding_old_selection,
        scan_lists_later_roots_in_pages_without_caching_the_full_list,
        scan_file_as_codex_root_is_issue_not_empty_success,
        preview_rejects_rewritten_source_without_relying_on_mtime,
        preview_events_page_reads_prepared_index_not_full_graph,
        preview_truncates_oversized_block_and_keeps_a_cursor,
        same_import_surfaces_unknown_block_missing_asset_and_oversized_as_partial,
        changing_selection_invalidates_plan_hash,
        changing_selection_changes_draft_hash_and_omits_secret,
        export_markdown_keeps_selected_leaf_and_drops_sibling_and_system,
        continue_create_same_request_repairs_missing_sidecar,
        continue_create_stays_prepared_without_dispatch,
        continue_stays_on_original_revision_after_later_refresh,
        preview_exposes_leaves_relations_and_default_selection_metadata,
        preview_first_page_is_bounded_for_ten_thousand_events,
        preview_reports_codex_fork_compaction_and_rollback,
        two_previews_keep_independent_selections_and_stale_plan_on_change,
        catalog_and_job_items_are_sql_paged,
        events_page_leaf_and_second_prepare_drops_draft,
        commit_loads_durable_plan_after_runtime_is_dropped,
        commit_fails_closed_when_durable_plan_is_missing,
        commit_fails_closed_on_plan_hash_mismatch,
        commit_fails_closed_when_selection_mapping_or_options_change,
        commit_rejects_same_request_id_on_a_different_plan,
        load_durable_plan_rejects_expired_record,
        recovery_restores_plan_after_runtime_drop_and_rejects_tamper,
        catalog_filter_is_bound_to_cursor_scope,
    );
