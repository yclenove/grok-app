//! Actual ownerless candidate consumption, using synthetic config and duplex ACP.
use super::prewarm_route::PrewarmSelection;
use super::*;
use crate::acp_client::{test_transport, AcpClient};
use crate::permission::PermissionPolicy;
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use std::time::{Duration, Instant};

fn home() -> IsolatedAppHome {
    let home = IsolatedAppHome::acquire("prewarm-route");
    let mut settings = store::load_settings();
    settings.session_data_mode = "independent".into();
    settings.cli_backend = "native".into();
    settings.acp_server_addr = None;
    store::save_settings(&settings).unwrap();
    home
}

fn config(mode: &str) -> String {
    format!("[models]\ndefault = \"synthetic-route\"\n[model.synthetic-route]\nmodel = \"grok-4.6\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-key-not-a-credential\"\napi_backend = \"responses\"\nsupports_reasoning_effort = true\napp_provider_mode = \"{mode}\"\nextra_headers = {{ X-Synthetic = \"first\" }}\napp_models = '[{{\"id\":\"grok-4.6\",\"name\":\"6\"}},{{\"id\":\"grok-4.5\",\"name\":\"5\"}}]'\n")
}

fn write_config(text: &str) {
    std::fs::write(crate::paths::agent_config_toml(), text).unwrap();
    // Heal the same App-owned fixture fields as a normal configured route.
    let _ = crate::providers::active_route();
}

fn ready(client: &Arc<AcpClient>) -> PrewarmedProcess {
    PrewarmedProcess {
        acp: client.clone(),
        process_id: "synthetic-ownerless-process".into(),
        policy: PermissionPolicy::Ask,
        effort: Some("low".into()),
        sandbox_profile: Some("off".into()),
        model_id: Some("grok-4.6".into()),
        created_at: Instant::now(),
        created_wall: chrono::Utc::now(),
        backend: "synthetic-acp".into(),
    }
}

fn select(manager: &SessionManager) -> PrewarmSelection {
    manager.take_prewarm(
        PermissionPolicy::Ask,
        "low",
        "off",
        &crate::paths::general_workspace_dir(),
    )
}

#[test]
fn new_chat_rejects_a_real_ownerless_process_that_scanned_skills_before_creation() {
    let _home = home();
    write_config(&config("grok_build_proxy"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            let mut candidate = ready(&client);
            let created = chrono::Utc::now();
            candidate.created_wall = created - chrono::Duration::minutes(5);
            *manager.prewarm.lock() = PrewarmState::Ready(candidate);
            assert!(matches!(
                manager.take_prewarm_for_session(
                    PermissionPolicy::Ask,
                    "low",
                    "off",
                    &crate::paths::general_workspace_dir(),
                    Some(created),
                ),
                PrewarmSelection::Rejected(_, "started before this chat; rescan skills")
            ));
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
            assert!(
                client.is_alive(),
                "caller must retire the candidate outside the slot lock"
            );
        });
}

fn rejection_after(change: impl Fn(&str) -> String) {
    for mode in ["generic", "grok_build_proxy"] {
        let _home = home();
        let initial = config(mode);
        write_config(&initial);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (client, _peer) = test_transport::client_for_current_route(
                    true,
                    SessionMcpPolicy::default(),
                    None,
                );
                let manager = SessionManager::new();
                *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
                write_config(&change(&initial));
                assert!(
                    matches!(select(&manager), PrewarmSelection::Rejected(_, _)),
                    "stale {mode} candidate must cold-spawn"
                );
                assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
                // Selection hands the candidate to the production caller; no kill
                // may run while holding the slot lock or mutate a foreign runtime.
                assert!(client.is_alive());
                assert!(manager.inner.lock().is_none());
                assert!(manager.background.lock().is_empty());
                assert!(manager.parked.lock().is_empty());
            });
    }
}

#[test]
fn same_configuration_consumes_exactly_one_ownerless_candidate() {
    let _home = home();
    write_config(&config("grok_build_proxy"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            let PrewarmSelection::Reused(p) = select(&manager) else {
                panic!("matching configured process must remain warm");
            };
            assert!(Arc::ptr_eq(&p.acp, &client));
            assert_eq!(p.process_id, "synthetic-ownerless-process");
            assert!(matches!(select(&manager), PrewarmSelection::Unavailable));
        });
}

#[test]
fn another_custom_provider_with_the_same_models_is_not_reused() {
    rejection_after(|text| text.replace("synthetic-route", "synthetic-foreign-route"));
}

#[test]
fn same_provider_changed_endpoint_is_not_reused() {
    rejection_after(|text| text.replace("synthetic.invalid", "synthetic-new.invalid"));
}

