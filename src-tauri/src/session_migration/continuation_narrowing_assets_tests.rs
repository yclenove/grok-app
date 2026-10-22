use super::*;
use crate::session_migration::{
    selection::AllowedSelection,
    types::{ContentBlock, ImportedEvent},
};
fn narrow_asset_message() -> ImportedEvent {
    use crate::session_migration::types::{EventBase, MessageRole, TimeOrigin};
    ImportedEvent::Message {
        base: EventBase {
            id: "keep".into(),
            ordinal: 0,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role: MessageRole::User,
        blocks: vec![ContentBlock::text("NARROW_MESSAGE_SENTINEL")],
        complete: true,
    }
}
#[test]
fn narrowing_removes_real_stored_asset_bytes_and_message_fields_without_mutating_archive() {
    use crate::session_migration::jobs::{
        prepare_revision, publish_item, write_preview_lease, CommitItem, PreviewLease,
    };
    use crate::session_migration::selection::{PreviewSelection, SelectionSet};
    use crate::session_migration::types::{
        AdapterOutput, AssetRecord, AssetState, ImportOutcome, Inventory, Provider as Prov,
        SourceEvidence, SourceSpan,
    };
    let home =
        crate::session_migration::test_home::IsolatedAppHome::acquire("continue-narrow-asset");
    let source = home.path.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("ok.bin"), b"allowed-blob-bytes").unwrap();
    std::fs::write(source.join("no.bin"), b"denied-blob-bytes").unwrap();
    let span = SourceSpan {
        artifact_id: "a".into(),
        record: "0".into(),
        block: None,
    };
    let mut keep = narrow_asset_message();
    if let ImportedEvent::Message { blocks, .. } = &mut keep {
        blocks.push(ContentBlock::Asset {
            asset_id: "asset-ok".into(),
            alt: Some("ok".into()),
        });
        blocks.push(ContentBlock::Asset {
            asset_id: "asset-no".into(),
            alt: Some("no".into()),
        });
    }
    let mut adapter = AdapterOutput {
        evidence: SourceEvidence {
            provider: Prov::GenericFile,
            source_version: None,
            schema_fingerprint: "t".into(),
            adapter_version: "1.0.0".into(),
            platform: "test".into(),
            artifacts: vec![],
        },
        graph: crate::session_migration::types::NormalizedGraph {
            events: vec![keep],
            relations: vec![],
            active_leaf: Some("keep".into()),
            native_session_id: "n-continue-narrow-asset".into(),
            title: Some("n-continue-narrow-asset".into()),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues: vec![],
        assets: vec![
            AssetRecord {
                asset_id: "asset-ok".into(),
                digest: None,
                mime: Some("image/png".into()),
                size: 0,
                state: AssetState::Unresolved,
                source_span: span.clone(),
                source_ref: Some("path:ok.bin".into()),
            },
            AssetRecord {
                asset_id: "asset-no".into(),
                digest: None,
                mime: Some("image/png".into()),
                size: 0,
                state: AssetState::Unresolved,
                source_span: span,
                source_ref: Some("path:no.bin".into()),
            },
        ],
        outcome: ImportOutcome::Complete,
    };
    let options = serde_json::json!({"rawRetention":"normalized-only"});
    let preview_id = uuid::Uuid::new_v4().to_string();
    let prepared =
        prepare_revision(&mut adapter, &options, Some(&source), Some(&preview_id)).unwrap();
    write_preview_lease(
        &prepared.path,
        &PreviewLease {
            source_member: None,
            preview_id: preview_id.clone(),
            candidate_id: "c-continue-narrow-asset".into(),
            source_revision: "test-source-revision".into(),
            prepare_batch_id: None,
            source_id: "asset-source".into(),
            root_path: source.to_string_lossy().into_owned(),
            legacy_id: "asset-legacy".into(),
            snapshot_digest: prepared.digest.clone(),
            expires_unix: chrono::Utc::now().timestamp() + 3600,
            retained_by_job: false,
            file_hashes: prepared.file_hashes.clone(),
        },
    )
    .unwrap();
    let mut selection = AllowedSelection::for_events(vec!["keep".into()]);
    selection.asset_ids = vec!["asset-ok".into(), "asset-no".into()];
    let item = CommitItem {
        candidate_id: "c-continue-narrow-asset".into(),
        installation_id: "inst-continue-narrow-asset".into(),
        provider: "generic-file".into(),
        root_path: source.to_string_lossy().into_owned(),
        adapter,
        prepared: Some(prepared),
        options,
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        identity_association: None,
        selection_set: SelectionSet::single(PreviewSelection {
            preview_id,
            leaf_id: "keep".into(),
            selection: selection.clone(),
        }),
    };
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let published = publish_item(&conn, "job-continue-narrow-asset", &item, &|| false).unwrap();
    let import_id = published.import_id.unwrap();
    let rev = published.revision_id.unwrap();
    let draft = crate::session_migration::engine::continue_prepare(
        &import_id,
        Some(&rev),
        Some("keep"),
        None,
        None,
    )
    .unwrap();

    let mut scope = draft.selection.clone();
    scope.asset_ids.retain(|id| id == "asset-ok");
    scope.fields.message_text = false;
    let revised =
        engine::continue_selection(&draft.draft_id, &draft.draft_hash, scope.clone()).unwrap();
    assert!(!revised.text.contains("NARROW_MESSAGE_SENTINEL"));
    let exported = engine::export_for_model(&import_id, Some(&rev), Some("keep"), &scope).unwrap();
    assert!(!exported.contains("NARROW_MESSAGE_SENTINEL"));
    assert!(!exported.contains("asset-no"));
    let sid = engine::continue_create(
        &revised.draft_id,
        &revised.draft_hash,
        "narrow-asset-created",
        Some("User-written new prompt".into()),
    )
    .unwrap();
    let grant = engine::grant_for_session(&sid).unwrap();
    let read = |method, args| crate::session_migration::mcp_stdio::call_tool(&grant, method, &args);
    let blob = read("asset_read", serde_json::json!({"assetId":"asset-ok"})).unwrap();
    assert!(blob.to_string().contains("asset-ok"));
    assert!(read("asset_read", serde_json::json!({"assetId":"asset-no"})).is_err());
    let text = read("read", serde_json::json!({"citation":"keep"}))
        .unwrap()
        .to_string();
    assert!(!text.contains("NARROW_MESSAGE_SENTINEL"));
    assert!(!text.contains("asset-no"));
    assert!(!read(
        "search",
        serde_json::json!({"query":"NARROW_MESSAGE_SENTINEL"})
    )
    .unwrap()
    .to_string()
    .contains("NARROW_MESSAGE_SENTINEL"));
    let auth =
        crate::session_migration::jobs::load_published_auth(&import_id, Some(&rev), Some("keep"))
            .unwrap();
    assert_eq!(auth.selection.hash(), selection.hash());
    let local = engine::export_markdown(&import_id, Some(&rev), Some("keep")).unwrap();
    assert!(local.contains("NARROW_MESSAGE_SENTINEL"));
}
