//! Real bounded SQLite failures; never fill the host disk or change its ACLs.
use super::super::{catalog, hash::sha256_bytes, test_home::IsolatedAppHome};
use super::{ErrorCode, ImportError};
use rusqlite::Connection;

fn cap_at_current_pages(conn: &Connection) -> u32 {
    let pages: u32 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    conn.pragma_update(None, "max_page_count", pages).unwrap();
    let limit: u32 = conn
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(limit, pages);
    pages
}

fn catalog_hash() -> String {
    sha256_bytes(&std::fs::read(catalog::catalog_path()).unwrap())
}

fn assert_intact(conn: &Connection, expected_hash: &str) {
    assert!(conn.is_autocommit());
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(catalog_hash(), expected_hash);
}

fn assert_space_error(error: ImportError) {
    assert_eq!(error.code, ErrorCode::DiskSpaceLow, "{error}");
    assert_eq!(error.retry, "retry");
    assert_eq!(error.scope, "item");
    assert_eq!(
        super::parse_command_err(&error.into_command_err()),
        Some(ErrorCode::DiskSpaceLow)
    );
}

#[test]
fn full_source_insert_preserves_old_catalog_and_retries_without_duplicates() {
    let _home = IsolatedAppHome::acquire("storage-full-source");
    let conn = catalog::open_catalog().unwrap();
    catalog::upsert_source(&conn, "old-source", "generic-file", "synthetic-old").unwrap();
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    // This is stored metadata, not a path opened on the filesystem.
    let large = "synthetic-new-".repeat(32 * 1024);
    let error = catalog::upsert_source(&conn, "new-source", "generic-file", &large).unwrap_err();
    assert_intact(&conn, &before);
    let count: u32 = conn
        .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    assert_intact(&conn, &before);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    for _ in 0..2 {
        catalog::upsert_source(&conn, "new-source", "generic-file", &large).unwrap();
    }
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT installation_id, root_path FROM sources ORDER BY installation_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("new-source".into(), large),
            ("old-source".into(), "synthetic-old".into())
        ]
    );
    assert_space_error(error);
}

fn projection_text(conn: &Connection) -> String {
    let chunks: Vec<(u32, String)> = conn
        .prepare("SELECT chunk_ordinal,text FROM body_search_chunks WHERE import_id='synthetic-import' AND revision_id='synthetic-revision' ORDER BY chunk_ordinal")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut result = String::new();
    for (index, (ordinal, text)) in chunks.into_iter().enumerate() {
        assert_eq!(ordinal as usize, index);
        if index == 0 {
            result.push_str(&text);
        } else {
            let overlap = catalog::BODY_SEARCH_CHUNK_OVERLAP_BYTES;
            assert!(result.ends_with(&text[..overlap]));
            result.push_str(&text[overlap..]);
        }
    }
    result
}

#[test]
fn full_projection_replacement_rolls_back_deletion_and_retries_complete_text() {
    let _home = IsolatedAppHome::acquire("storage-full-projection");
    let conn = catalog::open_catalog().unwrap();
    let mut old =
        catalog::BodySearchProjection::begin(&conn, "synthetic-import", "synthetic-revision")
            .unwrap();
    old.push_text("complete old projection").unwrap();
    old.commit().unwrap();
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    let replacement = "new-projection-".repeat(32 * 1024);
    let error = {
        let mut next =
            catalog::BodySearchProjection::begin(&conn, "synthetic-import", "synthetic-revision")
                .unwrap();
        next.push_text(&replacement).unwrap_err()
    };
    assert_intact(&conn, &before);
    assert_eq!(projection_text(&conn), "complete old projection");
    drop(conn);
    let conn = catalog::open_catalog().unwrap();
    assert_intact(&conn, &before);
    assert_eq!(projection_text(&conn), "complete old projection");
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    let mut next =
        catalog::BodySearchProjection::begin(&conn, "synthetic-import", "synthetic-revision")
            .unwrap();
    next.push_text(&replacement).unwrap();
    next.commit().unwrap();
    assert_eq!(projection_text(&conn), replacement);
    assert_space_error(error);
}

