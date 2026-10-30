//! Targeted `postToPid` and policy-gated foreground input.
//!
//! Chromium-class windows have no background click on macOS 27 (ADR 0006).
//! `Auto` and `TargetedOnly` return [`DriverError::CapabilityUnavailable`] for
//! those targets and when the field-51 probe fails. They do not post a HID
//! event and they do not activate the app. `AllowForeground` may activate and
//! post to the HID tap; success is [`Delivery::Foreground`]. A failed arrival
//! check is not retried and is not upgraded. Presses are recorded before send.
//! Cancel, timeout, and [`release_held`] post the matching ups.
//!
//! Calls stop at the budget passed in, which the driver caps at
//! [`gcu_core::driver::DRIVER_DEADLINE`]. `maybe_executed` is false until a
//! post has been entered.

pub mod activity;
pub mod clipboard;
pub mod clock;
mod events;
mod fake_events;
pub mod foreground;
pub mod hotkey;
mod keys;
mod ledger;
mod proof;
mod skylight;
pub mod text;
pub mod timing;
pub mod watchdog;

#[cfg(target_os = "macos")]
mod cg;
#[cfg(all(test, target_os = "macos"))]
mod cg_tests;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod pasteboard_native;
#[cfg(target_os = "macos")]
pub use pasteboard_native::NativePasteboard;

pub use events::{
    plan_click, plan_drag, plan_scroll, Call, EventApi, EventKind, InputTarget, PostFault,
    PostGate, PostPath, Probe, SyntheticEvent, DRAG_SAMPLE_GAP, FIELD_WINDOW_NUMBER,
};
pub use fake_events::FakeEvents;
pub use keys::utf16_chunks;
pub use ledger::InputLedger;
pub(crate) use proof::arm;
pub use skylight::{classify as classify_skylight, status as skylight_status};

#[cfg(target_os = "macos")]
pub use cg::{ensure_agent_appkit, ensure_appkit, product_major, RealEvents};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use events::{plan_click as clicks, plan_drag as drags, plan_scroll as scrolls};
use gcu_core::driver::{
    Cancel, Delivery, DriverAction, DriverError, KeyRoute, PointerRoute, Result,
};
use gcu_protocol::capabilities::{BackgroundOperation, Capabilities, ForegroundCapability};
use keys::{plan_chord, plan_text};

/// Copy probe results onto the driver's capability table.
///
/// Background pointer stays off until [`Probe::background_ready`]. Foreground
/// is `policy` only when AppKit is up and the probe allows a HID tap. SkyLight
/// is reported and is not treated as a Chromium background path.
pub fn apply_probe(caps: &mut Capabilities, probe: &Probe) {
    let background = probe.background_ready();
    let foreground =
        probe.foreground && probe.appkit_ready && probe.accessibility && !probe.secure_prompt;
    caps.delivery.targeted_pointer = background;
    caps.delivery.targeted_keys = background;
    caps.delivery.foreground = if foreground {
        ForegroundCapability::Policy
    } else {
        ForegroundCapability::Unavailable
    };
    caps.background_operation = BackgroundOperation::Partial;
    caps.private_apis.insert("skylight".into(), probe.skylight);
    caps.private_apis
        .insert("ax_window".into(), crate::scope_os::ax_window_status());
    if !caps.version.contains("P1-06") {
        caps.version.push_str("; targeted events (P1-06)");
    }
    caps.limits.type_text_max = if background || foreground { 8192 } else { 0 };
}

