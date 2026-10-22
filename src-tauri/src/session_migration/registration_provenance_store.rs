//! Optional exact historical identity proof. Reads never initialize or repair it.
use rusqlite::{params, Connection, OptionalExtension};

use super::errors::{sqlite_storage_error, ErrorCode, ImportError};
use super::registration_provenance::{Proof, ProofRecord};

pub(super) const VERSION_KEY: &str = "registration_provenance_schema_version";
pub(super) const ROW_METADATA_SQL: &str =
    "SELECT rowid,typeof(json),typeof(digest) FROM registration_provenance WHERE import_id=?1";
pub(super) const SQL: &str = "CREATE TABLE registration_provenance (
  import_id TEXT PRIMARY KEY NOT NULL,
  json TEXT NOT NULL CHECK(length(CAST(json AS BLOB))<=32768),
  digest TEXT NOT NULL CHECK(length(digest)=64)
)";

fn incompatible() -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        "registration provenance schema is incompatible",
    )
}

pub(super) fn verify_schema(conn: &Connection) -> Result<bool, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return verify_schema(&tx);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [VERSION_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    if version.as_deref().is_some_and(|v| v != "1") {
        return Err(
            if version
                .as_deref()
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|v| v > 1)
            {
                ImportError::new(
                    ErrorCode::ImportSchemaTooNew,
                    "registration provenance schema is newer than this app",
                )
            } else {
                incompatible()
            },
        );
    }
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='registration_provenance'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    if match &version {
        None => sql.is_some(),
        Some(_) => sql.as_deref().map(str::trim) != Some(SQL),
    } {
        return Err(incompatible());
    }
    let unexpected: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE tbl_name='registration_provenance'
         AND (type='trigger' OR (type='index' AND sql IS NOT NULL)))",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if unexpected {
        return Err(incompatible());
    }
    Ok(version.is_some())
}

pub(super) fn insert(conn: &Connection, proof: &Proof) -> Result<(), ImportError> {
    insert_record(conn, &ProofRecord::Legacy(Box::new(proof.clone())))
}

pub(super) fn insert_record(conn: &Connection, proof: &ProofRecord) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    proof.validate_shape()?;
    if !verify_schema(conn)? {
        conn.execute_batch(SQL).map_err(sqlite_storage_error)?;
        conn.execute("INSERT INTO meta(key,value) VALUES(?1,'1')", [VERSION_KEY])
            .map_err(sqlite_storage_error)?;
        verify_schema(conn)?;
    }
    let json = serde_json::to_string(proof).map_err(|_| incompatible())?;
    if json.len() > 32768 {
        return Err(incompatible());
    }
    conn.execute(
        "INSERT INTO registration_provenance(import_id,json,digest) VALUES(?1,?2,?3)",
        params![proof.import_id(), json, proof.digest()?],
    )
    .map_err(sqlite_storage_error)?;
    Ok(())
}

pub(super) fn get(conn: &Connection, import_id: &str) -> Result<Option<ProofRecord>, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return get(&tx, import_id);
    }
    if !verify_schema(conn)? {
        return Ok(None);
    }
    // A corrupt present row is refusal, not absence. Select only row identity
    // and type metadata; length(CAST(...)) could load the cell in SQLite itself.
    let row: Option<(i64, String, String)> = conn
        .query_row(ROW_METADATA_SQL, [import_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .optional()
        .map_err(sqlite_storage_error)?;
    let Some((row_id, json_kind, digest_kind)) = row else {
        return Ok(None);
    };
    if json_kind != "text" || digest_kind != "text" {
        return Err(incompatible().job_local());
    }
    for (column, bound) in [("json", 32768), ("digest", 64)] {
        let blob = conn
            .blob_open(
                rusqlite::DatabaseName::Main,
                "registration_provenance",
                column,
                row_id,
                true,
            )
            .map_err(sqlite_storage_error)?;
        if blob.len() > bound || (column == "digest" && blob.len() != bound) {
            return Err(incompatible().job_local());
        }
    }
    // The same transaction pins the checked bytes through the bounded fetch.
    let (json, digest): (String, String) = conn
        .query_row(
            "SELECT json,digest FROM registration_provenance WHERE import_id=?1 AND rowid=?2",
            params![import_id, row_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sqlite_storage_error)?;
    let proof = ProofRecord::parse(&json)?;
    if proof.import_id() != import_id || proof.digest()? != digest {
        return Err(incompatible().job_local());
    }
    Ok(Some(proof))
}