#[test]
fn sqlite_error_codes_not_message_substrings_determine_retryability() {
    for (raw, message, code, retry) in [
        (
            rusqlite::ffi::SQLITE_FULL,
            "custom full message",
            ErrorCode::DiskSpaceLow,
            "retry",
        ),
        (
            rusqlite::ffi::SQLITE_BUSY,
            "custom busy message",
            ErrorCode::SourceBusy,
            "retry",
        ),
        (
            rusqlite::ffi::SQLITE_LOCKED,
            "custom locked message",
            ErrorCode::SourceBusy,
            "retry",
        ),
        (
            rusqlite::ffi::SQLITE_CORRUPT,
            "database or disk is full",
            ErrorCode::SnapshotUnavailable,
            "none",
        ),
        (
            rusqlite::ffi::SQLITE_READONLY,
            "database is locked",
            ErrorCode::SnapshotUnavailable,
            "none",
        ),
    ] {
        let error = super::sqlite_storage_error(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(raw),
            Some(message.into()),
        ));
        assert_eq!(error.code, code);
        assert_eq!(error.retry, retry);
        assert_eq!(error.message, message);
    }
}

#[test]
fn full_write_under_store_lock_preserves_old_draft_and_retry_error() {
    let _home = IsolatedAppHome::acquire("storage-full-draft");
    let conn = catalog::open_catalog().unwrap();
    catalog::write_under_store_lock(&conn, |conn| {
        catalog::persist_draft_json(
            conn,
            "old-draft",
            "old-hash",
            "import",
            "old",
            "created",
            "expires",
        )
    })
    .unwrap();
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    let replacement = "new-draft".repeat(32 * 1024);
    let error = catalog::write_under_store_lock(&conn, |conn| {
        catalog::persist_draft_json(
            conn,
            "new-draft",
            "new-hash",
            "import",
            &replacement,
            "created",
            "expires",
        )
    })
    .unwrap_err();
    assert_intact(&conn, &before);
    assert_eq!(
        catalog::load_draft_json(&conn, "old-draft")
            .unwrap()
            .as_deref(),
        Some("old")
    );
    assert!(catalog::load_draft_json(&conn, "new-draft")
        .unwrap()
        .is_none());
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    catalog::write_under_store_lock(&conn, |conn| {
        catalog::persist_draft_json(
            conn,
            "new-draft",
            "new-hash",
            "import",
            &replacement,
            "created",
            "expires",
        )
    })
    .unwrap();
    assert!(catalog::load_draft_json(&conn, "old-draft")
        .unwrap()
        .is_none());
    assert_eq!(
        catalog::load_draft_json(&conn, "new-draft")
            .unwrap()
            .as_deref(),
        Some(replacement.as_str())
    );
    assert_space_error(error);
}

#[test]
fn full_creation_intent_update_preserves_digest_and_original_input() {
    use super::super::continuation_intent::{load, save, Intent};
    let _home = IsolatedAppHome::acquire("storage-full-intent");
    let conn = catalog::open_catalog().unwrap();
    let mut intent = Intent {
        version: 1,
        draft_id: uuid::Uuid::new_v4().to_string(),
        draft_hash: sha256_bytes(b"synthetic draft"),
        request_id: uuid::Uuid::new_v4().to_string(),
        meta: crate::store::import_continuation::new_meta(
            uuid::Uuid::new_v4().to_string(),
            None,
            Some("Synthetic intent".into()),
            false,
        ),
        workspace: None,
        text: "complete old input".into(),
        published: false,
        cancelled: false,
    };
    save(&conn, &intent).unwrap();
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    intent.text = "synthetic replacement ".repeat(16 * 1024);
    let error = save(&conn, &intent).unwrap_err();
    assert_intact(&conn, &before);
    let old = load(&conn, &intent.request_id).unwrap().unwrap();
    assert_eq!(old.text, "complete old input");
    assert!(!old.published);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    save(&conn, &intent).unwrap();
    let next = load(&conn, &intent.request_id).unwrap().unwrap();
    assert_eq!(next.text, intent.text);
    assert_eq!(next.meta.id, intent.meta.id);
    assert!(!next.published);
    assert_space_error(error);
}

