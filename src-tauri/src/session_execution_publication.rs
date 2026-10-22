//! The opened agent and its history source commit together, or neither does.
use crate::{session_execution::ExecutionReceipt, session_manager::RuntimeWorkspaceContext, store};

pub(crate) fn publish(
    expected: &store::SessionMeta,
    receipt: ExecutionReceipt,
    context: &RuntimeWorkspaceContext,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<store::SessionMeta, String> {
    receipt.validate(Some(&receipt.agent_session_id))?;
    // A profile must still own its marker; there is no scan/global fallback.
    receipt.source.home()?;
    store::update_sessions_index_strict_checked(
        |rows| {
            let current = rows
                .iter_mut()
                .find(|row| row.id == expected.id)
                .ok_or("connection target no longer exists")?;
            if current.agent_session_id != expected.agent_session_id
                || current.agent_execution != expected.agent_execution
                || current.fork_agent_session != expected.fork_agent_session
                || current.fork_rewind_prompt_index != expected.fork_rewind_prompt_index
                || current.worktree_branch != expected.worktree_branch
                || current.is_worktree_session != expected.is_worktree_session
            {
                return Err("connection execution generation changed".into());
            }
            let settings = store::load_settings_snapshot()?;
            context.validate_row_locked(current, &settings)?;
            current.agent_session_id = Some(receipt.agent_session_id.clone());
            current.agent_execution = Some(receipt);
            current.fork_agent_session = false;
            current.fork_rewind_prompt_index = None;
            current.updated_at = chrono::Utc::now().max(current.updated_at);
            // Desired prefs, title, bindings and foreign rows are the CURRENT row,
            // not a whole stale connection snapshot copied over the store.
            Ok(current.clone())
        },
        check,
    )
}
