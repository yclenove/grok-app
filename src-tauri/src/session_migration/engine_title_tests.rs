//! Readable titles must survive the same durable path used by the import UI.

use super::super::types::ImportOutcome;
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use serde_json::{json, Value};

fn source(home: &IsolatedAppHome, user: Option<Value>) -> SourceCandidate {
    reset_runtime();
    let root = home.path.join("source");
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    let mut records = vec![
        json!({"type":"session_meta","payload":{"id":"native-title-id","cwd":"/synthetic/project"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Previous response"}]}}),
        json!({"type":"event_msg","payload":{"type":"user_message","message":" \n\t "}}),
    ];
    records.extend(user);
    records.push(json!({"type":"event_msg","payload":{"type":"agent_message","message":"Title acceptance reply"}}));
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    std::fs::write(root.join("sessions/rollout-file-name.jsonl"), bytes).unwrap();
    let candidates = scan_start(vec![(Provider::Codex, root.to_string_lossy().into())])
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    candidates.into_iter().next().unwrap()
}

fn publish(candidate: &SourceCandidate) -> (String, JobRecord) {
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed, "{job:?}");
    (job.items[0].import_id.clone().unwrap(), job)
}

fn transcript_source(
    home: &IsolatedAppHome,
    provider: Provider,
    records: &[Value],
) -> SourceCandidate {
    reset_runtime();
    let root = home.path.join("source");
    std::fs::create_dir_all(&root).unwrap();
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    std::fs::write(root.join("transcript.jsonl"), bytes).unwrap();
    let candidates = scan_start(vec![(provider, root.to_string_lossy().into())])
        .unwrap()
        .candidates;
    assert_eq!(candidates.len(), 1);
    candidates.into_iter().next().unwrap()
}

fn assert_durable_title(candidate: &SourceCandidate, expected: &str) {
    let source_bytes = std::fs::read(&candidate.locator).unwrap();
    assert_eq!(candidate.title, expected);
    let (_, details) = preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert_eq!(details.title.as_deref(), Some(expected));
    let (id, job) = publish(candidate);
    assert_eq!(job.items[0].title.as_deref(), Some(expected));
    reset_runtime();
    let entries = list_imported().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].import_id, id);
    assert_eq!(entries[0].title, expected);
    assert_eq!(std::fs::read(&candidate.locator).unwrap(), source_bytes);
    reset_runtime();
}

#[test]
fn cursor_titles_survive_scan_preview_publish_and_restart() {
    let expected = "中文壁纸 🚀 保持兼容";
    for (role, nested) in [("user", false), ("human", true)] {
        let home = IsolatedAppHome::acquire("cursor-readable-title");
        let content = json!([
            {"type":"text","text":" \n\t "},
            {"type":"text","text":"中文壁纸 🚀"},
            {"type":"text","text":"\n保持兼容  "}
        ]);
        let prompt = if nested {
            json!({"role":role,"message":{"content":content}})
        } else {
            json!({"role":role,"content":content})
        };
        let records = [
            json!({"role":"assistant","content":"An assistant answer is not the title"}),
            json!({"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Tool output is not the title"}]}),
            prompt,
            json!({"role":"assistant","content":"done"}),
        ];
        let candidate = transcript_source(&home, Provider::Cursor, &records);
        assert_durable_title(&candidate, expected);
    }
}

#[test]
fn cursor_title_fallback_and_unicode_limit_survive_publication() {
    for (content, expected) in [
        (" \n\t ".to_string(), "transcript".to_string()),
        ("你🚀".repeat(100), "你🚀".repeat(40)),
    ] {
        let home = IsolatedAppHome::acquire("cursor-title-fallback");
        let records = [
            json!({"role":"user","content":content}),
            json!({"role":"assistant","content":"done"}),
        ];
        let candidate = transcript_source(&home, Provider::Cursor, &records);
        assert_durable_title(&candidate, &expected);
    }
}

