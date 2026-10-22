//! Persisted source instances. Paths locate an instance, never become its ID.

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::errors::{ErrorCode, ImportError};
use super::types::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRegistration {
    pub source_id: String,
    pub provider: Provider,
    pub root_path: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<super::source_locations::SourceLocation>,
}

pub fn get(conn: &Connection, source_id: &str) -> Result<SourceRegistration, ImportError> {
    if uuid::Uuid::parse_str(source_id).is_err() {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "invalid source installation ID",
        ));
    }
    // Registration metadata is sufficient even if the provider directory is offline.
    let row: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT provider,root_path,profile FROM sources WHERE installation_id=?1
         AND length(CAST(root_path AS BLOB))<=8192
         AND (profile IS NULL OR length(CAST(profile AS BLOB))<=65536)",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(error)?;
    let (provider, root_path, profile) = row.ok_or_else(|| {
        ImportError::new(
            ErrorCode::MappingConflict,
            "registered source is missing or invalid",
        )
    })?;
    let provider = Provider::parse(&provider).ok_or_else(|| error("invalid source provider"))?;
    let locations = profile
        .map(|json| serde_json::from_str::<Locations>(&json).map_err(error))
        .transpose()?
        .unwrap_or_default();
    Ok(SourceRegistration {
        source_id: source_id.into(),
        provider,
        root_path,
        profile: locations.profile,
        location: None,
    })
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Locations {
    aliases: Vec<String>,
    profile: String,
}

fn without_verbatim_prefix(path: &str) -> String {
    const VERBATIM_UNC: &str = r"\\?\UNC\";
    const VERBATIM: &str = r"\\?\";
    if let Some(rest) = path.strip_prefix(VERBATIM_UNC) {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(VERBATIM) {
        rest.to_string()
    } else {
        path.to_string()
    }
}

pub(super) fn identity_key(path: &str) -> String {
    #[cfg(windows)]
    {
        let mut key = without_verbatim_prefix(path).replace('/', "\\");
        while key.len() > 3 && key.ends_with('\\') {
            key.pop();
        }
        key.make_ascii_lowercase();
        key
    }
    #[cfg(not(windows))]
    {
        if path == "/" {
            path.into()
        } else {
            path.trim_end_matches('/').into()
        }
    }
}

pub(super) fn same_location(a: &str, b: &str) -> bool {
    a == b || identity_key(a) == identity_key(b)
}

pub(super) fn locator_spellings(locator: &str) -> Vec<String> {
    // Adapter family is part of the historical ID, not part of its path.
    // Never split unknown prefixes or reinterpret Composer/native identifiers.
    let (prefix, path) = [
        "codex:",
        "claude:",
        "cursor-transcript:",
        "cursor-store:",
        "file:",
    ]
    .iter()
    .find_map(|prefix| locator.strip_prefix(prefix).map(|path| (*prefix, path)))
    .unwrap_or(("", locator));
    let ordinary = without_verbatim_prefix(path);
    let mut out = Vec::new();
    let mut push = |value: String| {
        let value = format!("{prefix}{value}");
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    };
    push(path.to_string());
    push(ordinary.clone());
    if ordinary.starts_with(r"\\") && !ordinary.starts_with(r"\\?\") {
        let rest = ordinary.trim_start_matches(r"\\");
        push(format!(r"\\?\UNC\{rest}"));
    } else if ordinary.len() >= 2 && ordinary.as_bytes()[1] == b':' {
        push(format!(r"\\?\{ordinary}"));
    }
    out
}

/// A persisted historical registration cannot allocate a replacement entry
/// after accepted adoption moved its exact owner. This refusal also applies to
/// old explicit-reimport inputs; only registered-source reimport can restore it.
/// Committed receipt recovery is authenticated separately, before publication.
pub(super) fn verify_publication_owner(
    conn: &Connection,
    installation: &str,
    provider: &str,
    native: &str,
) -> Result<(), ImportError> {
    if !installation.starts_with("local:") {
        return super::registration_provenance::old_owner::verify_publication_owner(
            conn,
            installation,
            provider,
            native,
        );
    }
    let superseded: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE installation_id=?1)
         AND NOT EXISTS(SELECT 1 FROM entries
             WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3)",
            params![installation, provider, native],
            |row| row.get(0),
        )
        .map_err(error)?;
    if superseded {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "historical registration no longer owns this entry; reimport through the registered source",
        ));
    }
    Ok(())
}

