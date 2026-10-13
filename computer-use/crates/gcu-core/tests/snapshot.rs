//! Snapshot identity, bounded retention, and semantic diff properties.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;
use gcu_core::{
    driver::TreeOpts,
    model::*,
    render::render_text,
    snapshot::{Diff, ObservationMode, RenderedState, SnapshotError, SnapshotStore, WindowKey},
    tree::{Pipeline, ProcessedTree},
};
use proptest::prelude::*;
use support::*;

fn tree(ids: &[u64], changed: &[u64]) -> ProcessedTree {
    let mut input = raw(ids
        .iter()
        .map(|id| {
            let mut n = node(Role::Button, &format!("Button {id}"), vec![]);
            if changed.contains(id) {
                n.states |= States::CHECKED;
            }
            n
        })
        .collect());
    let root = input.root.as_mut().unwrap();
    root.fingerprint = Fingerprint(0);
    for (n, id) in root.children.iter_mut().zip(ids) {
        n.fingerprint = Fingerprint(*id + 1);
    }
    Pipeline::default().run(input, &TreeOpts::default())
}
fn key() -> WindowKey {
    WindowKey::from_options(&opts())
}
fn record(store: &mut SnapshotStore, input: ProcessedTree) -> gcu_core::snapshot::Observation {
    store.record(input, &opts(), None).unwrap()
}

#[test]
fn snapshot_turn_first_observation_always_full() {
    let mut store = SnapshotStore::default();
    assert_eq!(
        record(&mut store, tree(&[1, 2], &[])).mode,
        ObservationMode::Full
    );
    assert_eq!(
        record(&mut store, tree(&[1, 2], &[])).mode,
        ObservationMode::Diff
    );
    store.begin_turn();
    assert_eq!(
        record(&mut store, tree(&[1, 2], &[])).mode,
        ObservationMode::Full
    );
    assert_eq!(
        record(&mut store, tree(&[1, 2], &[])).mode,
        ObservationMode::Diff
    );
}

#[test]
fn snapshot_insert_remove_preserves_indices_and_preorder() {
    let mut store = SnapshotStore::default();
    let first = record(&mut store, tree(&[1, 2, 3, 4], &[]));
    let original = store
        .get(&key(), first.snapshot_id)
        .unwrap()
        .index()
        .index_of(Fingerprint(3))
        .unwrap();
    let second = record(&mut store, tree(&[9, 1, 2, 3, 4], &[2]));
    let snapshot = store.get(&key(), second.snapshot_id).unwrap();
    assert_eq!(snapshot.index().index_of(Fingerprint(3)), Some(original));
    let rendered = snapshot.rendered().text();
    assert!(rendered.find("Button 9").unwrap() < rendered.find("Button 1").unwrap());
    let third = record(&mut store, tree(&[9, 1, 3, 4], &[]));
    assert!(matches!(
        store.resolve(&key(), first.snapshot_id, original),
        Err(SnapshotError::StaleElement)
    ));
    let fourth = record(&mut store, tree(&[9, 1, 2, 3, 4], &[]));
    assert!(
        store
            .get(&key(), fourth.snapshot_id)
            .unwrap()
            .index()
            .index_of(Fingerprint(3))
            .unwrap()
            > original
    );
    assert!(store.get(&key(), third.snapshot_id).is_some());
}

#[test]
fn snapshot_fourth_observation_evicts_old_reference() {
    let mut store = SnapshotStore::default();
    let first = record(&mut store, tree(&[1], &[]));
    for _ in 0..3 {
        record(&mut store, tree(&[1], &[]));
    }
    assert_eq!(store.len(), 3);
    assert!(store.get(&key(), first.snapshot_id).is_none());
    assert!(matches!(
        store.resolve(&key(), first.snapshot_id, 1),
        Err(SnapshotError::StaleElement)
    ));
}

#[test]
fn snapshot_memory_limit_evicts_globally_oldest_and_rejects_oversize() {
    let mut probe = SnapshotStore::default();
    record(&mut probe, tree(&[1], &[]));
    let size = probe.memory_bytes();
    let mut store = SnapshotStore::with_memory_limit(size * 2);
    let first = record(&mut store, tree(&[1], &[]));
    let mut other = opts();
    other.window.id = "other".into();
    store.record(tree(&[1], &[]), &other, None).unwrap();
    record(&mut store, tree(&[1], &[]));
    assert!(store.get(&key(), first.snapshot_id).is_none());
    assert!(store.memory_bytes() <= size * 2);
    let mut tiny = SnapshotStore::with_memory_limit(1);
    assert!(matches!(
        tiny.record(tree(&[1], &[]), &opts(), None),
        Err(SnapshotError::MemoryLimit)
    ));
    assert_eq!(tiny.len(), 0);
}

