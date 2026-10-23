#![allow(dead_code)]
//! Deterministic clock and probe driver for `wait_for`. Copied from the settle
//! harness because integration tests are separate crates.

use async_trait::async_trait;
use futures_core::Stream;
use gcu_core::{
    driver::*,
    model::*,
    session::{InputRelease, Session},
    wait::{WaitEngine, WaitReport, WaitRequest},
};
use gcu_driver_fake::FakeDriver;
use gcu_host_api::testing::FakeUserActivity;
use gcu_protocol::tools::Condition;
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{mpsc, Notify};

#[derive(Default)]
pub struct ManualClock {
    millis: AtomicU64,
    changed: Notify,
}

impl ManualClock {
    pub fn advance(&self, ms: u64) {
        self.millis.fetch_add(ms, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    pub fn now_ms(&self) -> u64 {
        self.millis.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl gcu_core::settle::Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.now_ms())
    }

    async fn wait_until(&self, deadline: Duration) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.now() >= deadline {
                return;
            }
            changed.await;
        }
    }
}

#[derive(Clone, Copy)]
pub enum Subscription {
    None,
    Quiet,
    Closed,
    Flood,
}

struct TestStream {
    mode: Subscription,
    prelude: Option<UiEvent>,
    active: Arc<AtomicUsize>,
    polls: Arc<AtomicUsize>,
    rx: mpsc::UnboundedReceiver<UiEvent>,
}

impl Stream for TestStream {
    type Item = UiEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<UiEvent>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if let Some(event) = self.prelude.take() {
            return Poll::Ready(Some(event));
        }
        match self.mode {
            Subscription::Flood => Poll::Ready(Some(UiEvent::Changed)),
            Subscription::Closed => Poll::Ready(None),
            _ => self.rx.poll_recv(cx),
        }
    }
}

impl Drop for TestStream {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ReadDrop {
    cancel: Cancel,
    observed: Arc<AtomicBool>,
}

impl Drop for ReadDrop {
    fn drop(&mut self) {
        self.observed
            .store(self.cancel.is_cancelled(), Ordering::SeqCst);
    }
}

pub struct ProbeDriver {
    inner: FakeDriver,
    clock: Arc<ManualClock>,
    raw: Mutex<RawTree>,
    stalled: AtomicBool,
    hold: AtomicBool,
    release: Notify,
    times: Mutex<Vec<u64>>,
    timeouts: Mutex<Vec<Duration>>,
    cancelled: Arc<AtomicBool>,
    mode: Subscription,
    active: Arc<AtomicUsize>,
    polls: Arc<AtomicUsize>,
    releases: AtomicUsize,
    senders: Mutex<Vec<mpsc::UnboundedSender<UiEvent>>>,
    prime: Mutex<Option<UiEvent>>,
}

impl ProbeDriver {
    pub fn replace(&self, tree: RawTree) {
        *self.raw.lock().unwrap() = tree;
    }

    pub fn stall(&self) {
        self.stalled.store(true, Ordering::SeqCst);
    }

    /// The next snapshot returns the tree captured at entry, after `release_hold`.
    pub fn arm_hold(&self) {
        self.hold.store(true, Ordering::SeqCst);
    }

    pub fn release_hold(&self) {
        self.release.notify_waiters();
    }

    /// First quiet poll returns `event` before any later channel event.
    pub fn prime(&self, event: UiEvent) {
        *self.prime.lock().unwrap() = Some(event);
    }

    pub fn read_times(&self) -> Vec<u64> {
        self.times.lock().unwrap().clone()
    }

    pub fn timeouts(&self) -> Vec<Duration> {
        self.timeouts.lock().unwrap().clone()
    }

    pub fn cancelled_before_drop(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn subscriptions(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn event_polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }

    pub fn release_calls(&self) -> usize {
        self.releases.load(Ordering::SeqCst)
    }

    pub fn perform_calls(&self) -> usize {
        self.inner.perform_calls()
    }

    pub async fn oracle(&self) -> gcu_driver_fake::Oracle {
        self.inner.oracle().await.unwrap()
    }

    pub fn emit(&self, event: UiEvent) {
        self.senders
            .lock()
            .unwrap()
            .retain(|sender| sender.send(event.clone()).is_ok());
    }
}

#[async_trait]
impl Driver for ProbeDriver {
    fn platform(&self) -> Platform {
        self.inner.platform()
    }

    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }

    async fn doctor(&self) -> DoctorReport {
        self.inner.doctor().await
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        self.inner.list_apps().await
    }

    async fn resolve_app(&self, query: &AppQuery) -> Result<AppRef> {
        self.inner.resolve_app(query).await
    }

    async fn launch_app(&self, app: &AppRef) -> Result<AppRef> {
        self.inner.launch_app(app).await
    }

    async fn list_windows(&self, app: Option<&AppRef>) -> Result<Vec<WindowInfo>> {
        self.inner.list_windows(app).await
    }

