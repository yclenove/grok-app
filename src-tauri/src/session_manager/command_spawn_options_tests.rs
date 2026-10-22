//! Real setting-command core, real isolated store and synthetic duplex ACP.
//! No native AppHandle, real CLI process, provider or credential access.
use super::*;
use crate::acp_client::{test_transport, AcpClient};
use crate::commands::session_spawn_options::{self, SessionSpawnOption};
use crate::session_migration::{mcp_stdio::SessionMcpPolicy, test_home::IsolatedAppHome};
use crate::store;
use std::sync::{mpsc, Mutex as StdMutex};
use std::time::Duration;

#[path = "human_gate_consumer_tests.rs"]
pub(crate) mod human_gate_consumer_tests;
#[path = "human_gate_resolution_tests.rs"]
pub(crate) mod human_gate_resolution_tests;
#[path = "permission_request_tests.rs"]
pub(crate) mod permission_request_tests;
#[path = "permission_resolution_tests.rs"]
pub(crate) mod permission_resolution_tests;
#[path = "project_sandbox_tests.rs"]
mod project_sandbox_tests;
#[path = "runtime_transfer_tests.rs"]
pub(crate) mod runtime_transfer_tests;
#[path = "stream_owner_tests.rs"]
pub(crate) mod stream_owner_tests;
#[path = "tool_identity_consumer_tests.rs"]
mod tool_identity_consumer_tests;
#[path = "tool_journal_consumer_tests.rs"]
pub(crate) mod tool_journal_consumer_tests;
#[path = "tool_owner_tests.rs"]
pub(crate) mod tool_owner_tests;

struct Pause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn pauses() -> &'static StdMutex<HashMap<String, Pause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, Pause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_after_observation(id: &str) {
    let pause = pauses().lock().unwrap().remove(id);
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

struct Fixture {
    home: IsolatedAppHome,
    runtime: tokio::runtime::Runtime,
    _peers: Vec<tokio::io::DuplexStream>,
    manager: Arc<SessionManager>,
    target_meta: store::SessionMeta,
    other_meta: store::SessionMeta,
    target: Arc<AcpClient>,
    other: Arc<AcpClient>,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire_with_user_home("command-spawn-options");
        let mut projects = Vec::new();
        for name in ["项目甲", "项目乙"] {
            let path = home.path.join(name);
            std::fs::create_dir(&path).unwrap();
            let project = store::ensure_import_project(name, path.to_str().unwrap())
                .unwrap()
                .project;
            projects.push(store::trust_project(&project.id).unwrap());
        }
        let mut target_meta =
            store::create_session(Some(projects[0].id.clone()), None, false).unwrap();
        target_meta.agent_session_id = Some("synthetic-resume-target".into());
        store::update_session_meta(&target_meta).unwrap();
        let other_meta = store::create_session(Some(projects[1].id.clone()), None, false).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let entered = runtime.enter();
        let (target, target_peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        let (other, other_peer) =
            test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        let manager = Arc::new(SessionManager::new());
        let mut live = routing_tests::bare_live_session(&target_meta.id, "target-process");
        live.meta = target_meta.clone();
        live.acp = Some(target.clone());
        *manager.inner.lock() = Some(live);
        Self {
            home,
            runtime,
            _peers: vec![target_peer, other_peer],
            manager,
            target_meta,
            other_meta,
            target,
            other,
        }
    }

    fn move_focus(&self, busy: bool) {
        let mut target = self.manager.inner.lock().take().unwrap();
        if busy {
            target.fsm.begin_stream().unwrap();
            target.prompt_in_flight = true;
        }
        self.manager
            .background
            .lock()
            .insert(self.target_meta.id.clone(), target);
        let mut other = routing_tests::bare_live_session(&self.other_meta.id, "other-process");
        other.meta = self.other_meta.clone();
        other.acp = Some(self.other.clone());
        *self.manager.inner.lock() = Some(other);
    }

