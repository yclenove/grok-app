//! Driver action routing and dispatching for Linux AT-SPI and X11.

use crate::atspi::actions::execute_atspi_action;
use crate::driver::LinuxDriver;
use crate::x11::input::{
    activate_and_verify, execute_pointer_click, execute_pointer_drag, execute_pointer_scroll,
    execute_press_keys, execute_type_text_x11,
};
use crate::x11::watchdog::run_with_watchdog;
use gcu_core::driver::{
    Cancel, Delivery, Driver, DriverAction, DriverError, KeyRoute, PointerRoute, Result, WindowRef,
};
use gcu_core::model::NativeHandle;

fn require_foreground(driver: &LinuxDriver) -> Result<()> {
    if !driver.is_hotkey_available() || driver.is_stopped() {
        return Err(DriverError::CapabilityUnavailable);
    }
    if driver.capabilities().delivery.foreground
        == gcu_protocol::capabilities::ForegroundCapability::Unavailable
    {
        return Err(DriverError::CapabilityUnavailable);
    }
    Ok(())
}

/// Execute a driver action across AT-SPI or X11, releasing ledger inputs on failure.
pub async fn perform_action(
    driver: &LinuxDriver,
    req: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery> {
    if cancel.is_cancelled() {
        driver.release_all_input().await;
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    // An attached portal owns one exact window. Its DriverAction path is reachable only
    // through the same Driver boundary used by ActionRouter; X11 is never a fallback.
    if let Some(input) = driver.portal_input().cloned() {
        let window = crate::wayland::input_plan::window(req);
        if window == Some(input.window())
            && !matches!(req, DriverAction::TypeText { focus: Some(_), .. })
        {
            driver.admit_wayland_window(&input.window().id)?;
            return input.perform(req, cancel).await;
        }
    }
    let result = run_with_watchdog(perform_action_inner(driver, req, cancel)).await;
    if result.is_err() {
        driver.release_all_input().await;
    }
    result
}

async fn perform_action_inner(
    driver: &LinuxDriver,
    req: &DriverAction,
    cancel: &Cancel,
) -> Result<Delivery> {
    scoped_admit(driver, req)?;
    let result = match req {
        DriverAction::Press { .. }
        | DriverAction::SecondaryAction { .. }
        | DriverAction::SetValue { .. }
        | DriverAction::SelectText { .. } => {
            execute_atspi_action(
                driver.connection(),
                driver.fake_source().map(|s| s.as_ref()),
                driver.fake_root(),
                req,
                cancel,
            )
            .await
        }
        DriverAction::ScrollElement { .. } => {
            // First attempt AT-SPI component scrolling
            let atspi_res = execute_atspi_action(
                driver.connection(),
                driver.fake_source().map(|s| s.as_ref()),
                driver.fake_root(),
                req,
                cancel,
            )
            .await;

            if atspi_res.is_ok() {
                atspi_res
            } else {
                Err(DriverError::ElementNotActionable)
            }
        }
        DriverAction::PointerClick {
            win,
            at,
            button,
            count,
            route,
        } => {
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let scope = driver.x11_scope();
            execute_pointer_click(
                x,
                scope.as_ref(),
                driver.ledger(),
                xid,
                *at,
                *button,
                *count,
                cancel,
            )
            .await
        }
        DriverAction::PointerScroll {
            win,
            at,
            dx,
            dy,
            route,
        } => {
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let scope = driver.x11_scope();
            execute_pointer_scroll(
                x,
                scope.as_ref(),
                driver.ledger(),
                xid,
                *at,
                *dx,
                *dy,
                cancel,
            )
            .await
        }
        DriverAction::PointerDrag { win, path, route } => {
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let scope = driver.x11_scope();
            execute_pointer_drag(
                x,
                scope.as_ref(),
                driver.ledger(),
                xid,
                path,
                cancel,
            )
            .await
        }
        DriverAction::TypeText {
            win,
            focus,
            text,
            route,
        } => {
            // If focus handle is provided, first prioritize AT-SPI SetValue
            if let Some(focus_el) = focus {
                let atspi_req = DriverAction::SetValue {
                    el: focus_el.clone(),
                    value: text.clone(),
                };
                if let Ok(del) = execute_atspi_action(
                    driver.connection(),
                    driver.fake_source().map(|s| s.as_ref()),
                    driver.fake_root(),
                    &atspi_req,
                    cancel,
                )
                .await
                {
                    return Ok(del);
                }
            }

            // Otherwise fall back to X11 keyboard input
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            if *route == KeyRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let scope = driver.x11_scope();
            execute_type_text_x11(
                x,
                scope.as_ref(),
                driver.ledger(),
                xid,
                text,
                cancel,
            )
            .await
        }
        DriverAction::PressKeys { win, chord, route } => {
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            if *route == KeyRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            let scope = driver.x11_scope();
            execute_press_keys(
                x,
                scope.as_ref(),
                driver.ledger(),
                xid,
                chord,
                cancel,
            )
            .await
        }
        DriverAction::Activate { win } => {
            if driver.route_is_wayland(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = action_xid(driver, win)?;
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            // Scope was proved above. Activation is the delivery: a later refusal
            // is ScopeDenied only when this call did not request focus.
            let was_active = matches!(x.active_window(), Ok(Some(active)) if active == xid);
            activate_and_verify(x.as_ref(), xid, cancel).await?;
            if let Some(scope) = driver.x11_scope() {
                if let Err(error) = scope.allow_live(x, xid).await {
                    return Err(if was_active {
                        error
                    } else {
                        DriverError::Native {
                            message: "native driver failure".into(),
                            maybe_executed: true,
                        }
                    });
                }
            }
            Ok(Delivery::Foreground)
        }
    };

    result
}

fn scoped_admit(driver: &LinuxDriver, req: &DriverAction) -> Result<()> {
    if driver.scope().is_none() {
        return Ok(());
    }
    if action_element(req).is_some() {
        return Err(DriverError::ScopeDenied);
    }
    if let Some(win) = action_window(req) {
        driver.prove_target(win)?;
    }
    Ok(())
}

/// Element handle carried by `req`, when the action names one.
pub(crate) fn action_element(req: &DriverAction) -> Option<&NativeHandle> {
    match req {
        DriverAction::Press { el }
        | DriverAction::SecondaryAction { el, .. }
        | DriverAction::SetValue { el, .. }
        | DriverAction::SelectText { el, .. }
        | DriverAction::ScrollElement { el, .. } => Some(el),
        DriverAction::TypeText { focus, .. } => focus.as_ref(),
        _ => None,
    }
}

/// Window carried by `req`, when the action names one.
pub(crate) fn action_window(req: &DriverAction) -> Option<&WindowRef> {
    match req {
        DriverAction::PointerClick { win, .. }
        | DriverAction::PointerScroll { win, .. }
        | DriverAction::PointerDrag { win, .. }
        | DriverAction::PressKeys { win, .. }
        | DriverAction::TypeText { win, .. }
        | DriverAction::Activate { win } => Some(win),
        _ => None,
    }
}

fn action_xid(driver: &LinuxDriver, win: &WindowRef) -> Result<u32> {
    if driver.scope().is_some() {
        return driver.memory_xid(win).ok_or(DriverError::ScopeDenied);
    }
    driver.resolve_xid(win)
}
