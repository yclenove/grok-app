//! Actual permission command/store cores with synthetic homes and duplex ACP.
use super::*;
use crate::acp_client::{test_transport, AcpClient};
use crate::commands::permission_preferences;
use crate::permission::PermissionPolicy;
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use std::sync::{mpsc, Mutex as StdMutex};
use std::time::Duration;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_after_save(id: &str) {
    let pause = pauses().lock().unwrap().remove(id);
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Live,
    Background,
    Parked,
}

struct Fixture {
    home: IsolatedAppHome,
    runtime: tokio::runtime::Runtime,
    peers: Vec<tokio::io::DuplexStream>,
    manager: Arc<SessionManager>,
    projects: Vec<store::Project>,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire_with_user_home("permission-policy");
        let mut projects = Vec::new();
        for (index, name) in ["项目甲", "项目乙", "未信任项目"].into_iter().enumerate() {
            let path = home.path.join(name);
            std::fs::create_dir(&path).unwrap();
            let project = store::ensure_import_project(name, path.to_str().unwrap())
                .unwrap()
                .project;
            projects.push(if index < 2 {
                store::trust_project(&project.id).unwrap()
            } else {
                project
            });
        }
        let mut settings = store::load_settings();
        settings.composer_prefs_scope = "session".into();
        settings.permission_policy = "ask".into();
        store::save_settings(&settings).unwrap();
        Self {
            home,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            peers: Vec::new(),
            manager: Arc::new(SessionManager::new()),
            projects,
        }
    }

    fn add(
        &mut self,
        project: usize,
        policy: Option<&str>,
        slot: Slot,
    ) -> (store::SessionMeta, Arc<AcpClient>) {
        let mut meta =
            store::create_session(Some(self.projects[project].id.clone()), None, false).unwrap();
        meta.permission_policy = policy.map(str::to_owned);
        meta.agent_session_id = Some(format!("synthetic-{}", meta.id));
        store::update_session_meta(&meta).unwrap();
        let entered = self.runtime.enter();
        let (client, peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        self.peers.push(peer);
        let mut live = routing_tests::bare_live_session(&meta.id, &format!("process-{}", meta.id));
        live.meta = meta.clone();
        live.policy = PermissionPolicy::parse(
            &store::resolve_composer_prefs(meta.project_id.as_deref(), Some(&meta.id))
                .permission_policy,
        );
        live.acp = Some(client.clone());
        match slot {
            Slot::Live => *self.manager.inner.lock() = Some(live),
            Slot::Background => {
                live.fsm.begin_stream().unwrap();
                live.prompt_in_flight = true;
                self.manager.background.lock().insert(meta.id.clone(), live);
            }
            Slot::Parked => {
                self.manager.parked.lock().insert(
                    meta.id.clone(),
                    ParkedAgent {
                        turn_open_epoch: live.turn_open_epoch,
                        workspace_context: live.workspace_context,
                        process_id: live.process_id,
                        app_session_id: live.app_session_id,
                        meta: live.meta,
                        acp: live.acp.unwrap(),
                        last_activity: live.last_activity,
                        model_id: live.model_id,
                        effort: live.effort,
                        product_mode: live.product_mode,
                        project_path: live.project_path,
                        policy: live.policy,
                        sandbox_profile: None,
                        needs_history_bootstrap: live.needs_history_bootstrap,
                        backend: live.backend,
                    },
                );
            }
        }
        (meta, client)
    }

    fn stored(&self, id: &str) -> store::SessionMeta {
        store::load_sessions_snapshot()
            .unwrap()
            .into_iter()
            .find(|meta| meta.id == id)
            .unwrap()
    }

    fn policy(&self, id: &str) -> PermissionPolicy {
        if let Some(policy) = self
            .manager
            .inner
            .lock()
            .as_ref()
            .filter(|live| live.app_session_id == id)
            .map(|live| live.policy)
        {
            return policy;
        }
        self.manager.background.lock().get(id).unwrap().policy
    }

    fn project_permission(&self, project: usize, policy: Option<&str>) -> store::Project {
        self.runtime
            .block_on(permission_preferences::save_project_permission(
                None,
                &self.manager,
                &self.projects[project].id,
                policy.map(str::to_owned),
            ))
            .unwrap()
    }

    fn global_permission(&self, policy: &str) {
        let mut settings = store::load_settings();
        settings.permission_policy = policy.into();
        store::save_settings(&settings).unwrap();
        self.runtime
            .block_on(self.manager.refresh_permission_policies_core(None))
            .unwrap();
    }
}

