use super::{engine, json, AllowedSelection, AssetRecord, HIDDEN, LIVE};
use crate::session_migration::{
    continuation, mcp_stdio,
    types::{ContentBlock, ImportedEvent},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

pub(super) fn archive(import: &str, revision: &str, images: &[String]) {
    for (leaf, expected) in [
        ("codex:old-answer", vec!["codex:old", "codex:old-answer"]),
        (
            "codex:answer",
            vec!["codex:compact:3", "codex:live", "codex:answer"],
        ),
    ] {
        let mut ids = Vec::new();
        let mut cursor = None;
        loop {
            let (page, next, total) = engine::events_page(
                import,
                Some(revision),
                cursor.as_deref(),
                Some(1),
                Some(leaf),
            )
            .unwrap();
            assert_eq!(total as usize, expected.len());
            assert_eq!(page.len(), 1);
            let event: ImportedEvent = serde_json::from_value(page[0].clone()).unwrap();
            match event.id() {
                "codex:old" => {
                    assert_eq!(event.searchable_text(), HIDDEN);
                }
                "codex:old-answer" => {
                    assert_eq!(event.searchable_text(), "CODEX_COVERED_ANSWER_EXCLUDED")
                }
                "codex:compact:3" => assert_eq!(event.searchable_text(), "CODEX_SUMMARY"),
                "codex:answer" => assert_eq!(event.searchable_text(), "CODEX_LIVE_ANSWER"),
                "codex:live" => {
                    let ImportedEvent::Message { blocks, .. } = &event else {
                        panic!("expected message");
                    };
                    assert_eq!(blocks.len(), 4);
                    assert_eq!(blocks[0].plain_text(), Some(LIVE));
                    assert_eq!(blocks[2].plain_text(), Some("CODEX_AFTER_IMAGE"));
                    for (block, image) in [(1, &images[0]), (3, &images[1])] {
                        assert!(
                            matches!(&blocks[block],ContentBlock::Asset {asset_id,..} if asset_id == image)
                        );
                    }
                }
                other => panic!("unexpected archived event {other}"),
            }
            ids.push(event.id().to_string());
            cursor = next;
            if cursor.is_none() {
                break;
            }
            assert!(ids.len() < 4);
        }
        assert_eq!(ids, expected);
    }
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
    Ok(serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap())
}

pub(super) fn narrow_and_mcp(
    import: &str,
    revision: &str,
    selection: &AllowedSelection,
    assets: &[AssetRecord],
    png: &[u8],
) {
    assert!(
        engine::continue_prepare(import, Some(revision), Some("codex:old-answer"), None, None)
            .is_err()
    );
    let draft =
        engine::continue_prepare(import, Some(revision), Some("codex:answer"), None, None).unwrap();
    assert_eq!(draft.selection.canonicalized(), selection.canonicalized());
    assert!(draft.text.contains(LIVE));
    assert!(!draft.text.contains(HIDDEN));
    let mut narrow = draft.selection.clone();
    narrow.fields.message_text = false;
    narrow.fields.compaction_summary = false;
    let revised =
        engine::continue_selection(&draft.draft_id, &draft.draft_hash, narrow.clone()).unwrap();
    for hidden in [
        LIVE,
        HIDDEN,
        "CODEX_AFTER_IMAGE",
        "CODEX_LIVE_ANSWER",
        "CODEX_SUMMARY",
    ] {
        assert!(!revised.text.contains(hidden), "leaked {hidden}");
    }
    let sid = engine::continue_create(
        &revised.draft_id,
        &revised.draft_hash,
        "codex-media-narrow",
        None,
    )
    .unwrap();
    engine::reset_runtime();
    assert!(crate::store::load_messages(&sid).is_empty());
    let sidecar = continuation::read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(sidecar.state, continuation::DispatchState::Prepared);
    assert!(sidecar.dispatch_id.is_none() && sidecar.agent_session_id.is_none());
    let grant = engine::grant_with_selection(&sid).expect("persisted selection grant");
    assert_eq!(grant.branch_leaf, "codex:answer");
    assert_eq!(
        grant.granted_selection().unwrap().canonicalized(),
        narrow.canonicalized()
    );
    for asset in assets {
        let result = tool(
            &grant,
            "import_history_asset_read",
            json!({"assetId":asset.asset_id}),
        );
        if asset.asset_id == selection.asset_ids[0] {
            let value = result.unwrap();
            assert_eq!(value["assetId"], asset.asset_id);
            assert_eq!(value["size"].as_u64().unwrap() as usize, png.len());
            assert_eq!(value["truncated"], false);
            assert_eq!(
                STANDARD.decode(value["bytes"].as_str().unwrap()).unwrap(),
                png
            );
        } else {
            assert!(result.unwrap_err().contains("HISTORY_ACCESS_UNAVAILABLE"));
        }
    }
    // With message/summary fields off, assets remain readable through their
    // explicit byte API; an image with no source alt has no searchable prose.
    assert!(revised.readable_event_ids.is_empty());
    for id in [
        "codex:live",
        "codex:old",
        "codex:old-answer",
        "codex:answer",
        "codex:compact:3",
    ] {
        assert!(tool(&grant, "import_history_read", json!({"citation":id}))
            .unwrap_err()
            .contains("HISTORY_ACCESS_UNAVAILABLE"));
    }
    for query in [
        LIVE,
        HIDDEN,
        "CODEX_AFTER_IMAGE",
        "CODEX_LIVE_ANSWER",
        "CODEX_SUMMARY",
    ] {
        let result = tool(
            &grant,
            "import_history_search",
            json!({"query":query,"limit":1}),
        )
        .unwrap();
        assert_eq!(result["integrity"], "allowed-selection");
        assert!(result["hits"].as_array().unwrap().is_empty());
        assert!(result["nextCursor"].is_null());
    }
}