/// Post every pending up. Bounded by the caller. Does not retry a failed up.
pub async fn release_held(api: &dyn EventApi, ledger: &InputLedger) {
    // Poll all releases on the first turn, even when one native call blocks.
    let mut posts: Vec<_> = ledger
        .take_ups()
        .into_iter()
        .map(|up| api.post_release(up))
        .collect();
    std::future::poll_fn(|cx| {
        let mut index = 0;
        while index < posts.len() {
            match posts[index].as_mut().poll(cx) {
                std::task::Poll::Ready(result) => {
                    if result.is_err() {
                        tracing::warn!("macos input cleanup could not post an up");
                    }
                    drop(posts.remove(index));
                }
                std::task::Poll::Pending => index += 1,
            }
        }
        if posts.is_empty() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
}

/// Deliver one pointer, key, or text action. Other actions are refused.
///
/// `budget` includes cleanup. The function does not exceed it. Cancellation
/// before the first post is `maybe_executed: false`.
pub async fn deliver(
    api: Arc<dyn EventApi>,
    ledger: Arc<InputLedger>,
    target: &InputTarget,
    action: &DriverAction,
    cancel: &Cancel,
    budget: Duration,
) -> Result<Delivery> {
    let guards = api.guards();
    if let Some(guards) = &guards {
        guards.check(false)?;
    }
    let clock = guards
        .as_ref()
        .map(|g| Arc::clone(&g.clock))
        .unwrap_or_else(clock::runtime_clock);
    let deadline = clock.now() + budget.min(gcu_core::driver::DRIVER_DEADLINE);
    let sent = Arc::new(AtomicBool::new(false));
    let cleanup_budget = Duration::from_millis(100).min(budget);
    let work_budget = budget.saturating_sub(cleanup_budget);
    if work_budget.is_zero() {
        return Err(DriverError::Timeout {
            maybe_executed: false,
        });
    }
    let api_op = Arc::clone(&api);
    let ledger_op = Arc::clone(&ledger);
    let sent_op = Arc::clone(&sent);
    let clock_op = Arc::clone(&clock);
    let target = target.clone();
    let action = action.clone();
    let cancel_op = cancel.clone();
    let operation = async move {
        run(
            api_op.as_ref(),
            &ledger_op,
            &target,
            &action,
            &cancel_op,
            &sent_op,
            clock_op.as_ref(),
        )
        .await
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(DriverError::Cancelled {
            maybe_executed: sent.load(Ordering::Acquire),
        }),
        _ = async { if let Some(guards) = &guards { guards.stopped().await } else { std::future::pending().await } } => Err(DriverError::Cancelled { maybe_executed: sent.load(Ordering::Acquire) }),
        _ = clock.wait_until(deadline.saturating_sub(cleanup_budget)) => Err(DriverError::Timeout { maybe_executed: sent.load(Ordering::Acquire) }),
        result = operation => result,
    };
    tokio::select! {
        biased;
        _ = release_held(api.as_ref(), &ledger) => {},
        _ = clock.wait_until(deadline.min(clock.now() + cleanup_budget)) => {},
    }
    result
}

async fn run(
    api: &dyn EventApi,
    ledger: &InputLedger,
    target: &InputTarget,
    action: &DriverAction,
    cancel: &Cancel,
    sent: &AtomicBool,
    clock: &dyn clock::Clock,
) -> Result<Delivery> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    let probe = api.probe();
    refuse_before_post(&probe, target, api)?;
    let foreground = route_foreground(action)?;
    let chromium = crate::ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name);
    let hid_blocked = api.blocks_global_hid();
    if hid_blocked && matches!(action, DriverAction::Activate { .. }) {
        return Err(DriverError::CapabilityUnavailable);
    }
    let targeted =
        probe.background_ready() && !chromium && !matches!(action, DriverAction::Activate { .. });
    let path = if targeted {
        PostPath::ToPid
    } else if !hid_blocked && foreground && probe.foreground && probe.appkit_ready {
        PostPath::HidTap
    } else {
        return Err(DriverError::CapabilityUnavailable);
    };
    let events = plan(action, target, path)?;
    let delivery = if path == PostPath::HidTap {
        let guards = api.guards().ok_or(DriverError::CapabilityUnavailable)?;
        guards.check(true)?;
        foreground::activate_verified(api, target, sent).await?;
        Delivery::Foreground
    } else {
        Delivery::TargetedEvent
    };
    let gap = matches!(
        action,
        DriverAction::PointerDrag { .. } | DriverAction::PointerScroll { .. }
    )
    .then_some(events::DRAG_SAMPLE_GAP);
    post_all(api, ledger, &events, cancel, sent, gap, clock).await?;
    if delivery == Delivery::TargetedEvent {
        confirm_arrival(api, target, sent).await?;
    }
    Ok(delivery)
}

fn refuse_before_post(probe: &Probe, target: &InputTarget, api: &dyn EventApi) -> Result<()> {
    if !probe.appkit_ready {
        return Err(DriverError::Native {
            message: "AppKit is not initialized".into(),
            maybe_executed: false,
        });
    }
    if !probe.accessibility {
        return Err(DriverError::PermissionMissing {
            permission: "accessibility".into(),
        });
    }
    if probe.secure_prompt {
        return Err(DriverError::SecureDesktop);
    }
    if target.pid <= 0 || target.window_id == 0 || !api.window_open(target.pid, target.window_id) {
        return Err(DriverError::WindowGone);
    }
    Ok(())
}

