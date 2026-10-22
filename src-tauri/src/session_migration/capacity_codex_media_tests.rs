//! Public-wire-shaped synthetic Codex images through the durable import workflow.
use super::{
    engine, group_selection, job_control, json, prepare, scan, selected_references, wait_prepared,
    AllowedSelection, CapacityHome, Path, Provider,
};
use crate::session_migration::{
    catalog, jobs,
    types::{AssetRecord, AssetState, ImportOutcome},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};

#[path = "capacity_codex_media_verify_tests.rs"]
mod verify;

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aCM0AAAAASUVORK5CYII=";
const LIVE: &str = "CODEX_LIVE_TEXT_来源";
const HIDDEN: &str = "CODEX_COVERED_TEXT_EXCLUDED";

fn source(root: &Path) -> Vec<u8> {
    std::fs::create_dir_all(root).unwrap();
    let rows = [
        json!({"type":"session_meta","payload":{"id":"codex-media-offline","cli_version":"0.40.0"}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","id":"old",
            "content":[{"type":"input_text","text":HIDDEN},{"type":"input_image","image_url":PNG}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"old-answer",
            "content":[{"type":"output_text","text":"CODEX_COVERED_ANSWER_EXCLUDED"}]}}),
        json!({"type":"compacted","payload":{"message":"CODEX_SUMMARY","replacement_history":[
            {"type":"message","id":"live","role":"user","content":[
                {"type":"input_text","text":LIVE},{"type":"input_image","image_url":PNG},
                {"type":"input_text","text":"CODEX_AFTER_IMAGE"},{"type":"input_image","image_url":PNG}]},
            {"type":"message","id":"answer","role":"assistant","content":[{"type":"output_text","text":"CODEX_LIVE_ANSWER"}]}
        ]}}),
    ];
    let bytes = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>()
        .into_bytes();
    std::fs::write(root.join("media.jsonl"), &bytes).unwrap();
    bytes
}

#[test]
fn codex_media_offline_publication_reader_and_narrowed_mcp_keep_occurrence_scope() {
    let home = CapacityHome::new();
    let root = home.root.join("Codex images 来源 #");
    let source_bytes = source(&root);
    let scanned =
        scan::scan_start(vec![(Provider::Codex, root.to_string_lossy().into_owned())]).unwrap();
    assert!(scanned.complete && scanned.issues.is_empty(), "{scanned:?}");
    assert_eq!(scanned.discovered_count, 1);
    let reference = selected_references(&scanned).pop().unwrap();
    let (preview, details) = engine::preview_reference(
        reference.clone(),
        Some(&reference.candidate.source_revision),
        None,
    )
    .unwrap();
    assert_eq!(details.event_count, 5);
    assert_eq!(details.outcome, ImportOutcome::Complete);
    assert!(details.issues.is_empty(), "{:?}", details.issues);
    assert_eq!((details.assets.stored, details.assets.missing), (3, 0));
    let (events, next) =
        engine::preview_events_page(&preview, None, Some(10), Some("codex:answer")).unwrap();
    assert!(next.is_none());
    let ids: Vec<_> = events.iter().map(|event| event.event_id.clone()).collect();
    assert_eq!(ids, ["codex:compact:3", "codex:live", "codex:answer"]);
    let images = events[1].asset_ids.clone();
    assert_eq!(images.len(), 2);
    assert_ne!(images[0], images[1]);
    let mut selection = AllowedSelection::for_events(ids);
    selection.asset_ids = vec![images[0].clone()];
    let selected = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let batch = prepare::start(
        &scanned.scan_id,
        scanned.epoch,
        &selected.selection_id,
        vec![prepare::PrepareOverride {
            candidate_id: reference.candidate.candidate_id.clone(),
            preview_id: preview.clone(),
            leaf_id: "codex:answer".into(),
            selection: selection.clone(),
        }],
    )
    .unwrap();
    let prepared = wait_prepared(&batch.batch_id);
    assert_eq!(prepared.state, prepare::PrepareBatchState::Completed);
    assert_eq!(
        (prepared.total, prepared.prepared, prepared.failed),
        (1, 1, 0)
    );
    let plan = engine::plan_from_validated_prepare(
        prepare::validated_plan_inputs(&batch.batch_id).unwrap(),
        prepare::mappings(&batch.batch_id, None).unwrap(),
    )
    .unwrap();
    let offline = root.with_file_name("offline Codex images 来源 #");
    std::fs::rename(&root, &offline).unwrap();
    assert_eq!(
        std::fs::read(offline.join("media.jsonl")).unwrap(),
        source_bytes
    );
    engine::reset_runtime();
    prepare::reset_runtime();
    assert_eq!(
        engine::recovery().unwrap().plan.unwrap().plan_id,
        plan.plan_id
    );
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, "codex-media-offline").unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let result = engine::job_items_page(&queued.job_id, None, Some(2)).unwrap();
    assert_eq!(result.state, jobs::JobState::Completed, "{result:?}");
    assert_eq!((result.item_count, result.items.len()), (1, 1));
    let item = &result.items[0];
    assert_eq!(item.phase, "committed");
    assert_eq!(item.outcome.as_deref(), Some("complete"));
    let import = item.import_id.as_deref().unwrap();
    let revision = item.revision_id.as_deref().unwrap();
    let conn = catalog::open_catalog_readonly().unwrap();
    let receipt = jobs::latest_receipt(&conn, &queued.job_id, &item.candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.provider, "codex");
    assert_eq!(receipt.source_id, reference.source_id);
    assert_eq!(receipt.source_member, reference.source_member);
    assert_eq!(receipt.native_session_id, "codex-media-offline");
    assert_eq!(receipt.selection_set.entries.len(), 1);
    assert_eq!(receipt.selection_set.entries[0].preview_id, preview);
    assert_eq!(receipt.selection_set.entries[0].leaf_id, "codex:answer");
    assert_eq!(
        receipt.selection_set.entries[0].selection.canonicalized(),
        selection.canonicalized()
    );
    let dir = catalog::revision_dir(import, revision).unwrap();
    let assets: Vec<AssetRecord> =
        serde_json::from_slice(&std::fs::read(dir.join("assets.json")).unwrap()).unwrap();
    let png = STANDARD.decode(PNG.split_once(',').unwrap().1).unwrap();
    let digest = crate::session_migration::hash::sha256_bytes(&png);
    assert_eq!(assets.len(), 3);
    let mut distinct = std::collections::HashSet::new();
    for asset in &assets {
        assert!(distinct.insert(&asset.asset_id));
        assert_eq!(asset.state, AssetState::Stored);
        assert_eq!(asset.digest.as_deref(), Some(digest.as_str()));
        assert_eq!(asset.size as usize, png.len());
        assert_eq!(asset.mime.as_deref(), Some("image/png"));
        assert_eq!(std::fs::read(dir.join("blobs").join(&digest)).unwrap(), png);
    }
    verify::archive(import, revision, &images);
    verify::narrow_and_mcp(import, revision, &selection, &assets, &png);
    verify::archive(import, revision, &images);
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, "codex-media-offline")
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(
        std::fs::read(offline.join("media.jsonl")).unwrap(),
        source_bytes
    );
}