    fn assert_other_untouched(&self) {
        assert!(
            self.other.is_alive(),
            "setting command retired the newly focused foreign ACP"
        );
        assert!(self
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .acp
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &self.other)));
        let stored = store::load_sessions_index()
            .into_iter()
            .find(|s| s.id == self.other_meta.id)
            .unwrap();
        assert_eq!(stored.plugin_dirs, self.other_meta.plugin_dirs);
        assert_eq!(stored.max_agent_turns, self.other_meta.max_agent_turns);
        assert_eq!(stored.no_ask_user, self.other_meta.no_ask_user);
        assert_eq!(
            stored.fork_agent_session,
            self.other_meta.fork_agent_session
        );
    }

    fn park_target(&self) {
        let live = self
            .manager
            .background
            .lock()
            .remove(&self.target_meta.id)
            .unwrap();
        self.manager.parked.lock().insert(
            self.target_meta.id.clone(),
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

#[derive(Clone, Copy)]
enum Change {
    Fork,
    Plugins,
    MaxTurns,
    NoAsk,
    ProjectSandbox,
}

impl Change {
    fn option(self, plugin_path: String) -> SessionSpawnOption {
        match self {
            Self::Fork => SessionSpawnOption::ForkAgent(true),
            Self::Plugins => SessionSpawnOption::PluginDirs(vec![plugin_path]),
            Self::MaxTurns => SessionSpawnOption::MaxTurns(Some(23)),
            Self::NoAsk => SessionSpawnOption::NoAskUser(Some(true)),
            Self::ProjectSandbox => panic!("project change is not a session flag"),
        }
    }

    fn assert_saved(self, fixture: &Fixture) {
        if matches!(self, Self::ProjectSandbox) {
            let project = store::load_projects()
                .into_iter()
                .find(|p| Some(&p.id) == fixture.target_meta.project_id.as_ref())
                .unwrap();
            assert_eq!(project.sandbox_profile.as_deref(), Some("workspace"));
        } else {
            let saved = store::load_sessions_index()
                .into_iter()
                .find(|s| s.id == fixture.target_meta.id)
                .unwrap();
            match self {
                Self::Fork => assert!(saved.fork_agent_session),
                Self::Plugins => assert_eq!(
                    saved.plugin_dirs,
                    vec![fixture
                        .home
                        .path
                        .join("插件")
                        .to_string_lossy()
                        .into_owned()]
                ),
                Self::MaxTurns => assert_eq!(saved.max_agent_turns, Some(23)),
                Self::NoAsk => assert_eq!(saved.no_ask_user, Some(true)),
                Self::ProjectSandbox => unreachable!(),
            }
        }
    }
}

fn focus_race(change: Change, busy: bool) {
    let fixture = Fixture::new();
    let key = if matches!(change, Change::ProjectSandbox) {
        fixture.target_meta.project_id.clone().unwrap()
    } else {
        fixture.target_meta.id.clone()
    };
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    pauses().lock().unwrap().insert(
        key.clone(),
        Pause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = fixture.manager.clone();
    let id = key;
    let plugin_path = fixture
        .home
        .path
        .join("插件")
        .to_string_lossy()
        .into_owned();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                if matches!(change, Change::ProjectSandbox) {
                    session_spawn_options::apply_project_sandbox(
                        None,
                        &manager,
                        &id,
                        Some("workspace".into()),
                    )
                    .await
                    .map(|_| ())
                } else {
                    session_spawn_options::apply_session_option(
                        None,
                        &manager,
                        &id,
                        change.option(plugin_path),
                    )
                    .await
                    .map(|_| ())
                }
            })
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    change.assert_saved(&fixture);
    fixture.move_focus(busy);
    proceed_tx.send(()).unwrap();
    worker.join().unwrap().unwrap();
    fixture.assert_other_untouched();
    if busy {
        assert!(fixture.target.is_alive());
        assert!(fixture
            .manager
            .background
            .lock()
            .contains_key(&fixture.target_meta.id));
        assert!(fixture
            .manager
            .pending_soft_respawn
            .lock()
            .contains_key(&fixture.target_meta.id));
    } else {
        assert!(
            !fixture.target.is_alive(),
            "saved setting left its original idle ACP unchanged"
        );
        assert!(!fixture
            .manager
            .background
            .lock()
            .contains_key(&fixture.target_meta.id));
    }
}

#[test]
fn fork_setting_keeps_the_captured_chat_after_focus_changes() {
    focus_race(Change::Fork, false);
}
#[test]
fn plugin_setting_keeps_the_captured_chat_after_focus_changes() {
    focus_race(Change::Plugins, false);
}
#[test]
fn max_turn_setting_keeps_the_captured_chat_after_focus_changes() {
    focus_race(Change::MaxTurns, false);
}
#[test]
fn no_ask_setting_keeps_the_captured_chat_after_focus_changes() {
    focus_race(Change::NoAsk, false);
}
#[test]
fn project_sandbox_keeps_the_captured_chat_after_focus_changes() {
    focus_race(Change::ProjectSandbox, false);
}

#[test]
fn captured_setting_targets_that_become_busy_are_queued_without_losing_the_answer() {
    for change in [
        Change::Fork,
        Change::Plugins,
        Change::MaxTurns,
        Change::NoAsk,
        Change::ProjectSandbox,
    ] {
        focus_race(change, true);
    }
}

#[test]
fn session_options_rebind_background_and_parked_owners_without_touching_focus() {
    for change in [
        Change::Fork,
        Change::Plugins,
        Change::MaxTurns,
        Change::NoAsk,
    ] {
        for slot in ["background-idle", "background-busy", "parked"] {
            let fixture = Fixture::new();
            let busy = slot == "background-busy";
            fixture.move_focus(busy);
            if slot == "parked" {
                fixture.park_target();
            }
            let result = fixture
                .runtime
                .block_on(session_spawn_options::apply_session_option(
                    None,
                    &fixture.manager,
                    &fixture.target_meta.id,
                    change.option(
                        fixture
                            .home
                            .path
                            .join("插件")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ))
                .unwrap();
            assert_eq!(result.id, fixture.target_meta.id);
            change.assert_saved(&fixture);
            fixture.assert_other_untouched();
            if busy {
                assert!(fixture.target.is_alive());
                assert!(fixture
                    .manager
                    .background
                    .lock()
                    .contains_key(&fixture.target_meta.id));
                assert!(fixture
                    .manager
                    .pending_soft_respawn
                    .lock()
                    .contains_key(&fixture.target_meta.id));
            } else {
                assert!(
                    !fixture.target.is_alive(),
                    "off-focus {slot} kept stale spawn flags"
                );
                assert!(!fixture
                    .manager
                    .background
                    .lock()
                    .contains_key(&fixture.target_meta.id));
                assert!(!fixture
                    .manager
                    .parked
                    .lock()
                    .contains_key(&fixture.target_meta.id));
            }
        }
    }
}

#[test]
fn stored_chat_without_a_tracked_process_does_not_retire_a_foreign_live_chat() {
    for change in [
        Change::Fork,
        Change::Plugins,
        Change::MaxTurns,
        Change::NoAsk,
    ] {
        let fixture = Fixture::new();
        fixture.move_focus(false);
        fixture
            .manager
            .background
            .lock()
            .remove(&fixture.target_meta.id);
        fixture
            .runtime
            .block_on(session_spawn_options::apply_session_option(
                None,
                &fixture.manager,
                &fixture.target_meta.id,
                change.option(
                    fixture
                        .home
                        .path
                        .join("插件")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ))
            .unwrap();
        change.assert_saved(&fixture);
        fixture.assert_other_untouched();
        assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    }
}

#[test]
fn failed_session_save_never_changes_process_or_pending_ownership() {
    for change in [
        Change::Fork,
        Change::Plugins,
        Change::MaxTurns,
        Change::NoAsk,
    ] {
        let fixture = Fixture::new();
        fixture.move_focus(false);
        let missing = uuid::Uuid::new_v4().to_string();
        let before = std::fs::read(crate::paths::sessions_index_file()).unwrap();
        assert!(fixture
            .runtime
            .block_on(session_spawn_options::apply_session_option(
                None,
                &fixture.manager,
                &missing,
                change.option(
                    fixture
                        .home
                        .path
                        .join("插件")
                        .to_string_lossy()
                        .into_owned()
                ),
            ))
            .is_err());
        assert_eq!(
            std::fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        fixture.assert_other_untouched();
        assert!(fixture.target.is_alive());
        assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    }
}

#[test]
fn cancelled_or_unarmable_forks_do_not_reload_an_agent() {
    for agent in [Some("synthetic-resume-target"), None, Some(" ")] {
        let fixture = Fixture::new();
        let mut saved = fixture.target_meta.clone();
        saved.agent_session_id = agent.map(str::to_owned);
        saved.fork_agent_session = agent == Some("synthetic-resume-target");
        store::update_session_meta(&saved).unwrap();
        let armed = agent != Some("synthetic-resume-target");
        let result = fixture
            .runtime
            .block_on(session_spawn_options::apply_session_option(
                None,
                &fixture.manager,
                &saved.id,
                SessionSpawnOption::ForkAgent(armed),
            ))
            .unwrap();
        assert!(!result.fork_agent_session);
        assert!(fixture.target.is_alive());
        assert!(fixture
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .acp
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &fixture.target)));
        assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    }
}

#[test]
fn clearing_spawn_overrides_retires_their_owner_and_keeps_the_other_chat() {
    for option in [
        SessionSpawnOption::PluginDirs(vec![]),
        SessionSpawnOption::MaxTurns(Some(0)),
        SessionSpawnOption::NoAskUser(None),
    ] {
        let fixture = Fixture::new();
        let mut saved = fixture.target_meta.clone();
        saved.plugin_dirs = vec![fixture
            .home
            .path
            .join("插件")
            .to_string_lossy()
            .into_owned()];
        saved.max_agent_turns = Some(23);
        saved.no_ask_user = Some(true);
        store::update_session_meta(&saved).unwrap();
        let mut expected = saved.clone();
        match &option {
            SessionSpawnOption::PluginDirs(_) => expected.plugin_dirs.clear(),
            SessionSpawnOption::MaxTurns(_) => expected.max_agent_turns = None,
            SessionSpawnOption::NoAskUser(_) => expected.no_ask_user = None,
            SessionSpawnOption::ForkAgent(_) => unreachable!(),
        }
        fixture.move_focus(false);
        let result = fixture
            .runtime
            .block_on(session_spawn_options::apply_session_option(
                None,
                &fixture.manager,
                &saved.id,
                option,
            ))
            .unwrap();
        assert_eq!(result.plugin_dirs, expected.plugin_dirs);
        assert_eq!(result.max_agent_turns, expected.max_agent_turns);
        assert_eq!(result.no_ask_user, expected.no_ask_user);
        assert!(!fixture.target.is_alive());
        fixture.assert_other_untouched();
    }
}

#[test]
fn project_sandbox_with_no_matching_live_project_does_not_retire_focus() {
    let fixture = Fixture::new();
    let other_project = fixture.other_meta.project_id.as_deref().unwrap();
    let result = fixture
        .runtime
        .block_on(session_spawn_options::apply_project_sandbox(
            None,
            &fixture.manager,
            other_project,
            Some("workspace".into()),
        ))
        .unwrap();
    assert_eq!(result.id, other_project);
    assert_eq!(result.sandbox_profile.as_deref(), Some("workspace"));
    assert!(fixture.target.is_alive());
    assert_eq!(
        fixture
            .manager
            .inner
            .lock()
            .as_ref()
            .unwrap()
            .app_session_id,
        fixture.target_meta.id
    );
    assert!(store::load_projects()
        .into_iter()
        .find(|p| Some(&p.id) == fixture.target_meta.project_id.as_ref())
        .unwrap()
        .sandbox_profile
        .is_none());
}

#[test]
fn project_sandbox_errors_leave_the_live_agent_and_project_file_unchanged() {
    let fixture = Fixture::new();
    let path = fixture.home.path.join("untrusted");
    std::fs::create_dir(&path).unwrap();
    let untrusted = store::ensure_import_project("untrusted", path.to_str().unwrap())
        .unwrap()
        .project;
    let before = std::fs::read(crate::paths::projects_file()).unwrap();
    for (id, profile) in [
        (
            fixture.target_meta.project_id.as_deref().unwrap(),
            "unknown-profile",
        ),
        (untrusted.id.as_str(), "workspace"),
        ("missing-project", "workspace"),
    ] {
        assert!(fixture
            .runtime
            .block_on(session_spawn_options::apply_project_sandbox(
                None,
                &fixture.manager,
                id,
                Some(profile.into())
            ))
            .is_err());
        assert_eq!(
            std::fs::read(crate::paths::projects_file()).unwrap(),
            before
        );
        assert!(fixture.target.is_alive());
        assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    }
}
