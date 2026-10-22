//! An exact old-tuple selector, never publication or history authority.
//! Store1/proof2 stay unchanged. The optional ledger is written in the SAME
//! acceptance transaction; point reads authenticate its proof and live target.
use super::{refusal, AssociationProof, ProofRecord};
use crate::session_migration::{
    catalog,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    hash, registration_provenance_store as store,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::io::Read;

const VERSION_KEY: &str = "registration_association_old_owners_schema_version";
const PREFIX: &str = "registration_association_old_owner:";
const PREFIX_END: &str = "registration_association_old_owner;";
const SQL: &str = "CREATE TABLE registration_association_old_owners (
  tuple_digest TEXT PRIMARY KEY NOT NULL CHECK(length(tuple_digest)=64),
  import_id TEXT NOT NULL UNIQUE CHECK(length(import_id)=40),
  proof_digest TEXT NOT NULL CHECK(length(proof_digest)=64)
)";

fn incompatible() -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        "association old-owner schema is incompatible",
    )
}

fn tuple_digest(installation: &str, provider: &str, native: &str) -> String {
    hash::options_hash(&serde_json::json!([installation, provider, native]))
}

fn marker_key(tuple: &str) -> String {
    format!("{PREFIX}{tuple}")
}

fn schema(conn: &Connection) -> Result<bool, ImportError> {
    let version = catalog::retention_metadata_text(conn, VERSION_KEY, 1)?;
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='registration_association_old_owners'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    if version.is_none() && sql.is_none() {
        return Ok(false);
    }
    if version.as_deref() != Some("1") || sql.as_deref().map(str::trim) != Some(SQL) {
        return Err(incompatible());
    }
    let unexpected: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE tbl_name='registration_association_old_owners'
         AND (type='trigger' OR (type='index' AND sql IS NOT NULL)))",
        [], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if unexpected {
        return Err(incompatible());
    }
    Ok(true)
}

fn count(conn: &Connection, prefix: &str, end: &str) -> Result<i64, ImportError> {
    conn.query_row(
        "SELECT COUNT(*) FROM meta WHERE key COLLATE BINARY>=?1 AND key COLLATE BINARY<?2",
        [prefix, end],
        |row| row.get(0),
    )
    .map_err(sqlite_storage_error)
}

fn accepted_count(conn: &Connection) -> Result<i64, ImportError> {
    count(
        conn,
        "registration_association_accepted:",
        "registration_association_accepted;",
    )
}

fn complete(conn: &Connection) -> Result<(), ImportError> {
    // Independent selector/witness cardinalities prevent a missing index row or
    // marker from silently becoming ordinary absence. Counts select nothing and
    // never grant authority; the exact record must still authenticate below.
    let owners: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM registration_association_old_owners",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if owners == 0 || owners != accepted_count(conn)? || owners != count(conn, PREFIX, PREFIX_END)?
    {
        return Err(refusal());
    }
    let invalid: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM registration_association_old_owners WHERE typeof(tuple_digest)!='text'
         OR typeof(import_id)!='text' OR typeof(proof_digest)!='text')",
        [], |row| row.get(0),
    ).map_err(sqlite_storage_error)?;
    if invalid {
        return Err(incompatible());
    }
    Ok(())
}

pub(super) fn record(conn: &Connection, proof: &AssociationProof) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    proof.validate_shape()?;
    if !schema(conn)? {
        conn.execute_batch(SQL).map_err(sqlite_storage_error)?;
        conn.execute("INSERT INTO meta(key,value) VALUES(?1,'1')", [VERSION_KEY])
            .map_err(sqlite_storage_error)?;
    }
    let tuple = tuple_digest(
        &proof.original_installation,
        &proof.original_provider,
        &proof.original_native,
    );
    let digest = proof.digest()?;
    conn.execute(
        "INSERT INTO registration_association_old_owners(tuple_digest,import_id,proof_digest) VALUES(?1,?2,?3)",
        params![tuple, proof.import_id, digest],
    ).map_err(sqlite_storage_error)?;
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        params![marker_key(&tuple), format!("{}:{digest}", proof.import_id)],
    )
    .map_err(sqlite_storage_error)?;
    crate::session_migration::job_control::provenance_scope::publication::advance_fence(
        conn, &tuple,
    )?;
    schema(conn)?;
    complete(conn)
}