#[test]
fn project_permission_preserves_foreign_and_explicit_overrides() {
    let mut fixture = Fixture::new();
    let (target, target_client) = fixture.add(0, None, Slot::Live);
    let (foreign, foreign_client) = fixture.add(1, None, Slot::Background);
    let (explicit, explicit_client) = fixture.add(0, Some("deny"), Slot::Parked);
    fixture.project_permission(0, Some("accept_edits"));
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Ask);
    assert!(foreign_client.is_alive());
    assert!(explicit_client.is_alive());
    assert!(!target_client.is_alive());
    assert_eq!(fixture.stored(&target.id).permission_policy, None);
    assert_eq!(fixture.stored(&foreign.id).permission_policy, None);
    assert_eq!(
        fixture.stored(&explicit.id).permission_policy.as_deref(),
        Some("deny")
    );
}

#[test]
fn project_background_updates_when_another_project_is_focused() {
    let mut fixture = Fixture::new();
    let (foreign, foreign_client) = fixture.add(1, None, Slot::Live);
    let (target, target_client) = fixture.add(0, None, Slot::Background);
    fixture.project_permission(0, Some("accept_edits"));
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Ask);
    assert!(foreign_client.is_alive());
    assert!(target_client.is_alive());
    assert_eq!(fixture.stored(&target.id).permission_policy, None);
    assert!(fixture
        .manager
        .pending_soft_respawn
        .lock()
        .contains_key(&target.id));
}

#[test]
fn global_permission_preserves_session_override_and_inheritance() {
    let mut fixture = Fixture::new();
    let (explicit, explicit_client) = fixture.add(0, Some("deny"), Slot::Live);
    let (inherited, inherited_client) = fixture.add(1, None, Slot::Background);
    fixture.global_permission("accept_edits");
    assert_eq!(fixture.policy(&explicit.id), PermissionPolicy::Deny);
    assert!(explicit_client.is_alive());
    assert_eq!(fixture.policy(&inherited.id), PermissionPolicy::AcceptEdits);
    assert!(inherited_client.is_alive());
    assert_eq!(fixture.stored(&inherited.id).permission_policy, None);
    fixture.global_permission("ask");
    assert_eq!(fixture.policy(&inherited.id), PermissionPolicy::Ask);
    assert_eq!(fixture.stored(&inherited.id).permission_policy, None);
}

#[test]
fn global_permission_keeps_untrusted_project_ask() {
    let mut fixture = Fixture::new();
    let (untrusted, client) = fixture.add(2, Some("always_approve"), Slot::Live);
    fixture.global_permission("always_approve");
    assert_eq!(fixture.policy(&untrusted.id), PermissionPolicy::Ask);
    assert!(client.is_alive());
}

#[test]
fn session_permission_targets_explicit_background_owner() {
    let mut fixture = Fixture::new();
    let (foreign, foreign_client) = fixture.add(1, None, Slot::Live);
    let (target, target_client) = fixture.add(0, None, Slot::Background);
    fixture
        .runtime
        .block_on(permission_preferences::save_session_permission(
            None,
            &fixture.manager,
            "accept_edits",
            target.project_id.as_deref(),
            Some(&target.id),
        ))
        .unwrap();
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Ask);
    assert!(foreign_client.is_alive());
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert!(target_client.is_alive());
    assert_eq!(
        fixture.stored(&target.id).permission_policy.as_deref(),
        Some("accept_edits")
    );
    assert_eq!(fixture.stored(&foreign.id).permission_policy, None);
}

