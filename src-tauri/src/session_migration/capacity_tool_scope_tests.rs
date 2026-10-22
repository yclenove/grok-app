//! Tool-field switches through real adapters, offline publication and MCP grants.
use super::{
    engine, group_selection, job_control, json, prepare, scan, selected_references, wait_prepared,
    AllowedSelection, CapacityHome, HashMap, Path, PathBuf, Provider, SelectionFields,
};
use crate::session_migration::types::MessageRole;

#[path = "capacity_tool_scope_source_tests.rs"]
mod source;
#[path = "capacity_tool_scope_verify_tests.rs"]
mod verify;

struct Expected {
    reference: scan::CandidateRef,
    source: source::Source,
    selection: AllowedSelection,
    event_ids: Vec<String>,
}

fn assert_sources(root: &Path, offline: &Path, sources: &[source::Source]) {
    for source in sources {
        assert_eq!(
            std::fs::read(offline.join(source.path.strip_prefix(root).unwrap())).unwrap(),
            source.bytes
        );
    }
}

#[test]
fn vendor_tool_fields_stay_scoped_in_offline_publish_drafts_and_real_mcp_grants() {
    let home = CapacityHome::new();
    let root = home.root.join("synthetic tools 来源 #");
    let sources = source::create(&root);
    let scanned = scan::scan_start(
        [Provider::Codex, Provider::ClaudeCode, Provider::Cursor]
            .map(|provider| {
                (
                    provider,
                    root.join(provider.as_str()).to_string_lossy().into_owned(),
                )
            })
            .to_vec(),
    )
    .unwrap();
    assert!(scanned.complete, "{scanned:?}");
    assert!(scanned.issues.is_empty(), "{:?}", scanned.issues);
    assert_eq!(scanned.discovered_count, 12);
    let mut expected = HashMap::new();
    let mut overrides = Vec::new();
    for reference in selected_references(&scanned) {
        let source = sources
            .iter()
            .find(|source| {
                source.path.canonicalize().unwrap()
                    == Path::new(&reference.candidate.locator)
                        .canonicalize()
                        .unwrap()
            })
            .unwrap();
        assert_eq!(reference.candidate.provider, source.provider);
        assert_eq!(
            reference.source_member.is_some(),
            source.provider == Provider::Cursor
        );
        let (preview_id, details) = engine::preview_reference(
            reference.clone(),
            Some(&reference.candidate.source_revision),
            None,
        )
        .unwrap();
        assert_eq!(details.event_count, 5);
        assert_eq!(details.events.len(), 5);
        assert_eq!(details.inventory.imported.tool_calls, 1);
        assert_eq!(details.inventory.imported.tool_results, 1);
        assert_eq!(
            details
                .events
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            ["message", "message", "tool-call", "tool-result", "message"]
        );
        for ((index, role), text) in [
            (0, MessageRole::User),
            (1, MessageRole::Assistant),
            (4, MessageRole::Assistant),
        ]
        .into_iter()
        .zip(&source.messages)
        {
            assert_eq!(details.events[index].role, Some(role));
            assert_eq!(&details.events[index].text, text);
        }
        assert!(details.events[2].text.contains(&source.sentinels[0]));
        assert!(details.events[2].text.contains(&source.sentinels[1]));
        assert_eq!(details.events[3].text, source.sentinels[2]);
        let event_ids: Vec<_> = details
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        let mut selection = details.default_selection;
        // Select the real tools, then let the field policy suppress their bodies.
        selection.event_ids = event_ids.iter().rev().cloned().collect();
        selection.pinned_event_ids = vec![event_ids[0].clone()];
        selection.fields = source.mode.fields();
        overrides.push(prepare::PrepareOverride {
            candidate_id: reference.candidate.candidate_id.clone(),
            preview_id,
            leaf_id: details.active_leaf.unwrap(),
            selection: selection.clone(),
        });
        assert!(expected
            .insert(
                reference.candidate.candidate_id.clone(),
                Expected {
                    reference,
                    source: source.clone(),
                    selection: selection.canonicalized(),
                    event_ids,
                }
            )
            .is_none());
    }
    assert_eq!(overrides.len(), 12); // Deliberately below the real 50-override bound.
    let selected = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let batch = prepare::start(
        &scanned.scan_id,
        scanned.epoch,
        &selected.selection_id,
        overrides,
    )
    .unwrap();
    let prepared = wait_prepared(&batch.batch_id);
    assert_eq!(prepared.state, prepare::PrepareBatchState::Completed);
    assert_eq!(
        (prepared.total, prepared.prepared, prepared.failed),
        (12, 12, 0)
    );
    let plan = engine::plan_from_validated_prepare(
        prepare::validated_plan_inputs(&batch.batch_id).unwrap(),
        prepare::mappings(&batch.batch_id, None).unwrap(),
    )
    .unwrap();
    let offline = root.with_file_name("offline tools 来源 #");
    std::fs::rename(&root, &offline).unwrap();
    assert_sources(&root, &offline, &sources);
    engine::reset_runtime();
    prepare::reset_runtime();
    let request = "tool-fields-offline-publish";
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, request).unwrap();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    engine::reset_runtime();
    verify::published(&queued.job_id, &expected);
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, request)
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_sources(&root, &offline, &sources);
}