#[test]
fn diff_threshold_exact_sixty_lines_stays_diff_sixty_one_full() {
    for (count, large) in [(60, false), (61, true)] {
        let ids: Vec<_> = (0..220).collect();
        let changed: Vec<_> = (0..count).collect();
        let before = RenderedState::from_tree(&tree(&ids, &[]), &opts());
        let after = RenderedState::from_tree(&tree(&ids, &changed), &opts());
        let diff = Diff::between(&before, &after);
        assert_eq!(diff.is_large_change(), large);
        assert_eq!(diff.apply(&before).unwrap(), after);
    }
}

#[test]
fn diff_structure_exact_thirty_percent_stays_diff_above_full() {
    for (removed, large) in [(3, false), (4, true)] {
        let ids: Vec<_> = (0..9).collect(); // including window: 10 nodes
        let before = RenderedState::from_tree(&tree(&ids, &[]), &opts());
        let after = RenderedState::from_tree(&tree(&ids[removed..], &[]), &opts());
        assert_eq!(Diff::between(&before, &after).is_large_change(), large);
    }
}

#[test]
fn diff_changes_render_three_kinds_and_unchanged_summary() {
    let mut store = SnapshotStore::default();
    let first = record(&mut store, tree(&(0..20).collect::<Vec<_>>(), &[]));
    let mut ids: Vec<_> = (1..20).collect();
    ids.push(30);
    let next = record(&mut store, tree(&ids, &[1]));
    assert_eq!(next.mode, ObservationMode::Diff);
    assert!(next.text.contains("\n~ "));
    assert!(next.text.contains("\n+ "));
    assert!(next.text.contains("\n- "));
    assert!(next.text.contains("Unchanged: 19 nodes"));
    assert_eq!(
        next.diff
            .unwrap()
            .apply(store.get(&key(), first.snapshot_id).unwrap().rendered())
            .unwrap(),
        *store.get(&key(), next.snapshot_id).unwrap().rendered()
    );
}

#[test]
fn diff_reorder_and_footer_reconstruct_new_render() {
    let before = RenderedState::from_tree(&tree(&[1, 2, 3], &[]), &opts());
    let mut after_tree = tree(&[3, 2, 1], &[]);
    after_tree.focused = Some(1);
    let after = RenderedState::from_tree(&after_tree, &opts());
    assert_eq!(
        Diff::between(&before, &after)
            .apply(&before)
            .unwrap()
            .text(),
        render_text(&after_tree, &opts())
    );
}

proptest! {
    #[test]
    fn diff_random_tree_mutations_reconstruct_semantics(
        ids in proptest::collection::btree_set(0u64..100, 0..50),
        removed in proptest::collection::btree_set(0u64..100, 0..50),
        added in proptest::collection::btree_set(100u64..130, 0..20),
        changed in proptest::collection::btree_set(0u64..100, 0..50),
        reverse in any::<bool>(),
    ) {
        let mut store = SnapshotStore::default();
        let old: Vec<_> = ids.into_iter().collect();
        let a = record(&mut store, tree(&old, &[]));
        let before = store.get(&key(), a.snapshot_id).unwrap().rendered().clone();
        let mut new: Vec<_> = old.iter().copied().filter(|i| !removed.contains(i)).chain(added).collect();
        if reverse { new.reverse(); }
        let b = record(&mut store, tree(&new, &changed.into_iter().collect::<Vec<_>>()));
        let after = store.get(&key(), b.snapshot_id).unwrap().rendered();
        let diff = Diff::between(&before, after);
        prop_assert_eq!(diff.apply(&before).unwrap(), after.clone());
        let fingerprints: Vec<_> = after.nodes().iter().map(|n|n.fingerprint).collect();
        let expected: Vec<_> = std::iter::once(Fingerprint(0)).chain(new.iter().map(|id|Fingerprint(id+1))).collect();
        prop_assert_eq!(fingerprints, expected);
    }
    #[test]
    fn snapshot_random_lifetimes_never_reuse_indices(rounds in proptest::collection::vec(proptest::collection::btree_set(0u64..25, 0..20), 1..30)) {
        let mut store = SnapshotStore::default();
        let mut seen = std::collections::HashMap::new();
        let mut previous = std::collections::HashMap::new();
        for ids in rounds {
            let observation = record(&mut store, tree(&ids.into_iter().collect::<Vec<_>>(), &[]));
            let index = store.get(&key(), observation.snapshot_id).unwrap().index();
            let mut current = std::collections::HashMap::new();
            for (i, entry) in index.elements() {
                if let Some(old) = previous.get(&entry.fingerprint) { prop_assert_eq!(*i, *old); }
                else { prop_assert!(!seen.contains_key(i)); }
                seen.insert(*i, entry.fingerprint);
                current.insert(entry.fingerprint, *i);
            }
            previous = current;
        }
    }
    #[test]
    fn diff_random_attribute_threshold_obeys_sixty(changed in 0usize..100) {
        let ids: Vec<_> = (0..100).collect();
        let before = RenderedState::from_tree(&tree(&ids, &[]), &opts());
        let after = RenderedState::from_tree(&tree(&ids, &(0..changed as u64).collect::<Vec<_>>()), &opts());
        prop_assert_eq!(Diff::between(&before, &after).is_large_change(), changed > 60);
    }
}

