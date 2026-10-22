//! Creation intent -> files -> complete native index row. No agent dispatch.
use super::{
    catalog, continuation, continuation_intent as intents, continuation_state, continuation_target,
    errors::{ErrorCode, ImportError},
};
use continuation::{ContinuationSidecar, DispatchState, HandoffDraft};
use intents::{error, Intent};

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

pub(super) fn check_draft(draft: &HandoffDraft) -> Result<(), ImportError> {
    if let Some(version) = draft.creation_version {
        if version != 1 {
            return Err(stale("unsupported continuation draft version"));
        }
        let mut expected = draft.clone();
        expected.draft_hash.clear();
        if super::hash::sha256_bytes(&serde_json::to_vec(&expected).map_err(error)?)
            != draft.draft_hash
        {
            return Err(stale("continuation draft content changed"));
        }
    } else if draft.workspace.is_some() {
        return Err(stale("workspace requires a versioned draft"));
    }
    Ok(())
}

fn validate_history(
    conn: &rusqlite::Connection,
    side: &ContinuationSidecar,
) -> Result<catalog::CatalogEntry, ImportError> {
    let entry =
        catalog::get_entry(conn, &side.import_id)?.ok_or_else(|| stale("import not found"))?;
    if entry.hidden
        || entry.current_revision.as_deref() != Some(&side.revision_id)
        || catalog::is_tombstoned(
            conn,
            &entry.installation_id,
            &entry.provider,
            &entry.native_session_id,
        )?
    {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "continuation history is stale or revoked",
        ));
    }
    super::continuation_scope::bound_selection(
        &side.import_id,
        &side.revision_id,
        &side.branch_leaf,
        side.allowed_selection.as_ref(),
        &side.allowed_selection_hash,
    )?;
    Ok(entry)
}

fn history_title(entry: &catalog::CatalogEntry) -> String {
    // Copy the displayed source name into the durable creation intent. It is
    // user data, not an English-only "Continue: <internal UUID>" UI label.
    for candidate in [entry.alias.as_deref(), Some(entry.title.as_str())]
        .into_iter()
        .flatten()
    {
        let clean: String = candidate
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        if !clean.is_empty() {
            return clean.chars().take(80).collect();
        }
    }
    crate::tray_i18n::strings(crate::tray_i18n::app_locale())
        .untitled
        .into()
}

/// Only before acquiring any JSON store lock. Another confirmation may be
/// committing its receipt; that short writer must not make this read fail.
/// Restore fail-fast behavior before entering the JSON -> SQLite lock order.
fn lookup_before_store_lock<T>(
    conn: &rusqlite::Connection,
    read: impl FnOnce(&rusqlite::Connection) -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    conn.busy_timeout(std::time::Duration::from_millis(super::budget::DB_BUSY_MS))
        .map_err(error)?;
    let result = read(conn).map_err(|failure| {
        let classified = error(&failure.message);
        if classified.code == ErrorCode::SourceBusy {
            classified
        } else {
            failure
        }
    });
    conn.busy_timeout(std::time::Duration::ZERO)
        .map_err(error)?;
    result
}

