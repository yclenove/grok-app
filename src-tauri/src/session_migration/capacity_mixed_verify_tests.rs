use super::{engine, jobs, Expected, HashMap};
use crate::session_migration::{catalog, types::ImportedEvent};
use std::collections::HashSet;

pub(super) fn published(job_id: &str, expected: &HashMap<String, Expected>) {
    let mut page = engine::job_items_page(job_id, None, Some(2)).unwrap();
    let conn = catalog::open_catalog_readonly().unwrap();
    let mut seen = HashSet::new();
    let mut entries = HashSet::new();
    let mut revisions = HashSet::new();
    let mut providers = HashMap::new();
    let mut scoped = 0;
    loop {
        assert_eq!(page.state, jobs::JobState::Completed, "{page:?}");
        assert_eq!(page.item_count as usize, expected.len());
        assert!(page.items.len() <= 2);
        for item in &page.items {
            assert!(seen.insert(item.candidate_id.clone()));
            let expected = &expected[&item.candidate_id];
            let reference = &expected.reference;
            assert_eq!(item.phase, "committed", "{item:?}");
            assert!(item.error.is_none(), "{item:?}");
            let import = item.import_id.as_deref().unwrap();
            let revision = item.revision_id.as_deref().unwrap();
            assert!(entries.insert(import.to_string()));
            assert!(revisions.insert(revision.to_string()));
            let entry = catalog::get_entry(&conn, import).unwrap().unwrap();
            assert_eq!(entry.current_revision.as_deref(), Some(revision));
            let receipt = jobs::latest_receipt(&conn, job_id, &item.candidate_id)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.phase, "committed");
            assert_eq!(receipt.import_id.as_deref(), Some(import));
            assert_eq!(receipt.revision_id.as_deref(), Some(revision));
            assert_eq!(receipt.source_id, reference.source_id);
            assert_eq!(receipt.source_member, reference.source_member);
            assert_eq!(receipt.provider, reference.candidate.provider.as_str());
            assert_eq!(
                receipt.native_session_id,
                reference.candidate.native_session_id
            );
            assert_eq!(receipt.selection_set.entries.len(), 1);
            assert_eq!(
                receipt.selection_set.entries[0].selection.canonicalized(),
                expected.selection
            );
            *providers.entry(receipt.provider).or_insert(0usize) += 1;

            let auth = jobs::load_published_auth(import, Some(revision), None).unwrap();
            assert_eq!(auth.selection.canonicalized(), expected.selection);
            // Human archive reads remain complete, even when the model scope
            // excludes an event. Force multiple production reader pages.
            let mut cursor = None;
            let mut events = Vec::new();
            loop {
                let (rows, next, total) =
                    engine::events_page(import, Some(revision), cursor.as_deref(), Some(2), None)
                        .unwrap();
                assert_eq!(total, 3);
                assert!(rows.len() <= 2);
                events.extend(
                    rows.into_iter()
                        .map(|row| serde_json::from_value::<ImportedEvent>(row).unwrap()),
                );
                cursor = next;
                if cursor.is_none() {
                    break;
                }
            }
            assert_eq!(events.len(), 3);
            for (index, (event, text)) in events.iter().zip(&expected.texts).enumerate() {
                assert_eq!(&event.searchable_text(), text);
                match event {
                    ImportedEvent::Message { role, .. } => assert_eq!(
                        serde_json::to_value(role).unwrap(),
                        if index == 1 { "assistant" } else { "user" }
                    ),
                    other => panic!("unexpected archived event {other:?}"),
                }
                assert_eq!(
                    auth.selection.permits_event(event, &[]),
                    !expected.scoped || index != 1
                );
            }
            // Build a local draft only: no native continuation or provider send.
            let draft = engine::continue_prepare(import, Some(revision), None, None, None).unwrap();
            assert_eq!(draft.selection.canonicalized(), expected.selection);
            assert!(draft.text.contains(&expected.texts[0]));
            assert!(draft.text.contains(&expected.texts[2]));
            assert_eq!(draft.text.contains(&expected.texts[1]), !expected.scoped);
            assert_eq!(
                draft.readable_event_ids.len(),
                if expected.scoped { 2 } else { 3 }
            );
            scoped += usize::from(expected.scoped);
        }
        let Some(cursor) = page.next_item_cursor else {
            break;
        };
        page = engine::job_items_page(job_id, Some(&cursor), Some(2)).unwrap();
    }
    assert_eq!(seen.len(), expected.len());
    assert_eq!(entries.len(), expected.len());
    assert_eq!(revisions.len(), expected.len());
    assert_eq!(scoped, expected.len() / 2);
    assert_eq!(providers.len(), 3);
    for count in providers.values() {
        assert_eq!(*count, expected.len() / 3);
    }
    let counts: (usize, usize, usize) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM entries), (SELECT COUNT(*) FROM revisions),
             (SELECT COUNT(*) FROM receipts WHERE phase='committed')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (expected.len(), expected.len(), expected.len()));
}
