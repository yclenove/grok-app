//! Real Claude JSONL branches/assets through prepare, offline publish and grants.
use super::{
    engine, group_selection, job_control, json, prepare, scan, selected_references, wait_prepared,
    AllowedSelection, CapacityHome, Path, Provider,
};
use crate::session_migration::{catalog, jobs, types::AssetState};
use std::collections::HashSet;

#[path = "capacity_branch_asset_source_tests.rs"]
mod source;
#[path = "capacity_branch_asset_verify_tests.rs"]
mod verify;

fn branch(preview: &str, leaf: &str) -> Vec<engine::PreviewEvent> {
    let mut events = Vec::new();
    let mut cursor = None;
    let mut cursors = HashSet::new();
    loop {
        let (page, next) =
            engine::preview_events_page(preview, cursor.as_deref(), Some(2), Some(leaf)).unwrap();
        assert!(page.len() <= 2);
        events.extend(page);
        let Some(next) = next else { break };
        assert!(cursors.insert(next.clone()));
        assert!(cursors.len() < 9);
        cursor = Some(next);
    }
    events
}

#[test]
fn alternate_claude_branch_assets_stay_scoped_through_offline_publish_and_mcp() {
    let home = CapacityHome::new();
    let root = home.root.join("Claude branch images 来源 #");
    let source = source::Source::create(&root);
    let scanned = scan::scan_start(vec![(
        Provider::ClaudeCode,
        root.to_string_lossy().into_owned(),
    )])
    .unwrap();
    assert!(scanned.complete && scanned.issues.is_empty(), "{scanned:?}");
    assert_eq!(scanned.discovered_count, 1);
    let reference = selected_references(&scanned).pop().unwrap();
    let (preview_id, details) = engine::preview_reference(
        reference.clone(),
        Some(&reference.candidate.source_revision),
        None,
    )
    .unwrap();
    assert_eq!(details.event_count, 9);
    assert_eq!((details.assets.stored, details.assets.missing), (4, 0));
    assert_eq!(details.inventory.imported.attachments, 4);
    assert_eq!(
        details.outcome,
        crate::session_migration::types::ImportOutcome::Complete
    );
    assert!(details.issues.is_empty(), "{:?}", details.issues);
    let chosen = "claude:chosen-answer";
    let sibling = "claude:sibling-answer";
    assert_eq!(details.active_leaf.as_deref(), Some(sibling));
    assert_eq!(
        details.leaves.iter().cloned().collect::<HashSet<_>>(),
        HashSet::from([chosen.to_string(), sibling.to_string()])
    );
    assert!(details.default_selection.asset_ids.is_empty());
    let chosen_events = branch(&preview_id, chosen);
    let sibling_events = branch(&preview_id, sibling);
    assert_eq!(chosen_events.len(), 6);
    assert_eq!(sibling_events.len(), 4);
    assert!(chosen_events.iter().all(|row| row.default_selected));
    assert_eq!(chosen_events[0].text, source::ROOT_TEXT);
    assert_eq!(chosen_events[1].text, source::CHOSEN_TEXT);
    assert_eq!(chosen_events[5].text, source::ANSWER);
    let event_ids: Vec<_> = chosen_events
        .iter()
        .map(|row| row.event_id.clone())
        .collect();
    let asset_ids: Vec<_> = chosen_events
        .iter()
        .flat_map(|row| row.asset_ids.clone())
        .collect();
    assert_eq!(asset_ids, source::ASSET_IDS[..3]);
    let mut selection = AllowedSelection::for_events(event_ids.clone());
    selection.asset_ids = asset_ids[..2].to_vec();
    selection.pinned_event_ids = vec![event_ids[0].clone()];
    let selected = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let batch = prepare::start(
        &scanned.scan_id,
        scanned.epoch,
        &selected.selection_id,
        vec![prepare::PrepareOverride {
            candidate_id: reference.candidate.candidate_id.clone(),
            preview_id: preview_id.clone(),
            leaf_id: chosen.into(),
            selection: selection.clone(),
        }],
    )
    .unwrap();
    let status = wait_prepared(&batch.batch_id);
    assert_eq!(status.state, prepare::PrepareBatchState::Completed);
    assert_eq!((status.total, status.prepared, status.failed), (1, 1, 0));
    let plan = engine::plan_from_validated_prepare(
        prepare::validated_plan_inputs(&batch.batch_id).unwrap(),
        prepare::mappings(&batch.batch_id, None).unwrap(),
    )
    .unwrap();
    let offline = root.with_file_name("offline Claude branch images 来源 #");
    std::fs::rename(&root, &offline).unwrap();
    source.unchanged(&offline);
    engine::reset_runtime();
    prepare::reset_runtime();
    assert_eq!(
        engine::recovery().unwrap().plan.unwrap().plan_id,
        plan.plan_id
    );
    let request = "alternate-branch-assets-offline";
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, request).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let result = engine::job_items_page(&queued.job_id, None, Some(2)).unwrap();
    assert_eq!(result.state, jobs::JobState::Completed, "{result:?}");
    assert_eq!(result.item_count, 1);
    assert_eq!(result.items.len(), 1);
    assert!(result.next_item_cursor.is_none());
    let item = &result.items[0];
    assert_eq!(item.phase, "committed");
    assert_eq!(item.outcome.as_deref(), Some("complete"));
    let (import, revision) = (
        item.import_id.as_deref().unwrap(),
        item.revision_id.as_deref().unwrap(),
    );
    let conn = catalog::open_catalog_readonly().unwrap();
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.source_id, reference.source_id);
    assert_eq!(receipt.source_member, reference.source_member);
    assert_eq!(
        receipt.native_session_id,
        reference.candidate.native_session_id
    );
    assert_eq!(receipt.provider, "claude-code");
    assert_eq!(receipt.selection_set.entries.len(), 1);
    let committed = &receipt.selection_set.entries[0];
    assert_eq!(committed.preview_id, preview_id);
    assert_eq!(committed.leaf_id, chosen);
    assert_eq!(
        committed.selection.canonicalized(),
        selection.canonicalized()
    );
    let dir = catalog::revision_dir(import, revision).unwrap();
    let assets: Vec<crate::session_migration::types::AssetRecord> =
        serde_json::from_slice(&std::fs::read(dir.join("assets.json")).unwrap()).unwrap();
    assert_eq!(assets.len(), 4);
    for (index, asset_id) in source::ASSET_IDS.iter().enumerate() {
        let asset = assets
            .iter()
            .find(|asset| asset.asset_id == *asset_id)
            .unwrap();
        assert_eq!(asset.state, AssetState::Stored);
        let bytes = if index == 1 { &source.gif } else { &source.png };
        let digest = crate::session_migration::hash::sha256_bytes(bytes);
        assert_eq!(asset.digest.as_deref(), Some(digest.as_str()));
        assert_eq!(asset.size as usize, bytes.len());
        assert_eq!(
            std::fs::read(dir.join("blobs").join(digest)).unwrap(),
            *bytes
        );
    }
    let branch_ids: Vec<_> = sibling_events
        .iter()
        .map(|row| row.event_id.clone())
        .collect();
    verify::archive(import, revision, chosen, sibling, &event_ids, &branch_ids);
    verify::continuation_and_mcp(
        import,
        revision,
        (chosen, sibling),
        (&event_ids, &branch_ids),
        &selection,
        &source,
    );
    verify::archive(import, revision, chosen, sibling, &event_ids, &branch_ids);
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    source.unchanged(&offline);
}

