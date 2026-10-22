//! Bounded mixed prepare/publication: real historical Cursor plus three vendors.
use super::super::{
    explicit_path, normalizer, receipts, seed_home, text, ARCHIVE, ARCHIVE_FILES, IMPORT,
    INSTALLATION, OLD_JOB, REVISION,
};
use crate::session_migration::{
    engine, group_selection, hash, job_control, jobs, prepare, prepare_plan,
    prepared_association::{PreparedAssociationPlanRequest, PreparedAssociationRequest},
    scan,
    selection::{AllowedSelection, SelectionFields},
    types::{MessageRole, Provider},
};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[path = "engine_historical_prepared_mixed_sources.rs"]
mod sources;
#[path = "engine_historical_prepared_mixed_verify_tests.rs"]
mod verify;

const ORDINARY: usize = 72;

struct Expected {
    reference: scan::CandidateRef,
    preview_id: String,
    leaf: String,
    selection: AllowedSelection,
    event_ids: Vec<String>,
    messages: Vec<String>,
    scoped: bool,
    historical: bool,
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        engine::reset_runtime();
        prepare::reset_runtime();
    }
}

fn references(scanned: &scan::ScanState) -> Vec<scan::CandidateRef> {
    let current = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let frozen = group_selection::freeze_current(
        &scanned.scan_id,
        scanned.epoch,
        &current.selection_id,
        current.state_version,
    )
    .unwrap();
    let mut cursor = None;
    let mut result = Vec::new();
    loop {
        let (page, next, total) = group_selection::page_refs(
            &scanned.scan_id,
            scanned.epoch,
            &frozen.selection_id,
            cursor.as_deref(),
            Some(2),
        )
        .unwrap();
        assert_eq!(total as usize, ORDINARY + 1);
        assert!(page.len() <= 2);
        result.extend(page);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(result.len(), ORDINARY + 1);
    result
}

fn wait_prepared(batch_id: &str) -> prepare::PrepareBatchStatus {
    let started = Instant::now();
    loop {
        let status = prepare::status(batch_id).unwrap();
        if !prepare::running_for_test(batch_id) && !status.state.runnable() {
            assert!(started.elapsed() < Duration::from_secs(60), "{status:?}");
            return status;
        }
        if started.elapsed() >= Duration::from_secs(60) && !status.cancel_requested {
            prepare::cancel(batch_id).unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn preview(
    scanned: &scan::ScanState,
    sources: &[sources::Source],
) -> (HashMap<String, Expected>, Vec<prepare::PrepareOverride>) {
    let mut expected = HashMap::new();
    let mut overrides = Vec::new();
    for reference in references(scanned) {
        let historical = reference.source_id == INSTALLATION;
        let (messages, scoped) = if historical {
            assert_eq!(reference.candidate.provider, Provider::Cursor);
            (vec!["hello one".into(), "hi two".into()], true)
        } else {
            let source = sources
                .iter()
                .find(|source| {
                    source.path.canonicalize().unwrap()
                        == Path::new(&reference.candidate.locator)
                            .canonicalize()
                            .unwrap()
                })
                .unwrap();
            assert_eq!(source.provider, reference.candidate.provider);
            (source.messages.clone(), source.scoped)
        };
        assert_eq!(
            reference.source_member.is_some(),
            reference.candidate.provider == Provider::Cursor
        );
        let (preview_id, details) = engine::preview_reference(
            reference.clone(),
            Some(&reference.candidate.source_revision),
            None,
        )
        .unwrap();
        assert_eq!(details.event_count, messages.len());
        assert_eq!(details.events.len(), messages.len());
        for (index, (event, message)) in details.events.iter().zip(&messages).enumerate() {
            assert_eq!(event.kind, "message");
            assert_eq!(
                event.role,
                Some(if index == 1 {
                    MessageRole::Assistant
                } else {
                    MessageRole::User
                })
            );
            assert_eq!(&event.text, message);
        }
        let event_ids: Vec<_> = details
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        let mut selection = details.default_selection;
        let leaf = details.active_leaf.unwrap();
        if scoped {
            selection.event_ids = event_ids
                .iter()
                .enumerate()
                .rev()
                .filter(|(index, _)| *index != 1)
                .map(|(_, id)| id.clone())
                .collect();
            selection.pinned_event_ids = vec![event_ids[0].clone()];
            selection.fields = SelectionFields {
                tool_call_name: false,
                tool_call_input: false,
                tool_result_content: false,
                ..SelectionFields::default()
            };
            overrides.push(prepare::PrepareOverride {
                candidate_id: reference.candidate.candidate_id.clone(),
                preview_id: preview_id.clone(),
                leaf_id: leaf.clone(),
                selection: selection.clone(),
            });
        }
        assert!(expected
            .insert(
                reference.candidate.candidate_id.clone(),
                Expected {
                    reference,
                    preview_id,
                    leaf,
                    selection: selection.canonicalized(),
                    event_ids,
                    messages,
                    scoped,
                    historical,
                }
            )
            .is_none());
    }
    assert_eq!(overrides.len(), ORDINARY / 2 + 1);
    assert!(overrides.len() < 50);
    (expected, overrides)
}

fn prepared_members(batch_id: &str, expected: &mut HashMap<String, Expected>) {
    let mut cursor = None;
    let mut seen = HashSet::new();
    loop {
        let page = prepare::items_page(batch_id, cursor.as_deref(), Some(2)).unwrap();
        assert!(page.items.len() <= 2);
        for item in page.items {
            assert!(seen.insert(item.candidate_id.clone()));
            assert_eq!(item.state, prepare::PrepareItemState::Prepared);
            assert!(item.error.is_none());
            let expected = expected.get_mut(&item.candidate_id).unwrap();
            assert_eq!(
                item.provider,
                expected.reference.candidate.provider.as_str()
            );
            assert_eq!(
                item.source_revision,
                expected.reference.candidate.source_revision
            );
            let preview_id = item.preview_id.unwrap();
            if expected.scoped {
                assert_eq!(preview_id, expected.preview_id);
            }
            expected.preview_id = preview_id;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), expected.len());
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_prepared_mixed_vendor_scopes_publish_offline_without_replacing_old_history() {
    let home = seed_home();
    let _reset = Reset;
    prepare::reset_runtime();
    let root = home.path.join("prepared mixed 来源 #");
    let sources = sources::create(&root, ORDINARY);
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let registered = crate::session_migration::sources::get(&conn, INSTALLATION).unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let old_job: String = conn
        .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get(0)
        })
        .unwrap();
    let old_hashes: Vec<_> = ARCHIVE_FILES
        .iter()
        .map(|(name, _)| {
            (
                name.to_string(),
                hash::sha256_file(&home.path.join(ARCHIVE).join(name)).unwrap(),
            )
        })
        .collect();
    let fixture = explicit_path("GROK_IMPORT_HISTORICAL_FIXTURE");
    let fixture_bytes = std::fs::read(&fixture).unwrap();
    let mut paths: Vec<_> = [Provider::Codex, Provider::ClaudeCode, Provider::Cursor]
        .map(|provider| scan::ScanSourcePath {
            provider: provider.as_str().into(),
            path: root.join(provider.as_str()).to_string_lossy().into_owned(),
            source_id: None,
            location_id: None,
            host: Some("local".into()),
            profile: String::new(),
        })
        .to_vec();
    paths.push(scan::ScanSourcePath {
        provider: "cursor".into(),
        path: registered.root_path,
        source_id: Some(INSTALLATION.into()),
        location_id: None,
        host: Some("local".into()),
        profile: registered.profile,
    });
    let started = scan::begin(paths).unwrap();
    scan::run(&started.scan_id).unwrap();
    let scanned = scan::state(&started.scan_id, started.epoch).unwrap();
    assert!(scanned.complete && scanned.issues.is_empty(), "{scanned:?}");
    assert_eq!(scanned.discovered_count as usize, ORDINARY + 1);
    let (mut expected, overrides) = preview(&scanned, &sources);
    let selected = group_selection::get(&scanned.scan_id, scanned.epoch).unwrap();
    let batch = prepare::start(
        &scanned.scan_id,
        scanned.epoch,
        &selected.selection_id,
        overrides,
    )
    .unwrap();
    let ready = wait_prepared(&batch.batch_id);
    assert_eq!(ready.state, prepare::PrepareBatchState::Completed);
    assert_eq!(
        (ready.total as usize, ready.prepared as usize, ready.failed),
        (ORDINARY + 1, ORDINARY + 1, 0)
    );
    prepared_members(&batch.batch_id, &mut expected);
    let historical = expected.values().find(|item| item.historical).unwrap();
    let request = PreparedAssociationRequest {
        version: 1,
        batch_id: batch.batch_id.clone(),
        expected_state_version: ready.state_version,
        candidate_id: historical.reference.candidate.candidate_id.clone(),
        preview_id: historical.preview_id.clone(),
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
    };
    let review = request.inspect().unwrap();
    assert_eq!(review.candidate_id, request.candidate_id);
    assert_eq!(
        review.review.selection_hash,
        crate::session_migration::selection::SelectionSet::single(
            crate::session_migration::selection::PreviewSelection {
                preview_id: historical.preview_id.clone(),
                leaf_id: historical.leaf.clone(),
                selection: historical.selection.clone(),
            }
        )
        .hash()
    );
    let confirmation = crate::session_migration::identity_association::AssociationConfirmation {
        version: 1,
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
        preview_id: historical.preview_id.clone(),
        review_digest: review.review.review_digest,
    };
    let plan = PreparedAssociationPlanRequest {
        version: 1,
        batch_id: batch.batch_id.clone(),
        expected_state_version: ready.state_version,
        mappings: prepare::mappings(&batch.batch_id, None).unwrap(),
        identity_associations: vec![confirmation.clone()],
    }
    .build()
    .unwrap();
    assert_eq!(plan.identity_associations, vec![confirmation]);
    assert_eq!(plan.preview_ids.len(), ORDINARY + 1);
    assert_eq!(
        prepare::status(&batch.batch_id).unwrap().state,
        prepare::PrepareBatchState::Planned
    );
    prepare_plan::verify_receipt(
        &prepare_plan::PreparePlanOrigin {
            version: 1,
            batch_id: batch.batch_id.clone(),
            state_version: ready.state_version,
        },
        &plan.plan_id,
        &plan.plan_hash,
    )
    .unwrap();
    // Only disposable ordinary roots move. The registered immutable historical
    // fixture remains hash-pinned and read-only; do not rename or alter it.
    let offline = root.with_file_name("offline prepared mixed 来源 #");
    std::fs::rename(&root, &offline).unwrap();
    sources::unchanged(&root, &offline, &sources);
    engine::reset_runtime();
    prepare::reset_runtime();
    let recovered = engine::recovery().unwrap().plan.unwrap();
    assert_eq!(recovered.plan_id, plan.plan_id);
    assert_eq!(recovered.preview_count, ORDINARY + 1);
    assert_eq!(recovered.association_targets, plan.identity_associations);
    let request_id = "prepared-mixed-vendors-and-historical";
    let queued = engine::commit(&plan.plan_id, &plan.plan_hash, request_id).unwrap();
    assert_eq!(queued.item_count as usize, ORDINARY + 1);
    assert_eq!(job_control::run_pending().unwrap(), 1);
    engine::reset_runtime();
    verify::published(&queued.job_id, &expected);
    assert_eq!(
        engine::commit(&plan.plan_id, &plan.plan_hash, request_id)
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert_eq!(job_control::run_pending().unwrap(), 0);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(normalizer(&conn, IMPORT, REVISION), "1.0.0");
    for receipt in old_receipts {
        assert!(receipts(&conn).contains(&receipt));
    }
    assert_eq!(
        conn.query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        old_job
    );
    for (name, hash_before) in old_hashes {
        assert_eq!(
            hash::sha256_file(&home.path.join(ARCHIVE).join(name)).unwrap(),
            hash_before
        );
    }
    assert_eq!(std::fs::read(fixture).unwrap(), fixture_bytes);
    sources::unchanged(&root, &offline, &sources);
}
