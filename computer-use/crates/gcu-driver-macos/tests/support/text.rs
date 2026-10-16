#![allow(dead_code)]
use async_trait::async_trait;
use gcu_core::driver::{Cancel, Delivery, DriverError, KeyRoute, Result};
use gcu_driver_macos::{
    ax::{AxHandle, AxIdentity},
    input::{
        activity::ActivityClock,
        clipboard::{Pasteboard, PasteboardContent, PasteboardSnapshot},
        clock::FakeClock,
        hotkey::{Guards, SharedStopLatch},
        text::{type_text, TextApi, TextContext, TextFocus, TextOptions},
        Call, EventKind, FakeEvents, InputLedger, InputTarget,
    },
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub const BUDGET: Duration = Duration::from_secs(2);
pub fn content(s: &str) -> PasteboardContent {
    PasteboardContent::text(s)
}

pub struct FakePasteboard {
    inner: Mutex<Board>,
}
struct Board {
    snapshot: PasteboardSnapshot,
    writes: usize,
    reads: usize,
    foreign_after_write: Option<String>,
    foreign_on_read: Option<(usize, String, bool)>,
}
impl FakePasteboard {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Board {
                snapshot: PasteboardSnapshot {
                    change_count: 10,
                    content: PasteboardContent(vec![
                        vec![
                            ("public.utf8-plain-text".into(), b"original".to_vec()),
                            ("public.rtf".into(), b"{rtf}".to_vec()),
                        ],
                        vec![("public.png".into(), vec![0, 1, 2])],
                    ]),
                },
                writes: 0,
                reads: 0,
                foreign_after_write: None,
                foreign_on_read: None,
            }),
        }
    }
    pub fn snapshot(&self) -> PasteboardSnapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }
    pub fn writes(&self) -> usize {
        self.inner.lock().unwrap().writes
    }
    pub fn replace_after_write(&self, s: &str) {
        self.inner.lock().unwrap().foreign_after_write = Some(s.into());
    }
    pub fn replace_on_read(&self, n: usize, s: &str, increment: bool) {
        self.inner.lock().unwrap().foreign_on_read = Some((n, s.into(), increment));
    }
    pub fn foreign(&self, text: &str) {
        let mut b = self.inner.lock().unwrap();
        b.snapshot.change_count += 1;
        b.snapshot.content = content(text);
    }
}
impl Pasteboard for FakePasteboard {
    fn read(&self) -> Result<PasteboardSnapshot> {
        let mut b = self.inner.lock().unwrap();
        b.reads += 1;
        if b.foreign_on_read
            .as_ref()
            .is_some_and(|(n, _, _)| *n == b.reads)
        {
            let (_, text, increment) = b.foreign_on_read.take().unwrap();
            if increment {
                b.snapshot.change_count += 1;
            }
            b.snapshot.content = content(&text);
        }
        Ok(b.snapshot.clone())
    }
    fn replace_if(
        &self,
        expected: &PasteboardSnapshot,
        data: &PasteboardContent,
    ) -> Result<Option<PasteboardSnapshot>> {
        let mut b = self.inner.lock().unwrap();
        if b.snapshot != *expected {
            return Ok(None);
        }
        b.writes += 1;
        b.snapshot.change_count += 1;
        b.snapshot.content = data.clone();
        let owned = b.snapshot.clone();
        if let Some(text) = b.foreign_after_write.take() {
            b.snapshot.change_count += 1;
            b.snapshot.content = content(&text);
        }
        Ok(Some(owned))
    }
}

