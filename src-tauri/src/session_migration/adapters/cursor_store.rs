use super::*;

pub fn normalize_sqlite(path: &Path) -> Result<AdapterOutput, ImportError> {
    if path.file_name().and_then(|s| s.to_str()) == Some("store.db") {
        return normalize_store(path);
    }
    normalize_composer(path, None)
}

pub fn normalize_composer(
    path: &Path,
    composer_id: Option<&str>,
) -> Result<AdapterOutput, ImportError> {
    let companion = companion_composer_db(path);
    let mut last = None;
    for attempt in 0..=1 {
        let out = read_composer_attempt(path, composer_id, companion.as_deref())?;
        let missing = out.issues.iter().any(|i| i.code == "missing-bubble");
        if !missing || attempt == 1 {
            return Ok(out);
        }
        last = Some(out);
    }
    Ok(last.expect("composer attempt"))
}

/// Workspace Composer may point at bubbles that live in globalStorage.
/// Only that one companion is consulted — never every workspace DB.
pub(crate) fn companion_composer_db(path: &Path) -> Option<PathBuf> {
    let hash_dir = path.parent()?;
    let storage = hash_dir.parent()?;
    if storage.file_name()?.to_str()? != "workspaceStorage" {
        return None;
    }
    let global = storage.parent()?.join("globalStorage").join("state.vscdb");
    global.is_file().then_some(global)
}

pub(super) fn read_composer_attempt(
    path: &Path,
    composer_id: Option<&str>,
    companion: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    let conn = open_readonly(path)?;
    let tables = table_names(&conn)?;
    if !tables
        .iter()
        .any(|t| t == "ItemTable" || t == "cursorDiskKV")
    {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "Cursor composer DB is missing ItemTable/cursorDiskKV",
        ));
    }
    let mut kv = load_kv(&conn)?;
    let mut artifacts = vec![db_artifact(path, &conn, "cursor-composer-db")];
    let mut native = composer_id.unwrap_or("composer").to_string();
    let chosen = choose_composer_key(&kv, composer_id)?;
    let identity = database_identity(&chosen)?;
    if let Some(id) = chosen
        .strip_prefix("composerData:")
        .or_else(|| chosen.strip_prefix("chatData:"))
    {
        native = id.to_string();
    }
    let data: Value = serde_json::from_slice(kv.get(&chosen).map(|v| v.as_slice()).unwrap_or(&[]))
        .unwrap_or(Value::Null);
    let mut title = data
        .get("name")
        .or_else(|| data.get("text"))
        .and_then(|x| x.as_str())
        .unwrap_or(&native)
        .to_string();
    let bubbles = collect_bubble_ids(&data);
    let missing_before: Vec<_> = bubbles
        .iter()
        .filter(|bid| bubble_raw(&kv, &native, bid).is_none())
        .cloned()
        .collect();
    if !missing_before.is_empty() {
        if let Some(comp) = companion.filter(|p| p.is_file() && *p != path) {
            match open_readonly(comp) {
                Ok(extra) => {
                    artifacts.push(db_artifact(comp, &extra, "cursor-composer-companion"));
                    let extra_kv = load_kv(&extra)?;
                    for bid in &missing_before {
                        let key = bubble_key(&native, bid);
                        if let Some(value) = extra_kv.get(&key) {
                            kv.entry(key).or_insert_with(|| value.clone());
                        }
                    }
                }
                Err(e)
                    if matches!(
                        e.code,
                        ErrorCode::SourceBusy
                            | ErrorCode::SourceChanged
                            | ErrorCode::SnapshotUnavailable
                            | ErrorCode::SourcePermissionDenied
                    ) =>
                {
                    // Workspace rows stay readable. A busy/unreadable companion
                    // is Partial missing-bubble, not an empty-success or a
                    // whole-session SOURCE_BUSY that drops committed bubbles.
                }
                Err(e) => return Err(e),
            }
        }
    }
    let fingerprint = if chosen.starts_with("chatData:") {
        "cursor-legacy-chat-v1"
    } else {
        "cursor-composer-itemtable-v1"
    };
    let (events, relations, issues) = if data.get("fullConversationHeadersOnly").is_some() {
        messages_from_bubbles(&bubbles, &native, &kv, path)
    } else {
        messages_from_legacy_chat(&data, &native, &kv, path)
    };
    if events.is_empty() {
        return Err(ImportError::new(
            ErrorCode::PartialHistory,
            "composer session had no readable bubbles",
        ));
    }
    let outcome = if issues.iter().any(|i| i.code == "missing-bubble") {
        ImportOutcome::Partial
    } else {
        ImportOutcome::Complete
    };
    let mut out = finish_cursor(
        events,
        relations,
        issues,
        identity,
        path.to_string_lossy().into_owned(),
        path.metadata().map(|m| m.len()).unwrap_or(0),
        conn.source_revision(),
        Some(conn.source_revision()),
        fingerprint,
        outcome,
    )?;
    if title.is_empty() {
        title = native;
    }
    out.graph.title = Some(title);
    out.evidence.artifacts = artifacts;
    out.recount();
    Ok(out)
}