#[test]
fn snapshot_window_and_session_namespaces_do_not_resolve_foreign_references() {
    let mut a = SnapshotStore::default();
    let first = record(&mut a, tree(&[1], &[]));
    let other = WindowKey {
        app_id: "different-app".into(),
        window_id: key().window_id,
    };
    assert!(matches!(
        a.resolve(&other, first.snapshot_id, 1),
        Err(SnapshotError::StaleElement)
    ));
    let b = SnapshotStore::default();
    assert!(matches!(
        b.resolve(&key(), first.snapshot_id, 1),
        Err(SnapshotError::StaleElement)
    ));
    let mut options = opts();
    options.window.id = "second-window".into();
    let second_key = WindowKey::from_options(&options);
    a.record(tree(&[1], &[]), &options, None).unwrap();
    a.begin_turn();
    assert_eq!(record(&mut a, tree(&[1], &[])).mode, ObservationMode::Full);
    assert_eq!(
        a.record(tree(&[1], &[]), &options, None).unwrap().mode,
        ObservationMode::Full
    );
    assert_eq!(
        a.latest(&second_key)
            .unwrap()
            .index()
            .index_of(Fingerprint(2)),
        Some(1)
    );
}

#[test]
fn snapshot_global_eviction_retains_window_high_water_counter() {
    let mut probe = SnapshotStore::default();
    record(&mut probe, tree(&[1], &[]));
    let mut store = SnapshotStore::with_memory_limit(probe.memory_bytes() * 2 - 64);
    let first = record(&mut store, tree(&[1], &[]));
    let mut options = opts();
    options.window.id = "second".into();
    store.record(tree(&[1], &[]), &options, None).unwrap();
    assert!(store.get(&key(), first.snapshot_id).is_none());
    let next = record(&mut store, tree(&[1], &[]));
    let snapshot = store.get(&key(), next.snapshot_id).unwrap();
    assert_eq!(next.mode, ObservationMode::Full);
    assert!(snapshot.index().index_of(Fingerprint(0)).unwrap() > 1);
    assert!(snapshot.index().index_of(Fingerprint(2)).unwrap() > 1);
}

#[test]
fn snapshot_duplicate_fingerprint_rejected_without_changing_baseline() {
    let mut store = SnapshotStore::default();
    let original = record(&mut store, tree(&[1, 2], &[]));
    let mut duplicate = tree(&[1, 2], &[]);
    let children = &mut duplicate.root.as_mut().unwrap().children;
    children[1].node.fingerprint = children[0].node.fingerprint;
    assert!(matches!(
        store.record(duplicate, &opts(), None),
        Err(SnapshotError::DuplicateFingerprint)
    ));
    assert_eq!(store.latest(&key()).unwrap().id(), original.snapshot_id);
    let next = record(&mut store, tree(&[1, 2, 3], &[]));
    assert_eq!(
        store
            .get(&key(), next.snapshot_id)
            .unwrap()
            .index()
            .index_of(Fingerprint(4)),
        Some(3)
    );
}

#[test]
fn diff_reparented_node_with_stable_fingerprint_is_modified() {
    let mut before_tree = tree(&(0..20).collect::<Vec<_>>(), &[]);
    let before = RenderedState::from_tree(&before_tree, &opts());
    let root = before_tree.root.as_mut().unwrap();
    let child = root.children.pop().unwrap();
    let child_index = child.element_index;
    let parent_index = root.children[0].element_index;
    root.children[0].children.push(child);
    before_tree
        .element_index
        .get_mut(&child_index)
        .unwrap()
        .parent = Some(parent_index);
    let after = RenderedState::from_tree(&before_tree, &opts());
    let diff = Diff::between(&before, &after);
    assert!(diff.changes().iter().any(
        |c| matches!(c,gcu_core::snapshot::Change::Modified{after,..} if after.index==child_index)
    ));
    assert_eq!(diff.apply(&before).unwrap(), after);
}