fn push_alias(locations: &mut Locations, path: String) {
    if !locations.aliases.iter().any(|existing| existing == &path) {
        locations.aliases.push(path);
    }
}

fn error(e: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, e.to_string())
}

pub fn canonical_root(root: &Path) -> Result<PathBuf, ImportError> {
    std::fs::canonicalize(root).map_err(|e| {
        ImportError::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::SourceNotFound
            } else {
                ErrorCode::SourcePermissionDenied
            },
            format!("source root: {e}"),
        )
    })
}

pub fn list(conn: &Connection) -> Result<Vec<SourceRegistration>, ImportError> {
    let mut statement = conn
        .prepare(
            "SELECT installation_id,provider,root_path,profile FROM sources ORDER BY created_at",
        )
        .map_err(error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(error)?;
    let mut result = Vec::new();
    for row in rows {
        let (id, provider, root, locations) = row.map_err(error)?;
        // Pre-release per-candidate identities remain in the archive; they are
        // not presented as independently registered devices/profiles.
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let provider =
            Provider::parse(&provider).ok_or_else(|| error("invalid source provider"))?;
        let profile = locations
            .and_then(|s| serde_json::from_str::<Locations>(&s).ok())
            .map(|l| l.profile)
            .unwrap_or_default();
        result.push(SourceRegistration {
            source_id: id,
            provider,
            root_path: root,
            profile,
            location: None,
        });
    }
    Ok(result)
}

/// Reassociation is explicit: supplying source_id declares this folder is a
/// moved/copied location of that registered source. Preserve all old aliases.
pub fn register(
    conn: &Connection,
    provider: Provider,
    root: &Path,
    source_id: Option<&str>,
    profile: &str,
) -> Result<SourceRegistration, ImportError> {
    let root_path = canonical_root(root)?.to_string_lossy().into_owned();
    let ordinary = without_verbatim_prefix(&root_path);
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(error)?;
    super::source_location_store::guard_primary_path(&tx, provider, &root_path)?;
    let mut statement = tx.prepare("SELECT installation_id,root_path,profile FROM sources WHERE provider=?1 AND host='local'").map_err(error)?;
    let rows = statement
        .query_map(params![provider.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(error)?;
    let mut occupants = Vec::new();
    let mut explicit_row = None;
    for row in rows {
        let (id, current, json) = row.map_err(error)?;
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let locations: Locations = json
            .map(|s| serde_json::from_str(&s).map_err(error))
            .transpose()?
            .unwrap_or_default();
        if source_id == Some(id.as_str()) {
            explicit_row = Some((id.clone(), current.clone(), locations.clone()));
        }
        if same_location(&current, &root_path)
            || locations
                .aliases
                .iter()
                .any(|alias| same_location(alias, &root_path))
        {
            occupants.push((id, current, locations));
        }
    }
    drop(statement);
    if let Some(explicit) = source_id {
        if explicit_row.is_none() {
            return Err(ImportError::new(
                ErrorCode::SourceNotFound,
                "registered source/provider does not match",
            ));
        }
        if occupants.iter().any(|(id, _, _)| id != explicit) {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "path belongs to another registered source",
            ));
        }
    } else if occupants.len() > 1 {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "path matches multiple registered sources",
        ));
    }
    let (id, previous, mut locations) = if source_id.is_some() {
        explicit_row.ok_or_else(|| {
            ImportError::new(
                ErrorCode::SourceNotFound,
                "registered source/provider does not match",
            )
        })?
    } else if let Some(found) = occupants.into_iter().next() {
        found
    } else {
        (
            uuid::Uuid::new_v4().to_string(),
            root_path.clone(),
            Locations {
                aliases: vec![],
                profile: profile.into(),
            },
        )
    };
    push_alias(&mut locations, previous);
    push_alias(&mut locations, root_path.clone());
    if ordinary != root_path {
        push_alias(&mut locations, ordinary);
    }
    tx.execute("INSERT INTO sources(installation_id,provider,profile,host,root_path,created_at) VALUES(?1,?2,?3,'local',?4,?5)
        ON CONFLICT(installation_id) DO UPDATE SET root_path=excluded.root_path,profile=excluded.profile",
        params![id,provider.as_str(),serde_json::to_string(&locations).map_err(error)?,root_path,chrono::Utc::now().to_rfc3339()]).map_err(error)?;
    super::source_location_store::sync_primary(&tx, &id)?;
    tx.commit().map_err(error)?;
    Ok(SourceRegistration {
        source_id: id,
        provider,
        root_path,
        profile: locations.profile,
        location: None,
    })
}

