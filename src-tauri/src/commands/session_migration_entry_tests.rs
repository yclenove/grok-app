use super::session_import_entry;
use crate::session_migration::{catalog, test_home::IsolatedAppHome};

fn lookup(id: &str) -> Result<Option<catalog::CatalogEntry>, String> {
    tauri::async_runtime::block_on(session_import_entry(id.to_owned()))
}

#[test]
fn session_migration_entry_reads_exact_off_page_metadata_without_mutating_catalog() {
    let _home = IsolatedAppHome::acquire("entry-metadata-exact");
    let conn = catalog::open_catalog().unwrap();
    for i in 0..120 {
        conn.execute(
            "INSERT INTO entries(import_id,installation_id,provider,native_session_id,title,alias,hidden,current_revision,created_at) VALUES(?1,'fixture','generic-file',?2,?3,?4,0,?5,?6)",
            rusqlite::params![format!("imp:{i:03}"),format!("file-{i:03}.json"),format!("Archive {i:03}"),format!("Alias {i:03}"),format!("revision-{i:03}"),format!("2026-09-30T00:00:{i:03}Z")],
        ).unwrap();
    }
    let page = catalog::list_entries_page(&conn, None, Some(50)).unwrap();
    assert_eq!(page.0.len(), 50);
    assert!(!page.0.iter().any(|entry| entry.import_id == "imp:000"));
    drop(conn);
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    let entry = lookup("imp:000").unwrap().unwrap();
    assert_eq!(entry.import_id, "imp:000");
    assert_eq!(entry.native_session_id, "file-000.json");
    assert_eq!(entry.title, "Archive 000");
    assert_eq!(entry.alias.as_deref(), Some("Alias 000"));
    assert_eq!(entry.current_revision.as_deref(), Some("revision-000"));
    assert!(lookup("imp:missing").unwrap().is_none());
    assert!(lookup("' OR 1=1 --").unwrap().is_none());
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
}

#[test]
fn session_migration_entry_absent_catalog_does_not_create_import_storage() {
    let _home = IsolatedAppHome::acquire("entry-metadata-absent");
    let inventory = || {
        let mut entries: Vec<_> = std::fs::read_dir(catalog::imports_dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        entries.sort();
        entries
    };
    let before = inventory();
    assert!(!catalog::catalog_path().exists());
    assert!(lookup("imp:missing").unwrap().is_none());
    assert!(!catalog::catalog_path().exists());
    assert_eq!(inventory(), before);
}

#[test]
fn session_migration_entry_rejects_newer_schema_without_repairing_it() {
    let _home = IsolatedAppHome::acquire("entry-metadata-newer");
    let conn = catalog::open_catalog().unwrap();
    conn.execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
        .unwrap();
    drop(conn);
    let before = std::fs::read(catalog::catalog_path()).unwrap();
    assert!(lookup("imp:missing")
        .unwrap_err()
        .contains("IMPORT_SCHEMA_TOO_NEW"));
    assert_eq!(std::fs::read(catalog::catalog_path()).unwrap(), before);
}

#[test]
fn session_migration_entry_hidden_direct_links_retain_their_own_metadata() {
    let _home = IsolatedAppHome::acquire("entry-metadata-hidden");
    let conn = catalog::open_catalog().unwrap();
    conn.execute("INSERT INTO entries(import_id,installation_id,provider,native_session_id,title,hidden,created_at) VALUES('imp:hidden','fixture','generic-file','hidden.json','Hidden archive',1,'2026-09-30')", []).unwrap();
    drop(conn);
    let entry = lookup("imp:hidden").unwrap().unwrap();
    assert!(entry.hidden);
    assert_eq!(entry.title, "Hidden archive");
}
