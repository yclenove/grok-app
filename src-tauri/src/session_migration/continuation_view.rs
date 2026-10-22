//! Local continuation review. Never connects an agent or dispatches a prompt.
use super::continuation::{sidecar_path, ContinuationSidecar, DispatchState};
use super::continuation_state;
use super::errors::{storage_error, ErrorCode, ImportError};
use super::hash::sha256_bytes;
use std::io::Read;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationDraftView {
    pub app_session_id: String,
    pub import_id: String,
    pub revision_id: String,
    pub leaf: String,
    pub state: DispatchState,
    pub text: Option<String>,
    pub text_hash: Option<String>,
}

fn validate_id(sid: &str) -> Result<(), ImportError> {
    if uuid::Uuid::parse_str(sid).is_ok_and(|id| id.to_string() == sid) {
        Ok(())
    } else {
        Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "invalid native session identity",
        ))
    }
}

fn editable(state: DispatchState) -> bool {
    matches!(
        state,
        DispatchState::Prepared | DispatchState::FailedBeforeDispatch
    )
}

fn text(sid: &str) -> Result<String, ImportError> {
    let file = std::fs::File::open(sidecar_path(sid).with_file_name("handoff_draft.txt"))
        .map_err(storage_error)?;
    let limit = super::budget::JSON_RECORD_MAX_BYTES;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(storage_error)?;
    if bytes.len() > limit {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "handoff exceeds review limit",
        ));
    }
    let text = String::from_utf8(bytes).map_err(storage_error)?;
    if text.trim().is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "handoff is empty",
        ));
    }
    Ok(text)
}

const DELETED_SESSION: &str = "continuation session no longer exists";

fn published(sid: &str) -> Result<(), ImportError> {
    if crate::store::load_sessions_snapshot()
        .map_err(storage_error)?
        .iter()
        .any(|row| row.id == sid)
    {
        Ok(())
    } else {
        Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            DELETED_SESSION,
        ))
    }
}

/// Runs under the process lock, before any sidecar lock file is opened.
/// `delete_session` removes the index row under that same lock and only then
/// deletes the directory. A delete that already committed is visible here.
/// Fail without creating the session directory when the row or directory is gone.
fn draft_lock_guard(sid: &str) -> Result<(), String> {
    match published(sid) {
        Ok(()) => {}
        Err(error) if error.code == ErrorCode::HistoryAccessUnavailable => {
            return Err(DELETED_SESSION.into());
        }
        Err(error) => return Err(error.message),
    }
    match crate::paths::session_dir(sid).try_exists() {
        Ok(true) => Ok(()),
        Ok(false) => Err(DELETED_SESSION.into()),
        Err(error) => Err(format!("session dir: {error}")),
    }
}

fn draft_lock_error(error: String) -> ImportError {
    if error == DELETED_SESSION || error.starts_with("lock dir missing:") {
        ImportError::new(ErrorCode::HistoryAccessUnavailable, DELETED_SESSION)
    } else {
        storage_error(error)
    }
}

fn lock_draft(sid: &str) -> Result<crate::store_lock::ExclusiveLock, ImportError> {
    crate::store_lock::lock_exclusive_many_guarded(
        &[crate::paths::sessions_index_file(), sidecar_path(sid)],
        &|| draft_lock_guard(sid),
    )
    .map_err(draft_lock_error)
}

fn view(sid: &str, side: ContinuationSidecar) -> Result<ContinuationDraftView, ImportError> {
    let text = if editable(side.state) {
        Some(text(sid)?)
    } else {
        None
    };
    Ok(ContinuationDraftView {
        app_session_id: sid.into(),
        import_id: side.import_id,
        revision_id: side.revision_id,
        leaf: side.branch_leaf,
        state: side.state,
        text_hash: text.as_ref().map(|text| sha256_bytes(text.as_bytes())),
        text,
    })
}

pub fn get(sid: &str) -> Result<Option<ContinuationDraftView>, ImportError> {
    validate_id(sid)?;
    // Ordinary empty chats must not create a catalog, directory or lock file.
    if continuation_state::read_effective(sid)?.is_none() {
        return Ok(None);
    }
    // Same rule as save: locking the sidecar must not recreate a deleted chat.
    let _locks = lock_draft(sid)?;
    published(sid)?;
    continuation_state::read_effective(sid)?
        .map(|side| view(sid, side))
        .transpose()
}

pub fn save(
    sid: &str,
    expected_text_hash: &str,
    edited_text: &str,
) -> Result<ContinuationDraftView, ImportError> {
    validate_id(sid)?;
    if edited_text.trim().is_empty() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "handoff is empty",
        ));
    }
    if edited_text.len() > super::budget::JSON_RECORD_MAX_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "handoff exceeds review limit",
        ));
    }
    // Do not recreate an ordinary/deleted session merely to acquire its lock.
    published(sid)?;
    if continuation_state::read_effective(sid)?.is_none() {
        return Err(ImportError::new(
            ErrorCode::HistoryAccessUnavailable,
            "no continuation draft",
        ));
    }
    // Unlocked gap: delete_session can drop the index row and remove the
    // session directory before this lock is taken. create_dir_all on the
    // sidecar lock, or the draft's temp-file rename, would put that directory
    // back. Re-check under the process lock and never create a missing parent.
    #[cfg(test)]
    before_draft_lock(sid);
    let _locks = lock_draft(sid)?;
    published(sid)?;
    let side = continuation_state::read_effective(sid)?.ok_or_else(|| {
        ImportError::new(ErrorCode::HistoryAccessUnavailable, "no continuation draft")
    })?;
    if !editable(side.state) {
        return Err(ImportError::new(
            ErrorCode::DispatchUnknown,
            "cannot edit an already dispatched handoff",
        ));
    }
    let current = text(sid)?;
    if sha256_bytes(current.as_bytes()) != expected_text_hash {
        // An identical retry after a lost reply is safe, but never overwrites another edit.
        if current == edited_text {
            return view(sid, side);
        }
        return Err(ImportError::new(
            ErrorCode::HandoffChanged,
            "handoff changed since review",
        ));
    }
    continuation_state::write_file_existing(
        &sidecar_path(sid).with_file_name("handoff_draft.txt"),
        edited_text.as_bytes(),
    )?;
    view(sid, side)
}

#[cfg(test)]
type DraftLockHooks = std::sync::Mutex<std::collections::HashMap<String, Box<dyn FnOnce() + Send>>>;

#[cfg(test)]
static BEFORE_DRAFT_LOCK: std::sync::LazyLock<DraftLockHooks> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Pause in the unlocked gap after the draft is known and before the session
/// directory can be created. The hook runs on the save thread and must not
/// acquire store locks: the test calls `delete_session` while it blocks.
#[cfg(test)]
pub fn install_before_draft_lock_hook(sid: &str, hook: impl FnOnce() + Send + 'static) {
    BEFORE_DRAFT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(sid.to_string(), Box::new(hook));
}

#[cfg(test)]
pub fn clear_before_draft_lock_hook(sid: &str) {
    BEFORE_DRAFT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(sid);
}

#[cfg(test)]
fn before_draft_lock(sid: &str) {
    let hook = BEFORE_DRAFT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(sid);
    if let Some(hook) = hook {
        hook();
    }
}
