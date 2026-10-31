//! Windows targeted messages and explicitly permitted foreground input.
#[cfg(any(windows, test))]
mod admission;
pub mod api;
pub mod clipboard;
pub mod fake;
#[cfg(windows)]
mod ffi;
pub mod foreground;
#[cfg(any(windows, test))]
#[cfg_attr(test, allow(dead_code))]
mod hook_probe;
pub mod hooks;
pub mod hotkey;
#[cfg(test)]
mod injection;
pub mod ledger;
pub mod messages;
mod plan;
#[cfg(any(windows, test))]
mod pointer_settle;
pub(crate) mod sample_job;
pub mod text;
pub mod watchdog;
use crate::{api::NativeWindow, uia::patterns::ActionContext};
use api::{InputApi, Target};
#[cfg(windows)]
pub use ffi::{NativeRelease, RealInput, RealSafetyApi};
use gcu_core::driver::*;
use std::sync::{Arc, Mutex};

/// Same-thread guard for the production named pointer mutex on Windows.
#[cfg(all(windows, feature = "test-support", not(test)))]
#[doc(hidden)]
pub use hook_probe::PointerGateGuard;

/// Acquire the production pointer gate. Windows integration tests use this to prove the
/// named mutex. It does not send input. Unit tests must not treat their stand-in as this object.
/// A `CreateMutexW` failure names the Win32 code; contention does not.
#[cfg(all(windows, feature = "test-support", not(test)))]
#[doc(hidden)]
pub fn pointer_gate_enter_for_test(
    wait: std::time::Duration,
) -> std::result::Result<PointerGateGuard, String> {
    hook_probe::enter_pointer_gate_for_test(wait)
}

/// Bind the pointer-gate mutex name before the first `CreateMutexW`.
///
/// The production constant stays `Local\gcu-pointer-gate-v1`. The name-conflict
/// integration test injects a private name so it does not collide with another process.
/// Fails when the name is already bound or an override was already set.
#[cfg(all(windows, feature = "test-support", not(test)))]
#[doc(hidden)]
pub fn set_pointer_gate_name_for_test(name: &'static str) -> std::result::Result<(), &'static str> {
    hook_probe::set_pointer_gate_name_for_test(name)
}

/// The production mutex name. Reading it does not create the kernel object.
#[cfg(all(windows, feature = "test-support", not(test)))]
#[doc(hidden)]
pub fn pointer_gate_production_name_for_test() -> &'static str {
    hook_probe::pointer_gate_production_name_for_test()
}