#[test]
fn project_permission_does_not_follow_focus_after_save() {
    let mut fixture = Fixture::new();
    let (target, target_client) = fixture.add(0, None, Slot::Live);
    let (foreign, foreign_client) = fixture.add(1, None, Slot::Background);
    let (reached_tx, reached_rx) = mpsc::sync_channel(0);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(0);
    let project = fixture.projects[0].id.clone();
    pauses().lock().unwrap().insert(
        project.clone(),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(permission_preferences::save_project_permission(
            None,
            &manager,
            &project,
            Some("accept_edits".into()),
        ))
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    let target_live = fixture.manager.inner.lock().take().unwrap();
    let foreign_live = fixture
        .manager
        .background
        .lock()
        .remove(&foreign.id)
        .unwrap();
    fixture
        .manager
        .background
        .lock()
        .insert(target.id.clone(), target_live);
    *fixture.manager.inner.lock() = Some(foreign_live);
    proceed_tx.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Ask);
    assert!(foreign_client.is_alive());
    assert!(!target_client.is_alive());
    assert_eq!(fixture.stored(&target.id).permission_policy, None);
}

#[test]
fn clearing_project_permission_keeps_session_override_and_retires_inherited_parked() {
    let mut fixture = Fixture::new();
    store::set_project_permission_policy(&fixture.projects[0].id, Some("accept_edits".into()))
        .unwrap();
    let (explicit, explicit_client) = fixture.add(0, Some("deny"), Slot::Live);
    let (busy, busy_client) = fixture.add(0, None, Slot::Background);
    let (parked, parked_client) = fixture.add(0, None, Slot::Parked);
    let project = fixture.project_permission(0, None);
    assert_eq!(project.permission_policy, None);
    assert_eq!(fixture.policy(&explicit.id), PermissionPolicy::Deny);
    assert!(explicit_client.is_alive());
    assert_eq!(fixture.policy(&busy.id), PermissionPolicy::Ask);
    assert!(busy_client.is_alive());
    assert!(fixture
        .manager
        .pending_soft_respawn
        .lock()
        .contains_key(&busy.id));
    assert!(!parked_client.is_alive());
    assert!(!fixture.manager.parked.lock().contains_key(&parked.id));
    assert_eq!(fixture.stored(&parked.id).permission_policy, None);
    assert_eq!(fixture.stored(&busy.id).permission_policy, None);
}

#[test]
fn global_permission_preserves_project_override_in_all_slots() {
    let mut fixture = Fixture::new();
    store::set_project_permission_policy(&fixture.projects[0].id, Some("deny".into())).unwrap();
    let (live, live_client) = fixture.add(0, None, Slot::Live);
    let (busy, busy_client) = fixture.add(0, None, Slot::Background);
    let (parked, parked_client) = fixture.add(0, None, Slot::Parked);
    fixture.global_permission("always_approve");
    assert_eq!(fixture.policy(&live.id), PermissionPolicy::Deny);
    assert_eq!(fixture.policy(&busy.id), PermissionPolicy::Deny);
    assert_eq!(
        fixture
            .manager
            .parked
            .lock()
            .get(&parked.id)
            .unwrap()
            .policy,
        PermissionPolicy::Deny
    );
    assert!(live_client.is_alive() && busy_client.is_alive() && parked_client.is_alive());
    assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    for meta in [live, busy, parked] {
        assert_eq!(fixture.stored(&meta.id).permission_policy, None);
    }
}

#[test]
fn unchanged_permission_does_not_queue_background_restart() {
    let mut fixture = Fixture::new();
    let (_, live_client) = fixture.add(0, None, Slot::Live);
    let (busy, busy_client) = fixture.add(1, None, Slot::Background);
    fixture
        .manager
        .pending_soft_respawn
        .lock()
        .insert(busy.id.clone(), "newer_spawn_option".into());
    fixture.global_permission("ask");
    assert!(live_client.is_alive() && busy_client.is_alive());
    assert_eq!(
        fixture
            .manager
            .pending_soft_respawn
            .lock()
            .get(&busy.id)
            .map(String::as_str),
        Some("newer_spawn_option")
    );
    assert_eq!(fixture.stored(&busy.id).permission_policy, None);
}