#[test]
fn claude_compaction_cannot_supply_titles_in_scan_preview_or_catalog() {
    for real_prompt in [true, false] {
        let home = IsolatedAppHome::acquire("claude-compact-prompt-title");
        let mut records = vec![
            json!({"type":"user","uuid":"summary","sessionId":"compact-title-native","isCompactSummary":true,"message":{"content":[{"type":"text","text":"Synthetic compact summary is not a prompt"},{"type":"thinking","thinking":"Synthetic reasoning stays opt-in"}]}}),
            json!({"type":"assistant","uuid":"answer","parentUuid":"summary","message":{"content":"done"}}),
        ];
        if real_prompt {
            records.push(json!({"type":"user","uuid":"real","parentUuid":"answer","message":{"content":"用户的真实问题"}}));
        }
        let candidate = transcript_source(&home, Provider::ClaudeCode, &records);
        let (_, details) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        let summary = details
            .events
            .iter()
            .find(|event| event.event_id == "claude:summary")
            .unwrap();
        assert_eq!(summary.kind, "compaction");
        assert!(summary.role.is_none());
        assert!(summary.field_availability.compaction_summary);
        assert!(summary.field_availability.visible_reasoning);
        assert!(!summary.field_availability.message_text);
        assert_durable_title(
            &candidate,
            if real_prompt {
                "用户的真实问题"
            } else {
                "compact-title-native"
            },
        );
    }
}

