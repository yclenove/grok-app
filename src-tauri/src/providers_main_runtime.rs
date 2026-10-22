//! Main-process capability capture. No later global route/config lookup.
use super::{
    main_mcp_auth::MainMcpSources, CapturedMcpServer, ConfigSnapshot, ProcessModelBinding,
};
use crate::{
    extensions::ExtensionsPrefs,
    models_aux::{HostVisionContext, VisionEndpoint},
    store::AppSettings,
};
use std::sync::Arc;

/// Credential-free decisions, separate from private MCP/auxiliary definitions.
#[derive(Clone, Copy)]
pub(crate) struct MainRuntimePolicy {
    pub inject_official_aux: bool,
    pub load_user_mcp: bool,
}

/// Private definitions may contain credentials. No Debug/serde/UI surface.
pub(crate) struct MainRuntime {
    policy: MainRuntimePolicy,
    mcp: Vec<CapturedMcpServer>,
    prefs: ExtensionsPrefs,
    text_only_models: Vec<(String, bool)>,
    default_text_only: bool,
    custom: bool,
    vision_endpoint: Option<Arc<VisionEndpoint>>,
}

impl MainRuntime {
    pub(super) fn capture(
        final_config: &str,
        source_config: &str,
        mcp_sources: MainMcpSources<'_>,
        binding: &ProcessModelBinding,
        inject: bool,
        settings: &AppSettings,
        prefs: &ExtensionsPrefs,
    ) -> Result<Self, String> {
        let snapshot = ConfigSnapshot::parse(final_config)?;
        let source = ConfigSnapshot::parse(source_config)?;
        let custom = !matches!(binding, ProcessModelBinding::Catalog { .. });
        let inject = custom && inject;
        let policy = MainRuntimePolicy {
            inject_official_aux: inject,
            load_user_mcp: !inject || settings.official_aux_with_user_mcp,
        };
        let mut text_only_models = Vec::new();
        let default_text_only = match binding {
            ProcessModelBinding::Catalog { .. } => false,
            ProcessModelBinding::Generic {
                provider_id,
                request_model,
            } => capability(&source, provider_id, request_model)?,
            ProcessModelBinding::Native {
                provider_id,
                models,
                fallback,
            } => {
                for model in models {
                    text_only_models
                        .push((model.clone(), capability(&source, provider_id, model)?));
                }
                capability(&source, provider_id, fallback)?
            }
        };
        // Host auxiliary endpoints stay outside the projected main home. Resolve
        // an explicit image-description slot from the already-captured source,
        // never from a later global provider selection.
        let vision_endpoint = if custom {
            captured_vision_endpoint(&source)?.map(Arc::new)
        } else {
            None
        };
        snapshot.mcp_definitions()?;
        let mcp = if policy.load_user_mcp {
            mcp_sources.capture(&snapshot)?
        } else {
            Vec::new()
        };
        Ok(Self {
            policy,
            mcp,
            prefs: prefs.clone(),
            text_only_models,
            default_text_only,
            custom,
            vision_endpoint,
        })
    }

    pub(crate) fn mcp_servers_for_connect(
        &self,
        project_cwd: Option<&str>,
        app_session_id: Option<&str>,
    ) -> serde_json::Value {
        crate::extensions::build_main_mcp_servers_for_connect(
            &self.mcp,
            &self.prefs,
            self.policy,
            project_cwd,
            app_session_id,
        )
    }

    pub(crate) fn vision_context(&self, model_echo: Option<&str>) -> HostVisionContext {
        let text_only = if self.text_only_models.is_empty() {
            self.default_text_only
        } else {
            model_echo.map_or(self.default_text_only, |model| {
                // An unknown native echo cannot authorize sending image pixels.
                self.text_only_models
                    .iter()
                    .find(|(id, _)| id == model)
                    .is_none_or(|(_, text_only)| *text_only)
            })
        };
        HostVisionContext::captured(
            self.custom && text_only,
            self.policy.inject_official_aux,
            self.vision_endpoint.clone(),
        )
    }

    /// Model-dependent hooks/config are prepared once per generation. Native
    /// switches across the text/vision boundary need a fresh preparation too.
    pub(crate) fn matches_model_policy(&self, requested: &str) -> bool {
        self.text_only_models.is_empty()
            || self.text_only_models.iter().any(|(model, text_only)| {
                model == requested && *text_only == self.default_text_only
            })
    }
}

fn capability(snapshot: &ConfigSnapshot, provider: &str, model: &str) -> Result<bool, String> {
    let fields = snapshot
        .model_fields(provider)?
        .ok_or("main model capability is unavailable")?;
    let models = super::decode_app_models(
        fields.get(super::APP_MODELS_KEY).map(String::as_str),
        model,
        model,
    );
    Ok(crate::models_aux::custom_model_is_text_only(
        provider,
        fields.get("name").map(String::as_str).unwrap_or(provider),
        model,
        &models,
        super::supports_vision_from_fields(&fields),
    ))
}

fn captured_vision_endpoint(snapshot: &ConfigSnapshot) -> Result<Option<VisionEndpoint>, String> {
    let Some(target) = snapshot
        .model_slot("image_description")?
        .map(str::trim)
        .filter(|target| !target.is_empty() && *target != crate::models_aux::AUTO)
    else {
        return Ok(None);
    };
    let Some(fields) = snapshot.model_fields(target)? else {
        return Ok(None);
    };
    let Some(key) = fields.get("api_key").filter(|key| !key.trim().is_empty()) else {
        return Ok(None);
    };
    let official = super::is_official_catalog_model(target);
    let captured_base = crate::relay_stream_proxy::effective_upstream_base(&fields);
    let base = if captured_base.is_empty() && official {
        crate::models_aux::OFFICIAL_GROK_BASE_URL
    } else {
        &captured_base
    };
    if base.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(VisionEndpoint {
        base_url: base.into(),
        model: fields
            .get("model")
            .filter(|model| !model.trim().is_empty())
            .map(String::as_str)
            .unwrap_or(target)
            .into(),
        api_key: key.clone(),
        api_backend: fields.get("api_backend").cloned().unwrap_or_else(|| {
            if official {
                crate::models_aux::OFFICIAL_GROK_API_BACKEND
            } else {
                "chat_completions"
            }
            .into()
        }),
    }))
}

#[cfg(test)]
#[path = "providers_main_runtime_tests.rs"]
mod tests;
