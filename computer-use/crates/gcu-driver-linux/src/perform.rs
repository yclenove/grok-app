//! Driver action routing and dispatching for Linux AT-SPI and X11.

use crate::atspi::actions::execute_atspi_action;
use crate::driver::LinuxDriver;
use crate::x11::input::{
    activate_and_verify, execute_pointer_click, execute_pointer_drag, execute_pointer_scroll,
    execute_press_keys, execute_type_text_x11,
};
use crate::x11::watchdog::run_with_watchdog;
use gcu_core::driver::{
    Cancel, Delivery, Driver, DriverAction, DriverError, KeyRoute, PointerRoute, Result,
};

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
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            execute_pointer_click(
                x.as_ref(),
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
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            execute_pointer_scroll(x.as_ref(), driver.ledger(), xid, *at, *dx, *dy, cancel).await
        }
        DriverAction::PointerDrag { win, path, route } => {
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            if *route == PointerRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            execute_pointer_drag(x.as_ref(), driver.ledger(), xid, path, cancel).await
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
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            if *route == KeyRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            execute_type_text_x11(x.as_ref(), driver.ledger(), xid, text, cancel).await
        }
        DriverAction::PressKeys { win, chord, route } => {
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            if *route == KeyRoute::TargetedOnly {
                return Err(DriverError::CapabilityUnavailable);
            }
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            execute_press_keys(x.as_ref(), driver.ledger(), xid, chord, cancel).await
        }
        DriverAction::Activate { win } => {
            if driver.is_native_wayland_window(win) {
                return Err(DriverError::CapabilityUnavailable);
            }
            let xid = driver.resolve_xid(win)?;
            require_foreground(driver)?;
            let x = driver
                .x_server()
                .ok_or(DriverError::CapabilityUnavailable)?;
            activate_and_verify(x.as_ref(), xid, cancel).await?;
            Ok(Delivery::Foreground)
        }
    };

    result
}
