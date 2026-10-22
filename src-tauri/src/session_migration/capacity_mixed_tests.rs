//! Synthetic vendor-shaped files through discovery, fixed groups and scoped prepare.
//! This bounded contract is not vendor-data, association or large-capacity acceptance.
use super::{CapacityHome, Path, PathBuf, Provider};
use crate::session_migration::{
    engine, group_selection, job_control, jobs, prepare, scan,
    selection::{AllowedSelection, SelectionFields},
};
use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[path = "capacity_branch_asset_tests.rs"]
mod branch_assets;
#[path = "capacity_codex_media_tests.rs"]
mod codex_media;
#[path = "capacity_tool_scope_tests.rs"]
mod tool_scope;
#[path = "capacity_mixed_verify_tests.rs"]
mod verify;

struct SourceFile {
    path: PathBuf,
    bytes: Vec<u8>,
    provider: Provider,
    texts: [String; 3],
    scoped: bool,
}

struct Expected {
    reference: scan::CandidateRef,
    texts: [String; 3],
    selection: AllowedSelection,
    scoped: bool,
}

fn create_sources(root: &Path, count: usize) -> Vec<SourceFile> {
    (0..count)
        .map(|index| {
            let provider = [Provider::Codex, Provider::ClaudeCode, Provider::Cursor][index % 3];
            let directory = root.join(provider.as_str());
            std::fs::create_dir_all(&directory).unwrap();
            let id = format!("mixed-{index:05}");
            let texts = [
                format!("保留问题 {id} 🚀"),
                format!("EXCLUDED_REPLY_{id}"),
                format!("保留追问 {id}"),
            ];
            let records = match provider {
                Provider::Codex => vec![
                    json!({"type":"session_meta","payload":{"id":id}}),
                    json!({"type":"event_msg","payload":{"type":"user_message","message":texts[0]}}),
                    json!({"type":"event_msg","payload":{"type":"agent_message","message":texts[1]}}),
                    json!({"type":"event_msg","payload":{"type":"user_message","message":texts[2]}}),
                ],
                Provider::ClaudeCode => vec![
                    json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":id,
                        "message":{"role":"user","content":texts[0]}}),
                    json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":id,
                        "message":{"role":"assistant","content":texts[1]}}),
                    json!({"type":"user","uuid":"u2","parentUuid":"a1","sessionId":id,
                        "message":{"role":"user","content":texts[2]}}),
                ],
                Provider::Cursor => vec![
                    json!({"composerId":id,"role":"user","content":texts[0]}),
                    json!({"composerId":id,"role":"assistant","content":texts[1]}),
                    json!({"composerId":id,"role":"user","content":texts[2]}),
                ],
                _ => unreachable!(),
            };
            let bytes = records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>()
                .into_bytes();
            let path = directory.join(format!("{id}.jsonl"));
            std::fs::write(&path, &bytes).unwrap();
            SourceFile {
                path,
                bytes,
                provider,
                texts,
                scoped: index % 2 == 0,
            }
        })
        .collect()
}

fn selected_references(scanned: &scan::ScanState) -> Vec<scan::CandidateRef> {
    let selection = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let frozen = group_selection::freeze_current(
        &scanned.scan_id,
        scanned.epoch,
        &selection.selection_id,
        selection.state_version,
    )
    .unwrap();
    let mut cursor = None;
    let mut references = Vec::new();
    loop {
        let (page, next, total) = group_selection::page_refs(
            &scanned.scan_id,
            scanned.epoch,
            &frozen.selection_id,
            cursor.as_deref(),
            Some(2),
        )
        .unwrap();
        assert_eq!(total, scanned.discovered_count);
        assert!(page.len() <= 2);
        references.extend(page);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(references.len(), scanned.discovered_count as usize);
    references
}

fn wait_prepared(batch: &str) -> prepare::PrepareBatchStatus {
    let started = Instant::now();
    loop {
        let status = prepare::status(batch).unwrap();
        if !prepare::running_for_test(batch) && !status.state.runnable() {
            assert!(started.elapsed() < Duration::from_secs(60), "{status:?}");
            return status;
        }
        // Request cooperative cancellation, then drain the owned workers before
        // failing or allowing the isolated home to remove its temporary files.
        if started.elapsed() >= Duration::from_secs(60) && !status.cancel_requested {
            prepare::cancel(batch).unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn mixed_vendor_files_keep_scoped_selection_through_offline_publish_and_paged_reads() {
    const COUNT: usize = 12;
    let home = CapacityHome::new();
    let root = home.root.join("synthetic mixed 来源 #");
    let sources = create_sources(&root, COUNT);
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
    assert_eq!(scanned.discovered_count as usize, COUNT);
    let references = selected_references(&scanned);
    let mut expected = HashMap::new();
    let mut overrides = Vec::new();
    for reference in references {
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
        assert_eq!(details.event_count, 3);
        assert_eq!(details.events.len(), 3);
        for (event, text) in details.events.iter().zip(&source.texts) {
            assert_eq!(&event.text, text);
        }
        let mut selection = details.default_selection;
        assert_eq!(selection.event_ids.len(), 3);
        if source.scoped {
            selection.event_ids = vec![
                details.events[2].event_id.clone(),
                details.events[0].event_id.clone(),
            ];
            selection.pinned_event_ids = vec![details.events[0].event_id.clone()];
            selection.fields = SelectionFields {
                tool_call_name: false,
                tool_call_input: false,
                tool_result_content: false,
                ..SelectionFields::default()
            };
            overrides.push(prepare::PrepareOverride {
                candidate_id: reference.candidate.candidate_id.clone(),
                preview_id,
                leaf_id: details.active_leaf.unwrap(),
                selection: selection.clone(),
            });
        }
        assert!(expected
            .insert(
                reference.candidate.candidate_id.clone(),
                Expected {
                    reference,
                    texts: source.texts.clone(),
                    selection: selection.canonicalized(),
                    scoped: source.scoped,
                },
            )
            .is_none());
    }
    assert_eq!(overrides.len(), COUNT / 2);
    let selection = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let batch = prepare::start(
        &scanned.scan_id,
        scanned.epoch,
        &selection.selection_id,
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
    let offline = root.with_file_name("offline mixed 来源 #");
    std::fs::rename(&root, &offline).unwrap();
    for source in &sources {
        assert_eq!(
            std::fs::read(offline.join(source.path.strip_prefix(&root).unwrap())).unwrap(),
            source.bytes
        );
    }
    engine::reset_runtime();
    prepare::reset_runtime();
    let request = "mixed-scoped-offline-publish";
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
    for source in &sources {
        assert_eq!(
            std::fs::read(offline.join(source.path.strip_prefix(&root).unwrap())).unwrap(),
            source.bytes
        );
    }
}