    async fn key_window(&self, app: &AppRef) -> Result<WindowInfo> {
        self.inner.key_window(app).await
    }

    async fn snapshot_tree(
        &self,
        _: &WindowRef,
        opts: &TreeOpts,
        cancel: &Cancel,
    ) -> Result<RawTree> {
        self.times.lock().unwrap().push(self.clock.now_ms());
        self.timeouts.lock().unwrap().push(opts.timeout);
        if self.hold.swap(false, Ordering::SeqCst) {
            let captured = self.raw.lock().unwrap().clone();
            let _drop = ReadDrop {
                cancel: cancel.clone(),
                observed: self.cancelled.clone(),
            };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(DriverError::Cancelled { maybe_executed: false });
                }
                _ = self.release.notified() => return Ok(captured),
            }
        }
        if self.stalled.load(Ordering::SeqCst) {
            let _drop = ReadDrop {
                cancel: cancel.clone(),
                observed: self.cancelled.clone(),
            };
            std::future::pending::<()>().await;
        }
        Ok(self.raw.lock().unwrap().clone())
    }

    async fn capture(
        &self,
        win: &WindowRef,
        opts: &CaptureOpts,
        cancel: &Cancel,
    ) -> Result<Capture> {
        self.inner.capture(win, opts, cancel).await
    }

    async fn window_geometry(&self, win: &WindowRef) -> Result<Geometry> {
        self.inner.window_geometry(win).await
    }

    async fn focused_element(&self, app: &AppRef) -> Result<Option<NativeHandle>> {
        self.inner.focused_element(app).await
    }

    async fn perform(&self, req: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        self.inner.perform(req, cancel).await
    }

    fn subscribe(&self, _: &WindowRef) -> Option<BoxStream<'static, UiEvent>> {
        if matches!(self.mode, Subscription::None) {
            return None;
        }
        self.active.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        self.senders.lock().unwrap().push(tx);
        let prelude = if matches!(self.mode, Subscription::Quiet) {
            self.prime.lock().unwrap().take()
        } else {
            None
        };
        Some(Box::pin(TestStream {
            mode: self.mode,
            prelude,
            active: self.active.clone(),
            polls: self.polls.clone(),
            rx,
        }))
    }

    async fn release_all_input(&self) {
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl InputRelease for ProbeDriver {
    async fn release_all_input(&self) {
        Driver::release_all_input(self).await;
    }
}

pub struct Rig {
    pub driver: Arc<ProbeDriver>,
    pub clock: Arc<ManualClock>,
    pub activity: Arc<FakeUserActivity>,
    pub session: Arc<Session>,
    pub window: WindowRef,
    pub before: RawTree,
}

impl Rig {
    pub async fn new(mode: Subscription) -> Self {
        let inner = FakeDriver::from_scene("form").unwrap();
        let window = inner.fixture_window("fake.form").await.unwrap();
        let before = inner
            .snapshot_tree(&window, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let clock = Arc::new(ManualClock::default());
        let driver = Arc::new(ProbeDriver {
            inner,
            clock: clock.clone(),
            raw: Mutex::new(before.clone()),
            stalled: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            release: Notify::new(),
            times: Mutex::new(Vec::new()),
            timeouts: Mutex::new(Vec::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
            mode,
            active: Arc::new(AtomicUsize::new(0)),
            polls: Arc::new(AtomicUsize::new(0)),
            releases: AtomicUsize::new(0),
            senders: Mutex::new(Vec::new()),
            prime: Mutex::new(None),
        });
        let activity = Arc::new(FakeUserActivity::new());
        let session = Arc::new(Session::open("wait", activity.clone(), driver.clone()));
        session.begin_turn().unwrap();
        Self {
            driver,
            clock,
            activity,
            session,
            window,
            before,
        }
    }

    pub fn start(
        &self,
        until: Condition,
        timeout_ms: u64,
    ) -> tokio::task::JoinHandle<std::result::Result<WaitReport, gcu_protocol::ErrorCode>> {
        let engine = WaitEngine::new(self.clock.clone());
        let driver = self.driver.clone();
        let session = self.session.clone();
        let window = self.window.clone();
        tokio::spawn(async move {
            engine
                .wait(WaitRequest {
                    driver: driver.as_ref(),
                    session: &session,
                    window: &window,
                    tree_opts: &TreeOpts::default(),
                    events: driver.subscribe(&window),
                    until,
                    timeout: Duration::from_millis(timeout_ms),
                    scope_epoch: None,
                })
                .await
        })
    }

    pub async fn advance(&self, ms: u64) {
        self.clock.advance(ms);
        pump().await;
    }
}

pub async fn pump() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

pub fn changed(before: &RawTree) -> RawTree {
    let mut raw = before.clone();
    raw.root.as_mut().unwrap().name = Some("fixture changed".into());
    raw
}

pub fn within_driver_deadline(timeouts: &[Duration]) -> bool {
    timeouts.iter().all(|timeout| *timeout <= DRIVER_DEADLINE)
}