fn owner(conn: &Connection, tuple: &str) -> Result<Option<(String, String)>, ImportError> {
    let row: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM registration_association_old_owners WHERE tuple_digest=?1",
            [tuple],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let marker = catalog::retention_metadata_text(conn, &marker_key(tuple), 105)?;
    let Some(row) = row else {
        return if marker.is_some() {
            Err(refusal())
        } else {
            Ok(None)
        };
    };
    // No owned text fetch before BLOB length/type checks in this same snapshot.
    for (column, bytes) in [
        ("tuple_digest", 64),
        ("import_id", 40),
        ("proof_digest", 64),
    ] {
        let blob = conn
            .blob_open(
                rusqlite::DatabaseName::Main,
                "registration_association_old_owners",
                column,
                row,
                true,
            )
            .map_err(sqlite_storage_error)?;
        if blob.len() != bytes {
            return Err(incompatible());
        }
    }
    let (import, digest): (String, String) = conn.query_row(
        "SELECT import_id,proof_digest FROM registration_association_old_owners WHERE rowid=?1 AND tuple_digest=?2",
        params![row, tuple], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(sqlite_storage_error)?;
    if marker.as_deref() != Some(format!("{import}:{digest}").as_str()) {
        return Err(refusal());
    }
    Ok(Some((import, digest)))
}

fn verify_absence(conn: &Connection, ledger: bool) -> Result<(), ImportError> {
    verified_records(conn, ledger).map(|_| ())
}

fn verified_records(conn: &Connection, ledger: bool) -> Result<Vec<ProofRecord>, ImportError> {
    if !store::verify_schema(conn)? {
        return if ledger || accepted_count(conn)? != 0 {
            Err(refusal())
        } else {
            Ok(Vec::new())
        };
    }
    // Absence cannot be inferred from an unchecked, mutable selector. Stream
    // bounded proof rows to certify complete map membership in this snapshot;
    // this also catches coordinated selector+marker key substitution/deletion.
    // No full job/input/plan is loaded for unrelated proofs and no all-row JSON
    // extraction, path/native-prefix inference, repair or preview read occurs.
    let mut statement = conn
        .prepare("SELECT rowid,typeof(import_id) FROM registration_provenance ORDER BY rowid")
        .map_err(sqlite_storage_error)?;
    let mut rows = statement.query([]).map_err(sqlite_storage_error)?;
    let mut associations = 0i64;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let row_id: i64 = row.get(0).map_err(sqlite_storage_error)?;
        let kind: String = row.get(1).map_err(sqlite_storage_error)?;
        if kind != "text" {
            return Err(incompatible());
        }
        let mut blob = conn
            .blob_open(
                rusqlite::DatabaseName::Main,
                "registration_provenance",
                "import_id",
                row_id,
                true,
            )
            .map_err(sqlite_storage_error)?;
        if blob.is_empty() || blob.len() > 4096 {
            return Err(incompatible());
        }
        let mut bytes = vec![0; blob.len()];
        blob.read_exact(&mut bytes).map_err(|_| incompatible())?;
        drop(blob);
        let import = String::from_utf8(bytes).map_err(|_| incompatible())?;
        let record = store::get(conn, &import)?.ok_or_else(refusal)?;
        let ProofRecord::Association(proof) = &record else {
            candidates.push(record);
            continue;
        };
        if !ledger {
            return Err(refusal());
        }
        let tuple = tuple_digest(
            &proof.original_installation,
            &proof.original_provider,
            &proof.original_native,
        );
        let pair = owner(conn, &tuple)?.ok_or_else(refusal)?;
        let digest = proof.digest()?;
        let accepted = catalog::retention_metadata_text(conn, &proof.acceptance_key(), 64)?;
        if pair != (proof.import_id.clone(), digest.clone())
            || accepted.as_deref() != Some(digest.as_str())
        {
            return Err(refusal());
        }
        associations += 1;
        candidates.push(record);
    }
    if associations != accepted_count(conn)? {
        return Err(refusal());
    }
    Ok(candidates)
}

