//! Desired local main selection and retirement of incompatible owned processes.
//! Observed receipts select history; they never select the desired channel.
use super::*;
use crate::{
    acp_client::AcpClient,
    providers::{MainHistoryRequest, MainProfileRequest},
    session_execution::ExecutionSource,
    store::{AppSettings, ComposerPrefs, SessionMeta},
};

pub(super) fn needs_profile(meta: &SessionMeta) -> bool {
    meta.provider_id.is_some()
        || meta
            .agent_execution
            .as_ref()
            .is_some_and(|receipt| matches!(receipt.source, ExecutionSource::AppProfile { .. }))
}

// Blocking worker / tests only when a legacy row has no explicit selection.
fn selection(
    meta: &SessionMeta,
    prefs: &ComposerPrefs,
) -> Result<Option<(String, String)>, String> {
    if !needs_profile(meta) {
        return Ok(None);
    }
    let model = prefs.model_id.trim();
    if model.is_empty() {
        return Err("main execution selection has no request model".into());
    }
    if let Some(provider) = &meta.provider_id {
        if provider.trim().is_empty()
            || meta
                .model_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        {
            return Err("explicit channel requires a saved request model".into());
        }
        let provider = if provider.trim().eq_ignore_ascii_case("official") {
            "official".into()
        } else {
            provider.trim().to_owned()
        };
        return Ok(Some((provider, model.into())));
    }
    // A profile receipt may survive on a row that follows the global default.
    // Capture that desired default read-only; never infer it from the receipt.
    crate::providers::configured_main_selection(model).map(Some)
}

pub(super) fn matches_client(
    meta: &SessionMeta,
    prefs: &ComposerPrefs,
    client: &AcpClient,
) -> Result<bool, String> {
    if client.validate_execution_resume(meta).is_err() {
        return Ok(false);
    }
    Ok(match selection(meta, prefs)? {
        Some((provider, model)) => {
            client.matches_main_selection(&meta.id, &provider, &model)
                && client
                    .agent_session_id()
                    .is_none_or(|agent| meta.agent_session_id.as_deref() == Some(agent.as_str()))
        }
        None => !matches!(
            client.execution_source(),
            ExecutionSource::AppProfile { .. }
        ),
    })
}

pub(super) fn request(
    meta: &SessionMeta,
    prefs: &ComposerPrefs,
    settings: &AppSettings,
    cwd: &str,
    ssh: bool,
) -> Result<Option<MainProfileRequest>, String> {
    let Some((provider_id, model_id)) = selection(meta, prefs)? else {
        return Ok(None);
    };
    if ssh
        || settings
            .acp_server_addr
            .as_deref()
            .is_some_and(|addr| !addr.trim().is_empty())
    {
        return Err("local channel selection cannot use a remote agent transport".into());
    }
    if meta.agent_session_id.is_none() && meta.agent_execution.is_some() {
        return Err("execution receipt has no matching agent history".into());
    }
    let history = if meta.agent_session_id.is_some() {
        // Receipt-less rows use the unchanged legacy baseline, captured before
        // preparing the desired channel. Never use provider_id for this choice.
        let legacy_source = if meta.agent_execution.is_none() {
            crate::providers::ProcessConfigBinding::current(settings)?
                .1
                .execution_source()
        } else {
            ExecutionSource::Unavailable
        };
        Some(MainHistoryRequest {
            session: meta.clone(),
            cwd: cwd.into(),
            legacy_source,
        })
    } else {
        None
    };
    Ok(Some(MainProfileRequest {
        owner_session_id: meta.id.clone(),
        provider_id,
        model_id,
        history,
    }))
}

impl SessionManager {
    pub(super) async fn queue_main_route_rebind(
        &self,
        meta: &SessionMeta,
        prefs: &ComposerPrefs,
    ) -> Result<(), String> {
        let client = self
            .with_session_mut(&meta.id, |session| session.acp.clone())
            .flatten()
            .or_else(|| {
                self.parked
                    .lock()
                    .get(&meta.id)
                    .map(|session| session.acp.clone())
            });
        let Some(client) = client else {
            return Ok(());
        };
        let observed = meta.clone();
        let desired = prefs.clone();
        let checked_client = client.clone();
        let matches = tokio::task::spawn_blocking(move || {
            let current = workspace_context::session(&observed.id)?;
            if current.agent_session_id != observed.agent_session_id
                || current.agent_execution != observed.agent_execution
                || current.provider_id != observed.provider_id
                || current.model_id != observed.model_id
                || workspace_context::Binding::from(&current)
                    != workspace_context::Binding::from(&observed)
            {
                return Err("main execution observation changed during selection check".into());
            }
            matches_client(&observed, &desired, &checked_client)
        })
        .await
        .map_err(|_| "main selection check worker failed")??;
        let current = self
            .with_session_mut(&meta.id, |session| session.acp.clone())
            .flatten()
            .or_else(|| {
                self.parked
                    .lock()
                    .get(&meta.id)
                    .map(|session| session.acp.clone())
            });
        if !current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &client))
        {
            return Err("main execution owner changed during selection check".into());
        }
        if !matches {
            // The existing flush keeps a busy turn alive and only retires an
            // idle live/background/parked process. No map lock crosses IO.
            self.pending_soft_respawn
                .lock()
                .insert(meta.id.clone(), "session_channel".into());
        }
        Ok(())
    }
}
