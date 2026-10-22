use super::{engine, json, Expected, HashMap, MessageRole};
use crate::session_migration::{
    catalog, continuation, jobs, mcp_stdio,
    types::{ImportedEvent, ToolInput},
};
use serde_json::Value;
use std::collections::HashSet;

fn archive(import: &str, revision: &str, expected: &Expected) {
    let mut cursor = None;
    let mut events = Vec::new();
    loop {
        let (page, next, total) =
            engine::events_page(import, Some(revision), cursor.as_deref(), Some(2), None).unwrap();
        assert_eq!(total, 5);
        assert!(page.len() <= 2);
        events.extend(
            page.into_iter()
                .map(|row| serde_json::from_value::<ImportedEvent>(row).unwrap()),
        );
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        events
            .iter()
            .map(|event| event.id().to_string())
            .collect::<Vec<_>>(),
        expected.event_ids
    );
    for ((index, role), text) in [
        (0, MessageRole::User),
        (1, MessageRole::Assistant),
        (4, MessageRole::Assistant),
    ]
    .into_iter()
    .zip(&expected.source.messages)
    {
        assert!(
            matches!(&events[index], ImportedEvent::Message { role: found, .. } if *found == role)
        );
        assert_eq!(&events[index].searchable_text(), text);
    }
    let ImportedEvent::ToolCall {
        call_id,
        name,
        input,
        ..
    } = &events[2]
    else {
        panic!("missing archived tool call");
    };
    assert_eq!(name, &expected.source.sentinels[0]);
    assert!(
        matches!(input, ToolInput::Json { value } if *value == json!({"payload":expected.source.sentinels[1]}))
    );
    assert!(
        matches!(&events[3], ImportedEvent::ToolResult { call_id: result, .. } if result == call_id)
    );
    assert_eq!(events[3].searchable_text(), expected.source.sentinels[2]);
}

fn payload(result: Value) -> Value {
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    assert_eq!(result["content"][0]["type"], "text");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn tool(grant: &mcp_stdio::ImportHistoryGrant, name: &str, arguments: Value) -> Value {
    payload(
        mcp_stdio::handle_method(
            grant,
            "tools/call",
            &json!({"name":name,"arguments":arguments}),
        )
        .unwrap(),
    )
}

fn search(grant: &mcp_stdio::ImportHistoryGrant, query: &str) -> Vec<Value> {
    let mut cursor = None::<String>;
    let mut cursors = HashSet::new();
    let mut ids = HashSet::new();
    let mut hits = Vec::new();
    loop {
        let result = tool(
            grant,
            "import_history_search",
            json!({
                "query":query,"limit":1,"cursor":cursor
            }),
        );
        assert_eq!(result["integrity"], "allowed-selection");
        let page = result["hits"].as_array().unwrap();
        assert!(page.len() <= 1);
        for hit in page {
            assert!(ids.insert(hit["eventId"].as_str().unwrap().to_string()));
            hits.push(hit.clone());
        }
        let Some(next) = result["nextCursor"].as_str() else {
            break;
        };
        assert!(cursors.insert(next.to_string()));
        assert!(
            cursors.len() <= 5,
            "search must exhaust the five-event archive"
        );
        cursor = Some(next.to_string());
    }
    hits
}

fn assert_fields(text: &str, expected: &Expected) {
    for (index, sentinel) in expected.source.sentinels.iter().enumerate() {
        assert_eq!(
            text.contains(sentinel),
            expected.source.mode.allows(index),
            "field {index} in {:?}",
            expected.source.mode
        );
    }
}

fn continuation_and_mcp(import: &str, revision: &str, expected: &Expected, job_id: &str) -> String {
    let draft = engine::continue_prepare(import, Some(revision), None, None, None).unwrap();
    assert_eq!(draft.selection.canonicalized(), expected.selection);
    let readable = expected.source.mode.readable(&expected.event_ids);
    assert_eq!(draft.readable_event_ids, readable);
    assert_eq!(draft.fragment_event_ids, readable);
    for text in &expected.source.messages {
        assert!(draft.text.contains(text));
    }
    assert_fields(&draft.text, expected);
    // The persisted local session creates a genuine grant; no provider dispatch.
    let request = format!("tools-{job_id}-{import}");
    let sid = engine::continue_create(&draft.draft_id, &draft.draft_hash, &request, None).unwrap();
    engine::reset_runtime();
    assert!(crate::store::load_messages(&sid).is_empty());
    let grant = engine::grant_with_selection(&sid).expect("persisted local continuation grant");
    assert_eq!(grant.import_id, import);
    assert_eq!(grant.revision_id, revision);
    assert_eq!(
        grant.granted_selection().unwrap().canonicalized(),
        expected.selection
    );
    let side = continuation::read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.state, continuation::DispatchState::Prepared);
    assert!(side.dispatch_id.is_none());
    assert!(side.agent_session_id.is_none());
    for (index, sentinel) in expected.source.sentinels.iter().enumerate() {
        let hits = search(&grant, sentinel);
        assert_eq!(hits.len(), usize::from(expected.source.mode.allows(index)));
        if let Some(hit) = hits.first() {
            assert_eq!(
                hit["eventId"],
                expected.event_ids[if index == 2 { 3 } else { 2 }]
            );
            assert_fields(hit["snippet"].as_str().unwrap(), expected);
        }
    }
    for (index, id) in expected.event_ids.iter().enumerate() {
        let result = mcp_stdio::handle_method(
            &grant,
            "tools/call",
            &json!({
                "name":"import_history_read", "arguments":{"citation":id,"maxBytes":4096}
            }),
        );
        if readable.contains(id) {
            let result = payload(result.unwrap());
            assert_eq!(result["citation"], *id);
            assert_eq!(result["revision"], revision);
            assert_eq!(result["truncated"], false);
            assert!(result["nextCursor"].is_null());
            let text = result["text"].as_str().unwrap();
            if index == 2 || index == 3 {
                assert_fields(text, expected);
            } else {
                assert_eq!(
                    text,
                    expected.source.messages[if index == 4 { 2 } else { index }]
                );
            }
        } else {
            let error = result.unwrap_err();
            assert!(error.contains("HISTORY_ACCESS_UNAVAILABLE"));
            assert!(expected
                .source
                .sentinels
                .iter()
                .all(|sentinel| !error.contains(sentinel)));
        }
    }
    let question = search(&grant, &expected.source.messages[0]);
    assert_eq!(question.len(), 1);
    assert_eq!(question[0]["eventId"], expected.event_ids[0]);
    sid
}