#[test]
fn claude_asset_outcomes_preserve_source_and_materialization_failures() {
    use crate::session_migration::{
        adapters,
        types::{AssetState, ImportOutcome},
    };
    for case in ["complete", "truncated", "missing", "remote", "no-root"] {
        let home = CapacityHome::new();
        let root = home.root.join(case);
        std::fs::create_dir_all(&root).unwrap();
        let image = match case {
            "missing" => json!({"path":"missing.png","media_type":"image/png"}),
            "remote" => json!({"url":"https://example.invalid/image.png","media_type":"image/png"}),
            _ => {
                json!({"data":"R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7","media_type":"image/gif"})
            }
        };
        let record = json!({"type":"user","uuid":"image","sessionId":case,"message":{"content":[
            {"type":"text","text":"Attachment outcome regression"}, {"type":"image","source":image}
        ]}});
        let mut bytes = format!("{record}\n").into_bytes();
        if case == "truncated" {
            bytes.extend_from_slice(b"{\"type\":");
        }
        let path = root.join("source.jsonl");
        std::fs::write(&path, &bytes).unwrap();
        let mut adapter =
            adapters::normalize_in_location(Provider::ClaudeCode, &path, Some(&root), None)
                .unwrap();
        assert_eq!(adapter.assets[0].state, AssetState::Unresolved);
        assert_eq!(
            adapter.outcome,
            if case == "truncated" {
                ImportOutcome::Partial
            } else {
                ImportOutcome::Complete
            },
            "{case}"
        );
        let prepared = jobs::prepare_revision(
            &mut adapter,
            &json!({"rawRetention":"normalized-only"}),
            (case != "no-root").then_some(root.as_path()),
            None,
        )
        .unwrap();
        assert_eq!(
            adapter.outcome,
            if case == "complete" {
                ImportOutcome::Complete
            } else {
                ImportOutcome::Partial
            },
            "{case}"
        );
        match case {
            "missing" => {
                assert_eq!(adapter.assets[0].state, AssetState::Missing);
                assert!(adapter
                    .issues
                    .iter()
                    .any(|issue| issue.code == "asset-missing"));
            }
            "remote" => {
                assert_eq!(adapter.assets[0].state, AssetState::Unresolved);
                assert!(adapter
                    .issues
                    .iter()
                    .any(|issue| issue.code == "asset-unresolved"));
            }
            "no-root" => {
                assert_eq!(adapter.assets[0].state, AssetState::Unresolved);
                assert_eq!(adapter.inventory.missing.attachments, 1);
                assert!(adapter.issues.is_empty(), "{case}: {:?}", adapter.issues);
            }
            _ => {
                assert_eq!(adapter.assets[0].state, AssetState::Stored);
                assert_eq!(adapter.inventory.missing.attachments, 0);
                let codes: Vec<_> = adapter
                    .issues
                    .iter()
                    .map(|issue| issue.code.as_str())
                    .collect();
                let expected = if case == "truncated" {
                    vec!["trailing-incomplete"]
                } else {
                    vec![]
                };
                assert_eq!(codes, expected, "{case}: {:?}", adapter.issues);
            }
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(prepared.path.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["outcome"],
            if case == "complete" {
                "complete"
            } else {
                "partial"
            }
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
