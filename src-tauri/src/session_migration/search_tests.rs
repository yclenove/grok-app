    use crate::session_migration::errors::ErrorCode;
    use crate::session_migration::test_home::IsolatedAppHome;
    use crate::session_migration::types::{ContentBlock, EventBase, MessageRole, TimeOrigin};

    #[path = "search_cursor_home_contract_tests.rs"]
    mod cursor_home_source_fidelity_tests;

    fn msg(id: &str, text: &str) -> ImportedEvent {
        msg_at(id, 0, text)
    }

    fn msg_at(id: &str, ordinal: u32, text: &str) -> ImportedEvent {
        ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        }
    }

    #[test]
    fn cjk_query_hits_without_spaces() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let events = vec![msg("a", "请保持兼容"), msg("b", "hello world")];
        build_index(&dir, &events).unwrap();
        let (hits, _) = search(&dir, "保持兼容", None, Some(10), None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "a");
        let mut allow = std::collections::HashSet::new();
        allow.insert("b".into());
        let (hits, _) = search(&dir, "保持兼容", None, Some(10), Some(&allow)).unwrap();
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_search_and_read_exclude_reasoning_and_system_fields() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let mut mixed = msg("mixed", "public text");
        if let ImportedEvent::Message { blocks, .. } = &mut mixed {
            blocks.push(ContentBlock::VisibleReasoning {
                text: "private-reasoning".into(),
            });
        }
        let mut system = msg("system", "private-system");
        if let ImportedEvent::Message { role, .. } = &mut system {
            *role = MessageRole::System;
        }
        build_index(&dir, &[mixed, system]).unwrap();
        let allow = ["mixed".to_string(), "system".to_string()]
            .into_iter()
            .collect();
        assert_eq!(
            search(&dir, "private", None, None, None).unwrap().0.len(),
            2
        );
        assert!(search(&dir, "private", None, None, Some(&allow))
            .unwrap()
            .0
            .is_empty());
        let (text, cursor) = read_indexed_event_text(&dir, "mixed", None, None, &allow).unwrap();
        assert_eq!(text, "public text");
        assert!(cursor.is_none());
        assert!(read_indexed_event_text(&dir, "system", None, None, &allow)
            .unwrap()
            .0
            .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selection_search_and_read_hide_denied_reasoning_and_tool_input() {
        use crate::session_migration::selection::AllowedSelection;
        use crate::session_migration::types::ToolInput;
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let mut mixed = msg("mixed", "public text");
        if let ImportedEvent::Message { blocks, .. } = &mut mixed {
            blocks.push(ContentBlock::VisibleReasoning {
                text: "think-secret".into(),
            });
        }
        let tool = ImportedEvent::ToolCall {
            base: EventBase {
                id: "tool".into(),
                ordinal: 1,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            call_id: "c1".into(),
            name: "apply_patch".into(),
            input: ToolInput::Json {
                value: serde_json::json!({"path": "secret.diff"}),
            },
            parent_message_id: None,
        };
        build_index(&dir, &[mixed, tool]).unwrap();
        let mut sel = AllowedSelection::for_events(vec!["mixed".into(), "tool".into()]);
        sel.fields.tool_call_input = false;
        let (hits, _) = search_with_selection(&dir, "think-secret", None, None, &sel, &[]).unwrap();
        assert!(hits.is_empty());
        let (hits, _) = search_with_selection(&dir, "secret.diff", None, None, &sel, &[]).unwrap();
        assert!(hits.is_empty());
        let (hits, _) = search_with_selection(&dir, "public", None, None, &sel, &[]).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].snippet.contains("think-secret"));
        let (text, _) = read_with_selection(&dir, "mixed", None, None, &sel, &[]).unwrap();
        assert_eq!(text, "public text");
        assert!(!text.contains("think-secret"));
        let (text, _) = read_with_selection(&dir, "tool", None, None, &sel, &[]).unwrap();
        assert_eq!(text, "apply_patch");
        assert!(!text.contains("secret.diff"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn literal_search_preserves_phrase_path_and_cursor_scope() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        build_index(
            &dir,
            &[
                msg("a", "KEEP src/a.b exactly"),
                msg("b", "keep src/a.b too"),
                msg("c", "keep src a b"),
            ],
        )
        .unwrap();
        let (hits, cursor) = search(&dir, "keep src/a.b", None, Some(1), None).unwrap();
        assert_eq!(hits[0].event_id, "a");
        let (rest, next) = search(&dir, "keep src/a.b", cursor.as_deref(), Some(1), None).unwrap();
        assert_eq!(rest[0].event_id, "b");
        assert!(next.is_none());
        assert!(search(&dir, "different", cursor.as_deref(), Some(1), None).is_err());
        assert!(search(&dir, "src/a.b keep", None, None, None)
            .unwrap()
            .0
            .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn search_and_read_cursors_reject_bare_tamper_cross_owner_and_stale() {
        use crate::session_migration::cursor::{encode_bound_cursor, CursorScope};
        use crate::session_migration::selection::AllowedSelection;
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        build_index(
            &dir,
            &[
                msg("a", "alpha one unique-token"),
                msg("b", "alpha two unique-token"),
                msg("c", "other"),
            ],
        )
        .unwrap();
        let (hits, next) = search(&dir, "alpha", None, Some(1), None).unwrap();
        assert_eq!(hits[0].event_id, "a");
        let next = next.expect("search must page");
        assert!(
            next.parse::<u32>().is_err(),
            "search cursor must not be a bare number"
        );
        let (rest, _) = search(&dir, "alpha", Some(&next), Some(1), None).unwrap();
        assert_eq!(rest[0].event_id, "b");
        assert!(search(&dir, "alpha", Some("2"), Some(1), None).is_err());
        let other = encode_bound_cursor(
            CursorScope {
                owner: "other-rev",
                snapshot: "x",
                leaf: "",
                filter: "search",
            },
            0,
            Some("a".into()),
        )
        .unwrap();
        assert!(search(&dir, "alpha", Some(&other), Some(1), None).is_err());
        let stale = crate::session_migration::cursor::tamper_cursor_field(
            &next,
            "exp",
            serde_json::json!(1),
        );
        assert!(search(&dir, "alpha", Some(&stale), Some(1), None).is_err());
        let tampered = crate::session_migration::cursor::tamper_cursor_field(
            &next,
            "snapshot",
            serde_json::json!("tampered"),
        );
        assert!(search(&dir, "alpha", Some(&tampered), Some(1), None).is_err());
        let sel = AllowedSelection::for_events(vec!["a".into(), "b".into()]);
        let (_text, read_next) = read_with_selection(&dir, "a", None, Some(8), &sel, &[]).unwrap();
        if let Some(raw) = read_next {
            assert!(raw.parse::<u32>().is_err());
            assert!(read_with_selection(&dir, "a", Some("8"), Some(8), &sel, &[]).is_err());
            let expired = crate::session_migration::cursor::tamper_cursor_field(
                &raw,
                "exp",
                serde_json::json!(1),
            );
            assert!(read_with_selection(&dir, "a", Some(&expired), Some(8), &sel, &[]).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bounded_read_reassembles_unicode_and_rejects_foreign_cursor() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let text = "你好🌏 — 保持兼容";
        build_index(&dir, &[msg("a", text), msg("b", "other")]).unwrap();
        let allow = ["a".to_string(), "b".to_string()].into_iter().collect();
        let (first, mut next) = read_indexed_event_text(&dir, "a", None, Some(7), &allow).unwrap();
        assert!(read_indexed_event_text(&dir, "b", next.as_deref(), Some(7), &allow).is_err());
        let mut full = first;
        while let Some(cursor) = next {
            let (part, cursor) =
                read_indexed_event_text(&dir, "a", Some(&cursor), Some(7), &allow).unwrap();
            assert!(part.len() <= 7);
            full.push_str(&part);
            next = cursor;
        }
        assert_eq!(full, text);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn read_indexed_event_text_cursor_offset_is_bytes_not_chars() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let text = "你好🌏 — 保持兼容";
        build_index(&dir, &[msg("a", text)]).unwrap();
        let allow = ["a".to_string()].into_iter().collect();
        let (first, next) = read_indexed_event_text(&dir, "a", None, Some(7), &allow).unwrap();
        assert!(!first.is_empty());
        assert!(first.len() <= 7);
        assert!(text.starts_with(&first));
        let next = next.expect("7-byte window must leave a remainder");
        let payload = next.split('.').nth(1).expect("cursor payload");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("cursor b64");
        let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let offset = obj["offset"].as_u64().expect("offset") as usize;
        assert_eq!(
            offset,
            first.len(),
            "signed read cursor must store consumed bytes ({}) not char count",
            first.chars().count()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dual_invalid_live_and_backup_must_not_block_rebuild() {
        let dir = std::env::temp_dir().join(format!("idx-dual-bad-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        let live = index_path(&dir);
        let backup = index_backup_path(&live);
        std::fs::copy(&live, &backup).unwrap();
        std::fs::write(&live, b"not-a-sqlite").unwrap();
        std::fs::write(&backup, b"also-not-a-sqlite").unwrap();
        write_archive(&dir, &[msg("new", "archive-new")]);
        rebuild_index(&dir, &|| false).expect(
            "live+backup both invalid must fail closed into a rebuildable new index, not refuse replace",
        );
        assert_complete_sqlite_index(&index_path(&dir), "new");
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn write_archive(dir: &std::path::Path, events: &[ImportedEvent]) {
        use std::io::Write;
        std::fs::create_dir_all(dir).unwrap();
        let mut file = std::fs::File::create(dir.join("events.jsonl")).unwrap();
        for ev in events {
            serde_json::to_writer(&mut file, ev).unwrap();
            file.write_all(b"\n").unwrap();
        }
    }

    #[test]
    fn missing_index_rebuilds_from_archive() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        let events = vec![msg("keep", "archive-truth")];
        write_archive(&dir, &events);
        build_index(&dir, &events).unwrap();
        std::fs::remove_file(index_path(&dir)).unwrap();
        let (page, _, total) = page_indexed_events(&dir, None, Some(10)).unwrap();
        assert_eq!(total, 1);
        assert_eq!(page[0].id(), "keep");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_existing_index_fails_closed() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        let events = vec![msg("keep", "archive-truth")];
        write_archive(&dir, &events);
        build_index(&dir, &events).unwrap();
        std::fs::write(index_path(&dir), b"not-a-sqlite").unwrap();
        let err = page_indexed_events(&dir, None, Some(10)).unwrap_err();
        assert_ne!(err.code, ErrorCode::SourceNotFound);
        assert!(
            matches!(
                err.code,
                ErrorCode::HistoryAccessUnavailable | ErrorCode::SnapshotUnavailable
            ),
            "corrupt index must fail closed, got {:?}",
            err.code
        );
        let leftover = std::fs::read(index_path(&dir)).unwrap();
        assert_eq!(
            leftover, b"not-a-sqlite",
            "corrupt live index must not be silently overwritten"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_archive_is_not_an_empty_page() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("events.jsonl"), b"NOT-AN-EVENT\n").unwrap();
        let err = page_indexed_events(&dir, None, Some(10)).unwrap_err();
        assert_eq!(err.code, ErrorCode::SnapshotUnavailable);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_event_json_is_not_skipped() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        write_archive(&dir, &[msg("ok", "visible")]);
        build_index(&dir, &[msg("ok", "visible")]).unwrap();
        std::fs::write(dir.join("events.jsonl"), b"{\"broken\":true}\n").unwrap();
        let err = page_indexed_events(&dir, None, Some(10)).unwrap_err();
        assert_eq!(err.code, ErrorCode::SnapshotUnavailable);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_index_schema_is_rejected_without_rewrite() {
        let dir = std::env::temp_dir().join(format!("idx-new-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("a", "keep")]);
        build_index(&dir, &[msg("a", "keep")]).unwrap();
        let conn = rusqlite::Connection::open(index_path(&dir)).unwrap();
        conn.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(conn);
        let before = std::fs::read(index_path(&dir)).unwrap();
        assert_eq!(
            ensure_index(&dir).unwrap_err().code,
            ErrorCode::ImportSchemaTooNew
        );
        assert_eq!(before, std::fs::read(index_path(&dir)).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_index_schema_rebuilds() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        write_archive(&dir, &[msg("a", "from-archive")]);
        build_index(&dir, &[msg("a", "from-archive")]).unwrap();
        let conn = rusqlite::Connection::open(index_path(&dir)).unwrap();
        conn.execute_batch("PRAGMA user_version=1;").unwrap();
        drop(conn);
        let (page, _, _) = page_indexed_events(&dir, None, Some(10)).unwrap();
        assert_eq!(page[0].id(), "a");
        let ver = index_user_version(&index_path(&dir)).unwrap().unwrap();
        assert_eq!(ver, INDEX_SCHEMA_VERSION);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn page_cursor_rejects_another_revision() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let a = home.path.join("revision-a");
        let b = home.path.join("revision-b");
        write_archive(&a, &[msg("a1", "one"), msg("a2", "two")]);
        write_archive(&b, &[msg("b1", "other")]);
        build_index(&a, &[msg("a1", "one"), msg("a2", "two")]).unwrap();
        build_index(&b, &[msg("b1", "other")]).unwrap();
        let (_, next, _) = page_indexed_events(&a, None, Some(1)).unwrap();
        let next = next.expect("more on a");
        let err = page_indexed_events(&b, Some(&next), Some(1)).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        std::fs::remove_dir_all(a).unwrap();
        std::fs::remove_dir_all(b).unwrap();
    }

    #[test]
    fn rebuild_cancel_leaves_existing_index() {
        let dir = std::env::temp_dir().join(format!("idx-cancel-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("a", "keep")]);
        build_index(&dir, &[msg("a", "keep")]).unwrap();
        let before = std::fs::read(index_path(&dir)).unwrap();
        let err = rebuild_index(&dir, &|| true).unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        assert_eq!(before, std::fs::read(index_path(&dir)).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn event_page_stops_at_100_and_does_not_repeat() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        let events: Vec<_> = (0..120).map(|i| msg_at(&format!("e{i}"), i, "x")).collect();
        write_archive(&dir, &events);
        build_index(&dir, &events).unwrap();
        let (a, next, total) = page_indexed_events(&dir, None, Some(100)).unwrap();
        assert_eq!(total, 120);
        assert_eq!(a.len(), 100);
        let next = next.expect("more pages");
        let (b, next2, _) = page_indexed_events(&dir, Some(&next), Some(100)).unwrap();
        assert_eq!(b.len(), 20);
        assert!(next2.is_none());
        let ids: std::collections::HashSet<_> =
            a.iter().chain(&b).map(|e| e.id().to_string()).collect();
        assert_eq!(ids.len(), 120);
        let err =
            page_indexed_events(&dir, Some(&encode_page_cursor(&dir, 120).unwrap()), Some(10))
                .unwrap_err();
        assert_eq!(err.code, ErrorCode::PlanStale);
        let bare = page_indexed_events(&dir, Some("50"), Some(10)).unwrap_err();
        assert_eq!(bare.code, ErrorCode::PlanStale);
        assert!(bare.message.contains("bare numeric"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn event_page_byte_cap_keeps_unicode_and_oversized_first() {
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        let huge = "你".repeat(super::super::budget::READ_PAGE_TEXT_BYTES / 3 + 8);
        let events = vec![msg_at("h", 0, &huge), msg_at("t", 1, "tail-event")];
        write_archive(&dir, &events);
        build_index(&dir, &events).unwrap();
        let (first, next, _) = page_indexed_events(&dir, None, Some(100)).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id(), "h");
        let next = next.expect("oversized event must not swallow the rest");
        let (second, end, _) = page_indexed_events(&dir, Some(&next), Some(100)).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id(), "t");
        assert!(end.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn assert_complete_sqlite_index(path: &std::path::Path, expected_id: &str) {
        assert!(path.is_file(), "index missing: {}", path.display());
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() > 64, "index truncated");
        assert_ne!(&bytes, b"not-a-sqlite");
        let conn = rusqlite::Connection::open(path).unwrap();
        let id: String = conn
            .query_row(
                "SELECT event_id FROM events ORDER BY ordinal ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(id, expected_id);
    }

    #[test]
    fn read_with_selection_reassembles_unicode_by_byte_offset() {
        use crate::session_migration::selection::AllowedSelection;
        let home = IsolatedAppHome::acquire("search-cursor");
        let dir = home.path.join("revision");
        std::fs::create_dir_all(&dir).unwrap();
        let text = "你好🌏 — 保持兼容";
        let combining = "cafe\u{0301} au lait";
        build_index(
            &dir,
            &[
                msg("cjk", text),
                msg("combining", combining),
                msg("other", "z"),
            ],
        )
        .unwrap();
        let sel = AllowedSelection::for_events(vec!["cjk".into(), "combining".into()]);

        let mut full = String::new();
        let mut cursor = None;
        let mut pages = 0u32;
        loop {
            pages += 1;
            assert!(pages < 40, "read_with_selection must not loop");
            let (part, next) =
                read_with_selection(&dir, "cjk", cursor.as_deref(), Some(7), &sel, &[]).unwrap();
            assert!(part.len() <= 7, "page exceeded byte budget: {}", part.len());
            assert!(
                std::str::from_utf8(part.as_bytes()).is_ok(),
                "page is not valid UTF-8"
            );
            full.push_str(&part);
            match next {
                None => break,
                Some(next) => cursor = Some(next),
            }
        }
        assert_eq!(full, text);
        assert!(pages > 1, "tiny cap must span multiple pages");

        let mut full = String::new();
        let mut cursor = None;
        loop {
            let (part, next) =
                read_with_selection(&dir, "combining", cursor.as_deref(), Some(4), &sel, &[])
                    .unwrap();
            assert!(part.len() <= 4);
            full.push_str(&part);
            match next {
                None => break,
                Some(next) => cursor = Some(next),
            }
        }
        assert_eq!(full, combining);

        let err = read_with_selection(&dir, "cjk", None, Some(1), &sel, &[]).unwrap_err();
        assert!(
            err.message.contains("cannot fit one character"),
            "{}",
            err.message
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn index_replace_keeps_complete_old_or_complete_new() {
        let dir = std::env::temp_dir().join(format!("idx-swap-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        assert_complete_sqlite_index(&index_path(&dir), "old");
        let old_bytes = std::fs::read(index_path(&dir)).unwrap();

        write_archive(&dir, &[msg("new", "archive-new")]);

        set_index_swap_fault(Some("fail_rename_backup"));
        let err = rebuild_index(&dir, &|| false).unwrap_err();
        assert!(err.message.contains("injected backup rename failure"));
        assert_eq!(std::fs::read(index_path(&dir)).unwrap(), old_bytes);
        assert_complete_sqlite_index(&index_path(&dir), "old");

        set_index_swap_fault(Some("fail_rename_tmp"));
        let err = rebuild_index(&dir, &|| false).unwrap_err();
        assert!(err.message.contains("injected temp rename failure"));
        assert_complete_sqlite_index(&index_path(&dir), "old");
        assert!(
            !index_backup_path(&index_path(&dir)).exists() || {
                // rollback may leave no backup
                true
            }
        );

        let backup = index_backup_path(&index_path(&dir));
        std::fs::write(&backup, &old_bytes).unwrap();
        set_index_swap_fault(None);
        rebuild_index(&dir, &|| false)
            .expect("leftover live+backup must be recovered instead of blocking rebuild");
        assert_complete_sqlite_index(&index_path(&dir), "new");
        assert!(!index_backup_path(&index_path(&dir)).exists());

        set_index_swap_fault(Some("interrupt_after_backup"));
        assert!(rebuild_index(&dir, &|| false).is_err());
        let live = index_path(&dir);
        let tmp = dir.join("event-index.sqlite.tmp");
        let backup = index_backup_path(&live);
        assert!(
            live.exists() || backup.exists() || tmp.exists(),
            "interrupt must leave a complete old or complete new artifact"
        );
        set_index_swap_fault(None);
        rebuild_index(&dir, &|| false).unwrap();
        assert_complete_sqlite_index(&live, "new");
        assert!(!backup.exists());

        let reader = rusqlite::Connection::open(&live).unwrap();
        write_archive(&dir, &[msg("newer", "archive-newer")]);
        let swapped = rebuild_index(&dir, &|| false);
        let path_still = index_path(&dir);
        assert_complete_sqlite_index(&path_still, if swapped.is_ok() { "newer" } else { "new" });
        let _ = reader.query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0));
        drop(reader);

        set_index_swap_fault(Some("sharing_tmp"));
        write_archive(&dir, &[msg("share", "archive-share")]);
        rebuild_index(&dir, &|| false).unwrap();
        assert_complete_sqlite_index(&index_path(&dir), "share");
        set_index_swap_fault(None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn leftover_live_and_backup_must_not_block_rebuild() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("idx-live-bak");
        let dir = std::env::temp_dir().join(format!("idx-live-bak-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        let live = index_path(&dir);
        let backup = index_backup_path(&live);
        std::fs::copy(&live, &backup).unwrap();
        assert!(live.exists() && backup.exists());
        rebuild_index(&dir, &|| false).expect(
            "startup recovery must clear a leftover live+backup pair so rebuild is not stuck",
        );
        assert_complete_sqlite_index(&index_path(&dir), "old");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fail_sync_after_live_rename_keeps_backup() {
        let dir = std::env::temp_dir().join(format!("idx-sync-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        write_archive(&dir, &[msg("new", "archive-new")]);
        set_index_swap_fault(Some("fail_sync_live"));
        let err = rebuild_index(&dir, &|| false).unwrap_err();
        assert!(
            err.message.contains("injected live sync failure"),
            "{}",
            err.message
        );
        let live = index_path(&dir);
        let backup = index_backup_path(&live);
        assert!(
            backup.exists(),
            "backup must survive until the new live file is synced and verified"
        );
        set_index_swap_fault(None);
        rebuild_index(&dir, &|| false).unwrap();
        assert_complete_sqlite_index(&live, "new");
        assert!(!backup.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_live_with_valid_backup_restores_backup() {
        let dir = std::env::temp_dir().join(format!("idx-corrupt-live-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        let live = index_path(&dir);
        let backup = index_backup_path(&live);
        std::fs::copy(&live, &backup).unwrap();
        std::fs::write(&live, b"not-a-sqlite").unwrap();
        recover_index_files(&dir).expect("valid backup must replace a corrupt live index");
        assert_complete_sqlite_index(&index_path(&dir), "old");
        assert!(!index_backup_path(&index_path(&dir)).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore]
    fn index_swap_child_pauses_after_backup() {
        let dir = std::path::PathBuf::from(
            std::env::var("GROK_INDEX_CRASH_DIR").expect("GROK_INDEX_CRASH_DIR"),
        );
        set_index_swap_fault(Some("pause_after_backup"));
        let _ = rebuild_index(&dir, &|| false);
    }

    #[test]
    fn killed_child_after_backup_recovers_complete_old_or_new() {
        let dir = std::env::temp_dir().join(format!("idx-kill-{}", uuid::Uuid::new_v4()));
        write_archive(&dir, &[msg("old", "archive-old")]);
        rebuild_index(&dir, &|| false).unwrap();
        write_archive(&dir, &[msg("new", "archive-new")]);
        let exe = std::env::current_exe().expect("test exe");
        let mut child = std::process::Command::new(&exe)
            .args([
                "--ignored",
                "--test-threads=1",
                "--nocapture",
                "index_swap_child_pauses_after_backup",
            ])
            .env("GROK_INDEX_CRASH_DIR", &dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn index crash child");
        let live = index_path(&dir);
        let backup = index_backup_path(&live);
        let started = std::time::Instant::now();
        while !backup.exists() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "child never created a backup"
            );
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_millis(100));
        set_index_swap_fault(None);
        let mut last_err = None;
        for attempt in 0..16 {
            match rebuild_index(&dir, &|| false) {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(error) => {
                    last_err = Some(error);
                    std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
                }
            }
        }
        if let Some(error) = last_err {
            panic!("kill/restart must leave a rebuildable index: {error}");
        }
        let conn = rusqlite::Connection::open(index_path(&dir)).unwrap();
        let id: String = conn
            .query_row(
                "SELECT event_id FROM events ORDER BY ordinal ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            id == "old" || id == "new",
            "recovered index must be complete old or complete new, got {id}"
        );
        assert!(!index_backup_path(&index_path(&dir)).exists());
        drop(conn);
        for attempt in 0..16 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => break,
                Err(_error) if attempt + 1 < 16 => {
                    std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
                }
                Err(error) => panic!("cleanup {dir:?}: {error}"),
            }
        }
    }

    #[test]
    fn selection_search_reaches_a_late_hit_after_a_non_matching_prefix() {
        use crate::session_migration::selection::AllowedSelection;
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("idx-sel-late");
        let dir = std::env::temp_dir().join(format!("idx-sel-late-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut events = Vec::new();
        for i in 0..200 {
            events.push(msg_at(
                &format!("e{i}"),
                i,
                &format!("prefix-noise-{i}"),
            ));
        }
        events.push(msg_at("late", 200, "unique-late-needle"));
        build_index(&dir, &events).unwrap();
        let allowed: Vec<String> = (0..200)
            .map(|i| format!("e{i}"))
            .chain(std::iter::once("late".into()))
            .collect();
        let sel = AllowedSelection::for_events(allowed);
        let mut cursor = None;
        let mut found = false;
        let mut pages = 0u32;
        loop {
            pages += 1;
            assert!(pages < 40, "selection search must not loop");
            let (hits, next) =
                search_with_selection(&dir, "unique-late-needle", cursor.as_deref(), Some(20), &sel, &[])
                    .unwrap();
            if hits.iter().any(|hit| hit.event_id == "late") {
                found = true;
                break;
            }
            match next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert!(
            found,
            "a matching event after 200 non-matching selected events must still be reachable"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selection_search_zero_hit_page_is_not_terminal_while_events_remain() {
        use crate::session_migration::selection::AllowedSelection;
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("idx-sel-empty");
        let dir = std::env::temp_dir().join(format!("idx-sel-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut events = Vec::new();
        for i in 0..200 {
            events.push(msg_at(&format!("n{i}"), i, &format!("noise-{i}")));
        }
        events.push(msg_at("hit", 200, "keep-searching-needle"));
        build_index(&dir, &events).unwrap();
        let allowed: Vec<String> = (0..200)
            .map(|i| format!("n{i}"))
            .chain(std::iter::once("hit".into()))
            .collect();
        let sel = AllowedSelection::for_events(allowed);
        let (hits, next) =
            search_with_selection(&dir, "keep-searching-needle", None, Some(20), &sel, &[]).unwrap();
        assert!(hits.is_empty(), "first window should not yet contain the hit");
        assert!(
            next.is_some(),
            "0-hit page must carry a next cursor while allowed events remain unscanned"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn branch_search_rejects_sibling_hits_and_cross_leaf_cursor_replay() {
        use crate::session_migration::types::{RelationConfidence, RelationKind};
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire("idx-branch");
        let dir = std::env::temp_dir().join(format!("idx-branch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        build_index(
            &dir,
            &[
                msg_at("root", 0, "needle root"),
                msg_at("leaf-a", 1, "needle alpha-only"),
                msg_at("leaf-b", 2, "needle beta-only"),
            ],
        )
        .unwrap();
        let relations = vec![
            EventRelation {
                kind: RelationKind::Parent,
                from: "leaf-a".into(),
                to: "root".into(),
                confidence: RelationConfidence::Explicit,
            },
            EventRelation {
                kind: RelationKind::Parent,
                from: "leaf-b".into(),
                to: "root".into(),
                confidence: RelationConfidence::Explicit,
            },
        ];
        let (alpha, _) = search_branch(
            &dir,
            "alpha-only",
            None,
            Some(10),
            "leaf-a",
            &relations,
        )
        .unwrap();
        assert_eq!(alpha.iter().map(|hit| hit.event_id.as_str()).collect::<Vec<_>>(), ["leaf-a"]);
        assert!(search_branch(
            &dir,
            "beta-only",
            None,
            Some(10),
            "leaf-a",
            &relations,
        )
        .unwrap()
        .0
        .is_empty());
        let (_, cursor) = search_branch(
            &dir,
            "needle",
            None,
            Some(1),
            "leaf-a",
            &relations,
        )
        .unwrap();
        let cursor = cursor.expect("branch has another hit");
        let error = search_branch(
            &dir,
            "needle",
            Some(&cursor),
            Some(1),
            "leaf-b",
            &relations,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn branch_search_window_round_trips_and_rejects_cursor_replay() {
        let _home = crate::session_migration::test_home::IsolatedAppHome::acquire(
            "idx-branch-window",
        );
        let dir = std::env::temp_dir().join(format!(
            "idx-branch-window-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let events: Vec<_> = (0..55)
            .map(|i| msg_at(&format!("e{i:02}"), i, &format!("needle body {i}")))
            .collect();
        build_index(&dir, &events).unwrap();
        let ids: Vec<_> = events.iter().map(|event| event.id().to_owned()).collect();

        let first = search_branch_window(
            &dir,
            "needle",
            None,
            Some(20),
            "e54",
            &ids,
            SearchDirection::Forward,
        )
        .unwrap();
        assert_eq!(first.hits.len(), 20);
        assert_eq!(first.hits.first().unwrap().event_id, "e00");
        assert_eq!(first.hits.last().unwrap().event_id, "e19");
        assert!(first.previous_cursor.is_none());
        let first_next = first.next_cursor.expect("second result page");

        let second = search_branch_window(
            &dir,
            "needle",
            Some(&first_next),
            Some(20),
            "e54",
            &ids,
            SearchDirection::Forward,
        )
        .unwrap();
        assert_eq!(second.hits.len(), 20);
        assert_eq!(second.hits.first().unwrap().event_id, "e20");
        assert_eq!(second.hits.last().unwrap().event_id, "e39");
        let second_previous = second.previous_cursor.clone().expect("first result page");
        assert!(second.next_cursor.is_some());

        let first_again = search_branch_window(
            &dir,
            "needle",
            Some(&second_previous),
            Some(20),
            "e54",
            &ids,
            SearchDirection::Backward,
        )
        .unwrap();
        assert_eq!(first_again.hits.len(), 20);
        assert_eq!(first_again.hits.first().unwrap().event_id, "e00");
        assert_eq!(first_again.hits.last().unwrap().event_id, "e19");
        assert!(first_again.previous_cursor.is_none());
        assert!(first_again.next_cursor.is_some());

        let third = search_branch_window(
            &dir,
            "needle",
            second.next_cursor.as_deref(),
            Some(20),
            "e54",
            &ids,
            SearchDirection::Forward,
        )
        .unwrap();
        assert_eq!(third.hits.len(), 15);
        assert_eq!(third.hits.first().unwrap().event_id, "e40");
        assert_eq!(third.hits.last().unwrap().event_id, "e54");
        assert!(third.next_cursor.is_none());
        assert!(third.previous_cursor.is_some());

        assert!(search_branch_window(
            &dir,
            "needle",
            Some(&first_next),
            Some(20),
            "e54",
            &ids,
            SearchDirection::Backward,
        )
        .is_err());
        assert!(search_branch_window(
            &dir,
            "other-query",
            Some(&second_previous),
            Some(20),
            "e54",
            &ids,
            SearchDirection::Backward,
        )
        .is_err());
        assert!(search_branch_window(
            &dir,
            "needle",
            Some(&second_previous),
            Some(20),
            "other-leaf",
            &ids,
            SearchDirection::Backward,
        )
        .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