/// Bounded saved aliases for one installation; this lookup never relocates it.
pub(super) fn registered_paths(
    conn: &Connection,
    source: &SourceRegistration,
) -> Result<Vec<String>, ImportError> {
    let profile: Option<String> = conn
        .query_row(
            "SELECT profile FROM sources WHERE installation_id=?1
             AND (profile IS NULL OR length(CAST(profile AS BLOB))<=65536)",
            [&source.source_id],
            |row| row.get(0),
        )
        .map_err(error)?;
    let mut locations = profile
        .map(|raw| serde_json::from_str::<Locations>(&raw).map_err(error))
        .transpose()?
        .unwrap_or_default();
    push_alias(&mut locations, source.root_path.clone());
    Ok(locations.aliases)
}

/// Adopt pre-release per-candidate registrations at the first explicit import.
/// Import/revision IDs and continuation references remain unchanged.
#[cfg(test)]
pub fn commit_identity(
    conn: &Connection,
    source_id: &str,
    candidate_legacy_id: &str,
    provider: &str,
    native_id: &str,
) -> Result<String, ImportError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(error)?;
    let result =
        commit_identity_in_transaction(&tx, source_id, candidate_legacy_id, provider, native_id)?;
    tx.commit().map_err(error)?;
    Ok(result)
}