pub(super) fn run(
    draft: Option<&HandoffDraft>,
    draft_id: &str,
    draft_hash: &str,
    request_id: &str,
    edited_text: Option<String>,
    new_side: Option<ContinuationSidecar>,
) -> Result<String, ImportError> {
    if request_id.is_empty() || request_id.len() > 256 {
        return Err(stale("invalid creation request"));
    }
    let conn = catalog::open_catalog()?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(error)?;
    // A competing process can claim the request before locks are acquired.
    for _ in 0..3 {
        #[cfg(test)]
        tests::before_request_lookup();
        let known = lookup_before_store_lock(&conn, |conn| {
            continuation::load_ref_by_request(conn, request_id)
        })?;
        let sid = known
            .as_ref()
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if uuid::Uuid::parse_str(&sid).is_err() {
            return Err(error("invalid native continuation identity"));
        }
        let side_path = continuation::sidecar_path(&sid);
        let _locks = crate::store_lock::lock_exclusive_many(&[
            crate::paths::projects_file(),
            crate::paths::workspaces_file(),
            crate::paths::sessions_index_file(),
            side_path.clone(),
        ])
        .map_err(error)?;
        let reference = continuation::load_ref_by_request(&conn, request_id)?;
        if reference.as_ref().is_some_and(|(id, _)| id != &sid) {
            continue;
        }
        let existing = crate::store::load_sessions_snapshot()
            .map_err(error)?
            .into_iter()
            .find(|row| row.id == sid);
        let mut intent = intents::load(&conn, request_id)?;
        let side = match reference {
            Some((_, side)) => {
                if side.handoff_hash != draft_hash {
                    return Err(stale("request belongs to a different draft"));
                }
                side
            }
            None => {
                if intent.is_some() {
                    return Err(error("creation intent has no continuation reference"));
                }
                let draft = draft.ok_or_else(|| stale("draft not found"))?;
                // Re-read under the native writer lock: a cancel or target change
                // may have consumed the draft while this request was waiting.
                if catalog::load_draft_json(&conn, draft_id)?.is_none() {
                    return Err(stale("continuation draft was discarded or replaced"));
                }
                check_draft(draft)?;
                if draft.draft_id != draft_id || draft.draft_hash != draft_hash {
                    return Err(stale("draft identity mismatch"));
                }
                if intents::draft_claimed(&conn, draft_id)? {
                    return Err(stale(
                        "draft already has a creation request; retry that request",
                    ));
                }
                let side = new_side
                    .clone()
                    .ok_or_else(|| error("missing initial continuation sidecar"))?;
                let history = validate_history(&conn, &side)?;
                if let Some(workspace) = &draft.workspace {
                    continuation_target::validate(workspace)?;
                }
                let mut meta = crate::store::import_continuation::new_meta(
                    sid.clone(),
                    draft.project_id.clone(),
                    Some(history_title(&history)),
                    false,
                );
                if let Some(workspace) = &draft.workspace {
                    if meta.project_id.as_deref() != Some(&workspace.primary_project_id) {
                        return Err(stale("draft project differs from workspace"));
                    }
                    meta.workspace_id = Some(workspace.id.clone());
                    meta.workspace_root_snapshot =
                        Some(crate::workspace_store::root_snapshot(&workspace.roots));
                    meta.workspace_capability = Some(continuation_target::capability(workspace));
                }
                crate::store::import_continuation::validate(&meta).map_err(error)?;
                let text = edited_text
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&draft.text)
                    .clone();
                let new = Intent {
                    version: 1,
                    draft_id: draft_id.into(),
                    draft_hash: draft_hash.into(),
                    request_id: request_id.into(),
                    meta,
                    workspace: draft.workspace.clone(),
                    text,
                    published: false,
                    cancelled: false,
                };
                catalog::write_under_store_lock(&conn, |conn| {
                    intents::save(conn, &new)?;
                    continuation::save_continuation_ref(conn, &sid, &side)
                })?;
                intent = Some(new);
                checkpoint("intent")?;
                side
            }
        };
        let Some(mut intent) = intent else {
            // Legacy receipts did not retain a project/target. Never guess a new
            // Default-workspace session if its original native row is gone.
            if existing.is_none() {
                return Err(stale("legacy continuation target is unavailable"));
            }
            continuation_state::recover_unlocked(&sid, &side, edited_text)?;
            consume_published_draft(&conn, draft_id)?;
            return Ok(sid);
        };
        if intent.meta.id != sid
            || intent.draft_id != draft_id
            || intent.draft_hash != draft_hash
            || edited_text
                .as_ref()
                .filter(|text| !text.is_empty())
                .is_some_and(|text| text != &intent.text)
        {
            return Err(stale("retry must use the original creation input"));
        }
        if intent.cancelled {
            return Err(stale("continuation creation was cancelled"));
        }
        if intent.published && existing.is_none() {
            return Err(stale(
                "created continuation was removed; it will not be recreated",
            ));
        }
        if !intent.published && existing.is_none() {
            validate_history(&conn, &side)?;
            if let Some(workspace) = &intent.workspace {
                continuation_target::validate(workspace)?;
            }
            crate::store::import_continuation::validate(&intent.meta).map_err(error)?;
            validate_pending_files(&intent)?;
        }
        // Never replace user messages or edited handoff text on an ordinary retry.
        let messages = side_path.with_file_name("messages.json");
        if existing.is_none() && !messages.try_exists().map_err(error)? {
            continuation_state::write_file(&messages, b"[]")?;
        }
        let handoff = side_path.with_file_name("handoff_draft.txt");
        if !handoff.try_exists().map_err(error)? {
            if !matches!(
                side.state,
                DispatchState::Prepared | DispatchState::FailedBeforeDispatch
            ) {
                return Err(ImportError::new(
                    ErrorCode::DispatchUnknown,
                    "cannot reconstruct an already dispatched handoff",
                ));
            }
            continuation_state::write_file(&handoff, intent.text.as_bytes())?;
        }
        checkpoint("files")?;
        continuation_state::recover_unlocked(&sid, &side, None)?;
        checkpoint("sidecar")?;
        if !intent.published {
            if existing.is_none() {
                crate::store::import_continuation::publish(&intent.meta).map_err(error)?;
            }
            checkpoint("index")?;
            intent.published = true;
            // Keep receipt completion and draft consumption inside the native
            // writer lock. A later confirmation must never race a cleanup write
            // performed after this lock is released.
            catalog::write_under_store_lock(&conn, |conn| {
                intents::save(conn, &intent)?;
                catalog::delete_draft(conn, draft_id)
            })?;
        } else {
            consume_published_draft(&conn, draft_id)?;
        }
        return Ok(sid);
    }
    Err(ImportError::new(
        ErrorCode::SourceBusy,
        "creation request changed concurrently; retry",
    ))
}

