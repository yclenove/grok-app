//! Normal main-profile producer. Credentials/config remain private to the Host.
//! This is distinct from the official auxiliary override-home contract.
use super::{
    execution_home::PendingHome, profile_io, ConfigSnapshot, GrokBuildProxySpawn,
    ProcessConfigBinding, ProcessModelBinding,
};
use crate::{extensions::ExtensionsPrefs, store::AppSettings};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Credential-free desired selection. A new UUID generation is allocated by
/// the producer; a caller cannot authorize rewriting an existing generation.
#[derive(Debug, Clone)]
pub(crate) struct MainProfileRequest {
    pub owner_session_id: String,
    pub provider_id: String,
    pub model_id: String,
    /// Existing agent provenance, independent of the desired provider choice.
    pub history: Option<super::MainHistoryRequest>,
}

pub(crate) struct PreparedMainProfile {
    pub home: PathBuf,
    pub custom: bool,
    pub inject: bool,
    pub model_binding: ProcessModelBinding,
    pub native: Option<GrokBuildProxySpawn>,
    pub configuration: Option<ProcessConfigBinding>,
    pub migration: Option<super::MigrationWitness>,
    pub runtime: std::sync::Arc<super::MainRuntime>,
    channel_rules: Option<String>,
}

struct Inputs {
    resources: profile_io::Tree,
    runtime_path: PathBuf,
    runtime: Option<Vec<u8>>,
    official_path: Option<PathBuf>,
    official: Option<Vec<u8>>,
    auth_path: Option<PathBuf>,
    auth: Option<Vec<u8>>,
    mcp_paths: Vec<PathBuf>,
    mcp_credentials: Vec<Option<Vec<u8>>>,
    stamps: Vec<(PathBuf, profile_io::InputStamp)>,
}

fn text(bytes: &Option<Vec<u8>>) -> Result<&str, String> {
    std::str::from_utf8(bytes.as_deref().unwrap_or_default())
        .map_err(|_| "execution configuration is not UTF-8".into())
}

fn cancelled(stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::Acquire) {
        return Err("execution profile preparation was cancelled".into());
    }
    Ok(())
}

impl Inputs {
    fn capture(
        runtime_home: &Path,
        official_home: &Path,
        official: bool,
        key: bool,
        user_mcp: bool,
    ) -> Result<Self, String> {
        let runtime_path = runtime_home.join("config.toml");
        let official_path = (official || user_mcp).then(|| official_home.join("config.toml"));
        let auth_path = (official && !key).then(|| official_home.join("auth.json"));
        let mcp_paths = if user_mcp {
            vec![
                runtime_home.join("mcp_credentials.json"),
                official_home.join("mcp_credentials.json"),
            ]
        } else {
            Vec::new()
        };
        let stamps = [
            Some(&runtime_path),
            official_path.as_ref(),
            auth_path.as_ref(),
        ]
        .into_iter()
        .flatten()
        .chain(mcp_paths.iter())
        .map(|path| profile_io::input_stamp(path).map(|stamp| (path.clone(), stamp)))
        .collect::<Result<Vec<_>, _>>()?;
        let captured = Self {
            resources: profile_io::Tree::capture(runtime_home)?,
            runtime: profile_io::read_optional(&runtime_path)?,
            runtime_path,
            official: official_path
                .as_ref()
                .map(|path| profile_io::read_optional(path))
                .transpose()?
                .flatten(),
            official_path,
            auth: auth_path
                .as_ref()
                .map(|path| profile_io::read_optional(path))
                .transpose()?
                .flatten(),
            auth_path,
            mcp_credentials: mcp_paths
                .iter()
                .map(|path| profile_io::read_optional(path))
                .collect::<Result<Vec<_>, _>>()?,
            mcp_paths,
            stamps,
        };
        captured.unchanged()?;
        Ok(captured)
    }

    fn unchanged(&self) -> Result<(), String> {
        self.resources.unchanged()?;
        for (path, stamp) in &self.stamps {
            if profile_io::input_stamp(path)? != *stamp {
                return Err("execution profile source identity changed during preparation".into());
            }
        }
        for (path, expected) in [
            (Some(&self.runtime_path), &self.runtime),
            (self.official_path.as_ref(), &self.official),
            (self.auth_path.as_ref(), &self.auth),
        ] {
            if let Some(path) = path {
                if &profile_io::read_optional(path)? != expected {
                    return Err("execution profile source changed during preparation".into());
                }
            }
        }
        for (path, expected) in self.mcp_paths.iter().zip(&self.mcp_credentials) {
            if &profile_io::read_optional(path)? != expected {
                return Err("execution MCP authentication changed during preparation".into());
            }
        }
        Ok(())
    }
}

impl PreparedMainProfile {
    pub(crate) fn extra_rules(&self, user: Option<&str>) -> Option<String> {
        crate::official_aux::merge_extra_rules_for_main(
            user,
            self.channel_rules.as_deref(),
            self.inject,
        )
    }

