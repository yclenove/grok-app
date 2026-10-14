//! X11 protocol abstraction, window enumeration, and capture.
//!
//! Exposes the [`XServer`] trait with concrete [`RealX`] (backed by `x11rb`)
//! and [`FakeX`] (in-memory for platform-independent testing).

pub mod activity;
pub mod capture;
pub mod fake;
pub mod hotkey;
pub mod input;
pub mod keys;
pub mod ledger;
pub mod real;
pub mod server;
pub mod watchdog;
pub mod windows;

pub use activity::{
    is_xtest_device_name, ActivityClock, DeviceFilter, DeviceInfo, SharedActivityClock,
};
pub use hotkey::{
    attach_x11_listener, handle_hotkey_trigger, modifier_combinations, spawn_x11_listener,
    HotkeyConfig, HotkeyError, HotkeyManager, SharedStopLatch, StopLatch,
};
pub use watchdog::{
    run_with_timeout, run_with_watchdog, HeartbeatWatchdog, DEFAULT_HEARTBEAT_TIMEOUT,
};

pub use capture::{
    capture_window, check_window_visibility, decide_capture_strategy, CaptureStrategy,
    WindowVisibility,
};
pub use fake::{FakeWindow, FakeX};
pub use input::{
    activate_and_verify, execute_pointer_click, execute_pointer_drag, execute_pointer_scroll,
    execute_press_keys, execute_type_text_x11, TYPE_TEXT_CHUNK_SIZE,
};
pub use keys::{
    char_to_keysym, key_to_keysym, modifier_to_keysym, named_key_to_keysym, plan_text_chunk,
    resolve_or_remap_keysym, restore_remapped_keycode, ResolvedKeycode, TextPlan,
};
pub use ledger::InputLedger;
pub use real::RealX;
pub use server::{zpixmap_to_rgba, RawImage, XError, XRootGeometry, XServer, XWindowGeometry};
pub use windows::{align_windows, list_x11_windows, AtspiWindowCandidate, XWindowCandidate};

/// Build the four capability permutations based on user_activity_guard and stop_hotkey availability.
pub fn build_caps_quad(
    base: &gcu_protocol::Capabilities,
) -> (
    gcu_protocol::Capabilities,
    gcu_protocol::Capabilities,
    gcu_protocol::Capabilities,
    gcu_protocol::Capabilities,
) {
    let make = |guard: bool, fg: gcu_protocol::capabilities::ForegroundCapability| {
        let mut c = base.clone();
        c.user_activity_guard = guard;
        c.delivery.foreground = fg;
        c
    };
    (
        make(
            true,
            gcu_protocol::capabilities::ForegroundCapability::Policy,
        ),
        make(
            false,
            gcu_protocol::capabilities::ForegroundCapability::Policy,
        ),
        make(
            true,
            gcu_protocol::capabilities::ForegroundCapability::Unavailable,
        ),
        make(
            false,
            gcu_protocol::capabilities::ForegroundCapability::Unavailable,
        ),
    )
}

/// Update driver base capabilities with probed X11 extensions.
pub fn update_x11_caps(caps: &mut gcu_protocol::Capabilities, x: &dyn XServer) {
    let to_status = |ok| {
        if ok {
            gcu_protocol::capabilities::PrivateApiStatus::Available
        } else {
            gcu_protocol::capabilities::PrivateApiStatus::Missing
        }
    };
    caps.private_apis.insert(
        "x11_composite".into(),
        to_status(matches!(x.is_composite_available(), Ok(true))),
    );
    caps.private_apis.insert(
        "xtest".into(),
        to_status(matches!(x.is_xtest_available(), Ok(true))),
    );
}