/// Caller owns the short accepted-job transaction. Never commit an identity
/// before every durable input has been validated or independently of its job.
pub(super) fn commit_identity_in_transaction(
    tx: &Connection,
    source_id: &str,
    candidate_legacy_id: &str,
    provider: &str,
    native_id: &str,
) -> Result<String, ImportError> {
    if tx.is_autocommit() {
        return Err(error(
            "identity adoption requires an accepted-job transaction",
        ));
    }
    let adoption = resolve_identity(tx, source_id, candidate_legacy_id, provider, native_id)?;
    if let Some(old) = adoption {
        tx.execute(
            "UPDATE entries SET installation_id=?1,native_session_id=?2 WHERE import_id=?3
             AND installation_id=?4 AND provider=?5 AND native_session_id=?6",
            params![
                source_id,
                native_id,
                old.import_id,
                old.installation_id,
                provider,
                old.native_session_id
            ],
        )
        .map_err(error)?;
        tx.execute(
            "INSERT OR IGNORE INTO tombstones(installation_id,provider,native_session_id,deleted_at)
            SELECT ?1,provider,?2,deleted_at FROM tombstones WHERE installation_id=?3 AND provider=?4",
            params![source_id, native_id, old.installation_id, provider],
        )
        .map_err(error)?;
    }
    Ok(source_id.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LegacyIdentity {
    pub import_id: String,
    pub installation_id: String,
    pub native_session_id: String,
    pub current_revision: Option<String>,
    pub locator: String,
}

/// One bounded resolver shared by preflight and the accepted-job writer. It
/// deliberately uses only the historical locator hash, never display metadata.
pub(super) fn resolve_identity(
    conn: &Connection,
    source_id: &str,
    candidate_legacy_id: &str,
    provider: &str,
    native_id: &str,
) -> Result<Option<LegacyIdentity>, ImportError> {
    let mut migrate: Option<LegacyIdentity> = None;
    for locator in locator_spellings(candidate_legacy_id) {
        let legacy = format!("local:{}", super::hash::sha256_bytes(locator.as_bytes()));
        let mut statement = conn
            .prepare(
                "SELECT import_id,native_session_id,current_revision FROM entries
                      WHERE installation_id=?1 AND provider=?2 LIMIT 2",
            )
            .map_err(error)?;
        let matches: Vec<LegacyIdentity> = statement
            .query_map(params![legacy, provider], |row| {
                Ok(LegacyIdentity {
                    import_id: row.get(0)?,
                    installation_id: legacy.clone(),
                    native_session_id: row.get(1)?,
                    current_revision: row.get(2)?,
                    locator: locator.clone(),
                })
            })
            .map_err(error)?
            .collect::<Result<_, _>>()
            .map_err(error)?;
        drop(statement);
        if matches.len() > 1 {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "ambiguous legacy source identity",
            ));
        }
        if let Some(found) = matches.into_iter().next() {
            if migrate
                .as_ref()
                .is_some_and(|old| old.import_id != found.import_id)
            {
                return Err(ImportError::new(
                    ErrorCode::MappingConflict,
                    "legacy locators map to different imports",
                ));
            }
            migrate = Some(found);
        }
    }
    let current: Option<String> = conn
        .query_row(
            "SELECT import_id FROM entries WHERE installation_id=?1 AND provider=?2 AND native_session_id=?3",
            params![source_id, provider, native_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(error)?;
    if let Some(existing) = &current {
        if migrate
            .as_ref()
            .is_some_and(|old| &old.import_id != existing)
        {
            return Err(ImportError::new(
                ErrorCode::MappingConflict,
                "both legacy and registered imports exist; review their identities",
            ));
        }
    }
    Ok(migrate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::catalog::{
        list_entries, open_catalog, upsert_entry, CatalogEntry,
    };
    use crate::session_migration::errors::ErrorCode;
    use crate::session_migration::hash::sha256_bytes;

    struct Home {
        path: PathBuf,
        _lock: crate::session_migration::test_home::AppHomeEnvGuard,
    }

    impl Home {
        fn new() -> Self {
            let lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
            let path = std::env::temp_dir().join(format!("src-id-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            std::env::set_var("GROK_APP_HOME", path.join("app"));
            Self { path, _lock: lock }
        }

        fn dir(&self, name: &str) -> PathBuf {
            let dir = self.path.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn legacy_id(locator: &str) -> String {
        format!("local:{}", sha256_bytes(locator.as_bytes()))
    }

    #[cfg(unix)]
    #[test]
    fn posix_source_registration_keeps_literal_backslashes_and_case() {
        assert!(!same_location("/repo/a/b", r"/repo/a\b"));
        assert!(!same_location("/repo/Name", "/repo/name"));
        assert!(!same_location("/repo/name", "/repo/name "));
        assert!(same_location("/repo/name", "/repo/name/"));
    }

    fn insert_entry(conn: &Connection, installation: &str, native: &str, import_id: &str) {
        upsert_entry(
            conn,
            &CatalogEntry {
                import_id: import_id.into(),
                installation_id: installation.into(),
                provider: "codex".into(),
                native_session_id: native.into(),
                title: "same text".into(),
                alias: None,
                hidden: false,
                current_revision: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .unwrap();
    }

    fn snapshot_entries(conn: &Connection) -> Vec<(String, String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT import_id,installation_id,native_session_id FROM entries ORDER BY import_id",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn windows_stored_ordinary_path_matches_canonical_register() {
        let home = Home::new();
        let dir = home.dir("codex-home");
        let conn = open_catalog().unwrap();
        let canonical = canonical_root(&dir).unwrap().to_string_lossy().into_owned();
        let ordinary = canonical.replacen(r"\\?\", "", 1);
        let id = uuid::Uuid::new_v4().to_string();
        let locations = serde_json::json!({
            "aliases": [ordinary],
            "profile": "default"
        });
        conn.execute(
            "INSERT INTO sources(installation_id,provider,profile,host,root_path,created_at)
             VALUES(?1,'codex',?2,'local',?3,?4)",
            rusqlite::params![
                id,
                locations.to_string(),
                ordinary,
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        let found = register(&conn, Provider::Codex, &dir, None, "default").unwrap();
        assert_eq!(found.source_id, id);
        assert_eq!(list(&conn).unwrap().len(), 1);
        if ordinary != canonical {
            let aliases: Locations = serde_json::from_str(
                &conn
                    .query_row(
                        "SELECT profile FROM sources WHERE installation_id=?1",
                        rusqlite::params![id],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap(),
            )
            .unwrap();
            assert!(
                aliases.aliases.iter().any(|p| p == &ordinary)
                    && aliases.aliases.iter().any(|p| p == &canonical),
                "aliases should keep both spellings: {:?}",
                aliases.aliases
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_and_verbatim_prefix_do_not_create_a_second_source() {
        let home = Home::new();
        let dir = home.dir("MixCase");
        let conn = open_catalog().unwrap();
        let first = register(&conn, Provider::Codex, &dir, None, "default").unwrap();
        let ordinary = first.root_path.replacen(r"\\?\", "", 1);
        let flipped: String = ordinary
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        let again = register(&conn, Provider::Codex, Path::new(&flipped), None, "default").unwrap();
        assert_eq!(first.source_id, again.source_id);
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn explicit_relocate_onto_another_source_is_conflict() {
        let home = Home::new();
        let a = home.dir("device-a");
        let b = home.dir("device-b");
        let conn = open_catalog().unwrap();
        let first = register(&conn, Provider::Codex, &a, None, "default").unwrap();
        let other = register(&conn, Provider::Codex, &b, None, "default").unwrap();
        let err = register(
            &conn,
            Provider::Codex,
            &a,
            Some(&other.source_id),
            "default",
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::MappingConflict);
        let listed = list(&conn).unwrap();
        assert_eq!(listed.len(), 2);
        let root_of = |id: &str| {
            listed
                .iter()
                .find(|s| s.source_id == id)
                .unwrap()
                .root_path
                .clone()
        };
        assert_eq!(root_of(&first.source_id), first.root_path);
        assert_eq!(root_of(&other.source_id), other.root_path);
    }

    #[test]
    fn two_sources_same_native_id_and_text_stay_two_entries() {
        let home = Home::new();
        let a = home.dir("src-a");
        let b = home.dir("src-b");
        let conn = open_catalog().unwrap();
        let first = register(&conn, Provider::Codex, &a, None, "default").unwrap();
        let other = register(&conn, Provider::Codex, &b, None, "default").unwrap();
        insert_entry(&conn, &first.source_id, "native-same", "imp:aaaa");
        insert_entry(&conn, &other.source_id, "native-same", "imp:bbbb");
        let entries = list_entries(&conn).unwrap();
        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0].installation_id, entries[1].installation_id);
        assert!(entries.iter().all(|e| e.native_session_id == "native-same"));
        assert!(entries.iter().all(|e| e.title == "same text"));
    }

    #[test]
    fn legacy_identity_migrates_once_and_is_repeatable() {
        let home = Home::new();
        let _ = home.dir("keep-lock");
        let conn = open_catalog().unwrap();
        let source_id = uuid::Uuid::new_v4().to_string();
        let locator = r"C:\Users\me\.codex";
        insert_entry(&conn, &legacy_id(locator), "old-native", "imp:legacy");
        assert_eq!(
            commit_identity(&conn, &source_id, locator, "codex", "real-native").unwrap(),
            source_id
        );
        let after = snapshot_entries(&conn);
        assert_eq!(
            after,
            vec![("imp:legacy".into(), source_id.clone(), "real-native".into())]
        );
        assert_eq!(
            commit_identity(&conn, &source_id, locator, "codex", "real-native").unwrap(),
            source_id
        );
        assert_eq!(snapshot_entries(&conn), after);
    }

    #[test]
    fn verbatim_and_ordinary_legacy_locators_migrate_the_same_row() {
        let home = Home::new();
        let _ = home.dir("keep-lock");
        let conn = open_catalog().unwrap();
        let source_id = uuid::Uuid::new_v4().to_string();
        let ordinary = r"C:\Users\me\.codex";
        let verbatim = r"\\?\C:\Users\me\.codex";
        insert_entry(&conn, &legacy_id(ordinary), "sess", "imp:one");
        commit_identity(&conn, &source_id, verbatim, "codex", "sess").unwrap();
        assert_eq!(
            snapshot_entries(&conn),
            vec![("imp:one".into(), source_id.clone(), "sess".into())]
        );
        let unc_source = uuid::Uuid::new_v4().to_string();
        let unc = r"\\fileserver\share\codex";
        let unc_verbatim = r"\\?\UNC\fileserver\share\codex";
        insert_entry(&conn, &legacy_id(unc), "unc-sess", "imp:unc");
        commit_identity(&conn, &unc_source, unc_verbatim, "codex", "unc-sess").unwrap();
        let rows = snapshot_entries(&conn);
        assert!(rows.contains(&("imp:unc".into(), unc_source, "unc-sess".into())));
    }

    #[test]
    fn legacy_and_registered_conflict_leaves_catalog_unchanged() {
        let home = Home::new();
        let _ = home.dir("keep-lock");
        let conn = open_catalog().unwrap();
        let source_id = uuid::Uuid::new_v4().to_string();
        let locator = r"C:\Users\me\.codex";
        insert_entry(&conn, &legacy_id(locator), "sess", "imp:legacy");
        insert_entry(&conn, &source_id, "sess", "imp:fresh");
        let before = snapshot_entries(&conn);
        let err = commit_identity(&conn, &source_id, locator, "codex", "sess").unwrap_err();
        assert_eq!(err.code, ErrorCode::MappingConflict);
        assert_eq!(snapshot_entries(&conn), before);
        let tombs: i64 = conn
            .query_row("SELECT COUNT(*) FROM tombstones", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tombs, 0);
    }

    #[test]
    fn ambiguous_legacy_identity_is_conflict_and_rolls_back() {
        let home = Home::new();
        let _ = home.dir("keep-lock");
        let conn = open_catalog().unwrap();
        let source_id = uuid::Uuid::new_v4().to_string();
        let locator = r"C:\Users\me\.codex";
        let legacy = legacy_id(locator);
        insert_entry(&conn, &legacy, "a", "imp:a");
        insert_entry(&conn, &legacy, "b", "imp:b");
        let before = snapshot_entries(&conn);
        let err = commit_identity(&conn, &source_id, locator, "codex", "a").unwrap_err();
        assert_eq!(err.code, ErrorCode::MappingConflict);
        assert_eq!(snapshot_entries(&conn), before);
    }
    fn home_contract_fixture() {
        let _fixture = Home::new();
    }

    crate::session_migration::test_home::fixture_home_contract_tests!(home_contract_fixture);
}