pub(super) fn db_artifact(path: &Path, conn: &ReadonlySnapshot, kind: &str) -> ArtifactEvidence {
    ArtifactEvidence {
        artifact_id: path.to_string_lossy().into_owned(),
        kind: kind.into(),
        relative_locator: path.to_string_lossy().into_owned(),
        byte_length: path.metadata().map(|m| m.len()).unwrap_or(0),
        sha256: conn.source_revision(),
        complete_prefix_sha256: Some(conn.source_revision()),
    }
}

pub(super) fn choose_composer_key(
    kv: &HashMap<String, Vec<u8>>,
    composer_id: Option<&str>,
) -> Result<String, ImportError> {
    if let Some(id) = composer_id {
        if id.starts_with("composerData:") || id.starts_with("chatData:") {
            return Ok(id.to_string());
        }
        let composer = format!("composerData:{id}");
        if kv.contains_key(&composer) {
            return Ok(composer);
        }
        let chat = format!("chatData:{id}");
        if kv.contains_key(&chat) {
            return Ok(chat);
        }
        return Ok(composer);
    }
    kv.keys()
        .find(|k| k.starts_with("composerData:"))
        .cloned()
        .or_else(|| kv.keys().find(|k| k.starts_with("chatData:")).cloned())
        .ok_or_else(|| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "no composerData or chatData keys",
            )
        })
}

pub(super) fn bubble_key(native: &str, bid: &str) -> String {
    format!("bubbleId:{native}:{bid}")
}

pub(super) fn bubble_raw<'a>(
    kv: &'a HashMap<String, Vec<u8>>,
    native: &str,
    bid: &str,
) -> Option<&'a Vec<u8>> {
    kv.get(&bubble_key(native, bid))
}

pub(super) fn messages_from_bubbles(
    bubbles: &[String],
    native: &str,
    kv: &HashMap<String, Vec<u8>>,
    path: &Path,
) -> (Vec<ImportedEvent>, Vec<EventRelation>, Vec<ImportIssue>) {
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut issues = Vec::new();
    let mut prev = None;
    let mut ordinal = 0u32;
    for bid in bubbles {
        let key = bubble_key(native, bid);
        let Some(raw) = bubble_raw(kv, native, bid) else {
            issues.push(
                ImportIssue::new("missing-bubble", format!("bubble {bid} missing"))
                    .with_span(key)
                    .with_error(ErrorCode::PartialHistory),
            );
            continue;
        };
        let v: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
        let text = bubble_text(&v);
        if text.is_empty() {
            continue;
        }
        let ts = bubble_timestamp(&v);
        let id = format!("cursor:bubble:{bid}");
        events.push(ImportedEvent::Message {
            base: EventBase {
                id: id.clone(),
                ordinal,
                spans: vec![SourceSpan {
                    artifact_id: path.to_string_lossy().into_owned(),
                    record: key,
                    block: None,
                }],
                timestamp: ts.clone(),
                time_origin: if ts.is_some() {
                    TimeOrigin::Source
                } else {
                    TimeOrigin::Unknown
                },
            },
            role: bubble_role(&v),
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        });
        link_prev(&mut relations, &id, &mut prev);
        ordinal += 1;
    }
    (events, relations, issues)
}