/// Post-delivery evidence. No visible change never authorizes replay or escalation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// State or pixels changed after delivery.
    Changed,
    /// Delivery completed, but sampled evidence stayed unchanged.
    NoVisibleChange,
}
/// Injectable input coordinator. Each call has a single shared DRIVER_DEADLINE;
/// abandoned futures close dispatch and release held inputs independently.
pub struct InputBackend {
    api: Arc<dyn InputApi>,
    ledger: Arc<ledger::InputLedger>,
    active: Mutex<Vec<ActionContext>>,
    serial: Arc<tokio::sync::Mutex<()>>,
    effect: Mutex<Option<Effect>>,
    tracker: Option<Arc<dyn api::InputTracker>>,
    clipboard: Option<Arc<dyn clipboard::ClipboardApi>>,
    safety: Mutex<std::sync::Weak<hotkey::SafetyGuard>>,
}
impl InputBackend {
    /// Attach a complete native or fake input implementation, without OS work.
    pub fn new(api: Arc<dyn InputApi>) -> Self {
        Self {
            api,
            ledger: Arc::default(),
            active: Mutex::default(),
            serial: Arc::default(),
            effect: Mutex::default(),
            tracker: None,
            clipboard: None,
            safety: Mutex::default(),
        }
    }
    /// Attach the isolated worker's parent-ledger acknowledgement barrier.
    pub fn with_tracker(mut self, tracker: Arc<dyn api::InputTracker>) -> Self {
        self.tracker = Some(tracker);
        self
    }
    /// Enable bounded clipboard fallback for safe long foreground text.
    pub fn with_clipboard(mut self, clipboard: Arc<dyn clipboard::ClipboardApi>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }
    pub(crate) fn parent_controlled(&self) -> bool {
        self.tracker.is_some()
    }
    pub(crate) fn set_safety(&self, guard: Arc<hotkey::SafetyGuard>) {
        // Guard callbacks own the backend for independent cleanup; retain only
        // a weak backlink so dropping the driver also drops native monitoring.
        *self.safety.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&guard);
    }
    /// Last sampled effect for diagnostics/tests. Core determines the public diff.
    pub fn last_effect(&self) -> Option<Effect> {
        *self.effect.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Whether all synthetic inputs have acknowledged up events.
    pub fn ledger_empty(&self) -> bool {
        self.ledger.is_empty()
    }
    /// Stop current dispatches, then release inputs without waiting for a provider
    /// read or the serial action lock. Does not unlock core's persistent Stop.
    pub async fn release_all(&self) {
        for c in self.active.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            c.close();
        }
        release_bounded(self.api.clone(), self.ledger.clone()).await;
        if let Some(board) = &self.clipboard {
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_millis(100), board.restore()).await,
                Ok(Ok(()))
            ) {
                tracing::warn!("Stop clipboard recovery pending");
            }
        }
    }
    /// Deliver once to an exact stamped window. Auto/TargetedOnly are background
    /// only. AllowForeground/Activate are core's explicit policy authorization.
    pub async fn perform(
        &self,
        window: NativeWindow,
        req: &DriverAction,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
    ) -> Result<Delivery> {
        self.perform_scoped(
            window,
            req,
            cancel,
            deadline,
            &crate::scope_identity::JobScope::unscoped(),
        )
        .await
    }
    /// Deliver under one scope copied by the caller.
    ///
    /// [`crate::scope_identity::JobScope::Unscoped`] keeps the historical route,
    /// including foreground SendInput and the multi-read sample. A scoped job
    /// refuses foreground SendInput before resolve, sample, focus, paste, or
    /// send. Targeted delivery carries the admitted process into each send and
    /// into each sample, including every settle sample. A scoped sample reads
    /// window text once, proves that process immediately before the read, and
    /// proves it again before the hash leaves. A missing scope is refused and
    /// is not treated as unscoped. Nothing here runs inside the pointer gate.
    pub(crate) async fn perform_scoped(
        &self,
        window: NativeWindow,
        req: &DriverAction,
        cancel: &Cancel,
        deadline: tokio::time::Instant,
        scope: &crate::scope_identity::JobScope,
    ) -> Result<Delivery> {
        // Reserve cleanup time inside the shared deadline, including queueing.
        let work_deadline = deadline - std::time::Duration::from_millis(110);
        let serial = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(DriverError::Cancelled { maybe_executed: false }),
            lock = tokio::time::timeout_at(work_deadline, self.serial.clone().lock_owned()) => lock.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
        };
        if !self.ledger.is_empty() {
            release_bounded(self.api.clone(), self.ledger.clone()).await;
            if !self.ledger.is_empty() {
                return Err(DriverError::CapabilityUnavailable);
            }
        }
        *self.effect.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let context = ActionContext::until(cancel, work_deadline.into_std());
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(context.clone());
        let mut guard = Cleanup {
            backend: self,
            context: context.clone(),
            complete: false,
            serial: Some(serial),
        };
        let operation = async {
            context.check()?;

            let plan = plan::prepare(&window, req)?;
            let safety = self
                .safety
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .upgrade();
            if let Some(guard) = &safety {
                guard.check(false)?;
            }
            // Second guard. `WindowsDriver::perform` already refused a scoped
            // foreground request before any proof or native call. Close it again
            // before resolve. Capabilities report the same gap.
            if scope.claim()?.is_some() && plan.foreground {
                return Err(DriverError::CapabilityUnavailable);
            }
            let target = if let Some(claim) = scope.claim()? {
                self.api
                    .resolve_scoped(
                        window,
                        plan.point,
                        plan.focus.clone(),
                        claim.owner.pid,
                        claim.hwnd,
                    )
                    .await?
            } else {
                self.api
                    .resolve(window, plan.point, plan.focus.clone())
                    .await?
            };
            context.check()?;
            if let Some(claim) = scope.claim()? {
                let stamp = crate::desktop::HwndStamp::from(&target.window).id();
                crate::scope_identity::admit_delivery_element(
                    claim.hwnd,
                    target.window.hwnd,
                    &stamp,
                    Some(target.window.pid),
                    Some(claim.owner.pid),
                )?;
            }
            // One copy for every sample and send in this action, including settle.
            // Later pin changes are not read. The native job compares it.
            let carried = carried_target(scope)?;
            let foreground = plan.foreground;
            if !foreground
                && (!target.message_capable || messages::ineffective_class(&target.class))
            {
                return Err(DriverError::CapabilityUnavailable);
            }
            self.api.validate(target.clone(), foreground).await?;
            // Outside the pointer gate. Unproved scalars never reach SendInput.
            crate::scope_identity::require_delivery_scalars(
                target.window.hwnd,
                target.window.pid,
                target.window.process_created,
                target.window.created,
                target.hwnd,
            )?;
            if let Some(guard) = &safety {
                guard.check(foreground)?;
                if foreground {
                    guard.check_injection()?;
                }
            }
            let mut packets = plan.packets;
            let dwell_after = plan.dwell_after;
            if let Some(chord) = plan.chord {
                let character = match chord.key {
                    gcu_protocol::keys::Key::Character(c) => {
                        Some(self.api.character_key(target.clone(), c).await?)
                    }
                    _ => None,
                };
                packets.push(messages::chord_events(&chord, character)?);
            }
            let before = if packets.is_empty() {
                None
            } else {
                Some(self.api.sample(target.clone(), carried.clone()).await?)
            };
            if foreground {
                if let Some(guard) = &safety {
                    guard.check_injection()?;
                }
                foreground::activate(self.api.as_ref(), &target, &context).await?;
                if let Some(focus) = plan.focus {
                    self.api
                        .focus(target.clone(), focus, context.clone())
                        .await?;
                }
            }
            // Scoped foreground already returned above. This paste is unscoped.
            let paste = if foreground {
                if let (Some(board), DriverAction::TypeText { text, focus, .. }) =
                    (&self.clipboard, req)
                {
                    if text.encode_utf16().count() > messages::TEXT_MAX_UNITS
                        && self
                            .api
                            .clipboard_safe(target.clone(), focus.clone(), text.clone())
                            .await?
                    {
                        Some((board.clone(), text.clone(), focus.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let delivery = if paste.is_some() {
                Delivery::Clipboard
            } else if foreground {
                Delivery::Foreground
            } else {
                Delivery::TargetedEvent
            };
            if let Some((clipboard, text, focus)) = paste {
                text::Paste {
                    api: self.api.as_ref(),
                    clipboard,
                    tracker: self.tracker.clone(),
                    ledger: self.ledger.clone(),
                    target: target.clone(),
                    focus,
                    text,
                    context: context.clone(),
                    safety: safety.clone(),
                }
                .run()
                .await?;
                packets.clear();
            }
            for (index, packet) in packets.into_iter().enumerate() {
                context.check()?;
                self.api.validate(target.clone(), foreground).await?;
                if foreground {
                    foreground::verify(self.api.as_ref(), &target).await?;
                }
                context.check()?;
                if foreground {
                    if let Some(guard) = &safety {
                        guard.check_injection()?;
                    }
                }
                if let Some(tracker) = &self.tracker {
                    tracker
                        .prepare(target.clone(), packet.clone(), delivery)
                        .await?;
                }
                let result = async {
                    context.check()?;
                    crate::scope_identity::require_delivery_scalars(
                        target.window.hwnd,
                        target.window.pid,
                        target.window.process_created,
                        target.window.created,
                        target.hwnd,
                    )?;
                    self.api
                        .send(
                            target.clone(),
                            packet,
                            delivery,
                            self.ledger.clone(),
                            context.clone(),
                            carried.clone(),
                        )
                        .await
                }
                .await;
                if let Some(tracker) = &self.tracker {
                    if result.is_ok() || result.as_ref().is_err_and(|e| !e.maybe_executed()) {
                        tracker.complete(result.is_ok()).await?;
                    }
                }
                result?;
                tokio::task::yield_now().await;
                if dwell_after == Some(index) {
                    tokio::time::sleep(plan::APPROACH_DWELL).await;
                }
            }
            if let Some(before) = before {
                let effect = self
                    .settle(&target, before, plan.text, &context, work_deadline, carried)
                    .await?;
                *self.effect.lock().unwrap_or_else(|p| p.into_inner()) = Some(effect);
            }
            context.check()?;
            Ok(delivery)
        };
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: context.close() }),
            result = tokio::time::timeout_at(work_deadline, operation) => result.unwrap_or_else(|_| Err(DriverError::Timeout { maybe_executed: context.close() })),
        }.map_err(|e| context.error(e));
        context.close();
        release_bounded(self.api.clone(), self.ledger.clone()).await;
        // The serial guard owns recovery until this finishes. Never launch a
        // detached per-paste restore that could overwrite the next transaction.
        if let Some(board) = &self.clipboard {
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_millis(100), board.restore()).await,
                Ok(Ok(()))
            ) {
                tracing::warn!("clipboard recovery pending");
            }
        }
        guard.complete = true;
        result
    }
    async fn settle(
        &self,
        target: &Target,
        before: u64,
        text: bool,
        context: &ActionContext,
        deadline: tokio::time::Instant,
        carried: Option<api::AdmittedTarget>,
    ) -> Result<Effect> {
        let until = (tokio::time::Instant::now()
            + std::time::Duration::from_millis(if text { 1500 } else { 800 }))
        .min(deadline);
        let mut last = None;
        let mut changed = false;
        loop {
            context.check()?;
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            context.check()?;
            let state = self.api.sample(target.clone(), carried.clone()).await?;
            changed |= state != before;
            if last == Some(state) || tokio::time::Instant::now() >= until {
                return Ok(if changed {
                    Effect::Changed
                } else {
                    Effect::NoVisibleChange
                });
            }
            last = Some(state);
        }
    }
}
struct Cleanup<'a> {
    backend: &'a InputBackend,
    context: ActionContext,
    complete: bool,
    serial: Option<tokio::sync::OwnedMutexGuard<()>>,
}
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        self.context.close();
        self.backend
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|c| !c.closed());
        if !self.complete {
            let api = self.backend.api.clone();
            let ledger = self.backend.ledger.clone();
            let clipboard = self.backend.clipboard.clone();
            let serial = self.serial.take();
            tokio::spawn(async move {
                // Hold serialization until cleanup, so an abandoned call cannot
                // release the next action's newly pressed keys.
                let _serial = serial;
                release_bounded(api, ledger).await;
                if let Some(board) = clipboard {
                    if !matches!(
                        tokio::time::timeout(
                            std::time::Duration::from_millis(100),
                            board.restore()
                        )
                        .await,
                        Ok(Ok(()))
                    ) {
                        tracing::warn!("abandoned clipboard recovery pending");
                    }
                }
            });
        }
    }
}

/// Admitted window copied once for every sample and send in this action.
///
/// `None` is unscoped. The value is not read from the pin again.
fn carried_target(scope: &crate::scope_identity::JobScope) -> Result<Option<api::AdmittedTarget>> {
    let Some(claim) = scope.claim()? else {
        return Ok(None);
    };
    Ok(Some(api::AdmittedTarget {
        hwnd: claim.hwnd,
        incarnation: claim.incarnation,
        pid: claim.owner.pid,
        created: claim.owner.created,
        boot: claim.owner.boot.clone(),
    }))
}

async fn release_bounded(api: Arc<dyn InputApi>, ledger: Arc<ledger::InputLedger>) {
    if tokio::time::timeout(std::time::Duration::from_millis(100), api.release(ledger))
        .await
        .is_err()
    {
        tracing::warn!("Windows input cleanup deadline exceeded");
    }
}
