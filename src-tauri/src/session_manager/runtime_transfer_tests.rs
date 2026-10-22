//! Interleave the actual synchronous pool moves with actual runtime consumers.
use super::*;

struct TransferPause {
    reached: mpsc::SyncSender<()>,
    proceed: mpsc::Receiver<()>,
}

fn transfer_pauses() -> &'static StdMutex<HashMap<String, TransferPause>> {
    static PAUSES: OnceLock<StdMutex<HashMap<String, TransferPause>>> = OnceLock::new();
    PAUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(crate) fn pause_after_take(id: &str, stage: &str) {
    let pause = transfer_pauses()
        .lock()
        .unwrap()
        .remove(&format!("{id}:{stage}"));
    if let Some(pause) = pause {
        pause.reached.send(()).unwrap();
        pause.proceed.recv_timeout(Duration::from_secs(4)).unwrap();
    }
}

#[derive(Clone, Copy)]
pub(super) enum Move {
    BackgroundParked,
    BusyLiveBackground,
    LiveParked,
    BackgroundLive,
    ParkedLive,
    ParkedBackground,
}

impl Move {
    fn stage(self) -> &'static str {
        match self {
            Self::BackgroundParked => "background_parked",
            Self::BusyLiveBackground => "live_background",
            Self::LiveParked => "live_parked",
            Self::BackgroundLive => "background_live",
            Self::ParkedLive | Self::ParkedBackground => "parked_take",
        }
    }

    fn run(self, manager: &SessionManager, id: &str) {
        match self {
            Self::BackgroundParked => manager.promote_background_ready_to_parked(id),
            Self::BusyLiveBackground | Self::LiveParked => manager.try_park_live().unwrap(),
            Self::BackgroundLive => assert!(manager.promote_background_to_live(id)),
            Self::ParkedLive => assert!(manager.promote_parked_to_live(id)),
            Self::ParkedBackground => {
                assert!(manager.promote_parked_to_background(id).is_some());
            }
        }
    }
}

pub(super) struct Moving {
    proceed: mpsc::SyncSender<()>,
    worker: std::thread::JoinHandle<()>,
}

pub(super) fn start_move(manager: &Arc<SessionManager>, id: &str, movement: Move) -> Moving {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    transfer_pauses().lock().unwrap().insert(
        format!("{id}:{}", movement.stage()),
        TransferPause {
            reached: reached_tx,
            proceed: proceed_rx,
        },
    );
    let manager = manager.clone();
    let id = id.to_owned();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        movement.run(&manager, &id);
    });
    reached_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    Moving {
        proceed: proceed_tx,
        worker,
    }
}

pub(super) fn action<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> (mpsc::Receiver<()>, std::thread::JoinHandle<T>) {
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = work();
        let _ = finished_tx.send(());
        result
    });
    (finished_rx, worker)
}

pub(super) fn finish<T>(
    moving: Moving,
    finished: mpsc::Receiver<()>,
    consumer: std::thread::JoinHandle<T>,
) -> T {
    // Release the producer before assertions so a red failure cannot strand it.
    let _ = finished.recv_timeout(Duration::from_millis(100));
    moving.proceed.send(()).unwrap();
    moving.worker.join().unwrap();
    consumer.join().unwrap()
}

impl Fixture {
    pub(super) fn prepare_transfer(&self, movement: Move) {
        match movement {
            Move::BackgroundParked | Move::BackgroundLive => {
                self.move_focus(false);
                if matches!(movement, Move::BackgroundLive) {
                    self.manager.try_park_live().unwrap();
                }
            }
            Move::BusyLiveBackground => {
                let mut live = self.manager.inner.lock();
                let live = live.as_mut().unwrap();
                live.fsm.begin_stream().unwrap();
                live.prompt_in_flight = true;
            }
            Move::LiveParked => {}
            Move::ParkedLive | Move::ParkedBackground => {
                self.move_focus(false);
                self.park_target();
                if matches!(movement, Move::ParkedLive) {
                    self.manager.try_park_live().unwrap();
                } else {
                    // Sending on the parked chat must preserve the foreign turn.
                    let mut live = self.manager.inner.lock();
                    let live = live.as_mut().unwrap();
                    live.fsm.begin_stream().unwrap();
                    live.prompt_in_flight = true;
                }
            }
        }
    }
}

#[test]
fn sandbox_refresh_sees_an_owner_moving_from_background_to_parked() {
    let fixture = Fixture::new();
    fixture.prepare_transfer(Move::BackgroundParked);
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BackgroundParked,
    );
    let manager = fixture.manager.clone();
    let project = fixture.target_meta.project_id.clone().unwrap();
    let (finished, worker) = action(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(
            session_spawn_options::apply_project_sandbox(
                None,
                &manager,
                &project,
                Some("strict".into()),
            ),
        )
    });
    finish(moving, finished, worker).unwrap();
    assert!(
        !fixture.target.is_alive(),
        "sandbox apply missed the migrating process"
    );
    assert!(!fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
    fixture.assert_other_untouched();
}

