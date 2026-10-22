//! Partial identities through the production consumer and actual durable writer.
use super::tool_owner_tests::{finish_receipt, prepare_busy, receipt, start_receipt};
use super::*;
use crate::session_manager::tool_event::{ToolCallChunk, ToolEvent, ToolUpdate};
use serde_json::{json, Value};

fn fixture(background: bool) -> Fixture {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    if background {
        fixture.manager.try_park_live().unwrap();
    }
    fixture
}

fn event(title: &str, kind: &str, status: &str, raw: Value) -> ToolEvent {
    ToolEvent::Call(ToolCallChunk {
        id: "identity-call".into(),
        title: title.into(),
        kind: kind.into(),
        status: status.into(),
        raw,
    })
}

fn start(fixture: &Fixture) {
    receipt(
        fixture,
        event(
            "读取说明",
            "read",
            "in_progress",
            json!({
                "_meta": { "x.ai/tool": { "name": "read_file" } },
                "rawInput": { "target_file": "说明.md" },
            }),
        ),
    )
    .unwrap();
}

fn complete(fixture: &Fixture) -> ToolUpdate {
    receipt(
        fixture,
        event("", "", "completed", json!({ "content": ["实际中文输出"] })),
    )
    .unwrap()
}

fn assert_identity(fixture: &Fixture, update: ToolUpdate, name: &str, input: &str, title: &str) {
    let tool = update.tool.unwrap();
    assert_eq!(
        tool["input"], input,
        "actual tool payload lost its argument"
    );
    assert_eq!(tool["title"], title, "actual tool payload lost its title");
    let rows = store::load_messages(&fixture.target_meta.id);
    assert_eq!(rows.len(), 1, "completion must upsert one durable tool row");
    assert_eq!(
        rows[0].content.lines().next(),
        Some(format!("tool_step|completed|{name}|{title}").as_str())
    );
    assert_eq!(
        rows[0].content.lines().nth(1),
        Some(format!("input:{input}").as_str())
    );
    assert!(rows[0].content.contains("实际中文输出"));
    assert!(store::load_messages(&fixture.other_meta.id).is_empty());
    let cached = fixture.manager.tool_identities.lock().unwrap()[&fixture.target_meta.id]
        ["identity-call"]
        .clone();
    assert_eq!(cached.name, name);
    assert_eq!(cached.input.as_deref(), Some(input));
    assert_eq!(cached.title, title);
}

#[test]
fn actual_title_only_update_keeps_machine_name_kind_and_input() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        receipt(
            &fixture,
            event("再次读取说明", "", "in_progress", json!({})),
        )
        .unwrap();
        let update = complete(&fixture);
        assert_eq!(update.tool.as_ref().unwrap()["kind"], "read");
        assert_identity(&fixture, update, "read_file", "说明.md", "再次读取说明");
    }
}

#[test]
fn actual_kind_only_update_keeps_machine_name_title_and_input() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        receipt(&fixture, event("", "read", "in_progress", json!({}))).unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_input_only_update_replaces_argument_without_erasing_identity() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        receipt(
            &fixture,
            event(
                "",
                "",
                "in_progress",
                json!({"rawInput": {"path": "新说明.md"}}),
            ),
        )
        .unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "新说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_placeholder_update_never_replaces_specific_identity() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        receipt(
            &fixture,
            event(
                " TOOL ",
                "tool",
                "in_progress",
                json!({"toolName": "Tool", "rawInput": "null"}),
            ),
        )
        .unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_generic_other_kind_never_downgrades_known_category() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        let partial = receipt(&fixture, event("", "other", "in_progress", json!({}))).unwrap();
        assert_eq!(partial.tool.unwrap()["kind"], "read");
        let update = complete(&fixture);
        assert_eq!(update.tool.as_ref().unwrap()["kind"], "read");
        assert_identity(&fixture, update, "read_file", "说明.md", "读取说明");
    }
}

