//! Strict persistence of explicit chat choices, never a draft-default fallback.
use super::{update_sessions_index_strict, Utc};

pub(super) fn validate(id: &str) -> Result<(), String> {
    // A no-op strict transaction rejects missing/corrupt targets before the
    // settings loader can run its recovering legacy migrations. Save rechecks
    // under the actual writer lock, so deletion after validation is still safe.
    save(id, None, None, None, None, None)
}

pub(super) fn save(
    id: &str,
    model: Option<&str>,
    effort: Option<&str>,
    mode: Option<&str>,
    policy: Option<&str>,
    provider: Option<&str>,
) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("session not found".into());
    }
    update_sessions_index_strict(|list| {
        let row = list
            .iter_mut()
            .find(|row| row.id == id)
            .ok_or_else(|| "session not found".to_string())?;
        // Recheck the route inside the writer boundary. A provider always
        // commits with its request model, never as a separate partial update.
        let selected_provider = provider
            .map(|provider| crate::providers::validate_session_selection(provider, model))
            .transpose()?;
        if let Some(provider) = selected_provider {
            row.provider_id = Some(provider);
        }
        if let Some(model) = model {
            row.model_id = Some(
                if provider.is_some() {
                    model.trim()
                } else {
                    model
                }
                .into(),
            );
        }
        if let Some(effort) = effort {
            row.effort = Some(effort.into());
        }
        if let Some(mode) = mode {
            row.mode = Some(mode.into());
        }
        if let Some(policy) = policy {
            row.permission_policy = Some(policy.into());
        }
        if model.is_some()
            || effort.is_some()
            || mode.is_some()
            || policy.is_some()
            || provider.is_some()
        {
            row.updated_at = Utc::now();
        }
        Ok(())
    })
}
