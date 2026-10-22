//! Process-private config identity. Never log, serialize or expose its digests.
use super::*;
use crate::store::AppSettings;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq)]
enum LocalTransport {
    Native { manual_cli: Option<String> },
    Wsl(crate::wsl_backend::WslLaunch),
}

/// Includes the actual GROK_HOME, not only the App's provider catalog. Raw
/// configuration bytes also bind fields unknown to the App/older CLI version.
/// Only digests and the private endpoint lease are retained; no provider key or
/// header value survives here. The lease is never serialized or logged.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProcessConfigBinding {
    home: PathBuf,
    source: crate::session_execution::ExecutionSource,
    provider_config: [u8; 32],
    home_config: [u8; 32],
    transport: LocalTransport,
    frozen_routes: Option<crate::relay_stream_proxy::FrozenRoutes>,
    profile_cached_token: Option<bool>,
}

impl ProcessConfigBinding {
    pub(super) fn read_profile_config(home: &Path) -> Result<String, String> {
        let actual_home = home
            .canonicalize()
            .map_err(|_| "execution profile configuration is unavailable")?;
        let path = home.join("config.toml");
        let actual = path
            .canonicalize()
            .map_err(|_| "execution profile configuration is unavailable")?;
        if actual != actual_home.join("config.toml")
            || !fs::metadata(&actual).is_ok_and(|metadata| metadata.is_file())
        {
            return Err("execution profile configuration redirects its namespace".into());
        }
        fs::read_to_string(actual)
            .map_err(|_| "execution profile configuration could not be read".into())
    }