#[test]
fn claude_prompt_title_ignores_assistant_sidechain_meta_and_tool_result() {
    let expected = "中文验收 🚀 继续开发";
    for content in [
        json!(format!(" \n {expected}\t ")),
        json!([{"type":"text","text":"中文验收 🚀"},{"type":"text","text":"\n继续开发"}]),
    ] {
        let home = IsolatedAppHome::acquire("claude-readable-title");
        let records = [
            json!({"type":"assistant","uuid":"a0","sessionId":"claude-native-title","message":{"content":"An assistant answer is not the title"}}),
            json!({"type":"user","uuid":"side","parentUuid":"a0","isSidechain":true,"message":{"content":"Sidechain title"}}),
            json!({"type":"user","uuid":"meta","parentUuid":"a0","isMeta":true,"message":{"content":"System metadata title"}}),
            json!({"type":"assistant","uuid":"a1","parentUuid":"a0","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}),
            json!({"type":"user","uuid":"r1","parentUuid":"a1","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"Tool output title"}]}}),
            json!({"type":"user","uuid":"blank","parentUuid":"r1","message":{"content":" \n\t "}}),
            json!({"type":"user","uuid":"u1","parentUuid":"blank","message":{"content":content}}),
            json!({"type":"assistant","uuid":"a2","parentUuid":"u1","message":{"content":"done"}}),
        ];
        let candidate = transcript_source(&home, Provider::ClaudeCode, &records);
        assert_durable_title(&candidate, expected);
    }
}

#[test]
fn claude_appended_custom_title_is_preserved_and_wins_over_generated_metadata() {
    let home = IsolatedAppHome::acquire("claude-appended-title");
    let expected = "保留原始自定义标题🚀".repeat(12);
    let mut records = vec![
        json!({"type":"user","uuid":"u1","sessionId":"claude-title-native","message":{"content":"First prompt fallback"}}),
        json!({"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"content":"done"}}),
        json!({"type":"custom-title","customTitle":"Earlier custom title","sessionId":"claude-title-native"}),
    ];
    for index in 0..1600 {
        records.push(
            json!({"type":"progress","uuid":format!("progress-{index}"),"data":"x".repeat(100)}),
        );
    }
    records.extend([
        json!({"type":"custom-title","customTitle":format!("  {expected}  "),"sessionId":"claude-title-native"}),
        json!({"type":"ai-title","aiTitle":"A later generated title"}),
        json!({"type":"summary","summary":"A later summary"}),
        json!({"type":"last-prompt","lastPrompt":"A later prompt"}),
        json!({"type":"custom-title","customTitle":" \n\t "}),
        json!({"type":"custom-title","customTitle":"A sidechain name","isSidechain":true}),
    ]);
    let candidate = transcript_source(&home, Provider::ClaudeCode, &records);
    assert!(std::fs::metadata(&candidate.locator).unwrap().len() > 128 * 1024);
    let normalized = adapters::claude::normalize_file(Path::new(&candidate.locator)).unwrap();
    assert_eq!(
        normalized.outcome,
        ImportOutcome::Complete,
        "{:?}",
        normalized.issues
    );
    assert!(!normalized.graph.events.iter().any(|event| matches!(event,
        super::super::types::ImportedEvent::Unsupported { source_type, .. }
        if matches!(source_type.as_str(), "custom-title" | "ai-title" | "last-prompt")
    )));
    assert_durable_title(&candidate, &expected);
}

#[test]
fn claude_title_precedence_and_native_fallback_survive_publication() {
    for (metadata, expected) in [
        (vec![], "claude-title-native"),
        (
            vec![json!({"type":"summary","summary":"Source summary"})],
            "Source summary",
        ),
        (
            vec![
                json!({"type":"summary","summary":"Source summary"}),
                json!({"type":"last-prompt","lastPrompt":"Recent prompt"}),
            ],
            "Recent prompt",
        ),
        (
            vec![
                json!({"type":"ai-title","aiTitle":"Generated title"}),
                json!({"type":"summary","summary":"Later summary"}),
            ],
            "Generated title",
        ),
    ] {
        let home = IsolatedAppHome::acquire("claude-title-precedence");
        let mut records = vec![
            json!({"type":"assistant","uuid":"a1","sessionId":"claude-title-native","message":{"content":"done"}}),
        ];
        records.extend(metadata);
        let candidate = transcript_source(&home, Provider::ClaudeCode, &records);
        assert_durable_title(&candidate, expected);
    }
}

#[test]
fn repeated_cursor_and_claude_import_repairs_only_automatic_titles_and_keeps_aliases() {
    for (provider, records) in [
        (
            Provider::Cursor,
            vec![json!({"role":"user","content":"Readable source title"})],
        ),
        (
            Provider::ClaudeCode,
            vec![json!({"type":"user","uuid":"u1","message":{"content":"Readable source title"}})],
        ),
    ] {
        let home = IsolatedAppHome::acquire("transcript-title-reimport");
        let candidate = transcript_source(&home, provider, &records);
        let (id, first) = publish(&candidate);
        let conn = open_catalog().unwrap();
        let revision = first.items[0].revision_id.as_deref().unwrap();
        let archive = crate::session_migration::catalog::revision_dir(&id, revision)
            .unwrap()
            .join("manifest.json");
        let original = std::fs::read(&archive).unwrap();
        conn.execute(
            "UPDATE entries SET title=native_session_id, alias='My alias' WHERE import_id=?1",
            [&id],
        )
        .unwrap();
        let (_, repeated) = publish(&candidate);
        assert_eq!(repeated.items[0].outcome.as_deref(), Some("deduplicated"));
        assert_eq!(repeated.items[0].revision_id, first.items[0].revision_id);
        let entry = get_entry(&conn, &id).unwrap().unwrap();
        assert_eq!(entry.title, "Readable source title");
        assert_eq!(entry.alias.as_deref(), Some("My alias"));
        assert_eq!(std::fs::read(&archive).unwrap(), original);
        assert_eq!(list_import_revisions(&id).unwrap().len(), 1);
        conn.execute(
            "UPDATE entries SET title='Reviewed title' WHERE import_id=?1",
            [&id],
        )
        .unwrap();
        publish(&candidate);
        assert_eq!(
            get_entry(&conn, &id).unwrap().unwrap().title,
            "Reviewed title"
        );
        reset_runtime();
    }
}

#[test]
fn codex_titles_survive_scan_prepare_publish_catalog_and_restart() {
    let expected = "中文验收：导入会话和工作区";
    let variants = [
        json!({"type":"event_msg","payload":{"type":"user_message","message":format!("  {expected}\n ")}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":expected}]}}),
    ];
    for user in variants {
        let home = IsolatedAppHome::acquire("codex-readable-title");
        let candidate = source(&home, Some(user));
        assert_eq!(candidate.title, expected);
        // The legacy discovery path must also ignore a different rollout filename.
        assert_eq!(
            adapters::codex::discover(&home.path.join("source")).unwrap()[0].title,
            expected
        );
        let (_, details) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        assert_eq!(details.title.as_deref(), Some(expected));
        let (id, job) = publish(&candidate);
        assert_eq!(job.items[0].title.as_deref(), Some(expected));
        reset_runtime();
        let entries = list_imported().unwrap();
        assert_eq!(entries[0].title, expected);
        assert_eq!(entries[0].native_session_id, "native-title-id");
        let (events, _, _) = events_page(&id, None, None, Some(20), None).unwrap();
        assert!(events
            .iter()
            .any(|event| event.to_string().contains(expected)));
        let (found, _, count) =
            list_imported_page_filtered(None, Some(10), None, Some(expected), None).unwrap();
        assert_eq!(count, 1);
        assert_eq!(found[0].import_id, id);
        reset_runtime();
    }
}

#[test]
fn codex_title_fallback_is_native_identity_and_unicode_limit_is_safe() {
    for (user, expected) in [
        (None, "native-title-id".to_string()),
        (
            Some(
                json!({"type":"event_msg","payload":{"type":"user_message","message":"你🚀".repeat(100)}}),
            ),
            "你🚀".repeat(40),
        ),
    ] {
        let home = IsolatedAppHome::acquire("codex-title-fallback");
        let candidate = source(&home, user);
        assert_eq!(candidate.title, expected);
        let (_, job) = publish(&candidate);
        assert_eq!(job.items[0].title.as_deref(), Some(expected.as_str()));
        assert_eq!(list_imported().unwrap()[0].title, expected);
        reset_runtime();
    }
}

#[test]
fn repeated_codex_import_repairs_only_automatic_title_without_another_revision() {
    let home = IsolatedAppHome::acquire("codex-title-reimport");
    let candidate = source(
        &home,
        Some(
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Readable source title"}}),
        ),
    );
    // Create a genuine unsigned historical job before applying its historical
    // SQL semantic-key representation. Never rewrite a signed revision key.
    let (preview, _) = preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let first_plan = plan(vec![preview], vec![]).unwrap();
    let queued = enqueue_legacy_plan(&first_plan, &Uuid::new_v4().to_string());
    crate::session_migration::job_control::run_pending().unwrap();
    let first = job_get(&queued.job_id).unwrap();
    assert_eq!(first.state, JobState::Completed, "{first:?}");
    let id = first.items[0].import_id.clone().unwrap();
    let conn = open_catalog().unwrap();
    let revision = first.items[0].revision_id.as_deref().unwrap();
    let archive = crate::session_migration::catalog::revision_dir(&id, revision)
        .unwrap()
        .join("manifest.json");
    let original = std::fs::read(&archive).unwrap();
    // An older preview imported the same source with the native id as its title.
    let receipt = crate::session_migration::jobs::latest_receipt_for_revision(&conn, &id, revision)
        .unwrap()
        .unwrap();
    let legacy_key = crate::session_migration::jobs::revision_semantic_key(
        &receipt.options_hash,
        &receipt.selection_hash,
    );
    conn.execute(
        "UPDATE revisions SET options_hash=?1 WHERE revision_id=?2",
        rusqlite::params![legacy_key, revision],
    )
    .unwrap();
    conn.execute(
        "UPDATE entries SET title=native_session_id, alias='My alias' WHERE import_id=?1",
        [&id],
    )
    .unwrap();
    let (_, second) = publish(&candidate);
    assert_eq!(second.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(second.items[0].revision_id, first.items[0].revision_id);
    let entry = get_entry(&conn, &id).unwrap().unwrap();
    assert_eq!(entry.title, "Readable source title");
    assert_eq!(entry.alias.as_deref(), Some("My alias"));
    assert_eq!(std::fs::read(&archive).unwrap(), original);
    assert_eq!(list_import_revisions(&id).unwrap().len(), 1);
    conn.execute(
        "UPDATE entries SET title='Reviewed title' WHERE import_id=?1",
        [&id],
    )
    .unwrap();
    publish(&candidate);
    assert_eq!(
        get_entry(&conn, &id).unwrap().unwrap().title,
        "Reviewed title"
    );
    reset_runtime();
}

#[test]
fn signed_codex_reimport_repairs_display_title_without_changing_its_revision() {
    let home = IsolatedAppHome::acquire("codex-signed-title-reimport");
    let candidate = source(
        &home,
        Some(
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Signed source title"}}),
        ),
    );
    let (id, first) = publish(&candidate);
    let conn = open_catalog().unwrap();
    conn.execute(
        "UPDATE entries SET title=native_session_id,alias='Keep my alias' WHERE import_id=?1",
        [&id],
    )
    .unwrap();
    let (_, second) = publish(&candidate);
    assert_eq!(second.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(second.items[0].revision_id, first.items[0].revision_id);
    let entry = get_entry(&conn, &id).unwrap().unwrap();
    assert_eq!(entry.title, "Signed source title");
    assert_eq!(entry.alias.as_deref(), Some("Keep my alias"));
    assert_eq!(list_import_revisions(&id).unwrap().len(), 1);
    reset_runtime();
}

#[test]
fn fresh_previews_deduplicate_only_when_the_selected_content_and_fields_match() {
    let home = IsolatedAppHome::acquire("codex-selection-reimport");
    let candidate = source(
        &home,
        Some(
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Selection acceptance"}}),
        ),
    );
    let (id, first) = publish(&candidate);
    let (_, repeated) = publish(&candidate);
    assert_eq!(repeated.items[0].revision_id, first.items[0].revision_id);
    assert_eq!(repeated.items[0].outcome.as_deref(), Some("deduplicated"));
    let (preview_id, details) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mut selection = details.default_selection;
    selection.fields.message_text = false;
    let plan = plan_with_selection(vec![preview_id], vec![], Some(&selection)).unwrap();
    let job = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let changed = job_get(&job.job_id).unwrap();
    assert_eq!(changed.state, JobState::Completed);
    assert_ne!(changed.items[0].revision_id, first.items[0].revision_id);
    assert_ne!(changed.items[0].outcome.as_deref(), Some("deduplicated"));
    assert_eq!(list_import_revisions(&id).unwrap().len(), 2);
    reset_runtime();
}

#[test]
fn project_mapping_changes_are_published_even_when_the_history_is_identical() {
    let home = IsolatedAppHome::acquire("codex-mapping-reimport");
    let candidate = source(
        &home,
        Some(
            json!({"type":"event_msg","payload":{"type":"user_message","message":"Mapping acceptance"}}),
        ),
    );
    let mut projects = Vec::new();
    for name in ["project-one", "project-two"] {
        let path = home.path.join(name);
        std::fs::create_dir(&path).unwrap();
        projects.push(crate::store::add_project(path.to_string_lossy().into(), false).unwrap());
    }
    let conn = open_catalog().unwrap();
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let selection = default_selection_set(std::slice::from_ref(&preview_id)).unwrap();
    let mut item =
        engine_commit::commit_item_from_preview(&preview_id, &[], false, selection).unwrap();
    item.mapping_project_id = Some(projects[0].id.clone());
    let first = crate::session_migration::jobs::publish_item(
        &conn,
        &Uuid::new_v4().to_string(),
        &item,
        &|| false,
    )
    .unwrap();
    let repeated = crate::session_migration::jobs::publish_item(
        &conn,
        &Uuid::new_v4().to_string(),
        &item,
        &|| false,
    )
    .unwrap();
    assert_eq!(repeated.revision_id, first.revision_id);
    assert_eq!(repeated.outcome.as_deref(), Some("deduplicated"));
    item.mapping_project_id = Some(projects[1].id.clone());
    let changed = crate::session_migration::jobs::publish_item(
        &conn,
        &Uuid::new_v4().to_string(),
        &item,
        &|| false,
    )
    .unwrap();
    assert_ne!(changed.revision_id, first.revision_id);
    assert_eq!(
        changed.mapping_project_id.as_deref(),
        Some(projects[1].id.as_str())
    );
    assert_eq!(
        list_import_revisions(first.import_id.as_deref().unwrap())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(crate::store::load_projects().len(), 2);
    reset_runtime();
}
