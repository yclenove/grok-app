//! Windows targeted messages and explicitly permitted foreground input.
pub mod api;
pub mod clipboard;
pub mod fake;
#[cfg(windows)]
mod ffi;
pub mod foreground;
#[cfg(any(windows, test))]
mod hook_probe;
pub mod hooks;
pub mod hotkey;
pub mod ledger;
pub mod messages;
mod plan;
pub mod text;
pub mod watchdog;
use crate::{api::NativeWindow, uia::patterns::ActionContext};
use api::{InputApi, Target};
#[cfg(windows)]
pub use ffi::{NativeRelease, RealInput, RealSafetyApi};
use gcu_core::driver::*;
use std::sync::{Arc, Mutex};

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
                guard.check(plan.foreground)?;
            }
            let target = self
                .api
                .resolve(window, plan.point, plan.focus.clone())
                .await?;
            context.check()?;
            let foreground = plan.foreground;
            if !foreground
                && (!target.message_capable || messages::ineffective_class(&target.class))
            {
                return Err(DriverError::CapabilityUnavailable);
            }
            self.api.validate(target.clone(), foreground).await?;
            let mut packets = plan.packets;
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
                Some(self.api.sample(target.clone()).await?)
            };
            if foreground {
                foreground::activate(self.api.as_ref(), &target, &context).await?;
                if let Some(focus) = plan.focus {
                    self.api
                        .focus(target.clone(), focus, context.clone())
                        .await?;
                }
            }
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
                    safety,
                }
                .run()
                .await?;
                packets.clear();
            }
            for packet in packets {
                context.check()?;
                self.api.validate(target.clone(), foreground).await?;
                if foreground {
                    foreground::verify(self.api.as_ref(), &target).await?;
                }
                context.check()?;
                if let Some(tracker) = &self.tracker {
                    tracker
                        .prepare(target.clone(), packet.clone(), delivery)
                        .await?;
                }
                let result = match context.check() {
                    Ok(()) => {
                        self.api
                            .send(
                                target.clone(),
                                packet,
                                delivery,
                                self.ledger.clone(),
                                context.clone(),
                            )
                            .await
                    }
                    Err(error) => Err(error),
                };
                if let Some(tracker) = &self.tracker {
                    if result.is_ok() || result.as_ref().is_err_and(|e| !e.maybe_executed()) {
                        tracker.complete(result.is_ok()).await?;
                    }
                }
                result?;
                tokio::task::yield_now().await;
            }
            if let Some(before) = before {
                let effect = self
                    .settle(&target, before, plan.text, &context, work_deadline)
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
            let state = self.api.sample(target.clone()).await?;
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

async fn release_bounded(api: Arc<dyn InputApi>, ledger: Arc<ledger::InputLedger>) {
    if tokio::time::timeout(std::time::Duration::from_millis(100), api.release(ledger))
        .await
        .is_err()
    {
        tracing::warn!("Windows input cleanup deadline exceeded");
    }
}
