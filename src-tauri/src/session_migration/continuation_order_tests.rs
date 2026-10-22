// Regression from the native V107 draft: canonical permission IDs are not chronology.
fn source_order_fixture(home: &IsolatedAppHome) -> HandoffDraft {
    let path = home.path.join("source-order-history.json");
    let messages: Vec<_> = (0..50)
        .map(|i| {
            serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": format!("SOURCE_ORDER_{i:03} synthetic history only")
            })
        })
        .collect();
    let bytes = serde_json::to_vec(&messages).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let scanned =
        scan::scan_start(vec![(Provider::GenericFile, path.to_string_lossy().into())]).unwrap();
    let candidate = &scanned.candidates[0];
    let (preview, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = engine::propose_mappings(std::slice::from_ref(&preview), None).unwrap();
    let plan = engine::plan(vec![preview], mappings).unwrap();
    let job = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = engine::job_get(&job.job_id).unwrap();
    let id = job.items[0].import_id.as_ref().unwrap();
    let draft = engine::continue_prepare(id, None, None, None, None).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(store::load_sessions_snapshot().unwrap().is_empty());
    draft
}

fn source_order_ids(ids: impl IntoIterator<Item = usize>) -> Vec<String> {
    ids.into_iter().map(|i| format!("generic:{i}")).collect()
}

fn source_order_text_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|v| v["eventId"].as_str().map(str::to_owned))
        .collect()
}

fn assert_source_order_prose(text: &str, ids: &[usize]) {
    let positions: Vec<_> = ids
        .iter()
        .map(|i| text.find(&format!("SOURCE_ORDER_{i:03}")).unwrap())
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "wrong source order: {positions:?}"
    );
}

#[test]
fn continuation_source_order_full_handoff_not_canonical_id_order() {
    let home = IsolatedAppHome::acquire("source-order-full");
    let draft = source_order_fixture(&home);
    let expected = source_order_ids(0..50);
    assert_eq!(source_order_text_ids(&draft.text), expected);
    assert_eq!(draft.fragment_event_ids, expected);
    assert_eq!(draft.readable_event_ids, expected);
    assert_eq!(draft.coverage, "full-selection");
}

#[test]
fn continuation_source_order_prefix_preserves_source_order_and_ceiling() {
    let home = IsolatedAppHome::acquire("source-order-prefix");
    let full = source_order_fixture(&home);
    let draft = engine::continue_prepare(
        &full.import_id,
        Some(&full.revision_id),
        Some("generic:21"),
        None,
        None,
    )
    .unwrap();
    assert_eq!(source_order_text_ids(&draft.text), source_order_ids(0..22));
    assert!(!draft.text.contains("SOURCE_ORDER_022"));
    assert_eq!(draft.leaf, "generic:21");
}

#[test]
fn continuation_source_order_narrowed_rebuild_and_native_creation() {
    let home = IsolatedAppHome::acquire("source-order-narrowed");
    let full = source_order_fixture(&home);
    let mut selection = full.selection.clone();
    selection.event_ids = source_order_ids([49, 10, 0, 2]);
    // A second review can remove existing pins, never add new authority.
    selection
        .pinned_event_ids
        .retain(|id| selection.event_ids.contains(id));
    let hash = selection.hash();
    let draft = engine::continue_selection(&full.draft_id, &full.draft_hash, selection).unwrap();
    let expected = source_order_ids([0, 2, 10, 49]);
    assert_eq!(source_order_text_ids(&draft.text), expected);
    assert_eq!(draft.selection.hash(), hash);
    let sid = engine::continue_create(
        &draft.draft_id,
        &draft.draft_hash,
        "source-order-create",
        None,
    )
    .unwrap();
    let side = continuation::read_sidecar(&sid).unwrap();
    assert_eq!(side.allowed_selection_hash, hash);
    assert_eq!(side.state, DispatchState::Prepared);
    let prose = std::fs::read_to_string(
        continuation::sidecar_path(&sid).with_file_name("handoff_draft.txt"),
    )
    .unwrap();
    assert_eq!(source_order_text_ids(&prose), expected);
    assert!(store::load_messages(&sid).is_empty());
    assert!(!prose.contains("SOURCE_ORDER_001"));
}

