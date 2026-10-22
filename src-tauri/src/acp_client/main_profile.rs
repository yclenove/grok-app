//! Main-generation preparation on a blocking worker, separate from aux homes.
use super::SpawnOptions;
use crate::{providers::PreparedMainProfile, store::AppSettings};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct CancelPreparation(Arc<AtomicBool>);

impl Drop for CancelPreparation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(super) fn validate(options: &SpawnOptions, settings: &AppSettings) -> Result<(), String> {
    if let Some(request) = &options.main_profile {
        if options.grok_home_override.is_some()
            || options
                .ssh_alias
                .as_deref()
                .is_some_and(|alias| !alias.trim().is_empty())
            || settings
                .acp_server_addr
                .as_deref()
                .is_some_and(|addr| !addr.trim().is_empty())
            || options.empty_mcp_servers
        {
            return Err(
                "main execution profile cannot use an auxiliary or remote transport".into(),
            );
        }
        if options.model_id.as_deref() != Some(request.model_id.as_str()) {
            return Err("main execution profile model does not match its spawn selection".into());
        }
        if let Some(route) = options
            .route_provider_id
            .as_deref()
            .map(str::trim)
            .filter(|route| !route.is_empty())
        {
            let route = if crate::providers::is_session_provider_official(route) {
                crate::providers::SESSION_PROVIDER_OFFICIAL
            } else {
                route
            };
            let provider = if crate::providers::is_session_provider_official(&request.provider_id) {
                crate::providers::SESSION_PROVIDER_OFFICIAL
            } else {
                request.provider_id.as_str()
            };
            if route != provider {
                return Err(
                    "main execution profile provider does not match its spawn route".into(),
                );
            }
        }
    }
    Ok(())
}

pub(super) async fn prepare(
    options: &SpawnOptions,
    settings: &AppSettings,
) -> Result<Option<PreparedMainProfile>, String> {
    validate(options, settings)?;
    let Some(request) = options.main_profile.clone() else {
        return Ok(None);
    };
    let settings = settings.clone();
    let permission = options
        .permission_policy
        .clone()
        .unwrap_or_else(|| "ask".into());
    let cancelled = Arc::new(AtomicBool::new(false));
    // Dropping this async future also revokes its already-running worker.
    let _cancel = CancelPreparation(Arc::clone(&cancelled));
    tokio::task::spawn_blocking(move || {
        let official = request.provider_id.eq_ignore_ascii_case("official");
        let key = official
            .then(|| crate::store::load_secrets().official_api_key)
            .flatten()
            .filter(|key| !key.trim().is_empty());
        let available = !official
            && settings.official_aux_inject
            && crate::official_aux::official_aux_available();
        PreparedMainProfile::prepare(
            &request,
            &settings,
            &permission,
            available,
            &crate::extensions::load_prefs(),
            key.as_deref(),
            &cancelled,
        )
        .map(Some)
    })
    .await
    .map_err(|_| "main execution profile worker failed".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> SpawnOptions {
        SpawnOptions {
            model_id: Some("synthetic-model".into()),
            main_profile: Some(crate::providers::MainProfileRequest {
                owner_session_id: uuid::Uuid::new_v4().to_string(),
                provider_id: "selected".into(),
                model_id: "synthetic-model".into(),
                history: None,
            }),
            ..SpawnOptions::default()
        }
    }

    #[test]
    fn main_request_rejects_remote_auxiliary_and_mismatched_model_before_any_capture() {
        let settings = AppSettings::default();
        assert!(validate(&options(), &settings).is_ok());
        for change in [
            |opts: &mut SpawnOptions| opts.ssh_alias = Some("synthetic-ssh".into()),
            |opts: &mut SpawnOptions| opts.grok_home_override = Some("unexecuted-aux".into()),
            |opts: &mut SpawnOptions| opts.empty_mcp_servers = true,
            |opts: &mut SpawnOptions| opts.model_id = Some("different-model".into()),
        ] {
            let mut opts = options();
            change(&mut opts);
            assert!(validate(&opts, &settings).is_err());
        }
        assert!(validate(
            &options(),
            &AppSettings {
                acp_server_addr: Some("127.0.0.1:9".into()),
                ..settings
            }
        )
        .is_err());
        assert!(validate(
            &SpawnOptions {
                grok_home_override: Some("legacy-aux".into()),
                ..SpawnOptions::default()
            },
            &AppSettings::default()
        )
        .is_ok());
    }

    #[test]
    fn dropping_preparation_future_revokes_its_worker_signal() {
        let signal = Arc::new(AtomicBool::new(false));
        let guard = CancelPreparation(Arc::clone(&signal));
        assert!(!signal.load(Ordering::Acquire));
        drop(guard);
        assert!(signal.load(Ordering::Acquire));
    }
}