#[test]
fn project_memory_scope_uses_the_explicit_sessions_project() {
    let mut fixture = Fixture::new();
    let mut settings = store::load_settings();
    settings.composer_prefs_scope = "project".into();
    store::save_settings(&settings).unwrap();
    store::set_project_permission_policy(&fixture.projects[1].id, Some("deny".into())).unwrap();
    let (foreign, foreign_client) = fixture.add(1, None, Slot::Live);
    let (target, target_client) = fixture.add(0, None, Slot::Background);
    let prefs = fixture
        .runtime
        .block_on(permission_preferences::save_session_permission(
            None,
            &fixture.manager,
            "accept_edits",
            Some(&fixture.projects[1].id),
            Some(&target.id),
        ))
        .unwrap();
    assert_eq!(prefs.permission_policy, "accept_edits");
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Deny);
    assert!(target_client.is_alive() && foreign_client.is_alive());
    assert_eq!(
        fixture.stored(&target.id).permission_policy.as_deref(),
        Some("accept_edits")
    );
    let projects = store::load_projects_snapshot().unwrap();
    assert_eq!(
        projects
            .iter()
            .find(|project| project.id == fixture.projects[0].id)
            .unwrap()
            .permission_policy
            .as_deref(),
        Some("accept_edits")
    );
    assert_eq!(
        projects
            .iter()
            .find(|project| project.id == fixture.projects[1].id)
            .unwrap()
            .permission_policy
            .as_deref(),
        Some("deny")
    );
    // Project memory intentionally also seeds the global default, as before.
    assert_eq!(store::load_settings().permission_policy, "accept_edits");
}

#[test]
fn null_session_is_a_draft_and_does_not_overwrite_the_focused_session() {
    let mut fixture = Fixture::new();
    let (explicit, explicit_client) = fixture.add(0, Some("deny"), Slot::Live);
    let (inherited, inherited_client) = fixture.add(1, None, Slot::Background);
    fixture
        .runtime
        .block_on(permission_preferences::save_session_permission(
            None,
            &fixture.manager,
            "accept_edits",
            None,
            None,
        ))
        .unwrap();
    assert_eq!(fixture.policy(&explicit.id), PermissionPolicy::Deny);
    assert!(explicit_client.is_alive());
    assert_eq!(
        fixture.stored(&explicit.id).permission_policy.as_deref(),
        Some("deny")
    );
    assert_eq!(store::load_settings().permission_policy, "accept_edits");
    assert_eq!(fixture.policy(&inherited.id), PermissionPolicy::AcceptEdits);
    assert!(inherited_client.is_alive());
}

#[test]
fn invalid_explicit_target_and_untrusted_project_change_preserve_bytes_and_processes() {
    let mut fixture = Fixture::new();
    let (live, client) = fixture.add(0, None, Slot::Live);
    let index = crate::paths::sessions_index_file();
    let settings = crate::paths::settings_file();
    assert!(index.starts_with(&fixture.home.path) && settings.starts_with(&fixture.home.path));
    let before_index = std::fs::read(&index).unwrap();
    let before_settings = std::fs::read(&settings).unwrap();
    for id in ["missing-session", "", " "] {
        assert!(fixture
            .runtime
            .block_on(permission_preferences::save_session_permission(
                None,
                &fixture.manager,
                "always_approve",
                Some(&fixture.projects[0].id),
                Some(id),
            ))
            .is_err());
    }
    assert!(fixture
        .runtime
        .block_on(permission_preferences::save_project_permission(
            None,
            &fixture.manager,
            &fixture.projects[2].id,
            Some("always_approve".into()),
        ))
        .is_err());
    assert_eq!(std::fs::read(index).unwrap(), before_index);
    assert_eq!(std::fs::read(settings).unwrap(), before_settings);
    assert!(client.is_alive());
    assert_eq!(fixture.policy(&live.id), PermissionPolicy::Ask);
    assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
}

#[test]
fn runtime_refresh_rejects_corrupt_index_without_recovery_or_retirement() {
    let mut fixture = Fixture::new();
    let (live, client) = fixture.add(0, None, Slot::Live);
    let index = crate::paths::sessions_index_file();
    std::fs::write(&index, b"{not-json").unwrap();
    let result = fixture
        .runtime
        .block_on(fixture.manager.refresh_permission_policies_core(None));
    assert!(result.is_err());
    assert_eq!(std::fs::read(index).unwrap(), b"{not-json");
    assert!(client.is_alive());
    assert_eq!(fixture.policy(&live.id), PermissionPolicy::Ask);
}