#[test]
fn continuation_source_order_markdown_export_matches_reader_chronology() {
    let home = IsolatedAppHome::acquire("source-order-markdown");
    let draft = source_order_fixture(&home);
    let text = engine::export_markdown(
        &draft.import_id,
        Some(&draft.revision_id),
        Some(&draft.leaf),
    )
    .unwrap();
    assert_source_order_prose(&text, &(0..50).collect::<Vec<_>>());
}

#[test]
fn continuation_source_order_model_export_keeps_narrowed_chronology() {
    let home = IsolatedAppHome::acquire("source-order-model");
    let draft = source_order_fixture(&home);
    let mut selection = draft.selection.clone();
    selection.event_ids = source_order_ids([49, 10, 0, 2]);
    let text = engine::export_for_model(
        &draft.import_id,
        Some(&draft.revision_id),
        Some(&draft.leaf),
        &selection,
    )
    .unwrap();
    assert_source_order_prose(&text, &[0, 2, 10, 49]);
    assert!(!text.contains("SOURCE_ORDER_001"));
}

#[test]
fn continuation_source_order_budget_prefers_latest_turn_not_largest_id() {
    let home = IsolatedAppHome::acquire("source-order-budget");
    let full = source_order_fixture(&home);
    let mut selection = full.selection.clone();
    selection.pinned_event_ids = source_order_ids([0]);
    let draft = continuation::prepare_draft(
        &full.import_id,
        &full.revision_id,
        &full.leaf,
        Some(selection),
        None,
        "local-full",
        Some(3000),
    )
    .unwrap();
    assert!(draft.fragment_event_ids.contains(&"generic:0".into()));
    assert!(draft.fragment_event_ids.contains(&"generic:49".into()));
    assert!(!draft.fragment_event_ids.contains(&"generic:9".into()));
    let ordinals: Vec<_> = draft
        .fragment_event_ids
        .iter()
        .map(|id| {
            id.strip_prefix("generic:")
                .unwrap()
                .parse::<usize>()
                .unwrap()
        })
        .collect();
    assert!(ordinals.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(source_order_text_ids(&draft.text), draft.fragment_event_ids);
    assert_eq!(draft.coverage, "truncated-to-budget");
    assert!(draft.estimated_tokens <= 300);
}

#[test]
fn continuation_source_order_opaque_ids_keep_lookup_contract_and_scope() {
    use crate::session_migration::{
        search,
        types::{ContentBlock, EventBase, ImportedEvent, MessageRole, TimeOrigin},
    };
    let home = IsolatedAppHome::acquire("source-order-opaque");
    let dir = home.path.join("isolated-index");
    std::fs::create_dir_all(&dir).unwrap();
    let events: Vec<_> = [
        ("z-opaque", 4),
        ("a-late", 20),
        ("b-tie", 4),
        ("unselected", 1),
    ]
    .into_iter()
    .map(|(id, ordinal)| ImportedEvent::Message {
        base: EventBase {
            id: id.into(),
            ordinal,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role: MessageRole::User,
        blocks: vec![ContentBlock::text(id)],
        complete: true,
    })
    .collect();
    search::build_index(&dir, &events).unwrap();
    let ids: Vec<String> = ["a-late", "missing", "z-opaque", "z-opaque", "b-tie"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let before = ids.clone();
    let lookup = search::indexed_events_by_ids(&dir, &ids).unwrap();
    assert_eq!(
        lookup.iter().map(|e| e.id()).collect::<Vec<_>>(),
        ["a-late", "z-opaque", "b-tie"]
    );
    search::take_event_body_reads();
    let ordered = search::indexed_events_in_source_order(&dir, &ids).unwrap();
    assert_eq!(
        ordered.iter().map(|e| e.id()).collect::<Vec<_>>(),
        ["b-tie", "z-opaque", "a-late"]
    );
    assert_eq!(
        search::take_event_body_reads(),
        3,
        "must not read unselected archive bodies"
    );
    assert_eq!(ids, before, "sorting must not mutate permission IDs");
    assert!(
        search::indexed_events_in_source_order(&home.path.join("absent"), &[])
            .unwrap()
            .is_empty()
    );
}