    pub(super) fn read_config(path: &Path) -> Result<String, String> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(_) => Err("agent configuration could not be read".into()),
        }
    }

    fn transport(settings: &AppSettings) -> Result<LocalTransport, String> {
        if settings
            .acp_server_addr
            .as_deref()
            .is_some_and(|addr| !addr.trim().is_empty())
        {
            // TCP is server-owned. A local digest cannot certify that server's
            // credentials/config or the lifetime of its session actors.
            return Err("remote transport cannot consume a local prewarm".into());
        }
        Ok(match crate::wsl_backend::resolve_wsl_launch(settings) {
            Some(launch) => LocalTransport::Wsl(launch),
            None => LocalTransport::Native {
                manual_cli: settings
                    .manual_cli_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned),
            },
        })
    }

    pub(super) fn custom_from_text(text: &str) -> Result<bool, String> {
        let snapshot = ConfigSnapshot::parse(text)?;
        let Some(id) = snapshot.default_model()? else {
            return Ok(false);
        };
        if is_reserved_custom_provider_id(id) {
            return Ok(false);
        }
        Ok(snapshot
            .model_fields(id)?
            .is_some_and(|fields| is_custom(&fields)))
    }

    pub(super) fn from_capture(
        text: &str,
        home: &Path,
        settings: &AppSettings,
    ) -> Result<Self, String> {
        let profile = crate::session_execution::ExecutionSource::from_app_profile_home(home)?;
        if profile.is_some() {
            crate::session_execution::verify_profile_config_capture(home, text)?;
        }
        // Main profile consumers captured this exact home config, so its model
        // and private identity must not observe a second read of changed bytes.
        let home_text = if home == agent_home_dir() || profile.is_some() {
            text.to_owned()
        } else {
            Self::read_config(&home.join("config.toml"))?
        };
        let frozen_routes = match &profile {
            Some(crate::session_execution::ExecutionSource::AppProfile {
                owner_session_id,
                generation,
            }) => Self::capture_profile_routes(text, owner_session_id, generation)?,
            _ => None,
        };
        let profile_cached_token = if profile.is_some() {
            if Self::custom_from_text(text)? {
                Some(false)
            } else {
                let bytes = super::profile_io::read_optional(&home.join("auth.json"))?;
                Some(bytes.is_some_and(|bytes| {
                    serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|value| {
                        value.as_object().is_some_and(|entries| {
                            entries.values().any(|entry| {
                                ["key", "access_token", "refresh_token"]
                                    .iter()
                                    .any(|field| {
                                        entry
                                            .get(field)
                                            .and_then(serde_json::Value::as_str)
                                            .is_some_and(|value| !value.is_empty())
                                    })
                            })
                        })
                    })
                }))
            }
        } else {
            None
        };
        Ok(Self {
            home: home.to_owned(),
            source: if let Some(profile) = profile {
                profile
            } else if home == agent_home_dir() {
                crate::session_execution::ExecutionSource::LegacyIndependent
            } else if home == crate::process_util::user_home().join(".grok") {
                crate::session_execution::ExecutionSource::LegacyShared
            } else {
                crate::session_execution::ExecutionSource::Unavailable
            },
            provider_config: Sha256::digest(text.as_bytes()).into(),
            home_config: Sha256::digest(home_text.as_bytes()).into(),
            transport: Self::transport(settings)?,
            frozen_routes,
            profile_cached_token,
        })
    }

    pub(crate) fn execution_source(&self) -> crate::session_execution::ExecutionSource {
        self.source.clone()
    }

    pub(crate) fn profile_cached_token(&self) -> Option<bool> {
        self.profile_cached_token
    }

    fn capture_profile_routes(
        text: &str,
        owner: &str,
        generation: &str,
    ) -> Result<Option<crate::relay_stream_proxy::FrozenRoutes>, String> {
        use crate::relay_stream_proxy as relay;
        let snapshot = ConfigSnapshot::parse(text)?;
        let mut upstreams = std::collections::BTreeMap::new();
        for provider in snapshot.model_ids()? {
            let fields = snapshot
                .model_fields(&provider)?
                .ok_or("execution relay model is unavailable")?;
            let base = fields.get("base_url").map(String::as_str).unwrap_or("");
            if relay::is_legacy_proxy_url(base) {
                return Err("execution profile uses a mutable relay route".into());
            }
            let upstream = relay::effective_upstream_base(&fields);
            if !relay::is_profile_url(base) && !relay::host_needs_stream_sanitize(&upstream) {
                continue;
            }
            let port = relay::ensure_started_blocking()?;
            let expected = relay::profile_proxy_base_url(owner, generation, &provider, port);
            if base.trim_end_matches('/') != expected
                || !reqwest::Url::parse(base)
                    .is_ok_and(|url| url.as_str().trim_end_matches('/') == expected)
            {
                return Err("execution profile relay address does not match its generation".into());
            }
            upstreams.insert(relay::profile_provider_component(&provider), upstream);
        }
        if upstreams.is_empty() {
            return Ok(None);
        }
        relay::FrozenRoutes::register(
            owner,
            generation,
            Sha256::digest(text.as_bytes()).into(),
            upstreams,
        )
        .map(Some)
    }

    pub(crate) fn cancel_frozen_routes(&self) {
        if let Some(routes) = &self.frozen_routes {
            routes.cancel();
        }
    }

    /// Read-only: unlike active_route(), a reuse check must not repair a
    /// provider catalog or create directories while a candidate is selected.
    pub(crate) fn current(settings: &AppSettings) -> Result<(bool, Self), String> {
        let text = Self::read_config(&agent_config_toml())?;
        let custom = Self::custom_from_text(&text)?;
        // Validate the same native/alias contract as spawn, including missing
        // native keys. Never downgrade an invalid native route to official.
        let _ = ProcessModelBinding::from_text(&text, custom, false, OFFICIAL_CATALOG_MODEL)?;
        let home = if custom
            || !settings
                .session_data_mode
                .trim()
                .eq_ignore_ascii_case("shared")
        {
            agent_home_dir()
        } else {
            crate::process_util::user_home().join(".grok")
        };
        Ok((custom, Self::from_capture(&text, &home, settings)?))
    }
}
