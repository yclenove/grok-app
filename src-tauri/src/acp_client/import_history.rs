//! Bind import capability evidence to the ACP session that actually opened.
use super::AcpClient;
use crate::session_migration::{capability, continuation, errors::ImportError, mcp_stdio};
use std::collections::HashMap;

// A new session may advertise tools before its session/new reply. Buffer only a
// small number of stamped snapshots, then retain the returned session's entry.
const MAX_PENDING_SESSIONS: usize = 8;

#[derive(Default)]
pub(super) struct ImportHistoryBinding {
    pub grant: Option<mcp_stdio::ImportHistoryGrant>,
    agent_session_id: Option<String>,
    opening: bool,
    pending: HashMap<String, Vec<String>>,
}

impl ImportHistoryBinding {
    pub fn new(grant: Option<mcp_stdio::ImportHistoryGrant>) -> Self {
        Self {
            grant,
            ..Self::default()
        }
    }

    fn record(&self, names: Vec<String>) -> Result<(), ImportError> {
        if let Some(grant) = &self.grant {
            continuation::record_advertised_tools(
                &grant.app_session_id,
                names,
                capability::ProbeEvidence::AgentAdvertised,
            )?;
        }
        Ok(())
    }
}

impl AcpClient {
    pub(super) fn begin_import_history_open(&self) -> Result<(), ImportError> {
        let mut binding = self.import_history_grant.lock();
        binding.agent_session_id = None;
        binding.pending.clear();
        binding.opening = false;
        if let Some(grant) = &binding.grant {
            // Persisted proof describes a previous connection, not this one.
            // A failed reset must fail the open, not reuse that stale proof.
            continuation::record_advertised_tools(
                &grant.app_session_id,
                vec![],
                capability::ProbeEvidence::SelfToolsList,
            )?;
            binding.opening = self.session_mcp_policy().allows_local_import_mcp();
        }
        Ok(())
    }

    pub(super) fn bind_import_history_session(&self, sid: &str) -> Result<(), ImportError> {
        let mut binding = self.import_history_grant.lock();
        if !binding.opening {
            return Ok(());
        }
        binding.opening = false;
        binding.agent_session_id = Some(sid.to_string());
        let names = binding.pending.remove(sid);
        binding.pending.clear();
        if let Some(names) = names {
            binding.record(names)?;
        }
        Ok(())
    }

    pub(super) fn observe_import_history_tools(&self, params: &serde_json::Value) {
        let Some(sid) = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .filter(|sid| !sid.is_empty())
        else {
            return;
        };
        let Some(names) = capability::advertised_import_tools_snapshot(params) else {
            return;
        };
        let mut binding = self.import_history_grant.lock();
        if binding.grant.is_none() {
            return;
        }
        if binding.agent_session_id.as_deref() == Some(sid) {
            if let Err(error) = binding.record(names) {
                tracing::error!(
                    code = error.code.as_str(),
                    "failed to persist session-scoped import tool evidence"
                );
            }
        } else if binding.opening
            && (binding.pending.len() < MAX_PENDING_SESSIONS || binding.pending.contains_key(sid))
        {
            binding.pending.insert(sid.to_string(), names);
        }
    }
}
