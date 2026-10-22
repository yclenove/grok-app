use super::*;
use crate::session_migration::search::build_index;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::session_migration::types::{
    ContentBlock, EventBase, MessageRole, RelationConfidence, RelationKind, TimeOrigin,
};

fn message(id: &str, ordinal: u32, text: &str) -> ImportedEvent {
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

fn value_id(value: &serde_json::Value) -> &str {
    value.get("id").and_then(serde_json::Value::as_str).unwrap()
}

fn write_manifest(dir: &Path, relations: &[EventRelation], active_leaf: &str) {
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::json!({
            "graph": { "relations": relations, "activeLeaf": active_leaf }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn revision_diagnostics_are_shared_by_pages_and_context_without_reparsing() {
    let _home = IsolatedAppHome::acquire("reader-revision-health");
    let dir = std::env::temp_dir().join(format!("reader-health-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    build_index(&dir, &[message("e1", 0, "readable")]).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::json!({
            "graph": {"relations": [], "activeLeaf": "e1"},
            "outcome": "partial",
            "issues": [{"code":"unknown-role", "message":"Skipped event"}]
        })
        .to_string(),
    )
    .unwrap();
    MANIFEST_PARSES.with(|count| count.set(0));
    let page = event_page(
        &dir,
        "imp:health",
        "rev-a",
        None,
        None,
        Some(20),
        PageDirection::Forward,
    )
    .unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.health.outcome, "partial");
    assert_eq!(page.health.issue_count, Some(1));
    let context = event_context(
        &dir,
        "imp:health",
        "rev-a",
        None,
        &EventContextRequest {
            event_id: "e1".into(),
            before: None,
            after: None,
            cursor: None,
        },
    )
    .unwrap();
    assert_eq!(context.health.issues[0].code, "unknown-role");
    assert_eq!(MANIFEST_PARSES.with(std::cell::Cell::get), 1);
    assert_eq!(
        serde_json::to_value(page.health).unwrap(),
        serde_json::to_value(context.health).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn jumps_to_early_middle_and_late_events_without_scanning_pages() {
    let _home = IsolatedAppHome::acquire("reader-context-capacity");
    let dir = std::env::temp_dir().join(format!("reader-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let events: Vec<_> = (0..10_000)
        .map(|i| message(&format!("e{i}"), i, &format!("body-{i}")))
        .collect();
    let relations: Vec<_> = (1..10_000)
        .map(|i| EventRelation {
            kind: RelationKind::Parent,
            from: format!("e{i}"),
            to: format!("e{}", i - 1),
            confidence: RelationConfidence::Explicit,
        })
        .collect();
    build_index(&dir, &events).unwrap();
    write_manifest(&dir, &relations, "e9999");
    LINEAGE_BUILDS.with(|count| count.set(0));
    MANIFEST_PARSES.with(|count| count.set(0));
    for id in ["e0", "e5000", "e9999"] {
        let result = event_context(
            &dir,
            "imp:test",
            "rev-test",
            Some("e9999"),
            &EventContextRequest {
                event_id: id.into(),
                before: Some(3),
                after: Some(4),
                cursor: None,
            },
        )
        .unwrap();
        assert!(result.events.len() <= 8);
        assert_eq!(value_id(&result.events[result.target_index as usize]), id);
        assert_eq!(result.total, 10_000);
    }
    assert_eq!(
        LINEAGE_BUILDS.with(std::cell::Cell::get),
        1,
        "one immutable branch should build its lineage once"
    );
    assert_eq!(
        MANIFEST_PARSES.with(std::cell::Cell::get),
        1,
        "one immutable revision should parse its manifest once"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn branch_and_cursor_scope_fail_closed() {
    let _home = IsolatedAppHome::acquire("reader-context-auth");
    let dir = std::env::temp_dir().join(format!("reader-context-auth-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let events = vec![
        message("root", 0, "root"),
        message("a", 1, "a"),
        message("b", 2, "b"),
    ];
    let relations = vec![
        EventRelation {
            kind: RelationKind::Parent,
            from: "a".into(),
            to: "root".into(),
            confidence: RelationConfidence::Explicit,
        },
        EventRelation {
            kind: RelationKind::Parent,
            from: "b".into(),
            to: "root".into(),
            confidence: RelationConfidence::Explicit,
        },
    ];
    build_index(&dir, &events).unwrap();
    write_manifest(&dir, &relations, "a");
    let request = EventContextRequest {
        event_id: "a".into(),
        before: Some(1),
        after: Some(1),
        cursor: None,
    };
    let cursor = encode_cursor("imp:test", "rev-test", "a", CONTEXT_CURSOR_FILTER, 1, "a").unwrap();
    let mut bad = request.clone();
    bad.cursor = Some(cursor);
    assert_eq!(
        event_context(&dir, "imp:test", "rev-test", Some("missing"), &request)
            .unwrap_err()
            .code,
        ErrorCode::HistoryAccessUnavailable
    );
    assert!(event_context(&dir, "imp:test", "rev-test", Some("b"), &bad).is_err());
    assert!(event_context(&dir, "imp:test", "rev-other", Some("a"), &bad).is_err());
    assert!(event_context(
        &dir,
        "imp:test",
        "rev-test",
        Some("a"),
        &EventContextRequest {
            event_id: "b".into(),
            ..request
        }
    )
    .is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn event_pages_round_trip_forward_and_backward_with_bound_cursors() {
    let _home = IsolatedAppHome::acquire("reader-page-round-trip");
    let dir = std::env::temp_dir().join(format!("reader-page-round-trip-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let events: Vec<_> = (0..250)
        .map(|i| message(&format!("e{i}"), i, &format!("body-{i}")))
        .collect();
    let relations: Vec<_> = (1..250)
        .map(|i| EventRelation {
            kind: RelationKind::Parent,
            from: format!("e{i}"),
            to: format!("e{}", i - 1),
            confidence: RelationConfidence::Explicit,
        })
        .collect();
    build_index(&dir, &events).unwrap();
    write_manifest(&dir, &relations, "e249");

    let first = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e249"),
        None,
        Some(100),
        PageDirection::Forward,
    )
    .unwrap();
    assert_eq!((first.start, first.end, first.total), (0, 100, 250));
    assert_eq!(value_id(first.events.first().unwrap()), "e0");
    assert_eq!(value_id(first.events.last().unwrap()), "e99");
    assert!(first.previous_cursor.is_none());

    let second = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e249"),
        first.next_cursor.as_deref(),
        Some(100),
        PageDirection::Forward,
    )
    .unwrap();
    assert_eq!((second.start, second.end), (100, 200));
    assert_eq!(value_id(second.events.first().unwrap()), "e100");
    assert_eq!(value_id(second.events.last().unwrap()), "e199");

    let again = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e249"),
        second.previous_cursor.as_deref(),
        Some(100),
        PageDirection::Backward,
    )
    .unwrap();
    assert_eq!((again.start, again.end), (0, 100));
    assert_eq!(value_id(again.events.first().unwrap()), "e0");
    assert_eq!(value_id(again.events.last().unwrap()), "e99");
    assert!(again.previous_cursor.is_none());
    assert!(again.next_cursor.is_some());

    assert!(event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e249"),
        first.next_cursor.as_deref(),
        Some(100),
        PageDirection::Backward,
    )
    .is_err());
    assert!(PageDirection::parse(Some("sideways")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn context_cursors_open_the_adjacent_bounded_windows() {
    let _home = IsolatedAppHome::acquire("reader-context-adjacent");
    let dir =
        std::env::temp_dir().join(format!("reader-context-adjacent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let events: Vec<_> = (0..400)
        .map(|i| message(&format!("e{i}"), i, &format!("body-{i}")))
        .collect();
    let relations: Vec<_> = (1..400)
        .map(|i| EventRelation {
            kind: RelationKind::Parent,
            from: format!("e{i}"),
            to: format!("e{}", i - 1),
            confidence: RelationConfidence::Explicit,
        })
        .collect();
    build_index(&dir, &events).unwrap();
    write_manifest(&dir, &relations, "e399");
    let context = event_context(
        &dir,
        "imp:test",
        "rev-test",
        Some("e399"),
        &EventContextRequest {
            event_id: "e200".into(),
            before: Some(8),
            after: Some(8),
            cursor: None,
        },
    )
    .unwrap();
    assert_eq!(value_id(context.events.first().unwrap()), "e192");
    assert_eq!(value_id(context.events.last().unwrap()), "e208");

    let previous = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e399"),
        context.previous_cursor.as_deref(),
        Some(100),
        PageDirection::Backward,
    )
    .unwrap();
    assert_eq!((previous.start, previous.end), (92, 192));
    assert_eq!(value_id(previous.events.last().unwrap()), "e191");

    let next = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("e399"),
        context.next_cursor.as_deref(),
        Some(100),
        PageDirection::Forward,
    )
    .unwrap();
    assert_eq!((next.start, next.end), (209, 309));
    assert_eq!(value_id(next.events.first().unwrap()), "e209");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn oversized_reader_event_is_projected_before_body_io() {
    let _home = IsolatedAppHome::acquire("reader-response-budget");
    let dir = std::env::temp_dir().join(format!("reader-response-budget-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let events = vec![
        message("large", 0, &"x".repeat(READER_RESPONSE_EVENTS_BYTES + 4096)),
        message("tail", 1, "tail"),
    ];
    let relations = vec![EventRelation {
        kind: RelationKind::Parent,
        from: "tail".into(),
        to: "large".into(),
        confidence: RelationConfidence::Explicit,
    }];
    build_index(&dir, &events).unwrap();
    write_manifest(&dir, &relations, "tail");
    let page = event_page(
        &dir,
        "imp:test",
        "rev-test",
        Some("tail"),
        None,
        Some(100),
        PageDirection::Forward,
    )
    .unwrap();
    assert_eq!(page.events.len(), 2);
    assert_eq!(value_id(&page.events[0]), "large");
    assert_eq!(
        page.events[0]
            .get("truncated")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(value_id(&page.events[1]), "tail");
    assert!(serde_json::to_vec(&page.events).unwrap().len() <= READER_RESPONSE_EVENTS_BYTES);
    assert!(!page.events[0].to_string().contains(&"x".repeat(4096)));
    std::fs::remove_dir_all(dir).unwrap();
}