#[test]
fn project_override_never_becomes_the_independent_profile_default() {
    let mut fixture = Fixture::new();
    let mut settings = store::load_settings();
    settings.session_data_mode = "independent".into();
    store::save_settings(&settings).unwrap();
    let (target, _) = fixture.add(0, None, Slot::Live);
    fixture.project_permission(0, Some("always_approve"));
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AlwaysApprove);
    let path = crate::paths::agent_config_toml();
    assert!(path.starts_with(&fixture.home.path));
    let profile = std::fs::read_to_string(&path).unwrap();
    assert!(profile
        .lines()
        .any(|line| line.trim() == "permission_mode = \"default\""));
    let claude: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path.parent().unwrap().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        claude
            .pointer("/permissions/defaultMode")
            .and_then(serde_json::Value::as_str),
        Some("default")
    );
    assert_eq!(store::load_settings().permission_policy, "ask");
}

#[test]
fn permission_refresh_waits_for_connect_and_reads_the_latest_saved_default() {
    let mut fixture = Fixture::new();
    let (target, client) = fixture.add(0, None, Slot::Background);
    let manager = fixture.manager.clone();
    let connect = fixture.runtime.block_on(manager.connect_lock.lock());
    let mut settings = store::load_settings();
    settings.permission_policy = "always_approve".into();
    store::save_settings(&settings).unwrap();
    let (started_tx, started_rx) = mpsc::sync_channel(0);
    let worker_manager = manager.clone();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        started_tx.send(()).unwrap();
        runtime.block_on(worker_manager.refresh_permission_policies_core(None))
    });
    started_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    settings.permission_policy = "accept_edits".into();
    store::save_settings(&settings).unwrap();
    drop(connect);
    worker.join().unwrap().unwrap();
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert_eq!(fixture.stored(&target.id).permission_policy, None);
    assert!(client.is_alive());
}

fn assert_explicit_choice(scope: &str) {
    let mut fixture = Fixture::new();
    let mut settings = store::load_settings();
    settings.composer_prefs_scope = scope.into();
    store::save_settings(&settings).unwrap();
    let (foreign, foreign_client) = fixture.add(1, Some("deny"), Slot::Live);
    let (target, target_client) = fixture.add(0, Some("deny"), Slot::Background);
    let prefs = fixture
        .runtime
        .block_on(permission_preferences::save_session_permission(
            None,
            &fixture.manager,
            "accept_edits",
            target.project_id.as_deref(),
            Some(&target.id),
        ))
        .unwrap();
    assert_eq!(
        prefs.permission_policy, "accept_edits",
        "active choice was blocked by the target's previous override"
    );
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert_eq!(
        fixture.stored(&target.id).permission_policy.as_deref(),
        Some("accept_edits")
    );
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Deny);
    assert_eq!(
        fixture.stored(&foreign.id).permission_policy.as_deref(),
        Some("deny")
    );
    assert!(target_client.is_alive() && foreign_client.is_alive());
    assert_eq!(store::load_settings().permission_policy, "accept_edits");
}

#[test]
fn explicit_choice_in_global_scope_replaces_only_the_target_override() {
    assert_explicit_choice("global");
}

#[test]
fn explicit_choice_in_project_scope_replaces_only_the_target_override() {
    assert_explicit_choice("project");
}

#[test]
fn permission_retirement_keeps_an_unchanged_busy_process_tenant() {
    let mut fixture = Fixture::new();
    let (target, target_client) = fixture.add(0, None, Slot::Live);
    let (foreign, _) = fixture.add(1, Some("deny"), Slot::Background);
    let process = fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .process_id
        .clone();
    {
        let mut background = fixture.manager.background.lock();
        let tenant = background.get_mut(&foreign.id).unwrap();
        tenant.process_id = process;
        tenant.acp = Some(target_client.clone());
    }
    fixture.global_permission("accept_edits");
    assert!(target_client.is_alive());
    assert_eq!(fixture.policy(&target.id), PermissionPolicy::AcceptEdits);
    assert_eq!(fixture.policy(&foreign.id), PermissionPolicy::Deny);
    assert!(fixture.manager.inner.lock().as_ref().unwrap().acp.is_none());
    assert!(!fixture
        .manager
        .pending_soft_respawn
        .lock()
        .contains_key(&foreign.id));
}
