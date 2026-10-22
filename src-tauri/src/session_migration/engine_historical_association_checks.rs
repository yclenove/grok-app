//! Fault checks run only on the isolated clone of the pinned historical seed.
//! No invented positive historical archives or canonical entries are used.
use super::*;
use crate::session_migration::{
    catalog,
    errors::ImportError,
    identity_association::{self, AssociationRequest, AssociationReview},
};
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection,
};

fn catalog_digest() -> String {
    let conn = catalog::open_catalog_readonly().unwrap();
    let snapshot = conn.unchecked_transaction().unwrap();
    let schema: Vec<(String, String, String, Option<String>)> = snapshot
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut tables = Vec::new();
    for (_, name, _, _) in schema.iter().filter(|row| row.0 == "table") {
        let escaped = name.replace('"', "\"\"");
        let mut statement = snapshot
            .prepare(&format!("SELECT rowid,* FROM \"{escaped}\""))
            .or_else(|_| snapshot.prepare(&format!("SELECT * FROM \"{escaped}\"")))
            .unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        let mut contents = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let values: Vec<String> = (0..columns)
                .map(|column| match row.get_ref(column).unwrap() {
                    ValueRef::Null => "null".into(),
                    ValueRef::Integer(value) => format!("integer:{value}"),
                    ValueRef::Real(value) => format!("real:{}", value.to_bits()),
                    ValueRef::Text(value) => format!("text:{}", hash::sha256_bytes(value)),
                    ValueRef::Blob(value) => format!("blob:{}", hash::sha256_bytes(value)),
                })
                .collect();
            contents.push(values);
        }
        contents.sort();
        tables.push((name, contents));
    }
    hash::options_hash(&json!({"schema":schema,"tables":tables}))
}

pub(super) fn inspect_unchanged(
    request: &AssociationRequest,
) -> Result<AssociationReview, ImportError> {
    checked_unchanged(request, || identity_association::inspect(request))
}

pub(super) fn command_unchanged(request: &AssociationRequest) -> Result<AssociationReview, String> {
    checked_unchanged(request, || {
        tauri::async_runtime::block_on(
            crate::commands::session_import_identity_association_inspect(request.clone()),
        )
    })
}

pub(super) fn checked_unchanged<T, E>(
    request: &AssociationRequest,
    inspect: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let before = catalog_digest();
    let preview = jobs::find_preview_dir(&request.preview.preview_id).unwrap();
    let lease_before = preview
        .as_ref()
        .map(|path| hash::sha256_file(&path.join("lease.json")).unwrap());
    let result = inspect();
    assert_eq!(
        catalog_digest(),
        before,
        "inspection must not alter catalog schema or rows"
    );
    assert_eq!(
        preview
            .as_ref()
            .map(|path| hash::sha256_file(&path.join("lease.json")).unwrap()),
        lease_before,
        "inspection must not change or retain the preview lease"
    );
    result
}

fn rejected(request: &AssociationRequest, code: ErrorCode) {
    assert_eq!(inspect_unchanged(request).unwrap_err().code, code);
}

type RowFault<'a> = (&'a str, &'a str, &'a str, &'a str, SqlValue, ErrorCode);

