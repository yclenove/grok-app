use super::*;
use crate::session_migration::types::{EventBase, RelationConfidence, RelationKind, TimeOrigin};

fn message(id: &str, ordinal: u32, asset: Option<&str>) -> ImportedEvent {
    let mut blocks = vec![ContentBlock::text(id)];
    if let Some(id) = asset {
        blocks.push(ContentBlock::Asset {
            asset_id: id.into(),
            alt: Some(id.into()),
        });
    }
    ImportedEvent::Message {
        base: EventBase {
            id: id.into(),
            ordinal,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role: MessageRole::User,
        blocks,
        complete: true,
    }
}

fn relation(from: &str, to: &str, kind: RelationKind) -> EventRelation {
    EventRelation {
        from: from.into(),
        to: to.into(),
        kind,
        confidence: RelationConfidence::Explicit,
    }
}

fn fixture() -> (AllowedSelection, Vec<ImportedEvent>, Vec<EventRelation>) {
    let events = vec![
        message("root", 0, Some("before")),
        message("child-early", 2, None),
        message("other-branch", 3, None),
        message("middle", 4, None),
        message("child-later", 5, Some("child-after")),
        message("later", 6, Some("after")),
    ];
    let relations = vec![
        relation("middle", "root", RelationKind::Parent),
        relation("later", "middle", RelationKind::Parent),
        relation("other-branch", "root", RelationKind::Parent),
        relation("child-early", "root", RelationKind::Subagent),
        relation("child-later", "child-early", RelationKind::Parent),
    ];
    let mut selection = AllowedSelection::for_events(vec![
        "root".into(),
        "child-early".into(),
        "middle".into(),
        "child-later".into(),
        "later".into(),
    ]);
    selection.pinned_event_ids = vec!["root".into(), "later".into()];
    selection.asset_ids = vec![
        "before".into(),
        "after".into(),
        "child-after".into(),
        "unreferenced".into(),
    ];
    selection.include_subagents = true;
    selection.fields.visible_reasoning = true;
    selection.fields.tool_call_input = false;
    (selection, events, relations)
}

#[test]
fn prefix_preserves_reviewed_fields_but_never_future_pins_assets_or_subagent_events() {
    let (published, events, relations) = fixture();
    let before = published.hash();
    let narrowed = prefix(&published, "later", "middle", &events, &relations).unwrap();
    assert_eq!(narrowed.event_ids, vec!["root", "child-early", "middle"]);
    assert_eq!(narrowed.pinned_event_ids, vec!["root"]);
    assert_eq!(narrowed.asset_ids, vec!["before"]);
    assert_eq!(narrowed.fields, published.fields);
    assert_eq!(narrowed.include_subagents, published.include_subagents);
    assert_eq!(published.hash(), before);
    assert!(narrowed.project_event(&events[4], &relations).is_none());
    assert!(!narrowed.permits_asset("after"));
}

#[test]
fn prefix_rejects_other_branches_unselected_messages_and_non_message_cutoffs() {
    let (mut published, mut events, relations) = fixture();
    assert!(prefix(&published, "later", "other-branch", &events, &relations).is_err());
    published.event_ids.retain(|id| id != "middle");
    assert!(prefix(&published, "later", "middle", &events, &relations).is_err());
    published.event_ids.push("middle".into());
    let base = events[3].base().clone();
    events[3] = ImportedEvent::Compaction {
        base,
        summary: vec![ContentBlock::text("summary")],
        covered_event_ids: None,
        coverage_known: false,
    };
    assert!(prefix(&published, "later", "middle", &events, &relations).is_err());
}

#[test]
fn prefix_uses_graph_ancestry_and_cannot_enable_subagents_or_attachments() {
    let (mut published, events, relations) = fixture();
    published.event_ids.retain(|id| !id.starts_with("child-"));
    published.include_subagents = false;
    published.asset_ids.clear();
    published.fields.visible_reasoning = false;
    let narrowed = prefix(&published, "later", "middle", &events, &relations).unwrap();
    assert_eq!(narrowed.event_ids, vec!["root", "middle"]);
    assert!(!narrowed.include_subagents);
    assert!(narrowed.asset_ids.is_empty());
    assert!(!narrowed.fields.visible_reasoning);
}

#[test]
fn prefix_supports_an_explicitly_selected_subagent_message_without_other_threads() {
    let (published, events, relations) = fixture();
    let narrowed = prefix(&published, "later", "child-early", &events, &relations).unwrap();
    assert_eq!(narrowed.event_ids, vec!["child-early"]);
    assert!(narrowed.asset_ids.is_empty());
    assert!(narrowed.pinned_event_ids.is_empty());
    assert!(narrowed.project_event(&events[0], &relations).is_none());
    assert!(narrowed.project_event(&events[1], &relations).is_some());
    assert!(narrowed.project_event(&events[4], &relations).is_none());
}
