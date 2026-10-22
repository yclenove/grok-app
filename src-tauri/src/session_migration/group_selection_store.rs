//! New-scan extension only. Reads validate, never initialize or repair.
use super::{Choice, GroupSelection, GroupSelectionDomain, GROUP_SELECTION_VERSION};
use crate::session_migration::{
    cursor,
    errors::{sqlite_storage_error, ErrorCode, ImportError},
    hash,
    scan::CandidateRef,
    scan_group_store, source_member,
    types::Provider,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

const STATE_DOMAIN: &str = "session-import-group-selection-state-v1";
const CHOICE_DOMAIN: &str = "session-import-group-selection-choice-v1";
const STATE_SQL: &str = "CREATE TABLE group_selections (
  selection_id TEXT PRIMARY KEY,
  scan_id TEXT NOT NULL UNIQUE,
  epoch INTEGER NOT NULL,
  json TEXT NOT NULL
)";
const CHOICE_SQL: &str = "CREATE TABLE group_selection_choices (
  selection_id TEXT NOT NULL,
  seq INTEGER NOT NULL CHECK(seq>0),
  group_id TEXT NOT NULL,
  candidate_id TEXT NOT NULL,
  selected INTEGER NOT NULL CHECK(selected IN (0,1)),
  json TEXT NOT NULL,
  PRIMARY KEY(selection_id,group_id),
  UNIQUE(selection_id,seq),
  FOREIGN KEY(selection_id) REFERENCES group_selections(selection_id),
  FOREIGN KEY(group_id) REFERENCES source_groups(group_id),
  FOREIGN KEY(candidate_id) REFERENCES candidates(candidate_id)
)";
const PAGE_SQL: &str = "CREATE INDEX group_selection_choices_page ON group_selection_choices(selection_id,selected,seq)";
const OBJECTS: [(&str, &str); 3] = [
    ("group_selections", STATE_SQL),
    ("group_selection_choices", CHOICE_SQL),
    ("group_selection_choices_page", PAGE_SQL),
];
type MemberRow = (u32, String, String, String, String, String, String, String);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    descriptor: GroupSelection,
    choices_digest: String,
    seal: String,
}

pub(super) fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

pub(super) fn metadata_error(cause: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, cause.to_string())
}

fn incompatible() -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        "group selection schema is incompatible; scan again",
    )
}

pub(super) fn verify_schema(conn: &Connection) -> Result<(), ImportError> {
    scan_group_store::verify(conn)?;
    let marked = conn
        .prepare("PRAGMA table_info(meta)")
        .map_err(sqlite_storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage_error)?
        .iter()
        .any(|column| column == "group_selection_schema_version");
    if !marked {
        for (name, _) in OBJECTS {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
                    [name],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage_error)?;
            if exists {
                return Err(incompatible());
            }
        }
        return Err(stale(
            "legacy scan has no fixed group selection; scan again",
        ));
    }
    let version: u32 = conn
        .query_row(
            "SELECT group_selection_schema_version FROM meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    if version != GROUP_SELECTION_VERSION {
        return Err(if version > GROUP_SELECTION_VERSION {
            ImportError::new(
                ErrorCode::ImportSchemaTooNew,
                "group selection schema is newer than this app",
            )
        } else {
            incompatible()
        });
    }
    for (name, expected) in OBJECTS {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage_error)?;
        if sql.as_deref().map(str::trim) != Some(expected) {
            return Err(incompatible());
        }
    }
    let triggers: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('group_selections','group_selection_choices'))", [], |row| row.get(0)).map_err(sqlite_storage_error)?;
    if triggers {
        return Err(incompatible());
    }
    Ok(())
}

fn strict_json<T: serde::de::DeserializeOwned + Serialize>(raw: &str) -> Result<T, ImportError> {
    if raw.len() > crate::session_migration::budget::READ_PAGE_TEXT_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "fixed member metadata exceeds page byte budget",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| incompatible())?;
    let parsed: T = serde_json::from_value(value.clone()).map_err(|_| incompatible())?;
    if serde_json::to_value(&parsed).map_err(|_| incompatible())? != value {
        return Err(stale(
            "fixed member contains unrecognized or missing fields",
        ));
    }
    Ok(parsed)
}

fn state_payload(state: &State) -> Vec<u8> {
    hash::canonical_json(&json!([state.descriptor, state.choices_digest])).into_bytes()
}

fn verify_seal(domain: &str, payload: &[u8], raw: &str) -> Result<(), ImportError> {
    let seal = hex::decode(raw).map_err(|_| stale("invalid group selection seal"))?;
    if seal.len() != 32 || !cursor::verify_installation_payload(domain, payload, &seal)? {
        return Err(stale("group selection seal mismatch"));
    }
    Ok(())
}

