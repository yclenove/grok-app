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
}
/// Scriptable fake state. Failures/cancellation can be injected at exact events.
#[derive(Clone)]
pub struct FakeState {
    /// Child HWND chosen by hit testing/focus.
    pub child: u64,
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
            child: 9,
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
}
impl FakeInput {
    /// Change a scenario before/during delivery.
    pub fn update(&self, update: impl FnOnce(&mut FakeState)) {
        update(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()));
    }
    /// Snapshot recorded calls, without retaining the lock.
    pub fn state(&self) -> FakeState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
#[async_trait]
impl InputApi for FakeInput {
    async fn resolve(
        &self,
        window: NativeWindow,
        _: Option<WindowPoint>,
        _: Option<NativeHandle>,
    ) -> Result<Target> {
        let s = self.state();
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
            let n = self.state().sent.len();
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
            if let Some((at, delay)) = self.state().send_pause {
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
