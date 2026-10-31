//! XTEST pointer and keyboard automation, window activation, and text typing.

use crate::x11::{
    keys::{
        key_to_keysym, modifier_to_keysym, plan_text_chunk, resolve_or_remap_keysym,
        restore_remapped_keycode,
    },
    ledger::InputLedger,
    scope::X11Scope,
    server::XServer,
};
use gcu_core::{
    driver::{Cancel, Delivery, DriverError},
    model::WindowPoint,
};
use gcu_protocol::{keys::KeyChord, tools::MouseButton as Button};
use std::sync::Arc;
use std::time::Duration;

/// Threshold for chunking long text typing requests.
pub const TYPE_TEXT_CHUNK_SIZE: usize = 2000;
/// How long to wait after remapping keycodes before typing with them, and again before
/// restoring them, so the target client has refreshed its key table.
const REMAP_SETTLE: Duration = Duration::from_millis(120);

/// Prove the window, activate it, then prove it again before the first input event.
///
/// A refusal before focus is requested is [`DriverError::ScopeDenied`]. A refusal
/// after an activate request was sent reports that delivery may have started.
async fn gate_delivery(
    x: &Arc<dyn XServer>,
    xid: u32,
    scope: Option<&X11Scope<'_>>,
    cancel: &Cancel,
) -> Result<(), DriverError> {
    if let Some(scope) = scope {
        scope.allow_live(x, xid).await?;
    }
    let was_active = matches!(x.active_window(), Ok(Some(active)) if active == xid);
    activate_and_verify(x.as_ref(), xid, cancel).await?;
    if let Some(scope) = scope {
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
    Ok(())
}

/// Activate target window using `_NET_ACTIVE_WINDOW` and verify activation succeeded.
///
/// Returns `CapabilityUnavailable` if the window could not be activated or verification fails.
pub async fn activate_and_verify(
    x: &dyn XServer,
    xid: u32,
    cancel: &Cancel,
) -> Result<(), DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    if x.window_geometry(xid).is_err() {
        return Err(DriverError::WindowGone);
    }

    // If the target window is already active, do not issue another _NET_ACTIVE_WINDOW
    // request. Re-activating an already active window causes the window manager
    // (such as Openbox) to asynchronously re-focus the window, which can disrupt
    // intra-window focus state and race with immediate subsequent typing or key presses.
    if let Ok(Some(active)) = x.active_window() {
        if active == xid {
            return Ok(());
        }
    }

    x.activate_window(xid)
        .map_err(|_| DriverError::CapabilityUnavailable)?;

    // Poll briefly (up to 500ms) to verify that the target window became active
    for _ in 0..10 {
        if cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if let Ok(Some(active)) = x.active_window() {
            if active == xid {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Final check
    if let Ok(Some(active)) = x.active_window() {
        if active == xid {
            return Ok(());
        }
    }

    if x.window_geometry(xid).is_err() {
        return Err(DriverError::WindowGone);
    }

    Err(DriverError::CapabilityUnavailable)
}

/// Execute a physical pointer click via XTEST.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_pointer_click(
    x: &Arc<dyn XServer>,
    scope: Option<&X11Scope<'_>>,
    ledger: &InputLedger,
    xid: u32,
    at: WindowPoint,
    button: Button,
    count: u8,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    gate_delivery(x, xid, scope, cancel).await?;

    let geom = x.window_geometry(xid).map_err(|e| DriverError::Native {
        message: e.to_string(),
        maybe_executed: false,
    })?;

    if at.x < 0.0 || at.y < 0.0 || at.x > geom.width as f64 || at.y > geom.height as f64 {
        return Err(DriverError::OutOfBounds);
    }

    let root_x = (geom.x + at.x as i32) as i16;
    let root_y = (geom.y + at.y as i32) as i16;

    let btn_code = match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    };

    // Move mouse
    x.xtest_fake_motion(root_x, root_y)
        .map_err(|e| DriverError::Native {
            message: e.to_string(),
            maybe_executed: false,
        })?;

    let effective_count = count.max(1);
    for _ in 0..effective_count {
        if cancel.is_cancelled() {
            ledger.release_all(x);
            return Err(DriverError::Cancelled {
                maybe_executed: true,
            });
        }

        // Record before send
        ledger.record_button_down(btn_code);
        if let Err(e) = x.xtest_fake_button(btn_code, true) {
            ledger.release_all(x);
            return Err(DriverError::Native {
                message: e.to_string(),
                maybe_executed: true,
            });
        }
        tokio::time::sleep(Duration::from_millis(15)).await;

        if let Err(e) = x.xtest_fake_button(btn_code, false) {
            ledger.release_all(x);
            return Err(DriverError::Native {
                message: e.to_string(),
                maybe_executed: true,
            });
        }
        ledger.record_button_up(btn_code);

        tokio::time::sleep(Duration::from_millis(if effective_count > 1 {
            40
        } else {
            25
        }))
        .await;
    }

    Ok(Delivery::Foreground)
}

/// Execute pointer scroll events using X11 wheel buttons (4=up, 5=down, 6=left, 7=right).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_pointer_scroll(
    x: &Arc<dyn XServer>,
    scope: Option<&X11Scope<'_>>,
    ledger: &InputLedger,
    xid: u32,
    at: WindowPoint,
    dx: i32,
    dy: i32,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    gate_delivery(x, xid, scope, cancel).await?;

    let geom = x.window_geometry(xid).map_err(|e| DriverError::Native {
        message: e.to_string(),
        maybe_executed: false,
    })?;

    if at.x < 0.0 || at.y < 0.0 || at.x > geom.width as f64 || at.y > geom.height as f64 {
        return Err(DriverError::OutOfBounds);
    }

    let root_x = (geom.x + at.x as i32) as i16;
    let root_y = (geom.y + at.y as i32) as i16;

    x.xtest_fake_motion(root_x, root_y)
        .map_err(|e| DriverError::Native {
            message: e.to_string(),
            maybe_executed: false,
        })?;

    // Vertical wheel clicks
    if dy != 0 {
        let btn = if dy < 0 { 4 } else { 5 };
        let clicks = ((dy.abs() as f32) / 120.0).ceil().max(1.0) as usize;
        for _ in 0..clicks {
            if cancel.is_cancelled() {
                ledger.release_all(x);
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
            ledger.record_button_down(btn);
            let _ = x.xtest_fake_button(btn, true);
            let _ = x.xtest_fake_button(btn, false);
            ledger.record_button_up(btn);
        }
    }

    // Horizontal wheel clicks
    if dx != 0 {
        let btn = if dx < 0 { 6 } else { 7 };
        let clicks = ((dx.abs() as f32) / 120.0).ceil().max(1.0) as usize;
        for _ in 0..clicks {
            if cancel.is_cancelled() {
                ledger.release_all(x);
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
            ledger.record_button_down(btn);
            let _ = x.xtest_fake_button(btn, true);
            let _ = x.xtest_fake_button(btn, false);
            ledger.record_button_up(btn);
        }
    }

    Ok(Delivery::Foreground)
}

/// Execute an interpolated pointer drag along a path.
pub(crate) async fn execute_pointer_drag(
    x: &Arc<dyn XServer>,
    scope: Option<&X11Scope<'_>>,
    ledger: &InputLedger,
    xid: u32,
    path: &[WindowPoint],
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    if path.len() < 2 {
        return Err(DriverError::InvalidRequest);
    }

    gate_delivery(x, xid, scope, cancel).await?;

    let geom = x.window_geometry(xid).map_err(|e| DriverError::Native {
        message: e.to_string(),
        maybe_executed: false,
    })?;

    for pt in path {
        if pt.x < 0.0 || pt.y < 0.0 || pt.x > geom.width as f64 || pt.y > geom.height as f64 {
            return Err(DriverError::OutOfBounds);
        }
    }

    let start_x = (geom.x + path[0].x as i32) as i16;
    let start_y = (geom.y + path[0].y as i32) as i16;

    x.xtest_fake_motion(start_x, start_y)
        .map_err(|e| DriverError::Native {
            message: e.to_string(),
            maybe_executed: false,
        })?;

    // Press button 1 (Left) with press-before-send accounting
    ledger.record_button_down(1);
    if let Err(e) = x.xtest_fake_button(1, true) {
        ledger.release_all(x);
        return Err(DriverError::Native {
            message: e.to_string(),
            maybe_executed: true,
        });
    }

    // Interpolate along path
    for window in path.windows(2) {
        let p1 = window[0];
        let p2 = window[1];
        let dist = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let steps = (dist / 10.0).ceil().max(2.0) as usize;

        for step in 1..=steps {
            if cancel.is_cancelled() {
                ledger.release_all(x);
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }

            let t = step as f64 / steps as f64;
            let cx = p1.x + (p2.x - p1.x) * t;
            let cy = p1.y + (p2.y - p1.y) * t;
            let rx = (geom.x + cx as i32) as i16;
            let ry = (geom.y + cy as i32) as i16;

            let _ = x.xtest_fake_motion(rx, ry);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // Release button 1
    if let Err(e) = x.xtest_fake_button(1, false) {
        ledger.release_all(x);
        return Err(DriverError::Native {
            message: e.to_string(),
            maybe_executed: true,
        });
    }
    ledger.record_button_up(1);

    Ok(Delivery::Foreground)
}

/// Execute a key chord with modifiers and key press/release.
pub(crate) async fn execute_press_keys(
    x: &Arc<dyn XServer>,
    scope: Option<&X11Scope<'_>>,
    ledger: &InputLedger,
    xid: u32,
    chord: &KeyChord,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    gate_delivery(x, xid, scope, cancel).await?;

    let mut held_mod_keycodes = Vec::new();
    let mut remapped_keycodes = Vec::new();

    // 1. Resolve and press modifiers
    for &m in &chord.modifiers {
        if cancel.is_cancelled() {
            ledger.release_all(x);
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        let sym = modifier_to_keysym(m);
        let resolved = resolve_or_remap_keysym(x, sym)?;
        if let Some(r) = resolved.remapped_keycode {
            remapped_keycodes.push(r);
        }
        ledger.record_key_down(resolved.keycode);
        if let Err(e) = x.xtest_fake_key(resolved.keycode, true) {
            ledger.release_all(x);
            return Err(DriverError::Native {
                message: e.to_string(),
                maybe_executed: true,
            });
        }
        held_mod_keycodes.push(resolved.keycode);
    }

    // Settle modifier press so client event loops register active modifier mask before main key down
    if !held_mod_keycodes.is_empty() {
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    // 2. Resolve and press main key
    let main_sym = key_to_keysym(&chord.key);
    let main_resolved = resolve_or_remap_keysym(x, main_sym)?;
    if let Some(r) = main_resolved.remapped_keycode {
        remapped_keycodes.push(r);
    }

    // If main key needs shift and shift is not already pressed, press Shift
    let extra_shift = if main_resolved.needs_shift
        && !held_mod_keycodes.iter().any(|&k| {
            // Shift keycode check
            let shift_sym = modifier_to_keysym(gcu_protocol::keys::Modifier::Shift);
            resolve_or_remap_keysym(x, shift_sym)
                .map(|r| r.keycode == k)
                .unwrap_or(false)
        }) {
        let shift_sym = modifier_to_keysym(gcu_protocol::keys::Modifier::Shift);
        if let Ok(sr) = resolve_or_remap_keysym(x, shift_sym) {
            ledger.record_key_down(sr.keycode);
            let _ = x.xtest_fake_key(sr.keycode, true);
            Some(sr.keycode)
        } else {
            None
        }
    } else {
        None
    };

    ledger.record_key_down(main_resolved.keycode);
    if let Err(e) = x.xtest_fake_key(main_resolved.keycode, true) {
        ledger.release_all(x);
        return Err(DriverError::Native {
            message: e.to_string(),
            maybe_executed: true,
        });
    }

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Release main key
    let _ = x.xtest_fake_key(main_resolved.keycode, false);
    ledger.record_key_up(main_resolved.keycode);

    // Settle key release so main key up is dispatched before releasing modifiers
    if !held_mod_keycodes.is_empty() {
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    // Release extra shift if added
    if let Some(skc) = extra_shift {
        let _ = x.xtest_fake_key(skc, false);
        ledger.record_key_up(skc);
    }

    // 3. Release modifiers in reverse order
    for &mkc in held_mod_keycodes.iter().rev() {
        let _ = x.xtest_fake_key(mkc, false);
        ledger.record_key_up(mkc);
    }

    tokio::time::sleep(Duration::from_millis(15)).await;

    // 4. Restore any remapped keycodes
    for rkc in remapped_keycodes {
        restore_remapped_keycode(x, rkc);
    }

    Ok(Delivery::Foreground)
}

/// Execute type_text via X11 XTEST keysyms, chunking long inputs without splitting grapheme clusters.
pub(crate) async fn execute_type_text_x11(
    x: &Arc<dyn XServer>,
    scope: Option<&X11Scope<'_>>,
    ledger: &InputLedger,
    xid: u32,
    text: &str,
    cancel: &Cancel,
) -> Result<Delivery, DriverError> {
    gate_delivery(x, xid, scope, cancel).await?;

    let chars: Vec<char> = text.chars().collect();
    let shift_sym = modifier_to_keysym(gcu_protocol::keys::Modifier::Shift);
    let shift_kc = resolve_or_remap_keysym(x, shift_sym)
        .map(|r| r.keycode)
        .unwrap_or(50); // standard X11 keycode for Shift_L

    let mut pos = 0;
    while pos < chars.len() {
        let end = (pos + TYPE_TEXT_CHUNK_SIZE).min(chars.len());
        let plan = plan_text_chunk(x, &chars[pos..end])?;
        if !plan.remapped.is_empty() {
            // Clients rebuild their key tables asynchronously after MappingNotify; type only
            // once they have had time to, and never change these keycodes mid-chunk.
            tokio::time::sleep(REMAP_SETTLE).await;
        }
        for resolved in &plan.keys {
            if cancel.is_cancelled() {
                ledger.release_all(x);
                for &kc in &plan.remapped {
                    restore_remapped_keycode(x, kc);
                }
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
            if resolved.needs_shift {
                ledger.record_key_down(shift_kc);
                let _ = x.xtest_fake_key(shift_kc, true);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            ledger.record_key_down(resolved.keycode);
            let _ = x.xtest_fake_key(resolved.keycode, true);
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = x.xtest_fake_key(resolved.keycode, false);
            ledger.record_key_up(resolved.keycode);
            if resolved.needs_shift {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let _ = x.xtest_fake_key(shift_kc, false);
                ledger.record_key_up(shift_kc);
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        if !plan.remapped.is_empty() {
            // Let the application finish reading the last keystrokes before they change meaning.
            tokio::time::sleep(REMAP_SETTLE).await;
            for &kc in &plan.remapped {
                restore_remapped_keycode(x, kc);
            }
        }
        pos += plan.consumed;
        tokio::task::yield_now().await;
    }

    Ok(Delivery::Foreground)
}
