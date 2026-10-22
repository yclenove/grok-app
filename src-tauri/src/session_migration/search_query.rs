use super::*;

#[cfg(test)]
pub fn search(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    allowed: Option<&std::collections::HashSet<String>>,
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    let mut allowed_ids = allowed.map(|ids| ids.iter().cloned().collect::<Vec<_>>());
    if let Some(ids) = &mut allowed_ids {
        ids.sort();
    }
    search_scoped(
        revision_dir,
        query,
        cursor,
        limit,
        allowed,
        allowed_ids.as_deref(),
        "",
    )
}
/// Local reader search restricted to one branch lineage.  Unlike the
/// model-facing selection search below, this keeps the full local searchable
/// projection while constraining event membership to the selected leaf.
#[cfg(test)]
pub fn search_branch(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: &str,
    relations: &[EventRelation],
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    let ids = crate::session_migration::graph::lineage_ids(relations, leaf);
    if ids.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    search_branch_ids(revision_dir, query, cursor, limit, leaf, &ids)
}

#[cfg(test)]
pub fn search_branch_ids(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: &str,
    ids: &[String],
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    if ids.is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    if indexed_event_by_id(revision_dir, leaf)?.is_none() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    search_scoped(revision_dir, query, cursor, limit, None, Some(ids), leaf)
}

pub fn search_branch_window(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    leaf: &str,
    ids: &[String],
    direction: SearchDirection,
) -> Result<SearchWindow, ImportError> {
    if ids.is_empty() || indexed_event_by_id(revision_dir, leaf)?.is_none() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "reader leaf is not present in the archive",
        ));
    }
    let q = normalized_search_query(query)?;
    if q.is_empty() {
        return Ok(SearchWindow {
            hits: Vec::new(),
            previous_cursor: None,
            next_cursor: None,
        });
    }
    let (scope, forward_filter) = scoped_search_identity(revision_dir, &q, None, Some(ids), leaf);
    let backward_filter = if leaf.is_empty() {
        "search-backward".to_owned()
    } else {
        format!("search-backward|leaf={leaf}")
    };
    match direction {
        SearchDirection::Forward => {
            let (hits, next_cursor) =
                search_scoped(revision_dir, &q, cursor, limit, None, Some(ids), leaf)?;
            let previous_cursor = if cursor.is_some() {
                hits.first()
                    .map(|hit| {
                        encode_search_cursor(
                            revision_dir,
                            &scope,
                            &backward_filter,
                            leaf,
                            hit.ordinal,
                            &hit.event_id,
                        )
                    })
                    .transpose()?
            } else {
                None
            };
            Ok(SearchWindow {
                hits,
                previous_cursor,
                next_cursor,
            })
        }
        SearchDirection::Backward => {
            let raw = cursor
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    ImportError::new(
                        ErrorCode::PlanStale,
                        "backward search requires a page cursor",
                    )
                })?;
            let (boundary_ordinal, boundary_id) =
                decode_search_cursor(revision_dir, &scope, &backward_filter, leaf, Some(raw))?;
            let (hits, more_before) = search_scoped_backward(
                revision_dir,
                &q,
                limit,
                ids,
                boundary_ordinal,
                &boundary_id,
            )?;
            let previous_cursor = if more_before {
                hits.first()
                    .map(|hit| {
                        encode_search_cursor(
                            revision_dir,
                            &scope,
                            &backward_filter,
                            leaf,
                            hit.ordinal,
                            &hit.event_id,
                        )
                    })
                    .transpose()?
            } else {
                None
            };
            let next_cursor = hits
                .last()
                .map(|hit| {
                    encode_search_cursor(
                        revision_dir,
                        &scope,
                        &forward_filter,
                        leaf,
                        hit.ordinal,
                        &hit.event_id,
                    )
                })
                .transpose()?;
            Ok(SearchWindow {
                hits,
                previous_cursor,
                next_cursor,
            })
        }
    }
}

fn normalized_search_query(query: &str) -> Result<String, ImportError> {
    let q = query.trim().to_lowercase();
    if q.len() > 4096 {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "search query exceeds 4 KiB",
        ));
    }
    Ok(q)
}

