//! Project-wide sandbox changes through the actual command, store and ACP core.
use super::*;

struct PausedChange {
    proceed: mpsc::SyncSender<()>,
    finished: mpsc::Receiver<()>,
    worker: std::thread::JoinHandle<Result<store::Project, String>>,
}

impl Fixture {
    fn context(&self, meta: &store::SessionMeta) -> RuntimeWorkspaceContext {
        let project = store::load_projects_snapshot()
            .unwrap()
            .into_iter()
            .find(|project| Some(&project.id) == meta.project_id.as_ref())
            .unwrap();
        RuntimeWorkspaceContext::capture(meta, &project.path, &store::load_settings(), false)
            .unwrap()
    }

    fn capture_target_context(&self) {
        let context = self.context(&self.target_meta);
        self.manager
            .with_session_mut(&self.target_meta.id, |live| {
                live.project_path = Some(context.cwd.clone());
                live.workspace_context = Some(context);
            })
            .unwrap();
    }

    fn sandbox(&self, profile: Option<&str>) -> store::Project {
        self.runtime
            .block_on(session_spawn_options::apply_project_sandbox(
                None,
                &self.manager,
                self.target_meta.project_id.as_deref().unwrap(),
                profile.map(str::to_owned),
            ))
            .unwrap()
    }

    fn sibling(&mut self, slot: &str) -> (store::SessionMeta, Arc<AcpClient>) {
        let mut meta =
            store::create_session(self.target_meta.project_id.clone(), None, false).unwrap();
        meta.agent_session_id = Some(format!("synthetic-{}", meta.id));
        store::update_session_meta(&meta).unwrap();
        let entered = self.runtime.enter();
        let (client, peer) = test_transport::client_for(false, SessionMcpPolicy::default(), None);
        drop(entered);
        self._peers.push(peer);
        let mut live = routing_tests::bare_live_session(&meta.id, &format!("process-{}", meta.id));
        live.meta = meta.clone();
        live.workspace_context = Some(self.context(&meta));
        live.acp = Some(client.clone());
        if slot == "busy" {
            live.fsm.begin_stream().unwrap();
            live.prompt_in_flight = true;
        }
        self.manager.background.lock().insert(meta.id.clone(), live);
        if slot == "parked" {
            self.manager.promote_background_ready_to_parked(&meta.id);
            assert!(self.manager.parked.lock().contains_key(&meta.id));
        }
        (meta, client)
    }

    fn paused_sandbox(&self) -> PausedChange {
        let id = self.target_meta.project_id.clone().unwrap();
        let (reached_tx, reached_rx) = mpsc::sync_channel(1);
        let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        pauses().lock().unwrap().insert(
            id.clone(),
            Pause {
                reached: reached_tx,
                proceed: proceed_rx,
            },
        );
        let manager = self.manager.clone();
        let worker = std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(session_spawn_options::apply_project_sandbox(
                    None,
                    &manager,
                    &id,
                    Some("workspace".into()),
                ));
            let _ = finished_tx.send(());
            result
        });
        reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
        PausedChange {
            proceed: proceed_tx,
            finished: finished_rx,
            worker,
        }
    }
}

#[test]
fn off_focus_idle_project_chat_reloads_without_touching_foreign_focus() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    fixture.move_focus(false);
    fixture.sandbox(Some("strict"));
    fixture.assert_other_untouched();
    assert!(
        !fixture.target.is_alive(),
        "off-focus idle sandbox stayed stale"
    );
    assert!(!fixture
        .manager
        .background
        .lock()
        .contains_key(&fixture.target_meta.id));
}

#[test]
fn off_focus_busy_project_chat_queues_rebind_without_interrupting_the_answer() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    fixture.move_focus(true);
    fixture.sandbox(Some("strict"));
    fixture.assert_other_untouched();
    assert!(fixture.target.is_alive());
    assert!(
        fixture
            .manager
            .background
            .lock()
            .get(&fixture.target_meta.id)
            .unwrap()
            .prompt_in_flight
    );
    assert_eq!(
        fixture
            .manager
            .pending_soft_respawn
            .lock()
            .get(&fixture.target_meta.id)
            .map(String::as_str),
        Some("project_sandbox")
    );
}

