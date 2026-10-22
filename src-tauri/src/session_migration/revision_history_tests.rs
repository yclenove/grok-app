use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

const IMPORT: &str = "imp:00000000-0000-4000-8000-000000000001";
const OTHER: &str = "imp:00000000-0000-4000-8000-000000000002";

fn revision(n: u128) -> String {
    uuid::Uuid::from_u128(n).to_string()
}

fn add(conn: &rusqlite::Connection, id: &str, n: u128) {
    conn.execute(
        "INSERT INTO revisions(revision_id,import_id,source_snapshot_id,normalizer_version,
           options_hash,outcome,inventory_json,dir_path,created_at)
         VALUES(?1,?2,'snapshot','1','hash','complete','{}','unused','2026-10-15T00:00:00Z')",
        params![revision(n), id],
    )
    .unwrap();
}

fn fixture(count: u128) -> (IsolatedAppHome, rusqlite::Connection) {
    let home = IsolatedAppHome::acquire("revision-history");
    let conn = catalog::open_catalog().unwrap();
    for (id, n) in [(IMPORT, count), (OTHER, 1000)] {
        conn.execute(
            "INSERT INTO entries(import_id,installation_id,provider,native_session_id,title,
               hidden,current_revision,created_at) VALUES(?1,'fixture','generic-file',?1,'history',0,?2,'2026-10-15')",
            params![id, revision(n)],
        ).unwrap();
    }
    for n in 1..=count {
        add(&conn, IMPORT, n);
    }
    add(&conn, OTHER, 1000);
    (home, conn)
}

fn public(cursor: Option<String>, limit: Option<usize>) -> Result<RevisionPage, String> {
    tauri::async_runtime::block_on(crate::commands::session_import_revisions(
        IMPORT.into(),
        cursor,
        limit,
    ))
}

#[test]
fn public_revision_pages_are_bounded_ordered_readonly_and_bidirectional() {
    let (_home, conn) = fixture(55);
    drop(conn);
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    let first = public(None, Some(9999)).unwrap();
    assert_eq!((first.total, first.items.len()), (55, 50));
    assert_eq!(first.current_revision, revision(55));
    assert_eq!(first.items[0].revision_id, revision(55));
    assert_eq!(first.items[49].revision_id, revision(6));
    assert!(first.previous_cursor.is_none());
    let second = public(first.next_cursor.clone(), Some(50)).unwrap();
    assert_eq!(second.items.len(), 5);
    assert_eq!(second.items[0].revision_id, revision(5));
    assert!(second.next_cursor.is_none());
    assert_eq!(second.snapshot, first.snapshot);
    let back = public(second.previous_cursor, Some(50)).unwrap();
    assert_eq!(back.items[0].revision_id, revision(55));
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
    // A metadata row is never a receipt or permission to read missing bodies.
    assert!(crate::session_migration::engine::events_window(
        IMPORT,
        Some(&revision(55)),
        None,
        Some(1),
        None,
        None
    )
    .is_err());
}

#[test]
fn revision_cursor_rejects_other_history_tampering_and_changed_generation() {
    let (_home, conn) = fixture(55);
    let first = page(IMPORT, None, None).unwrap();
    let cursor = first.next_cursor.unwrap();
    assert!(page(OTHER, Some(&cursor), None).is_err());
    assert!(page(IMPORT, Some("50"), None).is_err());
    let tampered = format!("{}x", &cursor[..cursor.len() - 1]);
    assert!(page(IMPORT, Some(&tampered), None).is_err());
    add(&conn, IMPORT, 56);
    assert!(page(IMPORT, Some(&cursor), None).unwrap_err().code == ErrorCode::PlanStale);
    let fresh = page(IMPORT, None, None).unwrap().next_cursor.unwrap();
    conn.execute(
        "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
        params![revision(54), IMPORT],
    )
    .unwrap();
    assert!(page(IMPORT, Some(&fresh), None).unwrap_err().code == ErrorCode::PlanStale);
}

#[test]
fn invalid_missing_and_hidden_histories_fail_without_initializing_a_catalog() {
    let _home = IsolatedAppHome::acquire("revision-history-missing");
    assert!(!catalog::catalog_path().exists());
    assert!(public(None, None)
        .unwrap_err()
        .starts_with("SOURCE_NOT_FOUND"));
    assert!(
        page("native-session", None, None).unwrap_err().code == ErrorCode::ImportedHistoryReadOnly
    );
    assert!(!catalog::catalog_path().exists());
    assert!(!catalog::imports_dir().join(".cursor-hmac-secret").exists());
}

#[test]
fn hidden_missing_current_and_oversized_revision_metadata_are_not_empty_success() {
    let (_home, conn) = fixture(1);
    conn.execute("UPDATE entries SET hidden=1 WHERE import_id=?1", [IMPORT])
        .unwrap();
    assert!(public(None, None)
        .unwrap_err()
        .starts_with("HISTORY_ACCESS_UNAVAILABLE"));
    conn.execute(
        "UPDATE entries SET hidden=0,current_revision=?1 WHERE import_id=?2",
        params![revision(999), IMPORT],
    )
    .unwrap();
    assert!(public(None, None)
        .unwrap_err()
        .starts_with("SNAPSHOT_UNAVAILABLE"));
    conn.execute(
        "UPDATE entries SET current_revision=?1 WHERE import_id=?2",
        params![revision(1), IMPORT],
    )
    .unwrap();
    conn.execute(
        "UPDATE revisions SET created_at=?1 WHERE import_id=?2",
        params!["x".repeat(1_000_000), IMPORT],
    )
    .unwrap();
    assert!(public(None, None)
        .unwrap_err()
        .starts_with("SNAPSHOT_UNAVAILABLE"));
    conn.execute("UPDATE revisions SET created_at='2026-10-15T00:00:00Z',outcome='unknown' WHERE import_id=?1", [IMPORT]).unwrap();
    assert!(public(None, None)
        .unwrap_err()
        .starts_with("SNAPSHOT_UNAVAILABLE"));
}

#[test]
fn newer_catalog_is_not_repaired_by_the_public_revision_endpoint() {
    let (_home, conn) = fixture(1);
    conn.execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
        .unwrap();
    drop(conn);
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    assert!(public(None, None)
        .unwrap_err()
        .contains("IMPORT_SCHEMA_TOO_NEW"));
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
}
