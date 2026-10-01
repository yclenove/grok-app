use super::*;
use crate::GnomeNativePolicyWatch;
use std::sync::Mutex;
#[path = "gnome_consent.rs"]
mod consent;
const POLICY_PATH: &str = "/org/grok/ComputerUse/NativePolicy";
const POLICY_IFACE: &str = "org.grok.ComputerUse.NativePolicy1";
type State = (u32, String, u32, bool);
fn state() -> State {
    (1, "f0194527-4330-4029-91e1-7911127739ab".into(), 1, false)
}
struct HelperState {
    value: Mutex<State>,
    fail: AtomicBool,
    hold: AtomicBool,
    release: tokio::sync::Notify,
    pulse: AtomicBool,
    calls: AtomicU32,
}
struct Helper(Arc<HelperState>);
#[zbus::interface(name = "org.grok.ComputerUse.NativePolicy1")]
impl Helper {
    async fn get_state(
        &self,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<State> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        loop {
            let released = self.0.release.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            if !self.0.hold.load(Ordering::SeqCst) {
                break;
            }
            released.await;
        }
        if self.0.fail.load(Ordering::SeqCst) {
            return Err(zbus::fdo::Error::Failed("observer unavailable".into()));
        }
        if self.0.pulse.swap(false, Ordering::SeqCst) {
            let mut value = state();
            value.3 = true;
            connection
                .emit_signal(None::<&str>, POLICY_PATH, POLICY_IFACE, "Changed", &value)
                .await
                .unwrap();
        }
        Ok(self.0.value.lock().unwrap().clone())
    }
}
pub(in crate::tests) struct Source {
    _login: Fixture,
    shell: Shell,
    helper: Arc<HelperState>,
}
impl Source {
    async fn new(value: State) -> Self {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let helper = Arc::new(HelperState {
            value: Mutex::new(value),
            fail: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            release: tokio::sync::Notify::new(),
            pulse: AtomicBool::new(false),
            calls: AtomicU32::new(0),
        });
        shell
            .server
            .object_server()
            .at(POLICY_PATH, Helper(helper.clone()))
            .await
            .unwrap();
        Self {
            _login: login,
            shell,
            helper,
        }
    }
    async fn watch(&self) -> Result<GnomeNativePolicyWatch, String> {
        GnomeNativePolicyWatch::prepare(self.shell.watch(&self._login).await?).await
    }
    pub(in crate::tests) async fn start() -> (Self, GnomeNativePolicyWatch) {
        let source = Self::new(state()).await;
        let watch = source.watch().await.unwrap();
        (source, watch)
    }
    pub(in crate::tests) async fn consent_watch(
        &self,
    ) -> Result<(GnomeNativePolicyWatch, crate::GnomePolicyActivation), String> {
        GnomeNativePolicyWatch::prepare_for_consent(self.shell.watch(&self._login).await?).await
    }
    pub(in crate::tests) async fn consent_source() -> Self {
        Self::new(state()).await
    }
    pub(in crate::tests) fn hold_replies(&self) -> u32 {
        self.helper.hold.store(true, Ordering::SeqCst);
        self.helper.calls.load(Ordering::SeqCst)
    }
    pub(in crate::tests) fn release_replies(&self) {
        self.helper.hold.store(false, Ordering::SeqCst);
        self.helper.release.notify_waiters();
    }
    pub(in crate::tests) fn calls(&self) -> u32 {
        self.helper.calls.load(Ordering::SeqCst)
    }
    pub(in crate::tests) async fn cycle_shell_name(&self, shield: bool) {
        let name = if shield { SHIELD } else { SHELL };
        self.shell.server.release_name(name).await.unwrap();
        self.shell.server.request_name(name).await.unwrap();
    }
    async fn change(&self, value: State) {
        *self.helper.value.lock().unwrap() = value.clone();
        self.shell
            .server
            .emit_signal(None::<&str>, POLICY_PATH, POLICY_IFACE, "Changed", &value)
            .await
            .unwrap();
    }
    pub(in crate::tests) async fn takeover(&self) {
        let mut value = self.helper.value.lock().unwrap().clone();
        value.2 += 1;
        self.change(value).await;
    }
}
type Owner = tokio::task::JoinHandle<Result<(), String>>;
async fn running(
    watch: GnomeNativePolicyWatch,
) -> (
    Arc<dyn PortalInputPolicy>,
    tokio::sync::oneshot::Sender<()>,
    Owner,
) {
    let policy = watch.input_policy();
    assert!(!policy.input_available());
    let (stop, receive) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(receive));
    ready(&policy).await;
    (policy, stop, owner)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polls_helper_on_pinned_shell_and_joins_all_original_watchers() {
    let (source, watch) = Source::start().await;
    let (policy, stop, owner) = running(watch).await;
    tokio::time::sleep(Duration::from_millis(240)).await;
    assert!(source.helper.calls.load(Ordering::SeqCst) >= 2);
    assert!(policy.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    assert!(!policy.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn physical_generation_change_retires_old_grant_without_next_action() {
    let (source, watch) = Source::start().await;
    let (policy, _stop, owner) = running(watch).await;
    source.takeover().await;
    assert!(ended(owner).await.contains("new consent"));
    assert!(policy.user_input_active());
    assert!(!policy.input_available());
    source.change(state()).await;
    assert!(!policy.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_epoch_version_and_generation_changes_all_revoke() {
    for case in 0..4 {
        let (source, watch) = Source::start().await;
        let (policy, _stop, owner) = running(watch).await;
        let mut value = state();
        match case {
            0 => value.3 = true,
            1 => value.1 = uuid::Uuid::new_v4().to_string(),
            2 => value.0 = 2,
            _ => value.2 = 0,
        }
        source.change(value).await;
        assert!(ended(owner).await.contains("changed"));
        assert!(!policy.input_available());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_blocked_invalid_version_epoch_or_fault_never_grants() {
    for case in 0..6 {
        let mut value = state();
        match case {
            0 => value.3 = true,
            1 => value.0 = 2,
            2 => value.1 = "invalid".into(),
            _ => (),
        }
        let source = Source::new(value).await;
        if case == 3 {
            source.helper.fail.store(true, Ordering::SeqCst);
        }
        if case == 4 {
            source
                .shell
                .server
                .object_server()
                .remove::<Helper, _>(POLICY_PATH)
                .await
                .unwrap();
        }
        if case == 5 {
            source.helper.hold.store(true, Ordering::SeqCst);
        }
        let result = source.watch().await;
        source.helper.hold.store(false, Ordering::SeqCst);
        source.helper.release.notify_waiters();
        assert!(result.is_err(), "unsafe startup case {case}");
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialization_pulse_cannot_disappear_in_unlocked_snapshot() {
    let source = Source::new(state()).await;
    source.helper.pulse.store(true, Ordering::SeqCst);
    let watch = source.watch().await.unwrap();
    let policy = watch.input_policy();
    let (_stop, receive) = tokio::sync::oneshot::channel();
    assert!(watch
        .run_until(receive)
        .await
        .unwrap_err()
        .contains("initialization"));
    assert!(!policy.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn helper_unexport_silent_state_change_error_and_hang_revoke_by_heartbeat() {
    for case in 0..4 {
        let (source, watch) = Source::start().await;
        let (policy, _stop, owner) = running(watch).await;
        match case {
            0 => {
                source
                    .shell
                    .server
                    .object_server()
                    .remove::<Helper, _>(POLICY_PATH)
                    .await
                    .unwrap();
            }
            1 => {
                source.helper.value.lock().unwrap().2 += 1;
            }
            2 => source.helper.fail.store(true, Ordering::SeqCst),
            _ => source.helper.hold.store(true, Ordering::SeqCst),
        }
        let result = ended(owner).await;
        source.helper.hold.store(false, Ordering::SeqCst);
        source.helper.release.notify_waiters();
        assert!(!result.is_empty());
        assert!(!policy.input_available());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_sender_wrong_path_or_interface_does_not_revoke_valid_helper() {
    let (source, watch) = Source::start().await;
    let (policy, stop, owner) = running(watch).await;
    let attacker = source.shell.client().await;
    let mut bad = state();
    bad.3 = true;
    attacker
        .emit_signal(None::<&str>, POLICY_PATH, POLICY_IFACE, "Changed", &bad)
        .await
        .unwrap();
    source
        .shell
        .server
        .emit_signal(None::<&str>, "/elsewhere", POLICY_IFACE, "Changed", &bad)
        .await
        .unwrap();
    source
        .shell
        .server
        .emit_signal(None::<&str>, POLICY_PATH, "org.grok.Wrong", "Changed", &bad)
        .await
        .unwrap();
    source.change(state()).await;
    tokio::time::sleep(Duration::from_millis(240)).await;
    assert!(policy.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_signal_and_original_shell_loss_revoke() {
    for malformed in [true, false] {
        let (source, watch) = Source::start().await;
        let (policy, _stop, owner) = running(watch).await;
        if malformed {
            source
                .shell
                .server
                .emit_signal(
                    None::<&str>,
                    POLICY_PATH,
                    POLICY_IFACE,
                    "Changed",
                    &("bad",),
                )
                .await
                .unwrap();
        } else {
            source.shell.server.release_name(SHELL).await.unwrap();
        }
        assert!(!ended(owner).await.is_empty());
        assert!(!policy.input_available());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpolled_drop_precancel_abort_and_delayed_poll_never_leave_policy_ready() {
    for case in 0..4 {
        let (_source, watch) = Source::start().await;
        let policy = watch.input_policy();
        let (stop, receive) = tokio::sync::oneshot::channel();
        match case {
            0 => drop(watch),
            1 => {
                stop.send(()).unwrap();
                watch.run_until(receive).await.unwrap();
            }
            2 => {
                let owner = tokio::spawn(watch.run_until(receive));
                ready(&policy).await;
                owner.abort();
                assert!(owner.await.unwrap_err().is_cancelled());
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(520)).await;
                assert!(watch.run_until(receive).await.is_err());
            }
        }
        assert!(!policy.input_available());
    }
}
#[tokio::test]
async fn stalled_reactor_expires_policy_without_waiting_for_monitor_poll() {
    let (_source, watch) = Source::start().await;
    let (policy, _stop, owner) = running(watch).await;
    // Deliberately stop the sole runtime thread, not a synthetic signal fixture.
    std::thread::sleep(Duration::from_millis(520));
    assert!(
        !policy.input_available(),
        "stalled monitor kept granting input"
    );
    assert!(!ended(owner).await.is_empty());
    assert!(!policy.input_available());
}