#[test]
fn off_focus_parked_project_chat_cannot_reuse_the_old_sandbox() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    fixture.move_focus(false);
    fixture.park_target();
    fixture.sandbox(Some("strict"));
    fixture.assert_other_untouched();
    assert!(!fixture.target.is_alive(), "parked sandbox stayed stale");
    assert!(!fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
}

#[test]
fn one_project_change_covers_live_background_and_parked_chats() {
    let mut fixture = Fixture::new();
    fixture.capture_target_context();
    let (idle, idle_client) = fixture.sibling("idle");
    let (busy, busy_client) = fixture.sibling("busy");
    let (parked, parked_client) = fixture.sibling("parked");
    fixture.sandbox(Some("strict"));
    assert!(!fixture.target.is_alive());
    assert!(
        !idle_client.is_alive(),
        "project's idle sibling missed the change"
    );
    assert!(!parked_client.is_alive());
    assert!(busy_client.is_alive() && fixture.other.is_alive());
    assert!(!fixture.manager.background.lock().contains_key(&idle.id));
    assert!(!fixture.manager.parked.lock().contains_key(&parked.id));
    assert!(fixture
        .manager
        .pending_soft_respawn
        .lock()
        .contains_key(&busy.id));
    let stored = store::load_sessions_snapshot().unwrap();
    assert_eq!(
        stored
            .iter()
            .find(|row| row.id == busy.id)
            .unwrap()
            .agent_session_id,
        busy.agent_session_id
    );
}

#[test]
fn saving_an_unchanged_sandbox_keeps_the_current_process_and_other_pending_work() {
    let fixture = Fixture::new();
    store::set_project_sandbox_profile(
        fixture.target_meta.project_id.as_deref().unwrap(),
        Some("workspace".into()),
    )
    .unwrap();
    fixture.capture_target_context();
    fixture
        .manager
        .pending_soft_respawn
        .lock()
        .insert(fixture.target_meta.id.clone(), "newer_route".into());
    fixture.sandbox(Some("workspace"));
    assert!(
        fixture.target.is_alive(),
        "unchanged sandbox needlessly retired the process"
    );
    assert_eq!(
        fixture
            .manager
            .pending_soft_respawn
            .lock()
            .get(&fixture.target_meta.id)
            .map(String::as_str),
        Some("newer_route")
    );
}

#[test]
fn clearing_a_project_override_reloads_parked_chats_with_the_global_default() {
    let fixture = Fixture::new();
    store::set_project_sandbox_profile(
        fixture.target_meta.project_id.as_deref().unwrap(),
        Some("strict".into()),
    )
    .unwrap();
    fixture.capture_target_context();
    fixture.move_focus(false);
    fixture.park_target();
    let project = fixture.sandbox(None);
    assert!(project.sandbox_profile.is_none());
    assert!(
        !fixture.target.is_alive(),
        "cleared override left the parked process stale"
    );
    fixture.assert_other_untouched();
}

#[test]
fn explicit_override_with_the_same_effective_profile_still_refreshes_captured_context() {
    let fixture = Fixture::new();
    assert_eq!(store::load_settings().sandbox_profile, "workspace");
    fixture.capture_target_context();
    fixture.move_focus(false);
    fixture.sandbox(Some("workspace"));
    assert!(
        !fixture.target.is_alive(),
        "raw project binding changed but its execution context did not reload"
    );
    fixture.assert_other_untouched();
}

#[test]
fn waiting_project_change_does_not_retire_a_new_owner_in_another_project() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    let paused = fixture.paused_sandbox();
    let meta = store::set_session_project(
        &fixture.target_meta.id,
        fixture.other_meta.project_id.clone(),
    )
    .unwrap();
    let mut replacement = routing_tests::bare_live_session(&meta.id, "replacement-generation");
    replacement.workspace_context = Some(fixture.context(&meta));
    replacement.meta = meta;
    replacement.acp = Some(fixture.other.clone());
    *fixture.manager.inner.lock() = Some(replacement);
    paused.proceed.send(()).unwrap();
    paused.worker.join().unwrap().unwrap();
    assert!(
        fixture.other.is_alive(),
        "late project request retired a replacement owner from another project"
    );
    assert_eq!(
        fixture.manager.inner.lock().as_ref().unwrap().process_id,
        "replacement-generation"
    );
}

