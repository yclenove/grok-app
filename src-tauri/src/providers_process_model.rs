//! Immutable, credential-free model routing for one initialized ACP transport.
use super::*;

#[cfg(test)]
#[path = "providers_process_model_tests.rs"]
mod tests;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ProcessModelBinding {
    Catalog {
        blocked_aliases: Vec<String>,
    },
    Generic {
        provider_id: String,
        request_model: String,
    },
    Native {
        provider_id: String,
        models: Vec<String>,
        fallback: String,
    },
}

impl ProcessModelBinding {
    pub(crate) fn catalog() -> Self {
        Self::Catalog {
            blocked_aliases: Vec::new(),
        }
    }

    pub(crate) fn resolve(&self, requested: &str) -> String {
        let requested = requested.trim();
        match self {
            Self::Catalog { blocked_aliases } => {
                if requested.is_empty()
                    || requested == OFFICIAL_DEFAULT_MODEL
                    || blocked_aliases.iter().any(|id| id == requested)
                {
                    OFFICIAL_CATALOG_MODEL.to_owned()
                } else {
                    requested.to_owned()
                }
            }
            Self::Generic { provider_id, .. } => provider_id.clone(),
            Self::Native {
                models, fallback, ..
            } => {
                if models.iter().any(|id| id == requested) {
                    requested.to_owned()
                } else {
                    fallback.clone()
                }
            }
        }
    }

    pub(crate) fn matches_main_selection(&self, provider: &str, model: &str) -> bool {
        match self {
            Self::Catalog { .. } => provider == "official" && self.resolve(model) == model,
            Self::Generic {
                provider_id,
                request_model,
            } => provider == provider_id && model == request_model,
            Self::Native {
                provider_id,
                models,
                ..
            } => provider == provider_id && models.iter().any(|candidate| candidate == model),
        }
    }

    /// Native environment and later ACP model resolution share this exact
    /// config snapshot. Never read a new global default during live switching.
    pub(crate) fn capture_for_spawn(
        custom: bool,
        ssh: bool,
        override_home: bool,
        requested: &str,
        home: &Path,
        settings: &crate::store::AppSettings,
    ) -> Result<
        (
            Self,
            Option<GrokBuildProxySpawn>,
            Option<ProcessConfigBinding>,
        ),
        String,
    > {
        if override_home {
            return Ok((Self::catalog(), None, None));
        }
        let profile = if ssh {
            None
        } else {
            crate::session_execution::ExecutionSource::from_app_profile_home(home)?
        };
        let text = if profile.is_some() {
            ProcessConfigBinding::read_profile_config(home)?
        } else if ssh {
            read_text(&agent_config_toml())
        } else {
            ProcessConfigBinding::read_config(&agent_config_toml())?
        };
        if !ssh && ProcessConfigBinding::custom_from_text(&text)? != custom {
            return Err("provider route changed during spawn preparation".into());
        }
        let (model, mut native) = Self::from_text(&text, custom, ssh, requested)?;
        let configuration = if ssh {
            None
        } else {
            Some(ProcessConfigBinding::from_capture(&text, home, settings)?)
        };
        if profile.is_some() {
            if let (Self::Native { provider_id, .. }, Some(native)) = (&model, &mut native) {
                let fields = ConfigSnapshot::parse(&text)?
                    .model_fields(provider_id)?
                    .ok_or("execution relay model is unavailable")?;
                if let Some(base) = fields
                    .get("base_url")
                    .filter(|base| crate::relay_stream_proxy::is_profile_url(base))
                {
                    // The verified profile route is already leased by the
                    // private config binding. Never unwrap it into a direct
                    // native upstream and bypass SSE filtering/cancellation.
                    native.base_url = base.trim_end_matches('/').into();
                    native.models_url = models_list_endpoint(&native.base_url)
                        .map_err(|_| "execution relay models endpoint is invalid")?;
                }
            }
        }
        Ok((model, native, configuration))
    }

    pub(super) fn from_text(
        text: &str,
        custom: bool,
        ssh: bool,
        requested: &str,
    ) -> Result<(Self, Option<GrokBuildProxySpawn>), String> {
        // SSH remains remote-owned. A local provider file cannot certify or
        // reject the remote server's configuration/credentials.
        if ssh {
            return Ok((
                Self::Catalog {
                    blocked_aliases: parse_model_sections(text)
                        .into_iter()
                        .filter(|section| {
                            section.id != OFFICIAL_DEFAULT_MODEL && section.id != "official"
                        })
                        .map(|section| section.id)
                        .collect(),
                },
                None,
            ));
        }
        let snapshot = ConfigSnapshot::parse(text)?;
        if !custom {
            let mut blocked_aliases = Vec::new();
            for id in snapshot.model_ids()? {
                if is_reserved_custom_provider_id(&id) {
                    continue;
                }
                // A key-only override retains the real catalog id and endpoint.
                // Other aliases keep the existing conservative fallback, even
                // when their routing fields are arrays or nested header tables.
                if !snapshot.key_only_catalog_override(&id)? {
                    blocked_aliases.push(id);
                }
            }
            return Ok((Self::Catalog { blocked_aliases }, None));
        }
        let default = snapshot
            .default_model()?
            .filter(|id| !is_reserved_custom_provider_id(id))
            .ok_or("custom process route is missing")?;
        let fields = snapshot
            .model_fields(default)?
            .ok_or("custom process route is missing")?;
        if normalize_provider_mode(fields.get(APP_PROVIDER_MODE_KEY).map(String::as_str))
            != PROVIDER_MODE_GROK_BUILD_PROXY
        {
            return Ok((
                Self::Generic {
                    provider_id: default.into(),
                    request_model: fields
                        .get("model")
                        .map(|model| model.trim())
                        .filter(|model| !model.is_empty())
                        .unwrap_or(default)
                        .to_string(),
                },
                None,
            ));
        }
        let native = grok_build_proxy_spawn_from_fields(default, &fields, requested)
            .ok_or("native process route binding is unavailable")?;
        let configured = fields
            .get("model")
            .map(String::as_str)
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(default);
        let models = decode_app_models(
            fields.get(APP_MODELS_KEY).map(String::as_str),
            configured,
            configured,
        );
        let binding = Self::Native {
            provider_id: default.into(),
            fallback: resolve_active_model(&models, configured),
            models: models.into_iter().map(|model| model.id).collect(),
        };
        Ok((binding, Some(native)))
    }

    /// Only a real wire model echo is evidence. A requested id or a no-echo
    /// set_model ACK cannot prove that the agent changed its sampling model.
    pub(crate) fn execution_model(&self, echoed: Option<&str>) -> (Option<String>, Option<String>) {
        let echoed = echoed.map(str::trim).filter(|id| !id.is_empty());
        match self {
            Self::Generic {
                provider_id,
                request_model,
            } => match echoed {
                Some(id) if id == provider_id => {
                    (Some(provider_id.clone()), Some(request_model.clone()))
                }
                // Body model names can collide across provider sections. A load
                // echo alone is not proof that our configured alias owns it.
                Some(id) if id == request_model => (None, Some(request_model.clone())),
                _ => (None, None),
            },
            Self::Native {
                provider_id,
                models,
                ..
            } => match echoed.filter(|id| models.iter().any(|model| model == id)) {
                Some(model) => (Some(provider_id.clone()), Some(model.into())),
                None => (None, None),
            },
            // A shared CLI home can have non-App model overrides. Do not
            // label its transport official merely because no App relay is active.
            Self::Catalog { .. } => (None, echoed.map(str::to_owned)),
        }
    }
}