#[test]
fn codex_media_local_and_inline_materialization_report_final_outcomes_without_source_writes() {
    use crate::session_migration::adapters;
    for case in [
        "inline",
        "local",
        "missing",
        "remote",
        "malformed",
        "malformed-meta",
        "opaque",
        "no-root",
        "truncated",
    ] {
        let home = CapacityHome::new();
        let root = home.root.join(case);
        std::fs::create_dir_all(&root).unwrap();
        let png = STANDARD.decode(PNG.split_once(',').unwrap().1).unwrap();
        std::fs::write(root.join("same.png"), &png).unwrap();
        let row = if matches!(case, "local" | "missing") {
            json!({"type":"event_msg","payload":{"type":"user_message","message":"", "local_images":[if case == "local" {"same.png"} else {"missing.png"}]}})
        } else {
            let image = match case {
                "remote" => {
                    json!({"type":"input_image","image_url":"https://example.invalid/image.png"})
                }
                "malformed" => {
                    json!({"type":"input_image","image_url":"data:image/png;base64,%%%%"})
                }
                "malformed-meta" => {
                    json!({"type":"input_image","image_url":"data:image/png,NOT_IMAGE_BYTES"})
                }
                "opaque" => json!({"type":"input_image","file_id":"same.png"}),
                _ => json!({"type":"input_image","image_url":PNG}),
            };
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[image]}})
        };
        let mut bytes = format!("{row}\n").into_bytes();
        if case == "truncated" {
            bytes.extend_from_slice(b"{\"type\":");
        }
        let path = root.join("source.jsonl");
        std::fs::write(&path, &bytes).unwrap();
        let mut adapter =
            adapters::normalize_in_location(Provider::Codex, &path, Some(&root), None).unwrap();
        assert_eq!(adapter.assets.len(), 1);
        assert_eq!(adapter.assets[0].state, AssetState::Unresolved);
        if matches!(case, "inline" | "local" | "no-root") {
            assert_eq!(adapter.outcome, ImportOutcome::Complete, "{case}");
        }
        let prepared = jobs::prepare_revision(
            &mut adapter,
            &json!({"rawRetention":"normalized-only"}),
            (case != "no-root").then_some(root.as_path()),
            None,
        )
        .unwrap();
        let complete = matches!(case, "inline" | "local");
        assert_eq!(
            adapter.outcome,
            if complete {
                ImportOutcome::Complete
            } else {
                ImportOutcome::Partial
            },
            "{case}"
        );
        assert_eq!(
            adapter.assets[0].state,
            match case {
                "inline" | "local" | "truncated" => AssetState::Stored,
                "missing" => AssetState::Missing,
                _ => AssetState::Unresolved,
            },
            "{case}"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(prepared.path.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["outcome"],
            if complete { "complete" } else { "partial" }
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(root.join("same.png")).unwrap(), png);
    }
}