#[test]
fn project_change_waits_for_connect_and_preserves_an_already_current_generation() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    let manager = fixture.manager.clone();
    let connect = fixture.runtime.block_on(manager.connect_lock.lock());
    let paused = fixture.paused_sandbox();
    // The simulated in-flight connect publishes the newly saved project context.
    fixture.capture_target_context();
    paused.proceed.send(()).unwrap();
    let waited = paused
        .finished
        .recv_timeout(Duration::from_millis(100))
        .is_err();
    drop(connect);
    paused.worker.join().unwrap().unwrap();
    assert!(
        waited,
        "project retirement ran through an in-flight connect"
    );
    assert!(
        fixture.target.is_alive(),
        "latest matching generation was retired"
    );
    assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
}

#[test]
fn corrupt_session_snapshot_cannot_retire_any_project_process() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    let path = crate::paths::sessions_index_file();
    assert!(path.starts_with(&fixture.home.path));
    std::fs::write(&path, b"{not-json").unwrap();
    let result = fixture
        .runtime
        .block_on(session_spawn_options::apply_project_sandbox(
            None,
            &fixture.manager,
            fixture.target_meta.project_id.as_deref().unwrap(),
            Some("strict".into()),
        ));
    assert!(
        result.is_err(),
        "corrupt sessions were silently accepted by project refresh"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"{not-json");
    assert!(fixture.target.is_alive() && fixture.other.is_alive());
    assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
}

#[test]
fn shared_foreign_busy_tenant_keeps_its_process_while_changed_owner_detaches() {
    let fixture = Fixture::new();
    fixture.capture_target_context();
    fixture.move_focus(false);
    let process_id = fixture
        .manager
        .background
        .lock()
        .get(&fixture.target_meta.id)
        .unwrap()
        .process_id
        .clone();
    let context = fixture.context(&fixture.other_meta);
    {
        let mut live = fixture.manager.inner.lock();
        let foreign = live.as_mut().unwrap();
        foreign.process_id = process_id;
        foreign.acp = Some(fixture.target.clone());
        foreign.workspace_context = Some(context);
        foreign.fsm.begin_stream().unwrap();
        foreign.prompt_in_flight = true;
    }
    fixture.sandbox(Some("strict"));
    assert!(
        fixture.target.is_alive(),
        "project change killed a shared foreign answer"
    );
    assert!(!fixture
        .manager
        .background
        .lock()
        .contains_key(&fixture.target_meta.id));
    let live = fixture.manager.inner.lock();
    let foreign = live.as_ref().unwrap();
    assert_eq!(foreign.app_session_id, fixture.other_meta.id);
    assert!(foreign.prompt_in_flight && foreign.acp.as_ref().unwrap().is_alive());
    assert!(!fixture
        .manager
        .pending_soft_respawn
        .lock()
        .contains_key(&fixture.other_meta.id));
}

#[test]
fn project_with_only_stored_chats_keeps_foreign_focus_and_does_not_queue_a_process() {
    let fixture = Fixture::new();
    fixture.move_focus(false);
    fixture
        .manager
        .background
        .lock()
        .remove(&fixture.target_meta.id);
    fixture.sandbox(Some("strict"));
    fixture.assert_other_untouched();
    assert!(fixture.target.is_alive());
    assert!(fixture.manager.pending_soft_respawn.lock().is_empty());
    assert_eq!(
        store::load_sessions_snapshot()
            .unwrap()
            .into_iter()
            .find(|row| row.id == fixture.target_meta.id)
            .unwrap()
            .agent_session_id,
        fixture.target_meta.agent_session_id
    );
}