pub struct FakeText {
    inner: Mutex<Field>,
    pub observed: tokio::sync::Notify,
    pub resume: tokio::sync::Notify,
}
struct Field {
    focus: TextFocus,
    value: String,
    reads: usize,
    change_on: Option<usize>,
    append_fault: Option<DriverError>,
    hold_observed: bool,
    foreign_after_paste: Option<(Arc<FakePasteboard>, String)>,
}
impl FakeText {
    fn new(plain: Option<&str>) -> Self {
        Self {
            inner: Mutex::new(Field {
                focus: TextFocus {
                    handle: AxHandle {
                        pid: 4,
                        id: 1,
                        identity: AxIdentity {
                            role: "AXTextArea".into(),
                            ..Default::default()
                        },
                    },
                    editable: true,
                    plain_value: plain.map(str::to_owned),
                    paste_baseline: Some(plain.unwrap_or_default().into()),
                    sensitive: false,
                    high_risk: false,
                    frontmost: true,
                },
                value: plain.unwrap_or_default().into(),
                reads: 0,
                change_on: None,
                append_fault: None,
                hold_observed: false,
                foreign_after_paste: None,
            }),
            observed: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        }
    }
    pub fn value(&self) -> String {
        self.inner.lock().unwrap().value.clone()
    }
    pub fn editable(&self, value: bool) {
        self.inner.lock().unwrap().focus.editable = value;
    }
    pub fn sensitive(&self, value: bool) {
        self.inner.lock().unwrap().focus.sensitive = value;
    }
    pub fn high_risk(&self, value: bool) {
        self.inner.lock().unwrap().focus.high_risk = value;
    }
    pub fn frontmost(&self, value: bool) {
        self.inner.lock().unwrap().focus.frontmost = value;
    }
    pub fn change_on_read(&self, n: usize) {
        self.inner.lock().unwrap().change_on = Some(n);
    }
    pub fn fail_append(&self, fault: DriverError) {
        self.inner.lock().unwrap().append_fault = Some(fault);
    }
    pub fn hold_observed(&self) {
        self.inner.lock().unwrap().hold_observed = true;
    }
    pub fn foreign_after_paste(&self, board: Arc<FakePasteboard>, text: &str) {
        self.inner.lock().unwrap().foreign_after_paste = Some((board, text.into()));
    }
}
#[async_trait]
impl TextApi for FakeText {
    async fn focus(&self, _: &InputTarget) -> Result<TextFocus> {
        let mut f = self.inner.lock().unwrap();
        f.reads += 1;
        if f.change_on == Some(f.reads) {
            f.focus.handle.id += 1;
        }
        Ok(f.focus.clone())
    }
    async fn append(&self, _: &InputTarget, expected: &TextFocus, text: &str) -> Result<()> {
        let mut f = self.inner.lock().unwrap();
        if f.focus.handle != expected.handle {
            return Err(DriverError::StaleElement);
        }
        if let Some(fault) = f.append_fault.take() {
            return Err(fault);
        }
        f.value.push_str(text);
        f.focus.plain_value = Some(f.value.clone());
        Ok(())
    }
    async fn paste_observed(
        &self,
        _: &InputTarget,
        _: &TextFocus,
        _: &str,
        cancel: &Cancel,
    ) -> Result<()> {
        let (hold, foreign) = {
            let mut f = self.inner.lock().unwrap();
            (f.hold_observed, f.foreign_after_paste.take())
        };
        if let Some((board, text)) = foreign {
            board.foreign(&text);
        }
        self.observed.notify_one();
        if hold {
            tokio::select! { _ = self.resume.notified() => {}, _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: true }) }
        }
        Ok(())
    }
}

struct IdleActivity;
impl ActivityClock for IdleActivity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        Some(3000)
    }
    fn stop_available(&self) -> bool {
        true
    }
}

pub struct Rig {
    pub clock: Arc<FakeClock>,
    pub text: Arc<FakeText>,
    pub events: Arc<FakeEvents>,
    pub board: Arc<FakePasteboard>,
    pub ledger: Arc<InputLedger>,
    pub target: InputTarget,
}
impl Rig {
    pub fn plain(s: &str) -> Self {
        Self::new(Some(s))
    }
    pub fn rich() -> Self {
        Self::new(None)
    }
    fn new(value: Option<&str>) -> Self {
        let clock = Arc::new(FakeClock::new());
        let events = Arc::new(FakeEvents::ready());
        events.set_guards(Arc::new(Guards::new(
            clock.clone(),
            Arc::new(IdleActivity),
            Arc::new(SharedStopLatch::new(false)),
            3000,
        )));
        events.set_frontmost(Some(4));
        Self {
            clock,
            text: Arc::new(FakeText::new(value)),
            events,
            board: Arc::new(FakePasteboard::new()),
            ledger: Arc::new(InputLedger::new()),
            target: InputTarget {
                pid: 4,
                window_id: 7,
                bundle_id: "com.example.Editor".into(),
                app_name: "Editor".into(),
                origin_x: 0.0,
                origin_y: 0.0,
                width_px: 100.0,
                height_px: 100.0,
                scale: 1.0,
            },
        }
    }
    pub fn context(&self) -> TextContext {
        TextContext {
            text: self.text.clone(),
            events: self.events.clone(),
            pasteboard: self.board.clone(),
            ledger: self.ledger.clone(),
            clock: self.clock.clone(),
        }
    }
    pub async fn run(&self, text: &str, options: TextOptions) -> Result<Delivery> {
        self.run_cancel(text, options, &Cancel::default()).await
    }
    pub async fn run_cancel(
        &self,
        text: &str,
        options: TextOptions,
        cancel: &Cancel,
    ) -> Result<Delivery> {
        type_text(
            &self.context(),
            &self.target,
            None,
            text,
            KeyRoute::AllowForeground,
            options,
            cancel,
            BUDGET,
        )
        .await
    }
    pub fn unicode(&self) -> String {
        self.events
            .calls()
            .iter()
            .filter_map(|call| match call {
                Call::Post(e) if e.kind == EventKind::Unicode => e.unicode.clone(),
                _ => None,
            })
            .collect()
    }
}