    pub(crate) fn prepare(
        request: &MainProfileRequest,
        settings: &AppSettings,
        permission: &str,
        official_available: bool,
        prefs: &ExtensionsPrefs,
        official_key: Option<&str>,
        stop: &AtomicBool,
    ) -> Result<Self, String> {
        let runtime_home = crate::paths::agent_home_dir();
        let official_home = crate::process_util::user_home().join(".grok");
        cancelled(stop)?;
        if !uuid::Uuid::parse_str(&request.owner_session_id)
            .is_ok_and(|id| id.to_string() == request.owner_session_id)
        {
            return Err("execution profile owner is invalid".into());
        }
        if settings
            .acp_server_addr
            .as_deref()
            .is_some_and(|addr| !addr.trim().is_empty())
        {
            return Err("remote transport cannot prepare a local main profile".into());
        }
        let official = request.provider_id.eq_ignore_ascii_case("official");
        if request.provider_id.is_empty() || request.provider_id.trim() != request.provider_id {
            return Err("execution provider selection is invalid".into());
        }
        if !official && official_key.is_some() {
            return Err("custom main cannot consume official credentials".into());
        }
        crate::agent_home_config::with_config_write_lock(|| {
            let expected_inject = !official && settings.official_aux_inject && official_available;
            let inputs = Inputs::capture(
                &runtime_home,
                &official_home,
                official,
                official_key.is_some(),
                !expected_inject || settings.official_aux_with_user_mcp,
            )?;
            let snapshot = ConfigSnapshot::parse(text(&inputs.runtime)?)?;
            let projected = if official {
                snapshot.project_official(
                    &request.model_id,
                    ConfigSnapshot::parse(text(&inputs.official)?)?,
                    official_key,
                )?
            } else {
                snapshot.project_custom(&request.provider_id, &request.model_id)?
            };
            if let Some(auth) = &inputs.auth {
                if !serde_json::from_slice::<serde_json::Value>(auth)
                    .is_ok_and(|value| value.is_object())
                {
                    return Err("official execution authentication input is invalid".into());
                }
            }
            inputs.unchanged()?;
            cancelled(stop)?;
            let history = request
                .history
                .as_ref()
                .map(|history| {
                    super::main_history::Snapshot::capture(history, &request.owner_session_id)
                })
                .transpose()?;
            cancelled(stop)?;
            let pending = PendingHome::create(&request.owner_session_id)?;
            inputs.resources.copy_into(pending.home())?;
            let copied_history = history
                .as_ref()
                .map(|history| history.copy_into(&pending))
                .transpose()?;
            let crate::session_execution::ExecutionSource::AppProfile {
                owner_session_id,
                generation,
            } = pending.source()
            else {
                return Err("execution profile ownership is unavailable".into());
            };
            let projected = ConfigSnapshot::parse(&projected)?
                .freeze_profile_routes(owner_session_id, generation)?;
            profile_io::write_new(&pending.home().join("config.toml"), projected.as_bytes())?;
            if let Some(auth) = &inputs.auth {
                profile_io::write_new(&pending.home().join("auth.json"), auth)?;
            }
            let inject = super::runtime_config::prepare_pending_runtime(
                &pending,
                settings,
                permission,
                !official,
                official_available,
                prefs,
            )?;
            let prepared = profile_io::Tree::capture(pending.home())?;
            let final_config = profile_io::read_optional(&pending.home().join("config.toml"))?;
            let final_auth = profile_io::read_optional(&pending.home().join("auth.json"))?;
            // Validate and capture runtime consumers before publishing a usable
            // generation. Their malformed inputs must not leave a marker.
            let (runtime_model, _) = ProcessModelBinding::from_text(
                text(&final_config)?,
                !official,
                false,
                &request.model_id,
            )?;
            let runtime = std::sync::Arc::new(super::MainRuntime::capture(
                text(&final_config)?,
                text(&inputs.runtime)?,
                super::main_mcp_auth::MainMcpSources {
                    user_config: inputs
                        .official
                        .as_ref()
                        .map(|_| text(&inputs.official))
                        .transpose()?,
                    main_credentials: inputs.mcp_credentials.first().and_then(Option::as_deref),
                    user_credentials: inputs.mcp_credentials.get(1).and_then(Option::as_deref),
                },
                &runtime_model,
                inject,
                settings,
                prefs,
            )?);
            inputs.unchanged()?;
            prepared.unchanged()?;
            if final_auth != inputs.auth || (!official && final_auth.is_some()) {
                return Err("execution authentication changed during preparation".into());
            }
            ConfigSnapshot::parse(text(&final_config)?)?;
            if let Some(history) = &history {
                history.unchanged()?;
            }
            if let Some(copied) = &copied_history {
                copied.unchanged()?;
            }
            cancelled(stop)?;
            let home = pending.publish(
                final_config
                    .as_deref()
                    .ok_or("execution configuration is unavailable")?,
            )?;
            if profile_io::read_optional(&home.join("config.toml"))? != final_config {
                return Err("execution configuration changed during publication".into());
            }
            let (model_binding, native, configuration) = ProcessModelBinding::capture_for_spawn(
                !official,
                false,
                false,
                &request.model_id,
                &home,
                settings,
            )?;
            if model_binding != runtime_model {
                return Err("main runtime model changed during publication".into());
            }
            let migration = match (history, copied_history, &request.history) {
                (Some(history), Some(copied), Some(request)) => Some(
                    history.witness(
                        copied,
                        request,
                        configuration
                            .as_ref()
                            .ok_or("execution configuration is unavailable")?
                            .execution_source(),
                    )?,
                ),
                (None, None, None) => None,
                _ => return Err("execution migration preparation is incomplete".into()),
            };
            let channel_rules = if official {
                None
            } else {
                ConfigSnapshot::parse(text(&final_config)?)?
                    .model_fields(&request.provider_id)?
                    .and_then(|fields| {
                        crate::store::sanitize_extra_rules(
                            fields.get(super::APP_APPEND_PROMPT_KEY).cloned(),
                        )
                    })
            };
            Ok(Self {
                home,
                custom: !official,
                inject,
                model_binding,
                native,
                configuration,
                migration,
                runtime,
                channel_rules,
            })
        })
    }
}

#[cfg(test)]
#[path = "providers_main_profile_tests.rs"]
mod tests;