/// Stream exact stored rows, not an unbounded decoded reference array. Counts
/// plus the signed root detect deletion and replay of a previously valid choice.
fn inventory(conn: &Connection, id: &str) -> Result<(u32, u32, String), ImportError> {
    let mut digest = Sha256::new();
    digest.update(b"fixed-group-choice-rows-v1\0");
    let mut count = 0u32;
    let mut selected_count = 0u32;
    let mut query = conn.prepare("SELECT seq,group_id,candidate_id,selected,json FROM group_selection_choices WHERE selection_id=?1 ORDER BY seq").map_err(sqlite_storage_error)?;
    let mut rows = query.query([id]).map_err(sqlite_storage_error)?;
    while let Some(row) = rows.next().map_err(sqlite_storage_error)? {
        let seq: u32 = row.get(0).map_err(sqlite_storage_error)?;
        let selected: bool = row.get(3).map_err(sqlite_storage_error)?;
        digest.update(seq.to_be_bytes());
        digest.update([u8::from(selected)]);
        for column in [1, 2, 4] {
            let raw = row
                .get_ref(column)
                .map_err(sqlite_storage_error)?
                .as_str()
                .map_err(|_| incompatible())?;
            if raw.len() > crate::session_migration::budget::READ_PAGE_TEXT_BYTES {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "fixed choice row exceeds byte budget",
                ));
            }
            digest.update((raw.len() as u64).to_be_bytes());
            digest.update(raw.as_bytes());
        }
        count = count.checked_add(1).ok_or_else(incompatible)?;
        selected_count = selected_count
            .checked_add(u32::from(selected))
            .ok_or_else(incompatible)?;
    }
    Ok((count, selected_count, hex::encode(digest.finalize())))
}

pub(super) fn initialize(conn: &Connection, scan_id: &str, epoch: u64) -> Result<(), ImportError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_storage_error)?;
    for (_, sql) in OBJECTS {
        tx.execute_batch(sql).map_err(sqlite_storage_error)?;
    }
    verify_schema(&tx)?;
    let candidate_id: String = tx
        .query_row(
            "SELECT selection_id FROM candidate_selections WHERE scan_id=?1 AND epoch=?2",
            params![scan_id, epoch],
            |row| row.get(0),
        )
        .map_err(sqlite_storage_error)?;
    let id = loop {
        let id = uuid::Uuid::new_v4().to_string();
        if id != candidate_id {
            break id;
        }
    };
    let mut state = State {
        descriptor: GroupSelection {
            version: GROUP_SELECTION_VERSION,
            domain: GroupSelectionDomain::FixedGroups,
            selection_id: id,
            scan_id: scan_id.into(),
            epoch,
            state_version: 1,
            selected_count: 0,
            choice_count: 0,
        },
        choices_digest: inventory(&tx, "")?.2,
        seal: String::new(),
    };
    state.seal = hex::encode(cursor::sign_installation_payload(
        STATE_DOMAIN,
        &state_payload(&state),
    )?);
    tx.execute(
        "INSERT INTO group_selections(selection_id,scan_id,epoch,json) VALUES(?1,?2,?3,?4)",
        params![
            state.descriptor.selection_id,
            scan_id,
            epoch,
            serde_json::to_string(&state).map_err(metadata_error)?
        ],
    )
    .map_err(sqlite_storage_error)?;
    tx.commit().map_err(sqlite_storage_error)
}