fn row_fault(
    conn: &Connection,
    request: &AssociationRequest,
    review: &AssociationReview,
    fault: RowFault<'_>,
) {
    let (table, key, id, column, changed, code) = fault;
    // All identifiers are fixed test literals, never wire data.
    let original: SqlValue = conn
        .query_row(
            &format!("SELECT {column} FROM {table} WHERE {key}=?1"),
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let update = format!("UPDATE {table} SET {column}=?1 WHERE {key}=?2");
    assert_eq!(
        conn.execute(&update, rusqlite::params![changed, id])
            .unwrap(),
        1
    );
    let result = inspect_unchanged(request);
    assert!(result.is_err(), "{table}.{column} fault must be rejected");
    assert_eq!(result.unwrap_err().code, code, "{table}.{column}");
    conn.execute(&update, rusqlite::params![original, id])
        .unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
}

pub(super) fn exercise(request: &AssociationRequest, review: &AssociationReview) -> usize {
    let conn = catalog::open_catalog().unwrap();
    let mut checks = 0;
    let mut changed = request.clone();
    changed.import_id = format!("imp:{}", Uuid::new_v4());
    rejected(&changed, ErrorCode::MappingConflict);
    changed = request.clone();
    changed.expected_revision = Uuid::new_v4().to_string();
    rejected(&changed, ErrorCode::PlanStale);
    checks += 2;
    for field in ["duplicate", "event", "leaf"] {
        changed = request.clone();
        match field {
            "duplicate" => changed
                .preview
                .selection
                .event_ids
                .push(changed.preview.selection.event_ids[0].clone()),
            "event" => changed.preview.selection.event_ids = vec!["foreign:event".into()],
            "leaf" => changed.preview.leaf_id = "foreign:leaf".into(),
            _ => unreachable!(),
        }
        rejected(&changed, ErrorCode::HistoryAccessUnavailable);
        checks += 1;
    }
    let row_faults = [
        (
            "entries",
            "import_id",
            IMPORT,
            "hidden",
            SqlValue::Integer(1),
            ErrorCode::MappingConflict,
        ),
        (
            "entries",
            "import_id",
            IMPORT,
            "current_revision",
            SqlValue::Text(Uuid::new_v4().to_string()),
            ErrorCode::PlanStale,
        ),
        (
            "entries",
            "import_id",
            IMPORT,
            "provider",
            SqlValue::Text("codex".into()),
            ErrorCode::MappingConflict,
        ),
        (
            "entries",
            "import_id",
            IMPORT,
            "installation_id",
            SqlValue::Text(Uuid::new_v4().to_string()),
            ErrorCode::MappingConflict,
        ),
        (
            "sources",
            "installation_id",
            INSTALLATION,
            "host",
            SqlValue::Text("ssh:synthetic".into()),
            ErrorCode::SourceNotFound,
        ),
        (
            "sources",
            "installation_id",
            INSTALLATION,
            "provider",
            SqlValue::Text("codex".into()),
            ErrorCode::SourceNotFound,
        ),
        (
            "entries",
            "import_id",
            IMPORT,
            "native_session_id",
            SqlValue::Text("foreign:transcript".into()),
            ErrorCode::HistoryAccessUnavailable,
        ),
        (
            "revisions",
            "revision_id",
            REVISION,
            "normalizer_version",
            SqlValue::Text("1.0.5".into()),
            ErrorCode::FormatUnsupported,
        ),
    ];
    for fault in row_faults {
        row_fault(&conn, request, review, fault);
        checks += 1;
    }
    // Changing only the primary path does not revoke an explicitly registered
    // old alias. Remove that alias too to exercise a genuinely foreign root.
    let (root, profile): (String, String) = conn
        .query_row(
            "SELECT root_path,profile FROM sources WHERE installation_id=?1",
            [INSTALLATION],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut unregistered: serde_json::Value = serde_json::from_str(&profile).unwrap();
    unregistered["aliases"] = json!([]);
    conn.execute(
        "UPDATE sources SET root_path='unregistered/synthetic',profile=?1 WHERE installation_id=?2",
        rusqlite::params![unregistered.to_string(), INSTALLATION],
    )
    .unwrap();
    rejected(request, ErrorCode::SourceChanged);
    conn.execute(
        "UPDATE sources SET root_path=?1,profile=?2 WHERE installation_id=?3",
        rusqlite::params![root, profile, INSTALLATION],
    )
    .unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    checks += 1;
    for native in [
        &review.old_native_session_id,
        &review.current_native_session_id,
    ] {
        conn.execute("INSERT INTO tombstones(installation_id,provider,native_session_id,deleted_at) VALUES(?1,'cursor',?2,'synthetic-fault')",
            rusqlite::params![INSTALLATION, native]).unwrap();
        rejected(request, ErrorCode::MappingConflict);
        conn.execute("DELETE FROM tombstones WHERE installation_id=?1 AND provider='cursor' AND native_session_id=?2",
            rusqlite::params![INSTALLATION, native]).unwrap();
        assert_eq!(&inspect_unchanged(request).unwrap(), review);
        checks += 1;
    }
    // Fault-only extension rows, not fabricated positive lineage. The exact
    // existing schema is reused; a too-large row must not look like absence.
    let store = &conn;
    use crate::session_migration::registration_provenance_store as proof_store;
    store.execute_batch(proof_store::SQL).unwrap();
    store
        .execute(
            "INSERT INTO meta(key,value) VALUES(?1,'1')",
            [proof_store::VERSION_KEY],
        )
        .unwrap();
    for json in ["{}".to_string(), " ".repeat(32769)] {
        store
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        store
            .execute(
                "INSERT INTO registration_provenance(import_id,json,digest) VALUES(?1,?2,?3)",
                rusqlite::params![IMPORT, json, "0".repeat(64)],
            )
            .unwrap();
        store
            .execute_batch("PRAGMA ignore_check_constraints=OFF")
            .unwrap();
        rejected(request, ErrorCode::MappingConflict);
        store
            .execute(
                "DELETE FROM registration_provenance WHERE import_id=?1",
                [IMPORT],
            )
            .unwrap();
        checks += 1;
    }
    store
        .execute_batch("DROP TABLE registration_provenance")
        .unwrap();
    store
        .execute("DELETE FROM meta WHERE key=?1", [proof_store::VERSION_KEY])
        .unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    let receipt_id: String = conn.query_row("SELECT receipt_id FROM receipts WHERE import_id=?1 AND revision_id=?2 ORDER BY rowid DESC LIMIT 1",
        rusqlite::params![IMPORT, REVISION], |row| row.get(0)).unwrap();
    row_fault(
        &conn,
        request,
        review,
        (
            "receipts",
            "receipt_id",
            &receipt_id,
            "json",
            SqlValue::Text("{}".into()),
            ErrorCode::SchemaUnsupported,
        ),
    );
    row_fault(
        &conn,
        request,
        review,
        (
            "receipts",
            "receipt_id",
            &receipt_id,
            "json",
            SqlValue::Text(" ".repeat(16 * 1024 * 1024 + 1)),
            ErrorCode::ResourceLimit,
        ),
    );
    checks += 2;

    let preview = jobs::find_preview_dir(&request.preview.preview_id)
        .unwrap()
        .unwrap();
    let lease_bytes = std::fs::read(preview.join("lease.json")).unwrap();
    let lease = jobs::read_preview_lease(&preview).unwrap().unwrap();
    for retained in [false, true] {
        let mut faulty = lease.clone();
        let code = if retained {
            faulty.retained_by_job = true;
            ErrorCode::SourceBusy
        } else {
            faulty.expires_unix = 0;
            ErrorCode::PlanStale
        };
        jobs::write_preview_lease(&preview, &faulty).unwrap();
        rejected(request, code);
        std::fs::write(preview.join("lease.json"), &lease_bytes).unwrap();
        assert_eq!(&inspect_unchanged(request).unwrap(), review);
        checks += 1;
    }
    let manifest = preview.join("manifest.json");
    let bytes = std::fs::read(&manifest).unwrap();
    let mut faulty = bytes.clone();
    faulty.push(b' ');
    std::fs::write(&manifest, faulty).unwrap();
    rejected(request, ErrorCode::PlanStale);
    std::fs::write(&manifest, bytes).unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    checks += 1;

    let old_manifest = catalog::revision_dir(IMPORT, REVISION)
        .unwrap()
        .join("manifest.json");
    let old_bytes = std::fs::read(&old_manifest).unwrap();
    let mut old_fault = old_bytes.clone();
    old_fault.push(b' ');
    std::fs::write(&old_manifest, old_fault).unwrap();
    rejected(request, ErrorCode::HistoryAccessUnavailable);
    std::fs::write(&old_manifest, old_bytes).unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    checks += 1;

    // Valid, non-identity source metadata and an explicit narrower selection
    // change the digest; restoring exact metadata restores the old review.
    let profile: String = conn
        .query_row(
            "SELECT profile FROM sources WHERE installation_id=?1",
            [INSTALLATION],
            |row| row.get(0),
        )
        .unwrap();
    let mut updated: serde_json::Value = serde_json::from_str(&profile).unwrap();
    updated["profile"] = json!("synthetic review metadata update");
    conn.execute(
        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
        rusqlite::params![updated.to_string(), INSTALLATION],
    )
    .unwrap();
    let metadata_review = inspect_unchanged(request).unwrap();
    assert_ne!(metadata_review.review_digest, review.review_digest);
    conn.execute(
        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
        rusqlite::params![&profile, INSTALLATION],
    )
    .unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    let mut updated_aliases: serde_json::Value = serde_json::from_str(&profile).unwrap();
    updated_aliases["aliases"]
        .as_array_mut()
        .unwrap()
        .push(json!("synthetic/additional/registered-alias"));
    conn.execute(
        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
        rusqlite::params![updated_aliases.to_string(), INSTALLATION],
    )
    .unwrap();
    let alias_review = inspect_unchanged(request).unwrap();
    assert_ne!(alias_review.review_digest, review.review_digest);
    conn.execute(
        "UPDATE sources SET profile=?1 WHERE installation_id=?2",
        rusqlite::params![profile, INSTALLATION],
    )
    .unwrap();
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    changed = request.clone();
    assert_eq!(changed.preview.selection.event_ids.len(), 2);
    changed.preview.selection.event_ids.truncate(1);
    changed.preview.selection.pinned_event_ids.clear();
    let narrowed = inspect_unchanged(&changed).unwrap();
    assert_ne!(narrowed.selection_hash, review.selection_hash);
    assert_ne!(narrowed.review_digest, review.review_digest);
    assert_eq!(&inspect_unchanged(request).unwrap(), review);
    checks + 3
}
