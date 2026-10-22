use super::{engine, jobs, Expected, HashMap, HashSet, MessageRole, IMPORT, ORDINARY, REVISION};
use crate::session_migration::{catalog, types::ImportedEvent};

fn archive_and_draft(import: &str, revision: &str, expected: &Expected) {
    let mut cursor = None;
    let mut events = Vec::new();
    loop {
        let (page, next, total) =
            engine::events_page(import, Some(revision), cursor.as_deref(), Some(2), None).unwrap();
        assert_eq!(total as usize, expected.messages.len());
        assert!(page.len() <= 2);
        events.extend(
            page.into_iter()
                .map(|row| serde_json::from_value::<ImportedEvent>(row).unwrap()),
        );
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        events
            .iter()
            .map(|event| event.id().to_string())
            .collect::<Vec<_>>(),
        expected.event_ids
    );
    for (index, (event, text)) in events.iter().zip(&expected.messages).enumerate() {
        assert!(matches!(event, ImportedEvent::Message { role, .. }
            if *role == if index == 1 { MessageRole::Assistant } else { MessageRole::User }));
        assert_eq!(&event.searchable_text(), text);
    }
    let auth = jobs::load_published_auth(import, Some(revision), None).unwrap();
    assert_eq!(auth.selection.canonicalized(), expected.selection);
    let draft =
        engine::continue_prepare(import, Some(revision), Some(&expected.leaf), None, None).unwrap();
    assert_eq!(draft.selection.canonicalized(), expected.selection);
    let readable: Vec<_> = expected
        .event_ids
        .iter()
        .enumerate()
        .filter(|(index, _)| !expected.scoped || *index != 1)
        .map(|(_, id)| id.clone())
        .collect();
    assert_eq!(draft.readable_event_ids, readable);
    for (index, text) in expected.messages.iter().enumerate() {
        assert_eq!(draft.text.contains(text), !expected.scoped || index != 1);
    }
}

pub(super) fn published(job_id: &str, expected: &HashMap<String, Expected>) {
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut page = engine::job_items_page(job_id, None, Some(2)).unwrap();
    let (mut seen, mut entries, mut revisions) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut ordinary = 0;
    let mut associated = 0;
    let mut scoped = 0;
    let mut providers = HashMap::new();
    loop {
        assert_eq!(page.state, jobs::JobState::Completed);
        assert_eq!(page.item_count as usize, ORDINARY + 1);
        assert!(page.items.len() <= 2);
        for item in &page.items {
            assert!(seen.insert(item.candidate_id.clone()));
            let expected = &expected[&item.candidate_id];
            let reference = &expected.reference;
            assert_eq!(item.phase, "committed");
            assert!(item.error.is_none());
            let import = item.import_id.as_deref().unwrap();
            let revision = item.revision_id.as_deref().unwrap();
            assert!(entries.insert(import.to_string()));
            assert!(revisions.insert(revision.to_string()));
            let entry = catalog::get_entry(&conn, import).unwrap().unwrap();
            assert_eq!(entry.current_revision.as_deref(), Some(revision));
            assert_eq!(entry.installation_id, reference.source_id);
            assert_eq!(
                entry.native_session_id,
                reference.candidate.native_session_id
            );
            let receipt = jobs::latest_receipt(&conn, job_id, &item.candidate_id)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.phase, "committed");
            assert_eq!(receipt.import_id.as_deref(), Some(import));
            assert_eq!(receipt.revision_id.as_deref(), Some(revision));
            assert_eq!(receipt.source_id, reference.source_id);
            assert_eq!(receipt.source_member, reference.source_member);
            assert_eq!(
                receipt.native_session_id,
                reference.candidate.native_session_id
            );
            assert_eq!(receipt.provider, reference.candidate.provider.as_str());
            assert_eq!(receipt.selection_set.entries.len(), 1);
            let selection = &receipt.selection_set.entries[0];
            assert_eq!(selection.preview_id, expected.preview_id);
            assert_eq!(selection.leaf_id, expected.leaf);
            assert_eq!(selection.selection.canonicalized(), expected.selection);
            if expected.historical {
                assert_eq!(import, IMPORT);
                assert_ne!(revision, REVISION);
                assert!(!receipt.created_entry);
                assert_eq!(receipt.previous_revision.as_deref(), Some(REVISION));
                associated += 1;
            } else {
                assert_ne!(import, IMPORT);
                assert!(receipt.created_entry);
                assert!(receipt.previous_revision.is_none());
                ordinary += 1;
                *providers.entry(receipt.provider).or_insert(0usize) += 1;
            }
            archive_and_draft(import, revision, expected);
            scoped += usize::from(expected.scoped);
        }
        let Some(cursor) = page.next_item_cursor else {
            break;
        };
        page = engine::job_items_page(job_id, Some(&cursor), Some(2)).unwrap();
    }
    assert_eq!(seen.len(), expected.len());
    assert_eq!(entries.len(), ORDINARY + 1);
    assert_eq!(revisions.len(), ORDINARY + 1);
    assert_eq!(
        (ordinary, associated, scoped),
        (ORDINARY, 1, ORDINARY / 2 + 1)
    );
    assert_eq!(providers.len(), 3);
    assert!(providers.values().all(|count| *count == ORDINARY / 3));
    let entries_total: usize = conn
        .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(entries_total, ORDINARY + 1);
    let committed: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE job_id=?1 AND phase='committed'",
            [job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(committed, ORDINARY + 1);
}