fn route_foreground(action: &DriverAction) -> Result<bool> {
    Ok(match action {
        DriverAction::PointerClick { route, .. }
        | DriverAction::PointerScroll { route, .. }
        | DriverAction::PointerDrag { route, .. } => *route == PointerRoute::AllowForeground,
        DriverAction::PressKeys { route, .. } | DriverAction::TypeText { route, .. } => {
            *route == KeyRoute::AllowForeground
        }
        DriverAction::Activate { .. } => true,
        _ => return Err(DriverError::CapabilityUnavailable),
    })
}

fn plan(
    action: &DriverAction,
    target: &InputTarget,
    path: PostPath,
) -> Result<Vec<SyntheticEvent>> {
    match action {
        DriverAction::PointerClick {
            at, button, count, ..
        } => clicks(target, *at, *button, *count, path),
        DriverAction::PointerScroll { at, dx, dy, .. } => scrolls(target, *at, *dx, *dy, path),
        DriverAction::PointerDrag { path: points, .. } => drags(target, points, path),
        DriverAction::PressKeys { chord, .. } => plan_chord(target, chord, path),
        DriverAction::TypeText { text, .. } => plan_text(target, text, path),
        DriverAction::Activate { .. } => Ok(Vec::new()),
        _ => Err(DriverError::CapabilityUnavailable),
    }
}

async fn confirm_arrival(
    api: &dyn EventApi,
    target: &InputTarget,
    sent: &AtomicBool,
) -> Result<()> {
    if let Err(fault) = api.verify_arrival(target.pid, target.window_id).await {
        api.note_arrival_failed();
        sent.store(true, Ordering::Release);
        return Err(fault_error(fault, true));
    }
    Ok(())
}

async fn post_all(
    api: &dyn EventApi,
    ledger: &InputLedger,
    events: &[SyntheticEvent],
    cancel: &Cancel,
    sent: &AtomicBool,
    gap: Option<Duration>,
    clock: &dyn clock::Clock,
) -> Result<()> {
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            if let Some(gap) = gap {
                let until = clock.now() + gap;
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(DriverError::Cancelled {
                            maybe_executed: sent.load(Ordering::Acquire),
                        });
                    }
                    _ = clock.wait_until(until) => {}
                }
            }
        }
        if let Some(guards) = api.guards() {
            if let Err(error) = guards.check(event.path == PostPath::HidTap) {
                return Err(if sent.load(Ordering::Acquire) {
                    foreground::uncertain(error)
                } else {
                    error
                });
            }
        }
        if event.path == PostPath::HidTap {
            foreground::verify(api, event.pid).await?;
        }
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: sent.load(Ordering::Acquire),
            });
        }
        let recorded = if let Some(up) = event.release_event() {
            ledger.record_press(up);
            true
        } else {
            false
        };
        let prior = sent.load(Ordering::Acquire);
        sent.store(true, Ordering::Release);
        match api.post(event.clone()).await {
            Ok(()) => {
                if !event.down {
                    ledger.record_release(event);
                }
            }
            Err(PostFault::NotSent) => {
                if recorded {
                    ledger.pop_unsent();
                }
                if !prior {
                    sent.store(false, Ordering::Release);
                }
                return Err(DriverError::Native {
                    message: "event was not posted".into(),
                    maybe_executed: sent.load(Ordering::Acquire),
                });
            }
            Err(fault) => return Err(fault_error(fault, true)),
        }
    }
    Ok(())
}

fn fault_error(fault: PostFault, maybe_executed: bool) -> DriverError {
    match fault {
        PostFault::Permission if !maybe_executed => DriverError::PermissionMissing {
            permission: "accessibility".into(),
        },
        PostFault::Permission => DriverError::Native {
            message: "accessibility permission changed during post".into(),
            maybe_executed: true,
        },
        PostFault::NotSent => DriverError::Native {
            message: "event was not posted".into(),
            maybe_executed,
        },
        PostFault::Uncertain => DriverError::Native {
            message: "targeted event arrival was not confirmed".into(),
            maybe_executed: true,
        },
    }
}
