//! Cursor's DB metadata and ordered Merkle wire format. Reads only reachable blobs.
use super::*;
use crate::session_migration::budget;
use rusqlite::OptionalExtension;

#[path = "cursor_store_wire_codec.rs"]
mod codec;
#[path = "cursor_store_wire_events.rs"]
mod projection;

fn corrupt(message: impl Into<String>) -> ImportError {
    ImportError::new(ErrorCode::FormatUnsupported, message)
}

fn query_error(error: rusqlite::Error) -> ImportError {
    ImportError::new(
        ErrorCode::SchemaUnsupported,
        format!("Cursor Store query: {error}"),
    )
}

fn read_metadata(conn: &Connection) -> Result<Value, ImportError> {
    let length: Option<i64> = conn
        .query_row(
            "SELECT length(CAST(value AS BLOB)) FROM meta WHERE key='0'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(query_error)?;
    let length = length.ok_or_else(|| corrupt("Cursor Store DB metadata key 0 is missing"))?;
    if length < 0 || length as usize > budget::JSON_RECORD_MAX_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "Cursor Store metadata exceeds record budget",
        ));
    }
    let raw: String = conn
        .query_row("SELECT value FROM meta WHERE key='0'", [], |row| row.get(0))
        .map_err(query_error)?;
    let bytes = hex::decode(raw)
        .map_err(|_| corrupt("Cursor Store metadata is not valid even-length hex"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| corrupt("Cursor Store metadata is not UTF-8 JSON"))?;
    if !value.is_object() {
        return Err(corrupt("Cursor Store metadata must be an object"));
    }
    Ok(value)
}

enum Step {
    Tree { hash: String, depth: usize },
    Leaf(String),
    Exit(String),
}

struct Reader<'a> {
    conn: &'a Connection,
    table: &'a str,
    bytes_read: u64,
}

impl Reader<'_> {
    fn read(&mut self, hash: &str) -> Result<Option<Vec<u8>>, ImportError> {
        let sql = format!("SELECT length(data) FROM {} WHERE id=?1", self.table);
        let length: Option<i64> = self
            .conn
            .query_row(&sql, [hash], |row| row.get(0))
            .optional()
            .map_err(query_error)?;
        let Some(length) = length else {
            return Ok(None);
        };
        if length < 0 || length as usize > budget::JSON_RECORD_MAX_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "Cursor Store blob exceeds record budget",
            ));
        }
        self.bytes_read = self
            .bytes_read
            .checked_add(length as u64)
            .filter(|total| *total <= budget::SESSION_BODY_MAX_BYTES)
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::ResourceLimit,
                    "Cursor Store exceeds session body budget",
                )
            })?;
        let sql = format!("SELECT data FROM {} WHERE id=?1", self.table);
        self.conn
            .query_row(&sql, [hash], |row| row.get(0))
            .map(Some)
            .map_err(query_error)
    }
}

pub(super) fn normalize(
    path: &Path,
    conn: &ReadonlySnapshot,
    table: &str,
    identity: String,
) -> Result<AdapterOutput, ImportError> {
    let meta = read_metadata(conn)?;
    let root = meta
        .get("latestRootBlobId")
        .and_then(Value::as_str)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| corrupt("Cursor Store metadata has no valid selected root hash"))?
        .to_ascii_lowercase();
    let title = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".into());
    let locator = path.to_string_lossy().into_owned();
    let mut reader = Reader {
        conn,
        table,
        bytes_read: 0,
    };
    let mut pending = vec![Step::Tree {
        hash: root,
        depth: 0,
    }];
    let mut active = HashSet::new();
    let mut occurrences = HashMap::<String, usize>::new();
    let mut visited = 0usize;
    let mut output = projection::Projection::new(&locator);
    while let Some(step) = pending.pop() {
        let (hash, depth, leaf) = match step {
            Step::Exit(hash) => {
                active.remove(&hash);
                continue;
            }
            Step::Tree { hash, depth } => (hash, depth, false),
            Step::Leaf(hash) => (hash, 0, true),
        };
        visited += 1;
        if visited > budget::DISCOVER_BUDGET || depth > budget::JSON_NEST_MAX {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "Cursor Store traversal exceeds node/depth budget",
            ));
        }
        if !leaf && !active.insert(hash.clone()) {
            output.issue("store-cycle", &hash, "active Store subtree repeats");
            continue;
        }
        let Some(bytes) = reader.read(&hash)? else {
            output.issue(
                "missing-blob",
                &hash,
                "selected Store root/leaf/subtree is absent",
            );
            active.remove(&hash);
            continue;
        };
        if leaf {
            let occurrence = occurrences.entry(hash.clone()).or_default();
            match codec::message(&bytes) {
                Some(message) => output.message(&hash, *occurrence, &message),
                None => output.issue(
                    "unknown-block",
                    &hash,
                    "selected Store message is malformed",
                ),
            }
            *occurrence += 1;
            continue;
        }
        let remaining = budget::DISCOVER_BUDGET
            .saturating_sub(visited)
            .saturating_sub(pending.len());
        let references = codec::references(&bytes, remaining).ok_or_else(|| {
            ImportError::new(
                ErrorCode::ResourceLimit,
                "Cursor Store edge count exceeds traversal budget",
            )
        })?;
        if references.truncated {
            output.issue(
                "unknown-block",
                &hash,
                "selected Store reference frame is truncated",
            );
        }
        if references.edges.is_empty() {
            output.issue(
                "unknown-block",
                &hash,
                "selected Store subtree has no supported references",
            );
            active.remove(&hash);
            continue;
        }
        // Reverse the stack insertion, preserving the source's ordered DFS.
        pending.push(Step::Exit(hash));
        for (tag, hash) in references.edges.into_iter().rev() {
            pending.push(if tag == 0x0a {
                Step::Leaf(hash)
            } else {
                Step::Tree {
                    hash,
                    depth: depth + 1,
                }
            });
        }
    }
    if output.events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::PartialHistory,
            "selected Cursor Store root has no readable events",
        ));
    }
    let mut out = super::cursor_store::finish_cursor(
        output.events,
        output.relations,
        output.issues,
        identity,
        locator,
        path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
        conn.source_revision(),
        Some(conn.source_revision()),
        "cursor-store-merkle-v1",
        ImportOutcome::Complete,
    )?;
    out.graph.title = Some(title);
    if let Some(title) = meta
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    {
        out.graph.title = Some(title.to_string());
    }
    out.evidence.artifacts = vec![super::cursor_store::db_artifact(
        path,
        conn,
        "cursor-store-merkle-v1",
    )];
    out.recount();
    Ok(out)
}

#[cfg(test)]
#[path = "cursor_store_wire_tests.rs"]
mod source_fidelity_tests;