#[test]
fn permission_refresh_sees_an_owner_moving_from_background_to_parked() {
    let fixture = Fixture::new();
    fixture.prepare_transfer(Move::BackgroundParked);
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BackgroundParked,
    );
    let manager = fixture.manager.clone();
    let (finished, worker) = action(move || {
        let mut settings = store::load_settings();
        settings.permission_policy = "accept_edits".into();
        store::save_settings(&settings).unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(manager.refresh_permission_policies_core(None))
    });
    finish(moving, finished, worker).unwrap();
    assert!(
        !fixture.target.is_alive(),
        "permission apply missed the migrating process"
    );
    assert!(!fixture
        .manager
        .parked
        .lock()
        .contains_key(&fixture.target_meta.id));
    assert!(store::load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|s| s.id == fixture.target_meta.id)
        .unwrap()
        .permission_policy
        .is_none());
}

fn session_option_during_move(movement: Move) {
    let fixture = Fixture::new();
    fixture.prepare_transfer(movement);
    let moving = start_move(&fixture.manager, &fixture.target_meta.id, movement);
    let manager = fixture.manager.clone();
    let id = fixture.target_meta.id.clone();
    let (finished, worker) = action(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(
            session_spawn_options::apply_session_option(
                None,
                &manager,
                &id,
                SessionSpawnOption::MaxTurns(Some(23)),
            ),
        )
    });
    let saved = finish(moving, finished, worker).unwrap();
    assert_eq!(saved.max_agent_turns, Some(23));
    if matches!(movement, Move::BusyLiveBackground) {
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
        assert!(
            fixture
                .manager
                .pending_soft_respawn
                .lock()
                .contains_key(&fixture.target_meta.id),
            "busy migrating owner lost its rebind"
        );
    } else {
        assert!(
            !fixture.target.is_alive(),
            "idle migrating owner kept old spawn flags"
        );
        assert!(!fixture
            .manager
            .parked
            .lock()
            .contains_key(&fixture.target_meta.id));
    }
    assert!(fixture.other.is_alive());
    if matches!(movement, Move::ParkedBackground) {
        fixture.assert_other_untouched();
        assert!(
            fixture
                .manager
                .inner
                .lock()
                .as_ref()
                .unwrap()
                .prompt_in_flight
        );
        assert!(!fixture
            .manager
            .background
            .lock()
            .contains_key(&fixture.target_meta.id));
    }
}

#[test]
fn session_option_rebind_survives_busy_live_to_background() {
    session_option_during_move(Move::BusyLiveBackground);
}
#[test]
fn session_option_rebind_survives_live_to_parked() {
    session_option_during_move(Move::LiveParked);
}
#[test]
fn session_option_rebind_survives_background_to_live() {
    session_option_during_move(Move::BackgroundLive);
}

#[test]
fn session_option_rebind_survives_parked_to_live_publication() {
    session_option_during_move(Move::ParkedLive);
}

#[test]
fn session_option_rebind_survives_parked_to_background_without_stealing_a_busy_focus() {
    session_option_during_move(Move::ParkedBackground);
}

#[test]
fn actual_state_receipt_survives_busy_live_to_background() {
    let fixture = Fixture::new();
    let process = fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .process_id
        .clone();
    let agent = fixture.target_meta.agent_session_id.clone().unwrap();
    fixture.prepare_transfer(Move::BusyLiveBackground);
    let moving = start_move(
        &fixture.manager,
        &fixture.target_meta.id,
        Move::BusyLiveBackground,
    );
    let manager = fixture.manager.clone();
    let (finished, worker) = action(move || {
        manager.apply_state_event(
            &process,
            Some(&agent),
            Some(&agent),
            "migration-state",
            None,
        )
    });
    let snapshot = finish(moving, finished, worker);
    assert!(
        snapshot.is_some(),
        "valid state receipt was dropped during migration"
    );
    assert_eq!(
        fixture
            .manager
            .background
            .lock()
            .get(&fixture.target_meta.id)
            .unwrap()
            .backend,
        "migration-state"
    );
    assert!(fixture.target.is_alive());
}

#[test]
fn retirement_keeps_a_shared_tenant_that_is_moving_to_parked() {
    let fixture = Fixture::new();
    let process = fixture
        .manager
        .inner
        .lock()
        .as_ref()
        .unwrap()
        .process_id
        .clone();
    let mut foreign = routing_tests::bare_live_session(&fixture.other_meta.id, &process);
    foreign.meta = fixture.other_meta.clone();
    foreign.acp = Some(fixture.target.clone());
    fixture
        .manager
        .background
        .lock()
        .insert(fixture.other_meta.id.clone(), foreign);
    let moving = start_move(
        &fixture.manager,
        &fixture.other_meta.id,
        Move::BackgroundParked,
    );
    let manager = fixture.manager.clone();
    let id = fixture.target_meta.id.clone();
    let (finished, worker) = action(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(manager.soft_respawn_for_session_core(None, &id, "rules"))
    });
    finish(moving, finished, worker);
    assert!(
        fixture.target.is_alive(),
        "retirement killed a migrating foreign co-tenant"
    );
    assert!(fixture
        .manager
        .parked
        .lock()
        .get(&fixture.other_meta.id)
        .unwrap()
        .acp
        .is_alive());
    assert!(fixture.manager.inner.lock().as_ref().unwrap().acp.is_none());
}