#[test]
fn same_provider_rotated_key_is_not_reused() {
    rejection_after(|text| {
        text.replace(
            "synthetic-key-not-a-credential",
            "synthetic-rotated-fixture",
        )
    });
}

#[test]
fn same_provider_changed_headers_is_not_reused() {
    rejection_after(|text| text.replace("X-Synthetic = \"first\"", "X-Synthetic = \"second\""));
}

#[test]
fn same_provider_changed_backend_is_not_reused() {
    rejection_after(|text| {
        text.replace(
            "api_backend = \"responses\"",
            "api_backend = \"chat_completions\"",
        )
    });
}

#[test]
fn same_provider_changed_catalog_is_not_reused() {
    rejection_after(|text| text.replace("grok-4.5", "grok-synthetic-future"));
}

#[test]
fn same_provider_changed_mode_is_not_reused() {
    rejection_after(|text| {
        if text.contains("app_provider_mode = \"generic\"") {
            text.replace(
                "app_provider_mode = \"generic\"",
                "app_provider_mode = \"grok_build_proxy\"",
            )
        } else {
            text.replace(
                "app_provider_mode = \"grok_build_proxy\"",
                "app_provider_mode = \"generic\"",
            )
        }
    });
}

#[test]
fn unrecognized_cli_configuration_fields_are_also_bound() {
    rejection_after(|text| format!("{text}\n[synthetic_future_cli]\nfeature = true\n"));
}

#[test]
fn sandbox_cwd_mismatch_keeps_ownerless_candidate_for_its_original_directory() {
    let _home = home();
    write_config(&config("generic"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            let mut p = ready(&client);
            p.sandbox_profile = Some("workspace".into());
            *manager.prewarm.lock() = PrewarmState::Ready(p);
            assert!(matches!(
                manager.take_prewarm(
                    PermissionPolicy::Ask,
                    "low",
                    "workspace",
                    &crate::paths::app_data_root().join("synthetic-project")
                ),
                PrewarmSelection::KeptForCwd(_)
            ));
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::Ready(_)));
            assert!(client.is_alive());
        });
}

#[test]
fn spawning_wait_does_not_consume_or_extend_the_generation() {
    let _home = home();
    let manager = SessionManager::new();
    let since = Instant::now();
    *manager.prewarm.lock() = PrewarmState::Spawning { since };
    assert!(matches!(select(&manager), PrewarmSelection::Waiting));
    assert!(
        matches!(*manager.prewarm.lock(), PrewarmState::Spawning { since: time } if time == since)
    );
    let old = Instant::now() - Duration::from_secs(3);
    *manager.prewarm.lock() = PrewarmState::Spawning { since: old };
    assert!(matches!(select(&manager), PrewarmSelection::Unavailable));
    assert!(
        matches!(*manager.prewarm.lock(), PrewarmState::Spawning { since: time } if time == old)
    );
}

fn rejection_after_settings_change(change: impl Fn(&mut store::AppSettings)) {
    let _home = home();
    write_config(&config("generic"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            let mut settings = store::load_settings();
            change(&mut settings);
            store::save_settings(&settings).unwrap();
            assert!(matches!(select(&manager), PrewarmSelection::Rejected(_, _)));
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
        });
}

#[test]
fn changed_native_cli_selection_is_not_reused() {
    rejection_after_settings_change(|settings| {
        settings.manual_cli_path = Some("synthetic-cli-new".into());
    });
}

#[test]
fn tcp_server_selection_cannot_consume_a_local_prewarm() {
    rejection_after_settings_change(|settings| {
        settings.acp_server_addr = Some("127.0.0.1:1".into());
    });
}

#[cfg(windows)]
#[test]
fn wsl_selection_cannot_consume_a_native_prewarm() {
    rejection_after_settings_change(|settings| {
        settings.cli_backend = "wsl".into();
        settings.wsl_distro = Some("synthetic-distro".into());
        settings.wsl_cli_path = Some("synthetic-cli".into());
    });
}

#[test]
fn official_unreadable_provider_config_fails_closed_without_repair_writes() {
    let _home = home();
    write_config("[models]\ndefault = \"grok\"\n");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            let path = crate::paths::agent_config_toml();
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(matches!(select(&manager), PrewarmSelection::Rejected(_, _)));
            assert!(path.is_dir(), "reuse must not heal an unreadable config");
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
        });
}

