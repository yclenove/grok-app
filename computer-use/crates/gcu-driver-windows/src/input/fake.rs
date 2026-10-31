//! Deterministic input backend usable on any OS; never accesses real windows.
use super::{api::*, ledger::InputLedger};
use crate::{api::NativeWindow, uia::patterns::ActionContext};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{NativeHandle, WindowPoint},
};
use std::sync::{Arc, Mutex};
/// One posted/synthetic input, including release calls.
#[derive(Clone, Debug)]
pub struct Sent {
    /// Recipient HWND.
    pub target: u64,
    /// Delivered event; Debug omits Unicode contents.
    pub event: Event,
    /// Actual route.
    pub delivery: Delivery,
    /// Ledger had recorded a release before this down was sent.
    pub recorded_before_down: bool,
    /// Tokio clock reading when the event was recorded.
    pub at: tokio::time::Instant,
}
/// Scriptable fake state. Failures/cancellation can be injected at exact events.
#[derive(Clone)]
pub struct FakeState {
    /// Test-only eligibility for exercising parent clipboard accounting.
    pub clipboard_safe: bool,
    /// Child HWND chosen by hit testing/focus.
    pub child: u64,
    /// Child process id. Zero uses the window's pid so existing tests stay put.
    pub child_pid: u32,
    /// Child process creation time. Zero uses the window's creation time.
    pub child_created: u64,
    /// After this many recipient reads, install the pid and creation time.
    ///
    /// `None` keeps `child_pid`. The count starts at one for the first recipient
    /// read. Used to replace the process after the first admission and before a
    /// later sample or send.
    pub replace_recipient_after: Option<(usize, u32, u64)>,
    recipient_reads: usize,
    /// Exact screen points and the HWND under them. Checked before [`Self::hit_receiver`].
    pub hit_at: Vec<(f64, f64, u64)>,
    /// Mouse-capture owner. When set, every pointer query returns it.
    pub capture_hwnd: Option<u64>,
    /// Window under the pointer when capture and [`Self::hit_at`] do not match.
    /// `None` means the receiver is unproved.
    pub hit_receiver: Option<u64>,
    /// Live HWND for publication recheck. Zero inherits the claim.
    pub live_hwnd: u64,
    /// Live window generation. Zero inherits the claim.
    pub live_incarnation: u64,
    /// Live owner pid. Zero inherits the claim.
    pub live_pid: u32,
    /// Live process creation time. Zero inherits the claim.
    pub live_created: u64,
    /// Live boot identity. Empty inherits the claim.
    pub live_boot: String,
    /// Native class name.
    pub class: String,
    /// All attempted input events, including cleanup ups.
    pub sent: Vec<Sent>,
    /// Activation sequence count.
    pub activations: usize,
    /// Actual foreground HWND.
    pub foreground: u64,
    /// Whether activation really succeeds.
    pub activate_succeeds: bool,
    /// Simulate a user/app taking foreground after the first packet.
    pub foreground_after_packet: Option<u64>,
    /// Gate error returned before delivery.
    pub validation_error: Option<DriverError>,
    /// Zero-based normal input event to fail once.
    pub fail_at: Option<usize>,
    /// Error at fail_at; its maybe_executed bit is respected.
    pub send_error: DriverError,
    /// Cancel token after recording this zero-based event.
    pub cancel_at: Option<(usize, Cancel)>,
    /// Pause after one input, without holding the ledger lock (provider stall).
    pub send_pause: Option<(usize, std::time::Duration)>,
    /// Recorded packet sizes, exposing real text chunk boundaries.
    pub packet_sizes: Vec<usize>,
    /// Number of effect samples.
    pub samples: usize,
    /// Whether dispatched inputs change sampled state.
    pub changes: bool,
    /// Failure on post-delivery effect sampling.
    pub sample_error: Option<DriverError>,
    /// Simulated stalled preflight read (no dispatch).
    pub resolve_delay: std::time::Duration,
    /// Simulated stalled post-delivery read.
    pub sample_delay: std::time::Duration,
    /// Recorded exact focus operations.
    pub focuses: usize,
    /// Fail a release once, proving it stays pending.
    pub release_error: bool,
}
impl Default for FakeState {
    fn default() -> Self {
        Self {
            clipboard_safe: false,
            child: 9,
            child_pid: 0,
            child_created: 0,
            replace_recipient_after: None,
            recipient_reads: 0,
            hit_at: Vec::new(),
            capture_hwnd: None,
            hit_receiver: None,
            live_hwnd: 0,
            live_incarnation: 0,
            live_pid: 0,
            live_created: 0,
            live_boot: String::new(),
            class: "Edit".into(),
            sent: Vec::new(),
            activations: 0,
            foreground: 0,
            activate_succeeds: true,
            validation_error: None,
            foreground_after_packet: None,
            fail_at: None,
            send_error: DriverError::Native {
                message: "fake delivery failure".into(),
                maybe_executed: false,
            },
            cancel_at: None,
            samples: 0,
            send_pause: None,
            packet_sizes: Vec::new(),
            changes: true,
            sample_error: None,
            resolve_delay: std::time::Duration::ZERO,
            sample_delay: std::time::Duration::ZERO,
            focuses: 0,
            release_error: false,
        }
    }
}
/// Shared fake with atomic call recording; no platform-specific code.
#[derive(Default)]
pub struct FakeInput {
    state: Mutex<FakeState>,
    resolving: tokio::sync::Notify,
}
impl FakeInput {
    /// Change a scenario before/during delivery.
    pub fn update(&self, update: impl FnOnce(&mut FakeState)) {
        update(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()));
    }
    /// Wait for the next resolve entry; tests can mutate provenance while its
    /// configured read delay is pending. The caller supplies its own deadline.
    pub async fn wait_until_resolving(&self) {
        self.resolving.notified().await;
    }
    /// Snapshot recorded calls, without retaining the lock.
    pub fn state(&self) -> FakeState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