pub(super) fn load(
    conn: &Connection,
    scan_id: &str,
    epoch: u64,
    id: Option<&str>,
) -> Result<GroupSelection, ImportError> {
    verify_schema(conn)?;
    let raw: Option<(String, String)> = conn.query_row("SELECT selection_id,json FROM group_selections WHERE scan_id=?1 AND epoch=?2 AND (?3 IS NULL OR selection_id=?3)", params![scan_id, epoch, id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(sqlite_storage_error)?;
    let (key, raw) =
        raw.ok_or_else(|| stale("group selection does not belong to this scan epoch"))?;
    let state: State = strict_json(&raw)?;
    let d = &state.descriptor;
    if d.version != GROUP_SELECTION_VERSION {
        return Err(if d.version > GROUP_SELECTION_VERSION {
            ImportError::new(
                ErrorCode::ImportSchemaTooNew,
                "fixed group selection is newer than this app",
            )
        } else {
            incompatible()
        });
    }
    if d.selection_id != key
        || d.scan_id != scan_id
        || d.epoch != epoch
        || d.state_version == 0
        || uuid::Uuid::parse_str(&key).is_err()
    {
        return Err(stale("fixed group selection identity changed"));
    }
    verify_seal(STATE_DOMAIN, &state_payload(&state), &state.seal)?;
    let (count, selected, digest) = inventory(conn, &key)?;
    if (count, selected, digest) != (d.choice_count, d.selected_count, state.choices_digest) {
        return Err(stale(
            "fixed group selection rows differ from their signed snapshot",
        ));
    }
    Ok(state.descriptor)
}

pub(super) fn persist(
    conn: &Connection,
    mut descriptor: GroupSelection,
) -> Result<GroupSelection, ImportError> {
    let (count, selected, digest) = inventory(conn, &descriptor.selection_id)?;
    descriptor.choice_count = count;
    descriptor.selected_count = selected;
    let mut state = State {
        descriptor,
        choices_digest: digest,
        seal: String::new(),
    };
    state.seal = hex::encode(cursor::sign_installation_payload(
        STATE_DOMAIN,
        &state_payload(&state),
    )?);
    let changed = conn
        .execute(
            "UPDATE group_selections SET json=?2 WHERE selection_id=?1",
            params![
                state.descriptor.selection_id,
                serde_json::to_string(&state).map_err(metadata_error)?
            ],
        )
        .map_err(sqlite_storage_error)?;
    if changed != 1 {
        return Err(stale("fixed group selection disappeared"));
    }
    Ok(state.descriptor)
}

fn choice_payload(selection: &GroupSelection, choice: &Choice) -> Vec<u8> {
    hash::canonical_json(&json!([
        selection.selection_id,
        selection.scan_id,
        selection.epoch,
        choice.version,
        choice.group_id,
        choice.group_seq,
        choice.reference,
        choice.selected,
        choice.choice_version
    ]))
    .into_bytes()
}

pub(super) fn member(
    conn: &Connection,
    selection: &GroupSelection,
    group: &str,
    candidate: Option<&str>,
) -> Result<(u32, CandidateRef), ImportError> {
    let row: Option<MemberRow> = conn
        .query_row(
            "SELECT g.seq,c.json,g.source_id,g.provider,g.host,g.identity,g.proof,m.candidate_id
        FROM source_groups g JOIN source_group_members m ON m.group_id=g.group_id
        JOIN candidates c ON c.seq=m.seq AND c.candidate_id=m.candidate_id
        WHERE g.group_id=?1 AND ((?2 IS NULL AND m.seq=g.seq) OR m.candidate_id=?2)",
            params![group, candidate],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_storage_error)?;
    let (seq, raw, source, provider, host, identity, proof, id) =
        row.ok_or_else(|| stale("member does not belong to this scan group"))?;
    let reference: CandidateRef = strict_json(&raw)?;
    source_member::validate_scan_member(&selection.scan_id, selection.epoch, group, &reference)?;
    if reference.candidate.provider == Provider::Cursor
        && reference.location.is_some()
        && reference.source_member.is_none()
    {
        return Err(stale("new Cursor group member lost its fixed source pin"));
    }
    if scan_group_store::group_id(&selection.scan_id, &reference) != group
        || reference.candidate.candidate_id != id
        || reference.source_id != source
        || reference.candidate.provider.as_str() != provider
        || reference.candidate.native_session_id != identity
        || reference
            .location
            .as_ref()
            .map(|l| l.host.as_str())
            .unwrap_or("local")
            != host
        || scan_group_store::proof(&reference) != proof
    {
        return Err(stale("group metadata differs from its immutable member"));
    }
    Ok((seq, reference))
}

pub(super) fn read_choice(
    conn: &Connection,
    selection: &GroupSelection,
    group: &str,
) -> Result<Option<Choice>, ImportError> {
    let row: Option<(u32, String, bool, String)> = conn.query_row("SELECT seq,candidate_id,selected,json FROM group_selection_choices WHERE selection_id=?1 AND group_id=?2", params![selection.selection_id,group], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(sqlite_storage_error)?;
    let Some((seq, candidate, selected, raw)) = row else {
        return Ok(None);
    };
    let choice: Choice = strict_json(&raw)?;
    if choice.version != GROUP_SELECTION_VERSION
        || choice.group_id != group
        || choice.group_seq != seq
        || choice.selected != selected
        || choice.reference.candidate.candidate_id != candidate
        || choice.choice_version == 0
        || choice.choice_version > selection.state_version
    {
        return Err(stale("fixed group choice binding changed"));
    }
    verify_seal(
        CHOICE_DOMAIN,
        &choice_payload(selection, &choice),
        &choice.seal,
    )?;
    let (original_seq, original) = member(conn, selection, group, Some(&candidate))?;
    if original_seq != seq
        || serde_json::to_value(&original).map_err(metadata_error)?
            != serde_json::to_value(&choice.reference).map_err(metadata_error)?
    {
        return Err(stale("fixed choice differs from its immutable scan member"));
    }
    Ok(Some(choice))
}

pub(super) fn write_choice(
    conn: &Connection,
    selection: &GroupSelection,
    mut choice: Choice,
) -> Result<(), ImportError> {
    choice.choice_version = selection.state_version;
    choice.seal = hex::encode(cursor::sign_installation_payload(
        CHOICE_DOMAIN,
        &choice_payload(selection, &choice),
    )?);
    conn.execute("INSERT INTO group_selection_choices(selection_id,seq,group_id,candidate_id,selected,json) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(selection_id,group_id) DO UPDATE SET candidate_id=excluded.candidate_id,selected=excluded.selected,json=excluded.json", params![selection.selection_id,choice.group_seq,choice.group_id,choice.reference.candidate.candidate_id,choice.selected,serde_json::to_string(&choice).map_err(metadata_error)?]).map_err(sqlite_storage_error)?;
    Ok(())
}