#[test]
fn changed_app_home_cannot_reuse_the_previous_ownerless_process() {
    let home = home();
    write_config(&config("generic"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            // Still beneath the owned fixture; IsolatedAppHome restores the
            // byte-exact inherited GROK_APP_HOME before releasing its lock.
            let next = home.path.join("synthetic-next-home");
            std::env::set_var("GROK_APP_HOME", &next);
            crate::paths::ensure_app_dirs().unwrap();
            let mut settings = store::load_settings();
            settings.session_data_mode = "independent".into();
            store::save_settings(&settings).unwrap();
            write_config(&config("generic"));
            assert!(matches!(select(&manager), PrewarmSelection::Rejected(_, _)));
        });
}

#[test]
fn missing_native_key_cannot_certify_an_old_prewarmer() {
    let _home = home();
    write_config(&config("grok_build_proxy"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            write_config(
                &config("grok_build_proxy")
                    .replace("api_key = \"synthetic-key-not-a-credential\"\n", ""),
            );
            assert!(matches!(select(&manager), PrewarmSelection::Rejected(_, _)));
            assert!(!client.prewarm_config_is_current());
        });
}

#[test]
fn unbound_remote_or_aux_transport_cannot_certify_a_local_prewarmer() {
    let _home = home();
    write_config("[models]\ndefault = \"grok\"\n");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for policy in [
                SessionMcpPolicy {
                    tcp: true,
                    ..Default::default()
                },
                SessionMcpPolicy {
                    ssh: true,
                    ..Default::default()
                },
                SessionMcpPolicy {
                    official_aux: true,
                    ..Default::default()
                },
            ] {
                let (client, _peer) = test_transport::client_for_current_route(false, policy, None);
                let manager = SessionManager::new();
                *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
                assert!(matches!(select(&manager), PrewarmSelection::Rejected(_, _)));
                assert!(!client.prewarm_config_is_current());
            }
        });
}

#[test]
fn stale_configuration_is_recycled_even_when_the_sandbox_cwd_differs() {
    let _home = home();
    write_config(&config("generic"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(true, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            let mut p = ready(&client);
            p.sandbox_profile = Some("workspace".into());
            *manager.prewarm.lock() = PrewarmState::Ready(p);
            write_config(&config("generic").replace("synthetic.invalid", "synthetic-new.invalid"));
            assert!(matches!(
                manager.take_prewarm(
                    PermissionPolicy::Ask,
                    "low",
                    "workspace",
                    &crate::paths::app_data_root().join("synthetic-project")
                ),
                PrewarmSelection::Rejected(_, _)
            ));
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
        });
}

struct SyntheticUserHome(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl SyntheticUserHome {
    fn under(home: &IsolatedAppHome) -> Self {
        // Called with the existing shared environment lock held. Neither the
        // inherited path nor its contents are accessed or printed.
        let previous = Self(
            ["HOME", "USERPROFILE"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        let path = home.path.join("synthetic-user");
        std::fs::create_dir_all(path.join(".grok")).unwrap();
        for key in ["HOME", "USERPROFILE"] {
            std::env::set_var(key, &path);
        }
        previous
    }
}

impl Drop for SyntheticUserHome {
    fn drop(&mut self) {
        for (key, value) in self.0.iter().rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn official_shared(change: impl Fn(&std::path::Path, &mut store::AppSettings), reusable: bool) {
    let home = home();
    let _user = SyntheticUserHome::under(&home);
    write_config("[models]\ndefault = \"grok\"\n");
    let config_path = crate::process_util::user_home().join(".grok/config.toml");
    std::fs::write(&config_path, "[synthetic_cli]\nfeature = false\n").unwrap();
    let mut settings = store::load_settings();
    settings.session_data_mode = "shared".into();
    store::save_settings(&settings).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (client, _peer) =
                test_transport::client_for_current_route(false, SessionMcpPolicy::default(), None);
            let manager = SessionManager::new();
            *manager.prewarm.lock() = PrewarmState::Ready(ready(&client));
            change(&config_path, &mut settings);
            store::save_settings(&settings).unwrap();
            let selected = select(&manager);
            assert!(if reusable {
                matches!(selected, PrewarmSelection::Reused(_))
            } else {
                matches!(selected, PrewarmSelection::Rejected(_, _))
            });
            assert!(matches!(*manager.prewarm.lock(), PrewarmState::None));
        });
}

#[test]
fn official_shared_home_configuration_is_bound_not_only_the_app_catalog() {
    official_shared(
        |path, _| std::fs::write(path, "[synthetic_cli]\nfeature = true\n").unwrap(),
        false,
    );
}

#[test]
fn official_shared_to_independent_home_change_requires_a_new_process() {
    official_shared(
        |_, settings| settings.session_data_mode = "independent".into(),
        false,
    );
}

#[test]
fn shared_home_normalization_keeps_an_unchanged_process_reusable() {
    official_shared(
        |_, settings| settings.session_data_mode = " shared ".into(),
        true,
    );
}