/// Selection/absence closure only. Callers must still authenticate canonical
/// input, exact bounded job header and every retained-plan member before read.
pub(in crate::session_migration) fn job_candidates(
    conn: &Connection,
    job_id: &str,
) -> Result<Vec<AssociationProof>, ImportError> {
    Ok(migration_records(conn)?
        .into_iter()
        .filter_map(|record| match record {
            ProofRecord::Association(proof) if proof.selects_job(job_id) => Some(*proof),
            _ => None,
        })
        .collect())
}

/// Complete bounded proof closure for the existing acceptance owner's detached
/// compatibility preflight/CAS only. This is not a persisted reader grant.
pub(in crate::session_migration) fn migration_records(
    conn: &Connection,
) -> Result<Vec<ProofRecord>, ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    let ledger = schema(conn)?;
    if ledger {
        complete(conn)?;
    }
    verified_records(conn, ledger)
}

/// Job-scoped candidate selected by an independently accepted provenance scope.
/// No unrelated ledger cardinality or proof body grants/refuses this point.
pub(in crate::session_migration) fn scoped_job_candidate(
    conn: &Connection,
    job_id: &str,
    import_id: &str,
) -> Result<AssociationProof, ImportError> {
    if conn.is_autocommit() {
        return Err(incompatible());
    }
    let Some(ProofRecord::Association(proof)) = store::get(conn, import_id)? else {
        return Err(refusal());
    };
    if !proof.selects_job(job_id) || !schema(conn)? {
        return Err(refusal());
    }
    let tuple = tuple_digest(
        &proof.original_installation,
        &proof.original_provider,
        &proof.original_native,
    );
    let pair = owner(conn, &tuple)?.ok_or_else(refusal)?;
    let digest = proof.digest()?;
    let accepted = catalog::retention_metadata_text(conn, &proof.acceptance_key(), 64)?;
    if pair != (proof.import_id.clone(), digest.clone())
        || accepted.as_deref() != Some(digest.as_str())
    {
        return Err(refusal());
    }
    Ok(*proof)
}

pub(in crate::session_migration) fn verify_publication_owner(
    conn: &Connection,
    installation: &str,
    provider: &str,
    native: &str,
) -> Result<(), ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
        return verify_publication_owner(&tx, installation, provider, native);
    }
    store::verify_schema(conn)?;
    if !schema(conn)? {
        return verify_absence(conn, false);
    }
    complete(conn)?;
    let tuple = tuple_digest(installation, provider, native);
    let Some((import, digest)) = owner(conn, &tuple)? else {
        return verify_absence(conn, true);
    };
    let Some(ProofRecord::Association(proof)) = store::get(conn, &import)? else {
        return Err(refusal());
    };
    if proof.digest()? != digest
        || proof.original_installation != installation
        || proof.original_provider != provider
        || proof.original_native != native
    {
        return Err(refusal());
    }
    // The index and its duplicated marker can ONLY select a candidate owner.
    // Authority still requires proof2, accepted digest/input/header/full plan,
    // the exact old receipt/cutoff and the registered live target identity.
    proof.verify_accepted_input(conn)?;
    proof.require_anchor(conn)?;
    super::super::registered(conn, &proof.input)?;
    let entry = catalog::get_entry(conn, &proof.import_id)?.ok_or_else(refusal)?;
    if entry.hidden
        || entry.installation_id != proof.input.source_id
        || entry.provider != proof.input.provider
        || entry.native_session_id != proof.input.native_session_id
        || entry.current_revision.is_none()
    {
        return Err(refusal());
    }
    Err(ImportError::new(
        ErrorCode::PlanStale,
        "historical association no longer owns this entry; reimport through the registered source",
    ))
}