#[test]
fn full_dispatch_receipt_update_keeps_previous_state() {
    use super::super::continuation::{save_continuation_ref, ContinuationSidecar};
    let _home = IsolatedAppHome::acquire("storage-full-continuation");
    let conn = catalog::open_catalog().unwrap();
    let sid = uuid::Uuid::new_v4().to_string();
    let mut side = ContinuationSidecar {
        import_id: "synthetic-import".into(),
        revision_id: "synthetic-revision".into(),
        request_id: uuid::Uuid::new_v4().to_string(),
        ..Default::default()
    };
    save_continuation_ref(&conn, &sid, &side).unwrap();
    let read_json = |conn: &Connection| -> String {
        conn.query_row(
            "SELECT json FROM continuation_refs WHERE app_session_id=?1",
            [&sid],
            |row| row.get(0),
        )
        .unwrap()
    };
    let old_json = read_json(&conn);
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    side.allowed_event_ids = (0..12_000)
        .map(|index| format!("synthetic-event-{index:05}"))
        .collect();
    let error =
        catalog::write_under_store_lock(&conn, |conn| save_continuation_ref(conn, &sid, &side))
            .unwrap_err();
    assert_intact(&conn, &before);
    assert_eq!(read_json(&conn), old_json);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    catalog::write_under_store_lock(&conn, |conn| save_continuation_ref(conn, &sid, &side))
        .unwrap();
    let current: ContinuationSidecar = serde_json::from_str(&read_json(&conn)).unwrap();
    assert_eq!(current.allowed_event_ids, side.allowed_event_ids);
    assert_eq!(current.request_id, side.request_id);
    assert_eq!(current.state, side.state);
    assert_space_error(error);
}

#[test]
fn full_job_update_retains_last_durable_progress() {
    use super::super::jobs::{save_job, JobItem, JobRecord, JobState};
    let _home = IsolatedAppHome::acquire("storage-full-job");
    let conn = catalog::open_catalog().unwrap();
    let mut job = JobRecord {
        job_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        plan_id: uuid::Uuid::new_v4().to_string(),
        plan_hash: sha256_bytes(b"synthetic-plan"),
        state: JobState::Running,
        state_version: 1,
        items: vec![JobItem {
            candidate_id: "synthetic-candidate".into(),
            title: Some("Synthetic session".into()),
            phase: "prepared".into(),
            import_id: None,
            revision_id: None,
            outcome: None,
            error: None,
            mapping_project_id: None,
        }],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 1,
        next_item_cursor: None,
        progress: None,
    };
    save_job(&conn, &job).unwrap();
    let read_json = |conn: &Connection| -> String {
        conn.query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&job.job_id],
            |row| row.get(0),
        )
        .unwrap()
    };
    let old_json = read_json(&conn);
    let pages = cap_at_current_pages(&conn);
    let before = catalog_hash();
    job.items[0].error = Some("synthetic-error ".repeat(32 * 1024));
    job.state_version += 1;
    let error = save_job(&conn, &job).unwrap_err();
    assert_intact(&conn, &before);
    assert_eq!(read_json(&conn), old_json);
    conn.pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    save_job(&conn, &job).unwrap();
    let current: JobRecord = serde_json::from_str(&read_json(&conn)).unwrap();
    assert_eq!(current.state_version, 2);
    assert_eq!(current.items[0].error, job.items[0].error);
    assert_space_error(error);
}