pub(super) fn messages_from_legacy_chat(
    data: &Value,
    native: &str,
    kv: &HashMap<String, Vec<u8>>,
    path: &Path,
) -> (Vec<ImportedEvent>, Vec<EventRelation>, Vec<ImportIssue>) {
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut issues = Vec::new();
    let mut prev = None;
    let mut ordinal = 0u32;
    let rows = data
        .get("messages")
        .or_else(|| data.get("conversation"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, row) in rows.iter().enumerate() {
        let mut decoded = None;
        let mut record = format!("legacy:{i}");
        if bubble_text(row).is_empty() {
            if let Some(bid) = bubble_pointer(row) {
                record = bubble_key(native, bid);
                decoded = bubble_raw(kv, native, bid)
                    .and_then(|raw| serde_json::from_slice::<Value>(raw).ok());
                if decoded.is_none() {
                    issues.push(
                        ImportIssue::new("missing-bubble", format!("bubble {bid} missing"))
                            .with_span(record)
                            .with_error(ErrorCode::PartialHistory),
                    );
                    continue;
                }
            }
        }
        let row = decoded.as_ref().unwrap_or(row);
        let role = bubble_role(row);
        let text = bubble_text(row);
        if text.is_empty() {
            continue;
        }
        let id = format!("cursor:legacy:{i}");
        let timestamp = bubble_timestamp(row);
        events.push(ImportedEvent::Message {
            base: EventBase {
                id: id.clone(),
                ordinal,
                spans: vec![SourceSpan {
                    artifact_id: path.to_string_lossy().into_owned(),
                    record,
                    block: None,
                }],
                time_origin: if timestamp.is_some() {
                    TimeOrigin::Source
                } else {
                    TimeOrigin::Unknown
                },
                timestamp,
            },
            role,
            blocks: vec![ContentBlock::text(text)],
            complete: true,
        });
        link_prev(&mut relations, &id, &mut prev);
        ordinal += 1;
    }
    (events, relations, issues)
}

pub(super) fn collect_bubble_ids(data: &Value) -> Vec<String> {
    if let Some(arr) = data
        .get("fullConversationHeadersOnly")
        .and_then(|x| x.as_array())
    {
        return arr
            .iter()
            .filter_map(|x| {
                x.get("bubbleId")
                    .and_then(|b| b.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
    }
    if let Some(arr) = data.get("conversation").and_then(|x| x.as_array()) {
        return arr
            .iter()
            .filter_map(bubble_pointer)
            .map(str::to_string)
            .collect();
    }
    Vec::new()
}

fn bubble_pointer(row: &Value) -> Option<&str> {
    row.get("bubbleId").and_then(Value::as_str).or_else(|| {
        // Inline legacy messages have their own id; it is not a foreign key.
        if ["role", "type", "text", "content", "richText"]
            .iter()
            .any(|key| row.get(key).is_some())
        {
            None
        } else {
            row.get("id").and_then(Value::as_str)
        }
    })
}

pub(super) fn bubble_role(v: &Value) -> MessageRole {
    if let Some(kind) = v.get("type").and_then(Value::as_i64) {
        return if kind == 1 {
            MessageRole::User
        } else {
            MessageRole::Assistant
        };
    }
    let t = v
        .get("type")
        .or_else(|| v.get("role"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    match t.to_ascii_lowercase().as_str() {
        "1" | "user" | "human" => MessageRole::User,
        _ => MessageRole::Assistant,
    }
}

pub(super) fn bubble_text(v: &Value) -> String {
    v.get("text")
        .or_else(|| v.get("content"))
        .or_else(|| v.get("richText"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Composer/legacy numeric timestamps are stored epoch milliseconds. Do not
/// infer units or fill missing message times from session metadata or mtime.
pub(super) fn bubble_timestamp(v: &Value) -> Option<String> {
    let value = v
        .get("timestamp")
        .filter(|value| !value.is_null())
        .or_else(|| v.get("createdAt"))?;
    if let Some(text) = value.as_str() {
        chrono::DateTime::parse_from_rfc3339(text).ok()?;
        return Some(text.to_string());
    }
    chrono::DateTime::from_timestamp_millis(value.as_i64()?)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

pub(super) fn load_kv(conn: &Connection) -> Result<HashMap<String, Vec<u8>>, ImportError> {
    let mut map = HashMap::new();
    for table in ["ItemTable", "cursorDiskKV"] {
        let sql = format!("SELECT key, value FROM {table}");
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)
                        .or_else(|_| row.get::<_, String>(1).map(|s| s.into_bytes()))?,
                ))
            }) {
                for row in rows.flatten() {
                    map.insert(row.0, row.1);
                }
            }
        }
    }
    Ok(map)
}

pub fn normalize_store(path: &Path) -> Result<AdapterOutput, ImportError> {
    normalize_store_in_root(path, None)
}

pub(crate) fn normalize_store_in_root(
    path: &Path,
    root: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    normalize_store_in_location(path, root, None)
}

pub(crate) fn normalize_store_in_location(
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<AdapterOutput, ImportError> {
    let identity = store_identity_scoped(path, root, location_scope)?;
    normalize_store_snapshot(path, identity)
}

fn normalize_store_snapshot(path: &Path, identity: String) -> Result<AdapterOutput, ImportError> {
    let conn = open_readonly(path)?;
    let tables = table_names(&conn)?;
    if tables.is_empty() {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            "Cursor store.db has no tables",
        ));
    }
    if !tables.iter().any(|t| t == "blobs" || t == "Blobs") {
        return Err(ImportError::new(
            ErrorCode::SchemaUnsupported,
            format!("unknown store schema tables: {}", tables.join(",")),
        ));
    }
    let blob_table = if tables.iter().any(|t| t == "blobs") {
        "blobs"
    } else {
        "Blobs"
    };
    if tables.iter().any(|table| table == "meta") {
        return super::cursor_store_wire::normalize(path, &conn, blob_table, identity);
    }
    let meta_path = path.parent().map(|p| p.join("meta.json"));
    let meta: Value = meta_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let title = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".into());
    let root_id = meta
        .get("rootBlobId")
        .or_else(|| meta.get("root"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    if root_id.as_ref().is_none_or(|root| root.is_empty()) {
        return Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Cursor Store has no selected DB or sidecar root",
        ));
    }
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    let sql = format!("SELECT id, data FROM {blob_table}");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|error| ImportError::new(ErrorCode::SchemaUnsupported, error.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|error| ImportError::new(ErrorCode::SchemaUnsupported, error.to_string()))?;
    for row in rows {
        let (id, bytes) =
            row.map_err(|error| ImportError::new(ErrorCode::SchemaUnsupported, error.to_string()))?;
        blobs.insert(id, bytes);
    }
    let mut issues = Vec::new();
    let mut events = Vec::new();
    let mut relations = Vec::new();
    let mut ordinal = 0u32;
    let mut missing_blob = false;
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let mut emitted = HashSet::new();
    let locator = path.to_string_lossy().into_owned();
    let meta_leaf = meta
        .get("leaf")
        .or_else(|| meta.get("activeLeaf"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    if let Some(rid) = &root_id {
        if !blobs.contains_key(rid) {
            missing_blob = true;
            issues.push(
                ImportIssue::new("missing-blob", format!("root blob {rid} missing"))
                    .with_error(ErrorCode::PartialHistory),
            );
        } else {
            walk_store_blob(
                rid,
                None,
                &blobs,
                &locator,
                &mut events,
                &mut relations,
                &mut issues,
                &mut ordinal,
                &mut missing_blob,
                &mut visited,
                &mut stack,
                &mut emitted,
            );
        }
    }
    if events.is_empty() {
        return Err(ImportError::new(
            if missing_blob {
                ErrorCode::PartialHistory
            } else {
                ErrorCode::FormatUnsupported
            },
            "Cursor store had no readable messages",
        ));
    }
    let mut out = finish_cursor(
        events,
        relations,
        issues,
        identity,
        locator,
        path.metadata().map(|m| m.len()).unwrap_or(0),
        conn.source_revision(),
        Some(conn.source_revision()),
        "cursor-store-blobs-v1",
        if missing_blob {
            ImportOutcome::Partial
        } else {
            ImportOutcome::Complete
        },
    )?;
    out.graph.title = Some(title);
    if let Some(leaf) = meta_leaf.or_else(|| {
        root_id.as_ref().and_then(|rid| {
            blobs.get(rid).and_then(|b| {
                serde_json::from_slice::<Value>(b).ok().and_then(|v| {
                    v.get("leaf")
                        .or_else(|| v.get("activeLeaf"))
                        .and_then(Value::as_str)
                        .map(|s| s.to_string())
                })
            })
        })
    }) {
        let id = format!("cursor:store:{leaf}");
        if out.graph.events.iter().any(|e| e.id() == id) {
            out.graph.active_leaf = Some(id);
        }
    }
    out.recount();
    Ok(out)
}

// The mutable arguments are the single DFS accumulator. They intentionally
// stay borrowed across recursion so no nested walk can publish partial state.
#[allow(clippy::too_many_arguments)]
pub(super) fn walk_store_blob(
    blob_id: &str,
    parent: Option<&str>,
    blobs: &HashMap<String, Vec<u8>>,
    locator: &str,
    events: &mut Vec<ImportedEvent>,
    relations: &mut Vec<EventRelation>,
    issues: &mut Vec<ImportIssue>,
    ordinal: &mut u32,
    missing: &mut bool,
    visited: &mut HashSet<String>,
    stack: &mut HashSet<String>,
    emitted: &mut HashSet<String>,
) {
    if stack.contains(blob_id) {
        issues.push(
            ImportIssue::new("store-cycle", format!("store blob {blob_id} repeats"))
                .with_span(blob_id.to_string()),
        );
        return;
    }
    if !visited.insert(blob_id.to_string()) {
        if let Some(p) = parent {
            relations.push(EventRelation {
                kind: RelationKind::Parent,
                from: format!("cursor:store:{blob_id}"),
                to: format!("cursor:store:{p}"),
                confidence: RelationConfidence::Explicit,
            });
        }
        return;
    }
    stack.insert(blob_id.to_string());
    let Some(bytes) = blobs.get(blob_id) else {
        *missing = true;
        issues.push(
            ImportIssue::new("missing-blob", format!("blob {blob_id} missing"))
                .with_error(ErrorCode::PartialHistory),
        );
        return;
    };
    let Ok(node) = serde_json::from_slice::<Value>(bytes) else {
        issues.push(ImportIssue::new(
            "unknown-block",
            format!("store blob {blob_id} is not JSON"),
        ));
        return;
    };
    let node_id = node
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(blob_id)
        .to_string();
    let event_id = format!("cursor:store:{node_id}");
    let parent_id = parent.map(|p| format!("cursor:store:{p}")).or_else(|| {
        node.get("parentId")
            .or_else(|| node.get("parent"))
            .and_then(Value::as_str)
            .map(|p| format!("cursor:store:{p}"))
    });
    if let Some(role) = store_role(&node) {
        let text = store_text(&node);
        if !text.is_empty() && emitted.insert(event_id.clone()) {
            events.push(ImportedEvent::Message {
                base: EventBase {
                    id: event_id.clone(),
                    ordinal: *ordinal,
                    spans: vec![SourceSpan {
                        artifact_id: locator.into(),
                        record: blob_id.into(),
                        block: None,
                    }],
                    timestamp: node
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string()),
                    time_origin: if node.get("timestamp").and_then(Value::as_str).is_some() {
                        TimeOrigin::Source
                    } else {
                        TimeOrigin::Unknown
                    },
                },
                role,
                blocks: vec![ContentBlock::text(text)],
                complete: true,
            });
            *ordinal += 1;
            if let Some(p) = &parent_id {
                if emitted.contains(p) {
                    relations.push(EventRelation {
                        kind: RelationKind::Parent,
                        from: event_id.clone(),
                        to: p.clone(),
                        confidence: RelationConfidence::Explicit,
                    });
                }
            }
        }
    }
    let next_parent = if emitted.contains(&event_id) {
        Some(node_id)
    } else {
        parent.map(|s| s.to_string())
    };
    let children = store_children(&node);
    for (i, child) in children.iter().enumerate() {
        if i > 0 && emitted.contains(&event_id) {
            relations.push(EventRelation {
                kind: RelationKind::Fork,
                from: format!("cursor:store:{child}"),
                to: event_id.clone(),
                confidence: RelationConfidence::Inferred,
            });
        }
        walk_store_blob(
            child,
            next_parent.as_deref(),
            blobs,
            locator,
            events,
            relations,
            issues,
            ordinal,
            missing,
            visited,
            stack,
            emitted,
        );
    }
    stack.remove(blob_id);
}

pub(super) fn store_role(v: &Value) -> Option<MessageRole> {
    match v
        .get("role")
        .or_else(|| v.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "user" | "human" => Some(MessageRole::User),
        "assistant" | "ai" => Some(MessageRole::Assistant),
        _ => None,
    }
}

pub(super) fn store_text(v: &Value) -> String {
    v.get("text")
        .or_else(|| v.get("content"))
        .and_then(|x| {
            x.as_str().map(|s| s.to_string()).or_else(|| {
                x.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
        })
        .unwrap_or_default()
}

pub(super) fn store_children(v: &Value) -> Vec<String> {
    v.get("children")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_cursor(
    events: Vec<ImportedEvent>,
    relations: Vec<EventRelation>,
    issues: Vec<ImportIssue>,
    native: String,
    locator: String,
    byte_length: u64,
    sha: String,
    prefix: Option<String>,
    fingerprint: &str,
    outcome: ImportOutcome,
) -> Result<AdapterOutput, ImportError> {
    let leaf = events.last().map(|e| e.id().to_string());
    let mut out = AdapterOutput {
        evidence: SourceEvidence {
            provider: Provider::Cursor,
            source_version: None,
            schema_fingerprint: fingerprint.into(),
            adapter_version: NORMALIZER_VERSION.into(),
            platform: current_platform(),
            artifacts: vec![ArtifactEvidence {
                artifact_id: locator.clone(),
                kind: fingerprint.into(),
                relative_locator: locator,
                byte_length,
                sha256: sha,
                complete_prefix_sha256: prefix,
            }],
        },
        graph: NormalizedGraph {
            events,
            relations,
            active_leaf: leaf,
            native_session_id: native.clone(),
            title: Some(native),
            cwd: None,
        },
        inventory: Inventory::default(),
        issues,
        assets: vec![],
        outcome,
    };
    out.validate_graph();
    Ok(out)
}

/// Same native id from Store / Composer / transcript: keep one candidate, report field conflicts.
#[cfg(test)]
pub fn aggregate_cursor_identities(
    cands: Vec<SourceCandidate>,
) -> (Vec<SourceCandidate>, Vec<ImportIssue>) {
    let mut by_id: HashMap<String, Vec<SourceCandidate>> = HashMap::new();
    for c in cands {
        by_id
            .entry(c.native_session_id.clone())
            .or_default()
            .push(c);
    }
    let mut out = Vec::new();
    let mut issues = Vec::new();
    for (id, group) in by_id {
        if group.len() == 1 {
            out.push(group.into_iter().next().unwrap());
            continue;
        }
        let preferred = group
            .iter()
            .find(|c| c.completeness == "store")
            .or_else(|| group.iter().find(|c| c.completeness == "composer"))
            .cloned()
            .unwrap_or_else(|| group[0].clone());
        let titles: std::collections::BTreeSet<_> =
            group.iter().map(|c| c.title.as_str()).collect();
        let revisions: std::collections::BTreeSet<_> =
            group.iter().map(|c| c.source_revision.as_str()).collect();
        if titles.len() > 1 || revisions.len() > 1 {
            issues.push(
                ImportIssue::new(
                    "source-conflict",
                    format!("Cursor session {id} has conflicting titles or bodies across sources"),
                )
                .with_span(id.clone()),
            );
        }
        let mut merged = preferred;
        if merged.cwd.is_none() {
            merged.cwd = group.iter().find_map(|c| c.cwd.clone());
        }
        if merged.title == merged.native_session_id {
            if let Some(title) = group
                .iter()
                .map(|c| c.title.as_str())
                .find(|t| *t != merged.native_session_id)
            {
                merged.title = title.to_string();
            }
        }
        merged.candidate_id = format!("cursor-agg:{id}");
        out.push(merged);
    }
    merge_transcript_stems_into_store(&mut out, &mut issues);
    (out, issues)
}

#[cfg(test)]
pub(super) fn merge_transcript_stems_into_store(
    out: &mut Vec<SourceCandidate>,
    issues: &mut Vec<ImportIssue>,
) {
    let store_ids: std::collections::BTreeSet<_> = out
        .iter()
        .filter(|c| c.completeness == "store" || c.completeness == "composer")
        .map(|c| c.native_session_id.clone())
        .collect();
    let mut i = 0;
    while i < out.len() {
        let stem = std::path::Path::new(&out[i].locator)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let is_tx = out[i].completeness == "transcript" || out[i].completeness == "subagent";
        if is_tx && store_ids.contains(stem) && out[i].native_session_id != stem {
            let id = stem.to_string();
            issues.push(
                ImportIssue::new(
                    "source-conflict",
                    format!("Cursor session {id} appears in transcript and DB"),
                )
                .with_span(id),
            );
            out.remove(i);
            continue;
        }
        i += 1;
    }
}
