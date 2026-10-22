//! Explicit session context RPCs and first-empty-chat window alignment.
use super::{connected_composer::ConnectedOwner, *};

impl SessionManager {
    fn context_owner(&self, id: &str) -> Result<ConnectedOwner, String> {
        self.with_session_mut(id, |s| ConnectedOwner::capture(s))
            .ok_or("No active session")?
    }

    pub async fn set_context_window(
        &self,
        tokens: u64,
        session_id: Option<&str>,
    ) -> Result<(), String> {
        if tokens == 0 {
            return Err("context window must be positive".into());
        }
        let catalog = crate::models_catalog::list_available_models();
        let Some(id) = session_id.map(str::trim).filter(|id| !id.is_empty()) else {
            if !catalog
                .models
                .iter()
                .any(|model| crate::models_catalog::model_supports_context_window(model, tokens))
            {
                return Err(format!(
                    "context window {tokens} is not one of the advertised sizes"
                ));
            }
            *self.pending_context_window.lock() = Some(tokens);
            return Ok(());
        };
        let _connect = self.connect_lock.lock().await;
        let owner = self.context_owner(id)?;
        let prefs = owner.prefs(self, id)?;
        let model = owner.client.resolve_agent_model(&prefs.model_id);
        if !catalog.models.iter().any(|entry| {
            (entry.id == model || entry.id == prefs.model_id)
                && crate::models_catalog::model_supports_context_window(entry, tokens)
        }) {
            return Err(format!(
                "model '{model}' does not support context window {tokens}"
            ));
        }
        Self::with_soft_rpc_budget(owner.client.set_context_window_for(
            &owner.agent_id,
            &model,
            tokens,
        ))
        .await?;
        owner.check(self, id)?;
        // An existing chat never consumes a different new-chat draft's choice.
        Ok(())
    }

    pub async fn session_context_info(
        &self,
        session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let id = session_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or("No active session")?;
        let _connect = self.connect_lock.lock().await;
        let owner = self.context_owner(id)?;
        owner.prefs(self, id)?;
        let info = Self::with_soft_rpc_budget(owner.client.session_info(&owner.agent_id)).await?;
        owner.check(self, id)?;
        Ok(info)
    }

    pub(super) async fn align_model_context(
        &self,
        id: &str,
        owner: &ConnectedOwner,
        model: &str,
        new_empty_session: bool,
    ) -> Result<(), String> {
        let staged = new_empty_session
            .then(|| *self.pending_context_window.lock())
            .flatten();
        let catalog = crate::models_catalog::list_available_models();
        let entry = catalog.models.iter().find(|entry| entry.id == model);
        let window = if new_empty_session {
            staged.or_else(|| entry.and_then(crate::models_catalog::app_default_context_window))
        } else {
            None
        };
        if let Some(window) = window.filter(|window| {
            entry.is_some_and(|entry| {
                crate::models_catalog::model_supports_context_window(entry, *window)
            })
        }) {
            match Self::with_soft_rpc_budget(owner.client.set_context_window_for(
                &owner.agent_id,
                model,
                window,
            ))
            .await
            {
                Ok(()) => {
                    owner.check(self, id)?;
                    let mut pending = self.pending_context_window.lock();
                    if *pending == staged {
                        *pending = None;
                    }
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!("acp set_context_window after session open soft-fail: {error}")
                }
            }
            owner.check(self, id)?;
        }
        // Old agents may reject context metadata. Preserve model alignment;
        // keep an unconfirmed draft choice available instead of reporting it applied.
        Self::with_soft_rpc_budget(owner.client.set_model_for(&owner.agent_id, model)).await
    }
}
