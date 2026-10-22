use super::{engine, json, source, AllowedSelection, HashSet};
use crate::session_migration::{continuation, mcp_stdio};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

pub(super) fn archive(
    import: &str,
    revision: &str,
    chosen: &str,
    sibling: &str,
    chosen_ids: &[String],
    sibling_ids: &[String],
) {
    let mut all = HashSet::new();
    for (leaf, expected) in [(chosen, chosen_ids), (sibling, sibling_ids)] {
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let (page, next, total) = engine::events_page(
                import,
                Some(revision),
                cursor.as_deref(),
                Some(2),
                Some(leaf),
            )
            .unwrap();
            assert_eq!(total as usize, expected.len());
            assert!(page.len() <= 2);
            for row in page {
                let event: crate::session_migration::types::ImportedEvent =
                    serde_json::from_value(row).unwrap();
                use crate::session_migration::types::{ContentBlock, ImportedEvent, MessageRole};
                let ImportedEvent::Message { role, blocks, .. } = &event else {
                    panic!("source messages must remain typed messages");
                };
                let (expected_role, text, asset_id) = match event.id() {
                    "claude:root" => (MessageRole::User, source::ROOT_TEXT, None),
                    "claude:chosen" => (MessageRole::User, source::CHOSEN_TEXT, None),
                    "claude:chosen-answer" => (MessageRole::Assistant, source::ANSWER, None),
                    "claude:sibling" => (MessageRole::User, source::SIBLING_TEXT, None),
                    "claude:sibling-answer" => {
                        (MessageRole::Assistant, source::SIBLING_ANSWER, None)
                    }
                    "claude:chosen:block:1" => (MessageRole::User, "", Some(source::ASSET_IDS[0])),
                    "claude:chosen:block:2" => (MessageRole::User, "", Some(source::ASSET_IDS[1])),
                    "claude:chosen:block:3" => (MessageRole::User, "", Some(source::ASSET_IDS[2])),
                    "claude:sibling:block:1" => (MessageRole::User, "", Some(source::ASSET_IDS[3])),
                    other => panic!("unexpected archived event {other}"),
                };
                assert_eq!(*role, expected_role);
                if let Some(expected_asset) = asset_id {
                    assert!(
                        matches!(blocks.as_slice(), [ContentBlock::Asset { asset_id, .. }] if asset_id == expected_asset)
                    );
                } else {
                    assert_eq!(event.searchable_text(), text);
                }
                all.insert(event.id().to_string());
                ids.push(event.id().to_string());
            }
            cursor = next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids, expected);
    }
    assert_eq!(all.len(), 9);
    // The immutable source graph defaults to the sibling; publication selected
    // the alternate branch. Every unscoped human read must start there too.
    for leaf in [None, Some(""), Some("  ")] {
        let page =
            engine::events_window(import, Some(revision), None, Some(100), leaf, None).unwrap();
        assert_eq!(page.leaf, chosen);
        let ids: Vec<_> = page
            .events
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, chosen_ids);
        let context = engine::event_context(
            import,
            Some(revision),
            leaf,
            &crate::session_migration::reader::EventContextRequest {
                event_id: chosen.into(),
                before: Some(1),
                after: Some(1),
                cursor: None,
            },
        )
        .unwrap();
        assert_eq!(context.leaf, chosen);
        assert!(context.events.iter().any(|row| row["id"] == chosen));
        for (query, expected) in [(source::ANSWER, 1), (source::SIBLING_ANSWER, 0)] {
            let (hits, _) = engine::search_imported_with_leaf(
                import,
                Some(revision),
                leaf,
                query,
                None,
                Some(10),
            )
            .unwrap();
            assert_eq!(hits.len(), expected);
            let window = engine::search_imported_with_leaf_window(
                import,
                Some(revision),
                leaf,
                query,
                None,
                Some(10),
                None,
            )
            .unwrap();
            assert_eq!(window.hits.len(), expected);
        }
    }
    // Defaulting changes no archived graph or explicit human branch access.
    let dir = crate::session_migration::catalog::revision_dir(import, revision).unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["graph"]["activeLeaf"], sibling);
}

fn tool(
    grant: &mcp_stdio::ImportHistoryGrant,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let result = mcp_stdio::handle_method(
        grant,
        "tools/call",
        &json!({"name":name,"arguments":arguments}),
    )?;
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    assert_eq!(result["content"][0]["type"], "text");
    Ok(serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap())
}