#[test]
fn snapshot_sensitive_region_text_and_debug_never_store_payloads() {
    let mut input = tree(&[1], &[]);
    let child = &mut input.root.as_mut().unwrap().children[0];
    child.node.name = Some("sensitive-source-value".into());
    child.node.value = Some(NodeValue::Text("sensitive-source-value".into()));
    let fp = child.node.fingerprint;
    input
        .sensitive_regions
        .push(gcu_core::tree::SensitiveRegion {
            fingerprint: fp,
            frame: None,
        });
    let mut store = SnapshotStore::default();
    let result = record(&mut store, input);
    assert!(!result.text.contains("sensitive-source-value"));
    assert!(!format!("{result:?}").contains("sensitive-source-value"));
    let snapshot = store.get(&key(), result.snapshot_id).unwrap();
    assert!(!snapshot
        .rendered()
        .text()
        .contains("sensitive-source-value"));
    assert!(!format!("{snapshot:?}").contains("sensitive-source-value"));
}

#[test]
fn diff_stale_changed_baseline_is_rejected() {
    let before = RenderedState::from_tree(&tree(&[1, 2], &[]), &opts());
    let after = RenderedState::from_tree(&tree(&[1, 2], &[1]), &opts());
    let wrong = RenderedState::from_tree(&tree(&[1, 2], &[1, 2]), &opts());
    assert!(matches!(
        Diff::between(&before, &after).apply(&wrong),
        Err(SnapshotError::StaleDiff)
    ));
}

#[test]
fn snapshot_rendered_full_and_diff_match_reviewed_golden() {
    let mut store = SnapshotStore::default();
    let first = record(&mut store, tree(&(0..9).collect::<Vec<_>>(), &[]));
    insta::assert_snapshot!("snapshot_full", first.text);
    let second = record(&mut store, tree(&[1, 2, 3, 4, 5, 6, 7, 8, 30], &[1]));
    insta::assert_snapshot!("snapshot_diff", second.text);
    let third = record(&mut store, tree(&[1, 2], &[]));
    assert_eq!(third.mode, ObservationMode::FullLargeChange);
    insta::assert_snapshot!("snapshot_full_large_change", third.text);
}

#[test]
fn diff_transport_overflow_falls_back_to_bounded_full_state() {
    let mut before_tree = tree(&(0..50).collect::<Vec<_>>(), &[]);
    for node in &mut before_tree.root.as_mut().unwrap().children {
        node.node.description = Some("a".repeat(250));
    }
    let before = RenderedState::from_tree(&before_tree, &opts());
    for node in &mut before_tree.root.as_mut().unwrap().children {
        node.node.description = Some("b".repeat(250));
    }
    let after = RenderedState::from_tree(&before_tree, &opts());
    let diff = Diff::between(&before, &after);
    assert_eq!(diff.changes().len(), 50);
    assert!(diff.is_large_change());
    assert!(diff.text().contains("[full state: large change]"));
    assert!(diff.text().len() <= gcu_core::render::text::MAX_TEXT_BYTES);
    assert_eq!(diff.apply(&before).unwrap(), after);
}

proptest! {
    #[test]
    fn diff_random_structure_threshold_obeys_thirty(removed in 0usize..9) {
        let ids:Vec<_>=(0..9).collect();
        let before=RenderedState::from_tree(&tree(&ids,&[]),&opts());
        let after=RenderedState::from_tree(&tree(&ids[removed..],&[]),&opts());
        prop_assert_eq!(Diff::between(&before,&after).is_large_change(),removed>3);
    }
}

#[test]
fn snapshot_many_window_high_water_tables_respect_session_memory_cap() {
    let mut store = SnapshotStore::with_memory_limit(4096);
    let mut rejected = false;
    for window in 0..100 {
        let mut options = opts();
        options.window.id = format!("window-{window}");
        let old_count = store.len();
        let old_bytes = store.memory_bytes();
        match store.record(tree(&[1], &[]), &options, None) {
            Ok(_) => assert!(store.memory_bytes() <= 4096),
            Err(SnapshotError::MemoryLimit) => {
                assert_eq!(store.len(), old_count);
                assert_eq!(store.memory_bytes(), old_bytes);
                rejected = true;
                break;
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }
    assert!(rejected);
}
