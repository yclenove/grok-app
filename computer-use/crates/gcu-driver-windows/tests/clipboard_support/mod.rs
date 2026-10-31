//! Deterministic desktop clipboard shared by otherwise independent sessions.
#![allow(dead_code)]
use async_trait::async_trait;
use gcu_core::{driver::*, model::*};
use gcu_driver_windows::{
    input::{
        api::*,
        clipboard::{ClipboardApi, PublishProof},
        fake::FakeInput,
        ledger::InputLedger,
        InputBackend,
    },
    uia::patterns::ActionContext,
    WindowsDriver,
};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::{Notify, OwnedMutexGuard};

type Contents = Vec<(u32, Vec<u8>)>;
pub fn original() -> Contents {
    vec![
        (13, b"original".to_vec()),
        (0xc001, b"synthetic rich format".to_vec()),
    ]
}
pub fn external() -> Contents {
    vec![(13, b"external".to_vec())]
}
#[derive(Clone, Copy)]
pub enum Change {
    Sequence,
    Content,
    Both,
}
#[derive(Clone, Copy)]
pub enum Risk {
    Password,
    Web,
    SensitiveLabel,
    Unknown,
    Secret,
}
struct State {
    contents: Contents,
    sequence: u32,
}
pub struct Desktop {
    state: Mutex<State>,
    serial: Arc<tokio::sync::Mutex<()>>,
    writes: AtomicUsize,
    restores: AtomicUsize,
    staged: Notify,
}
impl Default for Desktop {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                contents: original(),
                sequence: 10,
            }),
            serial: Arc::default(),
            writes: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
            staged: Notify::new(),
        }
    }
}
struct Saved {
    original: Contents,
    published: Contents,
    sequence: u32,
    _lock: OwnedMutexGuard<()>,
}
pub struct FakeClipboard {
    desktop: Arc<Desktop>,
    saved: Mutex<Option<Saved>>,
    change: Mutex<Option<Change>>,
    cancel: Mutex<Option<Cancel>>,
    stop: Mutex<Option<Arc<gcu_driver_windows::input::hotkey::FakeSafetyApi>>>,
    stop_stage: AtomicBool,
    restore_failure: AtomicBool,
}
impl FakeClipboard {
    pub fn new(desktop: Arc<Desktop>) -> Self {
        Self {
            desktop,
            saved: Mutex::default(),
            change: Mutex::default(),
            cancel: Mutex::default(),
            stop: Mutex::default(),
            stop_stage: AtomicBool::new(false),
            restore_failure: AtomicBool::new(false),
        }
    }
    pub fn set_restore_failure(&self, failed: bool) {
        self.restore_failure.store(failed, Ordering::SeqCst);
    }
    pub fn writes(&self) -> usize {
        self.desktop.writes.load(Ordering::SeqCst)
    }
    pub fn restores(&self) -> usize {
        self.desktop.restores.load(Ordering::SeqCst)
    }
    pub fn contents(&self) -> Contents {
        self.desktop.state.lock().unwrap().contents.clone()
    }
    pub fn change_on_check(&self, change: Change) {
        *self.change.lock().unwrap() = Some(change);
    }
    pub fn cancel_on_check(&self, cancel: Cancel) {
        *self.cancel.lock().unwrap() = Some(cancel);
    }
    pub fn stop_on_check(&self, api: Arc<gcu_driver_windows::input::hotkey::FakeSafetyApi>) {
        *self.stop.lock().unwrap() = Some(api);
    }
    pub fn stop_after_stage(&self, api: Arc<gcu_driver_windows::input::hotkey::FakeSafetyApi>) {
        self.stop_on_check(api);
        self.stop_stage.store(true, Ordering::SeqCst);
    }
    pub fn copy_external(&self) {
        let mut s = self.desktop.state.lock().unwrap();
        s.contents = external();
        s.sequence += 1;
    }
    pub async fn wait_staged(&self) {
        while self.writes() == 0 {
            let wait = self.desktop.staged.notified();
            if self.writes() > 0 {
                break;
            }
            wait.await;
        }
    }
    async fn publish(
        &self,
        text: String,
        context: ActionContext,
        proof: Option<PublishProof>,
    ) -> Result<u32> {
        if self.pending() {
            return Err(DriverError::CapabilityUnavailable);
        }
        let lock = loop {
            context.check()?;
            if let Ok(lock) = self.desktop.serial.clone().try_lock_owned() {
                break lock;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        };
        context.check()?;
        if let Some(proof) = proof {
            proof()?;
        }
        let mut state = self.desktop.state.lock().unwrap();
        let published = vec![(13, text.into_bytes())];
        let original = std::mem::replace(&mut state.contents, published.clone());
        state.sequence += 1;
        *self.saved.lock().unwrap() = Some(Saved {
            original,
            published,
            sequence: state.sequence,
            _lock: lock,
        });
        self.desktop.writes.fetch_add(1, Ordering::SeqCst);
        self.desktop.staged.notify_waiters();
        if self.stop_stage.swap(false, Ordering::SeqCst) {
            if let Some(api) = self.stop.lock().unwrap().take() {
                api.trigger();
            }
        }
        Ok(state.sequence)
    }
}
#[async_trait]
impl ClipboardApi for FakeClipboard {
    async fn stage(&self, text: String, context: ActionContext) -> Result<u32> {
        self.publish(text, context, None).await
    }
    async fn stage_proved(
        &self,
        text: String,
        context: ActionContext,
        proof: PublishProof,
    ) -> Result<u32> {
        self.publish(text, context, Some(proof)).await
    }
    async fn matches(&self, sequence: u32, text: String) -> Result<bool> {
        let mut state = self.desktop.state.lock().unwrap();
        if let Some(change) = self.change.lock().unwrap().take() {
            if matches!(change, Change::Sequence | Change::Both) {
                state.sequence += 1;
            }
            if matches!(change, Change::Content | Change::Both) {
                state.contents = external();
            }
        }
        if let Some(cancel) = self.cancel.lock().unwrap().take() {
            cancel.cancel();
        }
        if let Some(api) = self.stop.lock().unwrap().take() {
            api.trigger();
        }
        Ok(state.sequence == sequence && state.contents == vec![(13, text.into_bytes())])
    }
    async fn restore(&self) -> Result<()> {
        if self.restore_failure.load(Ordering::SeqCst) {
            return Err(DriverError::Native {
                message: "synthetic restore failure".into(),
                maybe_executed: false,
            });
        }
        let mut state = self.desktop.state.lock().unwrap();
        if let Some(saved) = self.saved.lock().unwrap().take() {
            if state.sequence == saved.sequence && state.contents == saved.published {
                state.contents = saved.original;
                state.sequence += 1;
                self.desktop.restores.fetch_add(1, Ordering::SeqCst);
            }
        }
        Ok(())
    }
    fn pending(&self) -> bool {
        self.saved.lock().unwrap().is_some()
    }
}
pub struct TestInput {
    inner: FakeInput,
    board: Arc<FakeClipboard>,
    risk: Mutex<Option<Risk>>,
    stop: AtomicBool,
    focus_change: AtomicBool,
    paused: AtomicBool,
    resumed: Notify,
    external_after: AtomicBool,
}
impl TestInput {
    pub fn new(board: Arc<FakeClipboard>) -> Self {
        let inner = FakeInput::default();
        inner.update(|s| s.foreground = 8);
        Self {
            inner,
            board,
            risk: Mutex::default(),
            stop: AtomicBool::new(false),
            focus_change: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            resumed: Notify::new(),
            external_after: AtomicBool::new(false),
        }
    }
    pub fn events(&self) -> Vec<Event> {
        self.inner
            .state()
            .sent
            .into_iter()
            .map(|s| s.event)
            .collect()
    }
    pub fn recorded(&self) -> bool {
        self.inner
            .state()
            .sent
            .iter()
            .filter(|s| matches!(s.event, Event::Key(_, true, ..)))
            .all(|s| s.recorded_before_down)
    }
    pub fn risk(&self, risk: Risk) {
        *self.risk.lock().unwrap() = Some(risk);
    }
    pub fn stop_before_paste(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    pub fn change_focus_on_check(&self) {
        self.focus_change.store(true, Ordering::SeqCst);
    }
    pub fn external_copy_after_paste(&self) {
        self.external_after.store(true, Ordering::SeqCst);
    }
    pub fn pause_before_paste(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.resumed.notify_waiters();
    }
    pub fn cancel_during_paste(&self, cancel: Cancel) {
        self.inner.update(|s| s.cancel_at = Some((0, cancel)));
    }
}
fn node(role: Role, name: &str) -> RawNode {
    RawNode {
        role,
        native_role: "Fake".into(),
        subrole: None,
        name: Some(name.into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: States::ENABLED | States::FOCUSED | States::EDITABLE,
        actions: Default::default(),
        frame: Some(Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        }),
        text_extras: None,
        url: None,
        children: Vec::new(),
        handle: NativeHandle::new(()),
        fingerprint: Fingerprint(1),
    }
}
#[async_trait]
impl InputApi for TestInput {
    async fn clipboard_safe(
        &self,
        _: Target,
        _: Option<NativeHandle>,
        text: String,
    ) -> Result<bool> {
        let risk = *self.risk.lock().unwrap();
        let mut target = node(Role::TextArea, "Document editor");
        let mut text = text;
        match risk {
            Some(Risk::Password) => target.states.insert(States::PASSWORD),
            Some(Risk::SensitiveLabel) => target.name = Some("Ｐａｙ".into()),
            Some(Risk::Unknown) => {
                target.role = Role::Unknown;
                target.name = None;
            }
            Some(Risk::Secret) => text.push_str(" api_key=synthetic"),
            _ => {}
        }
        if matches!(risk, Some(Risk::Web)) {
            let mut parent = node(Role::WebArea, "page");
            parent.states.remove(States::FOCUSED);
            parent.children.push(target);
            target = parent;
        }
        let (_, _, app, _) = super::common::fixture().await;
        Ok(!gcu_core::policy::clipboard::is_sensitive(
            &app.info,
            RawTree {
                root: Some(target),
                incomplete: None,
            },
            &text,
        ))
    }
    async fn before_paste(
        &self,
        _: Target,
        _: Option<NativeHandle>,
        context: ActionContext,
    ) -> Result<()> {
        while self.paused.load(Ordering::SeqCst) {
            let wait = self.resumed.notified();
            if !self.paused.load(Ordering::SeqCst) {
                break;
            }
            wait.await;
        }
        context.check()?;
        if self.stop.load(Ordering::SeqCst) {
            return Err(DriverError::Cancelled {
                maybe_executed: context.maybe_executed(),
            });
        }
        if self.focus_change.load(Ordering::SeqCst) {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(())
    }
    async fn resolve(
        &self,
        window: gcu_driver_windows::api::NativeWindow,
        point: Option<WindowPoint>,
        focus: Option<NativeHandle>,
    ) -> Result<Target> {
        self.inner.resolve(window, point, focus).await
    }
    async fn character_key(
        &self,
        target: Target,
        c: char,
    ) -> Result<(u16, Vec<gcu_protocol::keys::Modifier>)> {
        self.inner.character_key(target, c).await
    }
    async fn validate(&self, target: Target, foreground: bool) -> Result<()> {
        self.inner.validate(target, foreground).await
    }
    async fn activate(&self, target: Target, context: ActionContext) -> Result<()> {
        self.inner.activate(target, context).await
    }
    async fn foreground(&self) -> Result<u64> {
        self.inner.foreground().await
    }
    async fn focus(
        &self,
        target: Target,
        focus: NativeHandle,
        context: ActionContext,
    ) -> Result<()> {
        self.inner.focus(target, focus, context).await
    }
    async fn send(
        &self,
        target: Target,
        events: Vec<Event>,
        delivery: Delivery,
        ledger: Arc<InputLedger>,
        context: ActionContext,
        claim: Option<AdmittedTarget>,
    ) -> Result<()> {
        self.inner
            .send(target, events, delivery, ledger, context, claim)
            .await?;
        if self.external_after.swap(false, Ordering::SeqCst) {
            self.board.copy_external();
        }
        Ok(())
    }
    async fn sample(&self, target: Target, claim: Option<AdmittedTarget>) -> Result<u64> {
        self.inner.sample(target, claim).await
    }
    async fn release(&self, ledger: Arc<InputLedger>) {
        InputApi::release(&self.inner, ledger).await;
    }
}
pub fn request(win: &WindowRef) -> DriverAction {
    DriverAction::TypeText {
        win: win.clone(),
        focus: None,
        text: "synthetic document ".repeat(500),
        route: KeyRoute::AllowForeground,
    }
}
pub async fn fixture() -> (Arc<TestInput>, Arc<FakeClipboard>, WindowsDriver, WindowRef) {
    fixture_on(Arc::new(Desktop::default())).await
}
pub async fn fixture_on(
    desktop: Arc<Desktop>,
) -> (Arc<TestInput>, Arc<FakeClipboard>, WindowsDriver, WindowRef) {
    let (_, driver, _, win) = super::common::fixture().await;
    let board = Arc::new(FakeClipboard::new(desktop));
    let input = Arc::new(TestInput::new(board.clone()));
    let backend = Arc::new(InputBackend::new(input.clone()).with_clipboard(board.clone()));
    (
        input,
        board,
        driver
            .with_input(backend)
            .with_safety(super::common::fake_guard()),
        win,
    )
}