// Caller still owns the native writer lock. Older completed receipts can
// retain a draft; ordinary retries without one need no new SQLite write.
fn consume_published_draft(conn: &rusqlite::Connection, draft_id: &str) -> Result<(), ImportError> {
    if catalog::load_draft_json(conn, draft_id)?.is_some() {
        catalog::write_under_store_lock(conn, |conn| catalog::delete_draft(conn, draft_id))?;
    }
    Ok(())
}

fn validate_pending_files(intent: &Intent) -> Result<(), ImportError> {
    let side_path = continuation::sidecar_path(&intent.meta.id);
    for (name, expected) in [
        ("messages.json", "[]"),
        ("handoff_draft.txt", intent.text.as_str()),
    ] {
        let path = side_path.with_file_name(name);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() == expected.len() as u64 => {
                if std::fs::read(&path).map_err(error)? != expected.as_bytes() {
                    return Err(error("unpublished continuation content changed"));
                }
            }
            Ok(_) => return Err(error("unpublished continuation content changed")),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(error(err)),
        }
    }
    Ok(())
}

/// Cancel only unpublished creation. Keep the cancelled receipt so stale retries
/// cannot resurrect it. Remove only the three files owned by this creation.
pub(super) fn discard(draft: &HandoffDraft) -> Result<(), ImportError> {
    let conn = catalog::open_catalog()?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(error)?;
    for _ in 0..3 {
        #[cfg(test)]
        tests::before_request_lookup();
        let known =
            lookup_before_store_lock(&conn, |conn| intents::for_draft(conn, &draft.draft_id))?;
        let mut paths = vec![crate::paths::sessions_index_file()];
        if let Some(intent) = &known {
            paths.push(continuation::sidecar_path(&intent.meta.id));
        }
        let _locks = crate::store_lock::lock_exclusive_many(&paths).map_err(error)?;
        let current = intents::for_draft(&conn, &draft.draft_id)?;
        if known.as_ref().map(|i| &i.meta.id) != current.as_ref().map(|i| &i.meta.id) {
            continue;
        }
        if let Some(mut intent) = current {
            if intent.draft_hash != draft.draft_hash {
                return Err(stale("draft identity mismatch"));
            }
            if intent.published
                || crate::store::load_sessions_snapshot()
                    .map_err(error)?
                    .iter()
                    .any(|row| row.id == intent.meta.id)
            {
                // Closing the review never deletes a native chat, including a
                // deliberately deleted chat whose creation reply was lost.
                intent.published = true;
                intents::save(&conn, &intent)?;
                catalog::write_under_store_lock(&conn, |conn| {
                    catalog::delete_draft(conn, &draft.draft_id)
                })?;
                return Ok(());
            }
            let (_, side) = continuation::load_ref_by_request(&conn, &intent.request_id)?
                .ok_or_else(|| error("creation reference missing"))?;
            let local = continuation::read_sidecar_strict(&intent.meta.id)?;
            if side.state != DispatchState::Prepared
                || local.is_some_and(|s| s.state != DispatchState::Prepared)
            {
                return Err(stale("cannot discard a dispatched continuation"));
            }
            validate_pending_files(&intent)?;
            intent.cancelled = true;
            intents::save(&conn, &intent)?;
            for name in [
                "messages.json",
                "handoff_draft.txt",
                "import_continuation.json",
            ] {
                let path = continuation::sidecar_path(&intent.meta.id).with_file_name(name);
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(error(err)),
                }
            }
        }
        catalog::write_under_store_lock(&conn, |conn| {
            catalog::delete_draft(conn, &draft.draft_id)
        })?;
        return Ok(());
    }
    Err(stale("creation changed concurrently; retry cancellation"))
}

#[cfg(test)]
thread_local! { static FAIL_AT: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
fn checkpoint(_point: &str) -> Result<(), ImportError> {
    #[cfg(test)]
    if FAIL_AT.with(|value| value.borrow().as_ref() == Some(&_point)) {
        return Err(error(format!("synthetic creation failure at {_point}")));
    }
    Ok(())
}

#[cfg(test)]
#[path = "continuation_create_tests.rs"]
mod tests;