pub(super) fn published(job_id: &str, expected: &HashMap<String, Expected>) {
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut page = engine::job_items_page(job_id, None, Some(2)).unwrap();
    let (mut seen, mut entries, mut revisions, mut sessions) = (
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
    );
    let mut providers = HashMap::new();
    loop {
        assert_eq!(page.state, jobs::JobState::Completed);
        assert_eq!(page.item_count, 12);
        assert!(page.items.len() <= 2);
        for item in &page.items {
            assert!(seen.insert(item.candidate_id.clone()));
            let expected = &expected[&item.candidate_id];
            let reference = &expected.reference;
            assert_eq!(item.phase, "committed");
            assert!(item.error.is_none());
            let import = item.import_id.as_deref().unwrap();
            let revision = item.revision_id.as_deref().unwrap();
            assert!(entries.insert(import.to_string()));
            assert!(revisions.insert(revision.to_string()));
            let entry = catalog::get_entry(&conn, import).unwrap().unwrap();
            assert_eq!(entry.current_revision.as_deref(), Some(revision));
            let receipt = jobs::latest_receipt(&conn, job_id, &item.candidate_id)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.phase, "committed");
            assert_eq!(receipt.import_id.as_deref(), Some(import));
            assert_eq!(receipt.revision_id.as_deref(), Some(revision));
            assert_eq!(receipt.source_id, reference.source_id);
            assert_eq!(receipt.source_member, reference.source_member);
            assert_eq!(receipt.provider, reference.candidate.provider.as_str());
            assert_eq!(
                receipt.native_session_id,
                reference.candidate.native_session_id
            );
            assert_eq!(receipt.selection_set.entries.len(), 1);
            assert_eq!(
                receipt.selection_set.entries[0].selection.canonicalized(),
                expected.selection
            );
            let auth = jobs::load_published_auth(import, Some(revision), None).unwrap();
            assert_eq!(auth.selection.canonicalized(), expected.selection);
            archive(import, revision, expected);
            assert!(sessions.insert(continuation_and_mcp(import, revision, expected, job_id)));
            archive(import, revision, expected);
            *providers.entry(receipt.provider).or_insert(0usize) += 1;
        }
        let Some(cursor) = page.next_item_cursor else {
            break;
        };
        page = engine::job_items_page(job_id, Some(&cursor), Some(2)).unwrap();
    }
    assert_eq!(
        (seen.len(), entries.len(), revisions.len(), sessions.len()),
        (12, 12, 12, 12)
    );
    assert_eq!(providers.len(), 3);
    assert!(providers.values().all(|count| *count == 4));
    let counts: (usize, usize, usize) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions),
         (SELECT COUNT(*) FROM receipts WHERE phase='committed')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (12, 12, 12));
}
