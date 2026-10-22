//! Read-only validation of a chat's desired channel/model, not activation.
use super::*;

#[cfg(test)]
#[path = "providers_session_selection_tests.rs"]
mod tests;

/// Read-only global intent for an existing profile row that follows defaults.
/// This returns a channel/request id pair, never an execution history source.
pub(crate) fn configured_main_selection(model: &str) -> Result<(String, String), String> {
    let snapshot =
        ConfigSnapshot::parse(&ProcessConfigBinding::read_config(&agent_config_toml())?)?;
    let provider = snapshot.default_model()?.unwrap_or("grok");
    if is_reserved_custom_provider_id(provider) {
        return Ok(("official".into(), model.into()));
    }
    let fields = snapshot
        .model_fields(provider)?
        .ok_or("configured main channel is unavailable")?;
    let model = if model == provider {
        fields
            .get("model")
            .map(String::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or("configured main channel has no request model")?
    } else {
        model
    };
    Ok((provider.into(), model.trim().into()))
}

pub(crate) fn validate(provider: &str, model: Option<&str>) -> Result<String, String> {
    let provider = provider.trim();
    let model = model.map(str::trim).filter(|id| !id.is_empty());
    let model = model.ok_or("provider selection requires a request model")?;
    if provider.eq_ignore_ascii_case("official") {
        let runtime =
            ConfigSnapshot::parse(&ProcessConfigBinding::read_config(&agent_config_toml())?)?;
        // Independent main configuration may contain a custom API-key pin.
        // Official auth policy always comes from its own CLI source, without
        // auth.json or a secret-store read in this preference validation path.
        let official = ConfigSnapshot::parse(&ProcessConfigBinding::read_config(
            &crate::process_util::user_home().join(".grok/config.toml"),
        )?)?;
        runtime.project_official(model, official, None)?;
        return Ok("official".into());
    }
    if provider.is_empty() || is_reserved_custom_provider_id(provider) {
        return Err("provider selection is invalid".into());
    }
    // Do not use list_custom_providers/active_route: those can repair config,
    // create homes or downgrade a missing section to the official channel.
    let text = ProcessConfigBinding::read_config(&agent_config_toml())?;
    let snapshot = ConfigSnapshot::parse(&text)?;
    // Validate that the exact snapshot can form an isolated execution config,
    // including its explicit provider/helper references. Do not change the
    // source or select the current global default as a fallback.
    snapshot.project_custom(provider, model)?;
    Ok(provider.into())
}

pub(super) fn custom_model(
    snapshot: &ConfigSnapshot,
    provider: &str,
    model: &str,
) -> Result<ProviderModelEntry, String> {
    let fields = match snapshot.model_fields(provider)? {
        Some(fields) if is_custom(&fields) => fields,
        _ => return Err("selected provider is unavailable".into()),
    };
    if fields
        .get("api_key")
        .is_none_or(|key| key.trim().is_empty())
    {
        return Err("selected provider has no API key".into());
    }
    let base = fields
        .get("base_url")
        .ok_or("selected provider has no endpoint")?;
    let url = reqwest::Url::parse(base).map_err(|_| "selected provider endpoint is invalid")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("selected provider endpoint is invalid".into());
    }
    let mode = fields
        .get(APP_PROVIDER_MODE_KEY)
        .map(String::as_str)
        .unwrap_or("generic");
    let backend = fields
        .get("api_backend")
        .map(String::as_str)
        .unwrap_or("responses");
    if !matches!(mode, PROVIDER_MODE_GENERIC | PROVIDER_MODE_GROK_BUILD_PROXY)
        || !matches!(backend, "responses" | "chat_completions" | "messages")
        || (mode == PROVIDER_MODE_GROK_BUILD_PROXY && backend != "responses")
    {
        return Err("selected provider protocol is invalid".into());
    }
    if let Some(raw) = fields.get(APP_MODELS_KEY) {
        serde_json::from_str::<Vec<ProviderModelEntry>>(raw)
            .map_err(|_| "selected provider model catalog is invalid")?;
    }
    let configured = fields.get("model").map(String::as_str).unwrap_or(provider);
    let models = decode_app_models(
        fields.get(APP_MODELS_KEY).map(String::as_str),
        configured,
        configured,
    );
    models
        .into_iter()
        .find(|entry| entry.id == model)
        .ok_or_else(|| "selected request model is unavailable on this provider".into())
}
