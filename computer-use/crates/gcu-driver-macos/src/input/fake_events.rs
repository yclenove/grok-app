//! Portable event backend for input and Stop tests.

use super::events::*;
use async_trait::async_trait;
use gcu_core::driver::{DriverError, Result};
use std::sync::{Mutex, MutexGuard};

/// In-memory backend. Records every call and never touches CoreGraphics.
#[derive(Debug)]
pub struct FakeEvents {
    inner: Mutex<FakeInner>,
    gate: GateSlot,
}

struct GateSlot(Mutex<Option<std::sync::Arc<dyn super::events::PostGate>>>);

impl std::fmt::Debug for GateSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GateSlot")
    }
}

#[derive(Debug)]
struct FakeInner {
    steal_after_post: Option<i32>,
    guards: std::sync::Arc<super::hotkey::Guards>,
    accept_without_activation: bool,
    probe: Probe,
    calls: Vec<Call>,
    frontmost: Option<i32>,
    /// Non-up posts wait until the future is dropped (cancel/timeout).
    hold: bool,
    hold_ups: bool,
    fault: Option<PostFault>,
    /// Posts of ups ignore `hold` and `fault` so cleanup can finish.
    fail_arrival: bool,
    closed: Vec<(i32, u32)>,
    activate_fails: bool,
}

impl FakeEvents {
    /// Backend whose probe is `probe`. Frontmost starts at pid 1.
    pub fn new(probe: Probe) -> Self {
        Self {
            gate: GateSlot(Mutex::new(None)),
            inner: Mutex::new(FakeInner {
                steal_after_post: None,
                guards: super::hotkey::Guards::fixture(),
                accept_without_activation: false,
                probe,
                calls: Vec::new(),
                frontmost: Some(1),
                hold: false,
                hold_ups: false,
                fault: None,
                fail_arrival: false,
                closed: Vec::new(),
                activate_fails: false,
            }),
        }
    }

    /// macOS 27 probe with symbols and AppKit.
    pub fn ready() -> Self {
        Self::new(Probe::verified_os())
    }

    fn lock(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Calls so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    /// Pid the fake considers frontmost.
    pub fn frontmost(&self) -> Option<i32> {
        self.lock().frontmost
    }

    /// Install deterministic safety sources for Stop and activity tests.
    pub fn set_guards(&self, guards: std::sync::Arc<super::hotkey::Guards>) {
        self.lock().guards = guards;
    }

    /// The OS accepts activation but leaves the old application in front.
    pub fn accept_without_activation(&self) {
        self.lock().accept_without_activation = true;
    }

    /// Simulate another app stealing foreground focus between posts.
    pub fn set_frontmost(&self, pid: Option<i32>) {
        self.lock().frontmost = pid;
    }

    /// Another app steals focus after the first down has been posted.
    pub fn steal_focus_after_post(&self, pid: i32) {
        self.lock().steal_after_post = Some(pid);
    }

    /// Next non-up post waits until the caller drops or times out the future.
    pub fn hold_posts(&self) {
        self.lock().hold = true;
    }

    /// Every up waits forever; tests verify other releases still get dispatched.
    pub fn hold_ups(&self) {
        self.lock().hold_ups = true;
    }

    /// Next non-up post returns this fault immediately.
    pub fn fail_next(&self, fault: PostFault) {
        self.lock().fault = Some(fault);
    }

    /// Arrival checks fail once each. The router must not post again.
    pub fn fail_arrival(&self) {
        self.lock().fail_arrival = true;
    }

    /// `window_open` returns false for this pair.
    pub fn close_window(&self, pid: i32, window_id: u32) {
        self.lock().closed.push((pid, window_id));
    }

    /// Activation returns [`DriverError::CapabilityUnavailable`] and does not
    /// change the frontmost pid.
    pub fn fail_activate(&self) {
        self.lock().activate_fails = true;
    }

    /// Replace the probe. Used for TCC revoke and OS-version cases.
    pub fn set_probe(&self, probe: Probe) {
        self.lock().probe = probe;
    }

    async fn post_inner(
        &self,
        event: SyntheticEvent,
        check_gate: bool,
    ) -> std::result::Result<(), PostFault> {
        if check_gate {
            let gate = self
                .gate
                .0
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone();
            if let Some(gate) = gate {
                gate.before_post(&event)?;
            }
        }
        let (hold, fault) = {
            let mut inner = self.lock();
            inner.calls.push(Call::Post(event.clone()));
            if event.down {
                if let Some(pid) = inner.steal_after_post.take() {
                    inner.frontmost = Some(pid);
                }
            }
            if !event.down && !inner.hold_ups {
                return Ok(());
            }
            let fault = if event.down { inner.fault.take() } else { None };
            (
                if event.down {
                    inner.hold
                } else {
                    inner.hold_ups
                },
                fault,
            )
        };
        if let Some(fault) = fault {
            return Err(fault);
        }
        if hold {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

#[async_trait]
impl EventApi for FakeEvents {
    fn guards(&self) -> Option<std::sync::Arc<super::hotkey::Guards>> {
        Some(self.lock().guards.clone())
    }
    async fn frontmost_pid(&self) -> Result<Option<i32>> {
        Ok(self.lock().frontmost)
    }
    fn probe(&self) -> Probe {
        self.lock().probe.clone()
    }

    fn note_arrival_failed(&self) {
        let mut inner = self.lock();
        inner.probe.field_51 = false;
        inner.probe.arrival_verified = false;
    }

    fn window_open(&self, pid: i32, window_id: u32) -> bool {
        !self.lock().closed.contains(&(pid, window_id))
    }

    async fn activate(&self, pid: i32) -> Result<()> {
        let mut inner = self.lock();
        inner.calls.push(Call::Activate(pid));
        if inner.activate_fails {
            return Err(DriverError::CapabilityUnavailable);
        }
        if !inner.accept_without_activation {
            inner.frontmost = Some(pid);
        }
        Ok(())
    }

    fn arm_post_gate(&self, gate: Option<std::sync::Arc<dyn super::events::PostGate>>) {
        *self.gate.0.lock().unwrap_or_else(|err| err.into_inner()) = gate;
    }

    async fn post(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault> {
        self.post_inner(event, true).await
    }

    async fn post_release(&self, event: SyntheticEvent) -> std::result::Result<(), PostFault> {
        self.post_inner(event, false).await
    }

    async fn verify_arrival(&self, pid: i32, window_id: u32) -> std::result::Result<(), PostFault> {
        let fail = {
            let mut inner = self.lock();
            inner.calls.push(Call::Verify { pid, window_id });
            inner.fail_arrival
        };
        if fail {
            Err(PostFault::Uncertain)
        } else {
            Ok(())
        }
    }
}