pub(super) fn continuation_and_mcp(
    import: &str,
    revision: &str,
    leaves: (&str, &str),
    events: (&[String], &[String]),
    selection: &AllowedSelection,
    source: &source::Source,
) {
    let (chosen, sibling) = leaves;
    let (event_ids, sibling_ids) = events;
    assert!(engine::continue_prepare(import, Some(revision), Some(sibling), None, None).is_err());
    let draft = engine::continue_prepare(import, Some(revision), None, None, None).unwrap();
    assert_eq!(draft.leaf, chosen);
    let markdown = engine::export_markdown(import, Some(revision), None).unwrap();
    assert!(markdown.contains(source::ANSWER));
    assert!(!markdown.contains(source::SIBLING_ANSWER));
    assert_eq!(draft.selection.canonicalized(), selection.canonicalized());
    let readable: Vec<_> = event_ids
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 4)
        .map(|(_, id)| id.clone())
        .collect();
    assert_eq!(draft.readable_event_ids, readable);
    assert_eq!(draft.fragment_event_ids, readable);
    for text in [source::ROOT_TEXT, source::CHOSEN_TEXT, source::ANSWER] {
        assert!(draft.text.contains(text), "missing {text}");
    }
    // Asset projection uses the source alt text; IDs remain in the explicit
    // selection/grant and byte-read contract below, not in the handoff prose.
    assert_eq!(draft.text.matches("\"text\":\"image\"").count(), 2);
    for text in [
        source::SIBLING_TEXT,
        source::SIBLING_ANSWER,
        source::ASSET_IDS[2],
        source::ASSET_IDS[3],
    ] {
        assert!(!draft.text.contains(text), "leaked {text}");
    }
    let sid = engine::continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        "branch-assets-continuation",
        None,
    )
    .unwrap();
    engine::reset_runtime();
    assert!(crate::store::load_messages(&sid).is_empty());
    let sidecar = continuation::read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(sidecar.state, continuation::DispatchState::Prepared);
    assert!(sidecar.dispatch_id.is_none() && sidecar.agent_session_id.is_none());
    let grant = engine::grant_with_selection(&sid).expect("persisted continuation grant");
    assert_eq!(grant.branch_leaf, chosen);
    assert_eq!(
        grant.granted_selection().unwrap().canonicalized(),
        selection.canonicalized()
    );
    for (index, asset_id) in source::ASSET_IDS.iter().enumerate() {
        let result = tool(
            &grant,
            "import_history_asset_read",
            json!({"assetId":asset_id}),
        );
        if index < 2 {
            let value = result.unwrap();
            let expected = if index == 0 { &source.png } else { &source.gif };
            assert_eq!(value["assetId"], *asset_id);
            assert_eq!(value["size"].as_u64().unwrap() as usize, expected.len());
            assert_eq!(value["truncated"], false);
            assert_eq!(
                STANDARD.decode(value["bytes"].as_str().unwrap()).unwrap(),
                *expected
            );
        } else {
            assert!(result.unwrap_err().contains("HISTORY_ACCESS_UNAVAILABLE"));
        }
    }
    for id in event_ids.iter().chain(sibling_ids) {
        let result = tool(
            &grant,
            "import_history_read",
            json!({"citation":id,"maxBytes":4096}),
        );
        if readable.contains(id) {
            let value = result.unwrap();
            assert_eq!(value["citation"], *id);
            assert_eq!(value["revision"], revision);
            assert_eq!(value["truncated"], false);
            let text = value["text"].as_str().unwrap();
            if id == &event_ids[2] || id == &event_ids[3] {
                assert_eq!(text, "image");
            }
            assert!(!text.contains(source::ASSET_IDS[2]) && !text.contains(source::ASSET_IDS[3]));
        } else {
            assert!(result.unwrap_err().contains("HISTORY_ACCESS_UNAVAILABLE"));
        }
    }
    for (query, expected_id) in [
        (source::CHOSEN_TEXT, Some(&event_ids[1])),
        (source::ANSWER, Some(&event_ids[5])),
        (source::SIBLING_TEXT, None),
        (source::SIBLING_ANSWER, None),
    ] {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut hits = Vec::new();
        loop {
            let value = tool(
                &grant,
                "import_history_search",
                json!({"query":query,"limit":1,"cursor":cursor}),
            )
            .unwrap();
            assert_eq!(value["integrity"], "allowed-selection");
            let page = value["hits"].as_array().unwrap();
            assert!(page.len() <= 1);
            hits.extend(
                page.iter()
                    .map(|hit| hit["eventId"].as_str().unwrap().to_string()),
            );
            let Some(next) = value["nextCursor"].as_str() else {
                break;
            };
            assert!(seen_cursors.insert(next.to_string()));
            assert!(seen_cursors.len() <= 9);
            cursor = Some(next.to_string());
        }
        assert_eq!(hits, expected_id.into_iter().cloned().collect::<Vec<_>>());
    }
}
