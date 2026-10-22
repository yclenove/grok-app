//! Runtime-setting changes for owned main processes, including off-focus actors.
use super::SessionManager;
use std::collections::HashSet;

impl SessionManager {
    /// An owned generation's MCP flags and definitions are immutable. Rebuild
    /// instead of hot-injecting a list from the mutable global channel. Busy
    /// actors retain their current turn; the normal flush retires idle actors.
    pub(super) fn queue_main_runtime_mcp_rebinds(&self) -> Vec<String> {
        let mut owners = HashSet::new();
        {
            let live = self.inner.lock();
            if let Some(session) = live.as_ref() {
                if session
                    .acp
                    .as_ref()
                    .is_some_and(|client| client.is_alive() && client.has_main_runtime())
                {
                    owners.insert(session.app_session_id.clone());
                }
            }
        }
        for session in self.background.lock().values() {
            if session
                .acp
                .as_ref()
                .is_some_and(|client| client.is_alive() && client.has_main_runtime())
            {
                owners.insert(session.app_session_id.clone());
            }
        }
        for session in self.parked.lock().values() {
            if session.acp.is_alive() && session.acp.has_main_runtime() {
                owners.insert(session.app_session_id.clone());
            }
        }
        let mut pending = self.pending_soft_respawn.lock();
        for owner in &owners {
            pending.insert(owner.clone(), "extensions_mcp".into());
        }
        owners.into_iter().collect()
    }
}
