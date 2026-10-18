//! Deliver a DOM action through the same router call that would reach the driver.
//! A DOM failure is returned as-is. It is not retried and not sent to L0.

use super::{DomEffect, DomFault, DomOperation, DomToken, Hub};
use crate::action::ActionRouter;
use crate::driver::{self, Button, Cancel, Delivery, Dir, DriverAction, DriverError};
use crate::model::{NativeHandle, RawNode, RawTree, Rect, WindowPoint};
use crate::session::Session;
use std::sync::Arc;

enum Kind {
    Click,
    SetValue(String),
    Scroll(f64, f64),
}

impl ActionRouter {
    /// Attach an in-memory DOM hub before the router is shared.
    ///
    /// Does not read a tab or wait. Timeout: none. Cancellation: none.
    /// Existing sessions keep using L0 until a share is bound on the hub.
    #[must_use]
    pub fn with_browser(mut self, hub: Arc<Hub>) -> Self {
        self.browser = Some(hub);
        self
    }
}

pub(crate) async fn dispatch(
    router: &ActionRouter,
    session: &Session,
    cancel: &Cancel,
    request: &DriverAction,
    raw: &RawTree,
) -> driver::Result<Delivery> {
    let Some(hub) = router.browser.clone() else {
        return driver_perform(router, session, cancel, request).await;
    };
    let Some((token, kind)) = classify(request, raw)? else {
        return driver_perform(router, session, cancel, request).await;
    };
    let resolved = hub.resolve(&token)?;
    let op = operation(resolved, kind);
    router
        .native(session, cancel, true, async move {
            match hub.channel.perform(&op).await {
                Ok(DomEffect::Done) => Ok(Delivery::Accessibility),
                Ok(DomEffect::Unknown) => Err(DriverError::Native {
                    message: "uncertain dom delivery".into(),
                    maybe_executed: true,
                }),
                Err(fault) => Err(map_fault(fault)),
            }
        })
        .await
}

async fn driver_perform(
    router: &ActionRouter,
    session: &Session,
    cancel: &Cancel,
    request: &DriverAction,
) -> driver::Result<Delivery> {
    router
        .native(
            session,
            cancel,
            true,
            router.driver.perform(request, cancel),
        )
        .await
}

fn operation(resolved: super::ResolvedDom, kind: Kind) -> DomOperation {
    let super::ResolvedDom {
        share_id,
        snapshot_id,
        node_id,
    } = resolved;
    match kind {
        Kind::Click => DomOperation::Click {
            share_id,
            snapshot_id,
            node_id,
        },
        Kind::SetValue(value) => DomOperation::SetValue {
            share_id,
            snapshot_id,
            node_id,
            value,
        },
        Kind::Scroll(dx, dy) => DomOperation::Scroll {
            share_id,
            snapshot_id,
            node_id,
            dx,
            dy,
        },
    }
}

fn classify(
    request: &DriverAction,
    raw: &RawTree,
) -> Result<Option<(DomToken, Kind)>, DriverError> {
    match request {
        DriverAction::Press { el } => Ok(token(el).map(|token| (token, Kind::Click))),
        DriverAction::SetValue { el, value } => {
            let Some(token) = token(el) else {
                return Ok(None);
            };
            if value.contains('\0') || value.chars().count() > 8192 {
                return Err(DriverError::InvalidRequest);
            }
            Ok(Some((token, Kind::SetValue(value.clone()))))
        }
        DriverAction::ScrollElement { el, dir, pages } => {
            let Some(token) = token(el) else {
                return Ok(None);
            };
            let (dx, dy) = scroll_pages(*dir, *pages)?;
            Ok(Some((token, Kind::Scroll(dx, dy))))
        }
        DriverAction::PointerClick {
            at, button, count, ..
        } => {
            let Some(token) = hit(raw, *at) else {
                return Ok(None);
            };
            if *count != 1 || *button != Button::Left {
                return Err(DriverError::ElementNotActionable);
            }
            Ok(Some((token, Kind::Click)))
        }
        DriverAction::PointerScroll { at, dx, dy, .. } => {
            let Some(token) = hit(raw, *at) else {
                return Ok(None);
            };
            if !dx_ok(*dx) || !dx_ok(*dy) {
                return Err(DriverError::InvalidRequest);
            }
            Ok(Some((token, Kind::Scroll(clamp_i32(*dx), clamp_i32(*dy)))))
        }
        DriverAction::SecondaryAction { el, .. } | DriverAction::SelectText { el, .. } => {
            if token(el).is_some() {
                Err(DriverError::ElementNotActionable)
            } else {
                Ok(None)
            }
        }
        DriverAction::TypeText { focus, .. } => {
            if focus.as_ref().is_some_and(|el| token(el).is_some()) {
                Err(DriverError::ElementNotActionable)
            } else {
                Ok(None)
            }
        }
        DriverAction::PressKeys { .. }
        | DriverAction::PointerDrag { .. }
        | DriverAction::Activate { .. } => Ok(None),
    }
}

fn token(handle: &NativeHandle) -> Option<DomToken> {
    handle.downcast_ref::<DomToken>().cloned()
}

fn hit(raw: &RawTree, at: WindowPoint) -> Option<DomToken> {
    let mut best: Option<(u32, DomToken)> = None;
    if let Some(root) = &raw.root {
        walk(root, at, 0, &mut best);
    }
    best.map(|(_, token)| token)
}

fn walk(node: &RawNode, at: WindowPoint, depth: u32, best: &mut Option<(u32, DomToken)>) {
    for child in &node.children {
        walk(child, at, depth.saturating_add(1), best);
    }
    if let Some(token) = node.handle.downcast_ref::<DomToken>() {
        if contains(node.frame, at) && best.as_ref().is_none_or(|(found, _)| depth >= *found) {
            *best = Some((depth, token.clone()));
        }
    }
}

fn contains(frame: Option<Rect>, at: WindowPoint) -> bool {
    frame.is_some_and(|frame| {
        at.x >= frame.x && at.y >= frame.y && at.x < frame.x + frame.w && at.y < frame.y + frame.h
    })
}

fn scroll_pages(dir: Dir, pages: f32) -> Result<(f64, f64), DriverError> {
    if !pages.is_finite() || pages < 0.0 {
        return Err(DriverError::InvalidRequest);
    }
    let amount = (f64::from(pages) * 360.0).clamp(0.0, 2400.0);
    Ok(match dir {
        Dir::Up => (0.0, -amount),
        Dir::Down => (0.0, amount),
        Dir::Left => (-amount, 0.0),
        Dir::Right => (amount, 0.0),
    })
}

fn dx_ok(value: i32) -> bool {
    value != i32::MIN
}

fn clamp_i32(value: i32) -> f64 {
    f64::from(value).clamp(-2400.0, 2400.0)
}

fn map_fault(fault: DomFault) -> DriverError {
    match fault {
        DomFault::Unshared | DomFault::Frame | DomFault::Stale => DriverError::StaleElement,
        DomFault::NotActionable => DriverError::ElementNotActionable,
        DomFault::NotSettable { maybe_executed } => DriverError::NotSettable { maybe_executed },
        DomFault::Handoff => DriverError::SecureDesktop,
        DomFault::Unavailable => DriverError::CapabilityUnavailable,
        DomFault::Busy => DriverError::Native {
            message: "dom busy".into(),
            maybe_executed: false,
        },
        DomFault::Driver { maybe_executed } => DriverError::Native {
            message: "dom driver".into(),
            maybe_executed,
        },
    }
}