#[async_trait]
impl InputApi for FakeInput {
    async fn before_paste(
        &self,
        target: Target,
        _: Option<NativeHandle>,
        context: ActionContext,
    ) -> Result<()> {
        context.check()?;
        if self.state().foreground != target.window.hwnd {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(())
    }

    async fn clipboard_safe(&self, _: Target, _: Option<NativeHandle>, _: String) -> Result<bool> {
        Ok(self.state().clipboard_safe)
    }
    async fn recipient_process(&self, target: Target) -> Result<(u32, u64)> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.recipient_reads = state.recipient_reads.saturating_add(1);
        if let Some((after, pid, created)) = state.replace_recipient_after {
            if state.recipient_reads > after {
                state.child_pid = pid;
                state.child_created = created;
            }
        }
        let child_pid = state.child_pid;
        let child_created = state.child_created;
        drop(state);
        Ok((
            if child_pid == 0 {
                target.window.pid
            } else {
                child_pid
            },
            if child_created == 0 {
                target.window.process_created
            } else {
                child_created
            },
        ))
    }
    async fn pointer_receiver(&self, point: WindowPoint) -> Result<Option<u64>> {
        let state = self.state();
        if let Some(hwnd) = state.capture_hwnd {
            return Ok(Some(hwnd));
        }
        if let Some((_, _, hwnd)) = state
            .hit_at
            .iter()
            .find(|(x, y, _)| *x == point.x && *y == point.y)
        {
            return Ok(Some(*hwnd));
        }
        Ok(state.hit_receiver)
    }
    fn recheck_publish(
        &self,
        hwnd: u64,
        incarnation: u64,
        pid: u32,
        created: u64,
        boot: &str,
    ) -> Result<()> {
        let state = self.state();
        let live_hwnd = if state.live_hwnd == 0 {
            hwnd
        } else {
            state.live_hwnd
        };
        let live_incarnation = if state.live_incarnation == 0 {
            incarnation
        } else {
            state.live_incarnation
        };
        let live_pid = if state.live_pid == 0 {
            pid
        } else {
            state.live_pid
        };
        let live_created = if state.live_created == 0 {
            created
        } else {
            state.live_created
        };
        let live_boot = if state.live_boot.is_empty() {
            boot
        } else {
            state.live_boot.as_str()
        };
        if live_hwnd == hwnd
            && live_incarnation == incarnation
            && live_pid == pid
            && live_created == created
            && live_boot == boot
        {
            Ok(())
        } else {
            Err(DriverError::ScopeDenied)
        }
    }
    async fn resolve(
        &self,
        window: NativeWindow,
        _: Option<WindowPoint>,
        _: Option<NativeHandle>,
    ) -> Result<Target> {
        let s = self.state();
        self.resolving.notify_one();
        tokio::time::sleep(s.resolve_delay).await;
        Ok(Target {
            window,
            hwnd: s.child,
            class: s.class,
            message_capable: true,
        })
    }
    async fn character_key(
        &self,
        _: Target,
        c: char,
    ) -> Result<(u16, Vec<gcu_protocol::keys::Modifier>)> {
        if !c.is_ascii_alphanumeric() {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok((
            c.to_ascii_uppercase() as u16,
            if c.is_ascii_uppercase() {
                vec![gcu_protocol::keys::Modifier::Shift]
            } else {
                Vec::new()
            },
        ))
    }
    async fn validate(&self, _: Target, _: bool) -> Result<()> {
        self.state().validation_error.map_or(Ok(()), Err)
    }
    async fn activate(&self, target: Target, context: ActionContext) -> Result<()> {
        context.begin()?;
        self.update(|s| {
            s.activations += 1;
            if s.activate_succeeds {
                s.foreground = target.window.hwnd;
            }
        });
        Ok(())
    }
    async fn foreground(&self) -> Result<u64> {
        Ok(self.state().foreground)
    }
    async fn focus(&self, _: Target, _: NativeHandle, context: ActionContext) -> Result<()> {
        context.begin()?;
        self.update(|s| s.focuses += 1);
        Ok(())
    }
    async fn send(
        &self,
        target: Target,
        events: Vec<Event>,
        delivery: Delivery,
        ledger: Arc<InputLedger>,
        context: ActionContext,
    ) -> Result<()> {
        self.update(|s| s.packet_sizes.push(events.len()));
        for event in events {
            let n = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .sent
                .len();
            // The callback runs inside the ledger lock; inspect through the
            // explicit down marker instead of recursively acquiring that lock.
            ledger.dispatch(&target, event, delivery, &context, |recorded| {
                let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
                let n = s.sent.len();
                s.sent.push(Sent {
                    target: target.hwnd,
                    event,
                    delivery,
                    recorded_before_down: recorded,
                    at: tokio::time::Instant::now(),
                });
                if let Some((at, c)) = &s.cancel_at {
                    if *at == n {
                        c.cancel();
                    }
                }
                if s.fail_at == Some(n) {
                    s.fail_at = None;
                    return Err(s.send_error.clone());
                }
                Ok(())
            })?;
            let pause = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .send_pause;
            if let Some((at, delay)) = pause {
                if at == n {
                    tokio::time::sleep(delay).await;
                }
            }
        }
        self.update(|s| {
            if let Some(hwnd) = s.foreground_after_packet {
                s.foreground = hwnd;
            }
        });
        Ok(())
    }
    async fn sample(&self, _: Target) -> Result<u64> {
        let s = self.state();
        if !s.sent.is_empty() {
            tokio::time::sleep(s.sample_delay).await;
            if let Some(e) = s.sample_error {
                return Err(e);
            }
        }
        self.update(|s| s.samples += 1);
        Ok(u64::from(s.changes && !s.sent.is_empty()))
    }
    async fn release(&self, ledger: Arc<InputLedger>) {
        ledger.release(|target, event, delivery| {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.sent.push(Sent {
                target: target.hwnd,
                event,
                delivery,
                recorded_before_down: false,
                at: tokio::time::Instant::now(),
            });
            if s.release_error {
                s.release_error = false;
                return Err(DriverError::Timeout {
                    maybe_executed: false,
                });
            }
            Ok(())
        });
    }
}
#[async_trait]
impl InputReleaser for FakeInput {
    async fn release(&self, ledger: Arc<InputLedger>) {
        InputApi::release(self, ledger).await;
    }
}