fn scoped_search_identity(
    revision_dir: &Path,
    q: &str,
    allowed: Option<&std::collections::HashSet<String>>,
    scope_ids: Option<&[String]>,
    leaf: &str,
) -> (String, String) {
    let mut allowed_ids = allowed.map(|set| set.iter().cloned().collect::<Vec<_>>());
    if let Some(ids) = &mut allowed_ids {
        ids.sort();
    }
    let scope = crate::session_migration::hash::options_hash(&serde_json::json!({
        "revision": revision_dir.to_string_lossy(),
        "query": q,
        "allowed": allowed_ids,
        "scopeIds": scope_ids,
        "leaf": leaf,
    }));
    let filter = if leaf.is_empty() {
        "search".to_owned()
    } else {
        format!("search|leaf={leaf}")
    };
    (scope, filter)
}

fn search_scoped_backward(
    revision_dir: &Path,
    q: &str,
    limit: Option<usize>,
    scope_ids: &[String],
    mut scan_ordinal: i64,
    start_id: &str,
) -> Result<(Vec<SearchHit>, bool), ImportError> {
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let scope_json = serde_json::to_string(scope_ids).map_err(index_error)?;
    let limit = search_limit(limit);
    let fetch = (limit + 1).saturating_mul(8).max(limit + 1) as i64;
    let mut hits = Vec::new();
    let mut more = false;
    let mut scan_id = start_id.to_owned();
    loop {
        let mut stmt = conn
            .prepare(
                "SELECT event_id, ordinal, kind, json_off, json_len FROM events
                 WHERE (ordinal<?1 OR (ordinal=?1 AND event_id<?2))
                   AND event_id IN (SELECT value FROM json_each(?3))
                 ORDER BY ordinal DESC, event_id DESC LIMIT ?4",
            )
            .map_err(index_error)?;
        let rows = stmt
            .query_map(params![scan_ordinal, scan_id, scope_json, fetch], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(index_error)?;
        let mut batch = 0usize;
        for row in rows {
            let (event_id, ordinal, kind, json_off, json_len) = row.map_err(index_error)?;
            batch += 1;
            scan_ordinal = ordinal;
            scan_id = event_id.clone();
            let event = event_from_jsonl(revision_dir, json_off, json_len)?;
            let text = event.searchable_text();
            let lower = text.to_lowercase();
            let Some(match_byte) = lower.find(q) else {
                continue;
            };
            if hits.len() == limit {
                more = true;
                break;
            }
            let match_char = lower[..match_byte].chars().count();
            hits.push(SearchHit {
                event_id,
                ordinal: ordinal as u32,
                kind,
                snippet: text
                    .chars()
                    .skip(match_char.saturating_sub(60))
                    .take(240)
                    .collect(),
            });
        }
        if more || batch < fetch as usize {
            break;
        }
    }
    hits.reverse();
    Ok((hits, more))
}

fn search_scoped(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    allowed: Option<&std::collections::HashSet<String>>,
    scope_ids: Option<&[String]>,
    leaf: &str,
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    ensure_index(revision_dir)?;
    let q = normalized_search_query(query)?;
    if q.is_empty() {
        return Ok((vec![], None));
    }
    let (scope, filter) = scoped_search_identity(revision_dir, &q, allowed, scope_ids, leaf);
    let (start_ordinal, start_id) =
        decode_search_cursor(revision_dir, &scope, &filter, leaf, cursor)?;
    let conn = open_index(revision_dir)?;
    let scope_json = scope_ids
        .map(serde_json::to_string)
        .transpose()
        .map_err(index_error)?;
    let limit = search_limit(limit);
    let fetch = (limit + 1).saturating_mul(8).max(limit + 1) as i64;
    let mut hits = Vec::new();
    let mut more = false;
    let mut scan_ordinal = start_ordinal;
    let mut scan_id = start_id;
    loop {
        let mut stmt = conn
            .prepare(
                "SELECT event_id, ordinal, kind, json_off, json_len FROM events
                 WHERE (ordinal>?1 OR (ordinal=?1 AND event_id>?2))
                   AND (?3 IS NULL OR event_id IN (SELECT value FROM json_each(?3)))
                 ORDER BY ordinal, event_id LIMIT ?4",
            )
            .map_err(index_error)?;
        let rows = stmt
            .query_map(params![scan_ordinal, scan_id, scope_json, fetch], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(index_error)?;
        let mut batch = 0usize;
        for row in rows {
            let (event_id, ordinal, kind, json_off, json_len) = row.map_err(index_error)?;
            batch += 1;
            scan_ordinal = ordinal;
            scan_id = event_id.clone();
            let event = event_from_jsonl(revision_dir, json_off, json_len)?;
            let text = if allowed.is_some() {
                event.model_visible_text(false)
            } else {
                event.searchable_text()
            };
            let lower = text.to_lowercase();
            let Some(match_byte) = lower.find(&q) else {
                continue;
            };
            if hits.len() == limit {
                more = true;
                break;
            }
            let match_char = lower[..match_byte].chars().count();
            let snippet = text
                .chars()
                .skip(match_char.saturating_sub(60))
                .take(240)
                .collect();
            hits.push(SearchHit {
                event_id,
                ordinal: ordinal as u32,
                kind,
                snippet,
            });
        }
        if more || batch < fetch as usize {
            break;
        }
    }
    let next = if more {
        hits.last()
            .map(|hit| {
                encode_search_cursor(
                    revision_dir,
                    &scope,
                    &filter,
                    leaf,
                    hit.ordinal,
                    &hit.event_id,
                )
            })
            .transpose()?
    } else {
        None
    };
    Ok((hits, next))
}

/// Model-facing search: snippets come only from `AllowedSelection::project_event`.
pub fn search_with_selection(
    revision_dir: &Path,
    query: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
    selection: &AllowedSelection,
    relations: &[EventRelation],
) -> Result<(Vec<SearchHit>, Option<String>), ImportError> {
    ensure_index(revision_dir)?;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok((vec![], None));
    }
    if q.len() > 4096 {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "search query exceeds 4 KiB",
        ));
    }
    let mut allowed_ids = selection.event_ids.clone();
    allowed_ids.sort();
    let scope = crate::session_migration::hash::options_hash(&serde_json::json!({
        "revision": revision_dir.to_string_lossy(),
        "query": q,
        "selection": selection.hash(),
        "index": INDEX_SCHEMA_VERSION,
    }));
    let (start_ordinal, start_id) =
        decode_search_cursor(revision_dir, &scope, "search-sel", "", cursor)?;
    let conn = open_index(revision_dir)?;
    let allow_json = serde_json::to_string(&allowed_ids).map_err(index_error)?;
    let limit = search_limit(limit);
    let fetch = (limit + 1).saturating_mul(8).max(limit + 1);
    let mut stmt = conn
        .prepare(
            "SELECT event_id, ordinal, kind, json_off, json_len FROM events
             WHERE (ordinal>?1 OR (ordinal=?1 AND event_id>?2))
               AND event_id IN (SELECT value FROM json_each(?3))
             ORDER BY ordinal, event_id LIMIT ?4",
        )
        .map_err(index_error)?;
    let mut hits = Vec::new();
    let mut last_scanned: Option<(u32, String)> = None;
    let rows = stmt
        .query_map(
            params![start_ordinal, start_id, allow_json, fetch as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(index_error)?;
    let mut batch = 0usize;
    for row in rows {
        if hits.len() == limit {
            break;
        }
        let (event_id, ordinal, kind, json_off, json_len) = row.map_err(index_error)?;
        batch += 1;
        last_scanned = Some((ordinal as u32, event_id.clone()));
        let event = event_from_jsonl(revision_dir, json_off, json_len)?;
        let Some(text) = selection.project_event(&event, relations) else {
            continue;
        };
        let lower = text.to_lowercase();
        let Some(match_byte) = lower.find(&q) else {
            continue;
        };
        let match_char = lower[..match_byte].chars().count();
        let snippet = text
            .chars()
            .skip(match_char.saturating_sub(60))
            .take(240)
            .collect();
        hits.push(SearchHit {
            event_id,
            ordinal: ordinal as u32,
            kind,
            snippet,
        });
    }
    let exhausted = batch < fetch && hits.len() < limit;
    let next = if !exhausted {
        if let Some((ordinal, event_id)) = last_scanned {
            Some(encode_search_cursor(
                revision_dir,
                &scope,
                "search-sel",
                "",
                ordinal,
                &event_id,
            )?)
        } else {
            None
        }
    } else {
        None
    };
    Ok((hits, next))
}

pub fn read_with_selection(
    revision_dir: &Path,
    event_id: &str,
    cursor: Option<&str>,
    max_bytes: Option<usize>,
    selection: &AllowedSelection,
    relations: &[EventRelation],
) -> Result<(String, Option<String>), ImportError> {
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let (json_off, json_len): (i64, i64) = conn
        .query_row(
            "SELECT json_off, json_len FROM events WHERE event_id=?1",
            params![event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(index_error)?;
    let event = event_from_jsonl(revision_dir, json_off, json_len)?;
    let Some(text) = selection.project_event(&event, relations) else {
        return Err(index_error("event is outside the allowed selection"));
    };
    let scope = crate::session_migration::hash::options_hash(&serde_json::json!({
        "revision": revision_dir.to_string_lossy(),
        "event": event_id,
        "selection": selection.hash(),
    }));
    let offset = decode_read_offset(revision_dir, &scope, "read-sel", cursor)?;
    let cap = read_byte_limit(max_bytes);
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return Ok((String::new(), None));
    }
    if !text.is_char_boundary(offset) {
        return Err(index_error("read cursor is inside a UTF-8 character"));
    }
    let mut end = offset.saturating_add(cap).min(bytes.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset {
        return Err(index_error("read byte limit cannot fit one character"));
    }
    let body = text[offset..end].to_owned();
    let next = if end < bytes.len() {
        Some(encode_read_cursor(revision_dir, &scope, "read-sel", end)?)
    } else {
        None
    };
    Ok((body, next))
}
/// One page of stored events. Does not load the whole revision into memory.
#[cfg(test)]
pub fn page_indexed_events(
    revision_dir: &Path,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<ImportedEvent>, Option<String>, u32), ImportError> {
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let total: u32 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
        .map_err(index_error)?
        .try_into()
        .unwrap_or(0);
    let start = decode_page_cursor(revision_dir, cursor)?;
    if total == 0 {
        if start == 0 {
            return Ok((Vec::new(), None, 0));
        }
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "cursor past end of revision",
        ));
    }
    if start >= total {
        return Err(ImportError::new(
            ErrorCode::PlanStale,
            "cursor past end of revision",
        ));
    }
    let event_cap = crate::session_migration::budget::page_limit(limit)
        .min(crate::session_migration::budget::READ_PAGE_EVENTS);
    let mut stmt = conn
        .prepare(
            "SELECT json_off, json_len FROM events ORDER BY ordinal, event_id LIMIT ?1 OFFSET ?2",
        )
        .map_err(index_error)?;
    let rows = stmt
        .query_map(params![(event_cap + 1) as i64, start as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(index_error)?;
    let mut events = Vec::new();
    let mut text_bytes = 0usize;
    let mut saw_more = false;
    for span in rows {
        let (json_off, json_len) = span.map_err(index_error)?;
        let ev = event_from_jsonl(revision_dir, json_off, json_len)?;
        let add = ev.searchable_text().len();
        if events.len() >= event_cap {
            saw_more = true;
            break;
        }
        if !events.is_empty()
            && text_bytes.saturating_add(add)
                > crate::session_migration::budget::READ_PAGE_TEXT_BYTES
        {
            saw_more = true;
            break;
        }
        text_bytes = text_bytes.saturating_add(add);
        events.push(ev);
    }
    let next = if saw_more || start as usize + events.len() < total as usize {
        Some(encode_page_cursor(
            revision_dir,
            start + events.len() as u32,
        )?)
    } else {
        None
    };
    Ok((events, next, total))
}

pub fn indexed_events_by_ids(
    revision_dir: &Path,
    ids: &[String],
) -> Result<Vec<ImportedEvent>, ImportError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let json = serde_json::to_string(ids).map_err(index_error)?;
    let mut stmt = conn
        .prepare(
            "SELECT event_id, json_off, json_len FROM events
             WHERE event_id IN (SELECT value FROM json_each(?1))",
        )
        .map_err(index_error)?;
    let rows = stmt
        .query_map(params![json], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(index_error)?;
    let mut by_id = std::collections::HashMap::new();
    for row in rows {
        let (id, json_off, json_len) = row.map_err(index_error)?;
        let event = event_from_jsonl(revision_dir, json_off, json_len)?;
        by_id.insert(id, event);
    }
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Read only the requested events, ordered by immutable archive ordinal.
/// Permission IDs are canonicalized lexically for hashing, not conversation order.
/// Keep `indexed_events_by_ids` request-order semantics for graph/reader callers.
pub fn indexed_events_in_source_order(
    revision_dir: &Path,
    ids: &[String],
) -> Result<Vec<ImportedEvent>, ImportError> {
    let mut events = indexed_events_by_ids(revision_dir, ids)?;
    events.sort_by(|a, b| {
        a.base()
            .ordinal
            .cmp(&b.base().ordinal)
            .then_with(|| a.id().cmp(b.id()))
    });
    Ok(events)
}

pub fn indexed_event_by_id(
    revision_dir: &Path,
    event_id: &str,
) -> Result<Option<ImportedEvent>, ImportError> {
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let span = conn
        .query_row(
            "SELECT json_off, json_len FROM events WHERE event_id=?1",
            params![event_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(index_error)?;
    span.map(|(offset, length)| event_from_jsonl(revision_dir, offset, length))
        .transpose()
}

pub fn indexed_event_metadata_by_ids(
    revision_dir: &Path,
    ids: &[String],
) -> Result<Vec<IndexedEventMetadata>, ImportError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let mut metadata = Vec::with_capacity(ids.len().min(128));
    for batch in ids.chunks(128) {
        let json = serde_json::to_string(batch).map_err(index_error)?;
        let mut statement = conn
            .prepare(
                "SELECT event_id, ordinal, kind, json_len, model_visible, asset_ids_json FROM events
                 WHERE event_id IN (SELECT value FROM json_each(?1))",
            )
            .map_err(index_error)?;
        let rows = statement
            .query_map(params![json], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(index_error)?;
        for row in rows {
            let (event_id, ordinal, kind, json_len, model_visible, asset_ids_json) =
                row.map_err(index_error)?;
            metadata.push(IndexedEventMetadata {
                event_id,
                ordinal: ordinal.try_into().map_err(index_error)?,
                kind,
                json_len: json_len.try_into().map_err(index_error)?,
                model_visible: model_visible != 0,
                asset_ids: serde_json::from_str(&asset_ids_json).map_err(index_error)?,
            });
        }
    }
    Ok(metadata)
}

/// Retrieve one authorized record without loading the whole conversation.
#[cfg(test)]
pub fn read_indexed_event_text(
    revision_dir: &Path,
    event_id: &str,
    cursor: Option<&str>,
    max_bytes: Option<usize>,
    allowed: &std::collections::HashSet<String>,
) -> Result<(String, Option<String>), ImportError> {
    if !allowed.contains(event_id) {
        return Err(index_error("event is outside the allowed selection"));
    }
    let scope = crate::session_migration::hash::options_hash(&serde_json::json!({
        "revision": revision_dir.to_string_lossy(), "event": event_id,
    }));
    let offset = decode_read_offset(revision_dir, &scope, "read", cursor)?;
    if offset > crate::session_migration::budget::SESSION_BODY_MAX_BYTES as usize {
        return Err(index_error("read cursor exceeds budget"));
    }
    let cap = read_byte_limit(max_bytes);
    ensure_index(revision_dir)?;
    let conn = open_index(revision_dir)?;
    let (json_off, json_len): (i64, i64) = conn
        .query_row(
            "SELECT json_off, json_len FROM events WHERE event_id=?1",
            params![event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(index_error)?;
    let event = event_from_jsonl(revision_dir, json_off, json_len)?;
    let full = event.model_visible_text(false);
    let bytes = full.as_bytes();
    if offset >= bytes.len() {
        return Ok((String::new(), None));
    }
    if !full.is_char_boundary(offset) {
        return Err(index_error("read cursor is inside a UTF-8 character"));
    }
    let mut end = offset.saturating_add(cap).min(bytes.len());
    while end > offset && !full.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "read byte limit cannot fit one character",
        ));
    }
    let body = full[offset..end].to_owned();
    let next = if end < bytes.len() {
        Some(encode_read_cursor(revision_dir, &scope, "read", end)?)
    } else {
        None
    };
    Ok((body, next))
}