#[test]
fn actual_complementary_notifications_accumulate_identity() {
    for background in [false, true] {
        let fixture = fixture(background);
        for update in [
            event("读取说明", "", "in_progress", json!({})),
            event("", "read", "in_progress", json!({})),
            event(
                "",
                "",
                "in_progress",
                json!({"_meta": {"x.ai/tool": {"name": "read_file"}}}),
            ),
            event(
                "",
                "",
                "in_progress",
                json!({"raw_input": {"path": "说明.md"}}),
            ),
        ] {
            receipt(&fixture, update).unwrap();
        }
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_empty_camel_name_does_not_mask_meaningful_snake_alias() {
    for background in [false, true] {
        let fixture = fixture(background);
        receipt(
            &fixture,
            event(
                "读取说明",
                "read",
                "in_progress",
                json!({
                    "toolName": "", "tool_name": "read_file", "rawInput": {"path": "说明.md"},
                }),
            ),
        )
        .unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_meaningful_correction_updates_all_identity_fields() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        receipt(
            &fixture,
            event(
                "编辑说明",
                "edit",
                "in_progress",
                json!({
                    "toolName": "edit_file", "rawInput": {"path": "新说明.md"},
                }),
            ),
        )
        .unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "edit_file",
            "新说明.md",
            "编辑说明",
        );
    }
}

#[test]
fn actual_status_only_update_keeps_full_identity() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "读取说明",
        );
    }
}

#[test]
fn actual_prepared_completion_observes_identity_accepted_before_its_mutation() {
    for background in [false, true] {
        let fixture = fixture(background);
        let paused = start_receipt(
            &fixture,
            event("", "", "completed", json!({"content": ["实际中文输出"]})),
            "mutation",
        );
        start(&fixture);
        let update = finish_receipt(paused).unwrap();
        assert_identity(&fixture, update, "read_file", "说明.md", "读取说明");
        let events = fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| session.tools_this_turn)
            .unwrap();
        assert_eq!(events, 2, "a preparation retry consumed the event twice");
    }
}

#[test]
fn actual_identity_retry_rechecks_media_gate_before_accepting_terminal_output() {
    for background in [false, true] {
        let fixture = fixture(background);
        let paused = start_receipt(
            &fixture,
            event(
                "",
                "",
                "completed",
                json!({
                    "content": [{"type": "content", "content": {"type": "text", "text": "实际中文输出 https://example.com/arbitrary.png"}}],
                }),
            ),
            "mutation",
        );
        receipt(
            &fixture,
            event(
                "运行命令",
                "execute",
                "in_progress",
                json!({
                    "toolName": "run_terminal_command", "rawInput": {"command": "echo 合成输出"},
                }),
            ),
        )
        .unwrap();
        let update = finish_receipt(paused).unwrap();
        assert!(
            update.generated.is_none(),
            "old preparation bypassed the accepted terminal tool's media gate"
        );
        assert_eq!(update.tool.as_ref().unwrap()["kind"], "execute");
        assert_identity(
            &fixture,
            update,
            "run_terminal_command",
            "echo 合成输出",
            "运行命令",
        );
        let attachments = fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                session.stream_attachments.len()
            })
            .unwrap();
        assert_eq!(attachments, 0);
    }
}

#[test]
fn actual_identity_updates_at_capacity_keep_existing_entries_without_growing_cache() {
    for background in [false, true] {
        let fixture = fixture(background);
        start(&fixture);
        for index in 0..1999 {
            let mut filler = match event(
                "读取文件",
                "read",
                "in_progress",
                json!({"toolName": "read_file"}),
            ) {
                ToolEvent::Call(call) => call,
                ToolEvent::Released(_) => unreachable!(),
            };
            filler.id = format!("filler-{index}");
            receipt(&fixture, ToolEvent::Call(filler)).unwrap();
        }
        let mut overflow = match event(
            "读取文件",
            "read",
            "in_progress",
            json!({"toolName": "read_file"}),
        ) {
            ToolEvent::Call(call) => call,
            ToolEvent::Released(_) => unreachable!(),
        };
        overflow.id = "overflow".into();
        receipt(&fixture, ToolEvent::Call(overflow)).unwrap();
        receipt(&fixture, event("更新标题", "", "in_progress", json!({}))).unwrap();
        assert_identity(
            &fixture,
            complete(&fixture),
            "read_file",
            "说明.md",
            "更新标题",
        );
        let cache = fixture.manager.tool_identities.lock().unwrap();
        assert_eq!(cache[&fixture.target_meta.id].len(), 2000);
        assert!(!cache[&fixture.target_meta.id].contains_key("overflow"));
    }
}
