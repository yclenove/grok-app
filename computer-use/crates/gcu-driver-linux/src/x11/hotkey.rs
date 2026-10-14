//! Global Stop hotkey registration and event handling via `XGrabKey`.
//!
//! In accordance with architecture §7.7 and reviews #9, #21:
//! - Default hotkey: `Ctrl+Alt+Shift+BackSpace` (configurable).
//! - Covers NumLock (Mod2) and CapsLock (Lock) modifier combinations.
//! - Registration failure (occupied/conflict) -> explicit error.
//! - If the hotkey capability fails, foreground input routes (XTEST) are disabled.
//! - When pressed, triggers an independent Stop channel:
//!   - Latches Stop within <= 100ms.
//!   - Releases all held inputs (via InputLedger) within <= 500ms.
//! - Stop latch is never automatically cleared by engine/driver code.

use crate::x11::ledger::InputLedger;
use crate::x11::server::XServer;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Trait representing the Stop latch.
///
/// In accordance with architecture §7.7 and review #9:
/// Once latched, Stop is NEVER cleared automatically by model turns, reconnects, or engine restarts.
/// Only trusted Host UI or CLI resume can clear it.
pub trait StopLatch: Send + Sync {
    /// Return whether Stop is currently latched.
    fn is_stopped(&self) -> bool;

    /// Request a permanent stop. Idempotent.
    fn request_stop(&self);
}

/// Standard thread-safe stop latch backed by an `AtomicBool`.
#[derive(Debug, Default)]
pub struct SharedStopLatch {
    latched: AtomicBool,
}

impl SharedStopLatch {
    /// Create a new stop latch with the given initial state.
    pub fn new(initial: bool) -> Self {
        Self {
            latched: AtomicBool::new(initial),
        }
    }

    /// Clear the stop latch (for test fixtures only).
    pub fn clear_for_test(&self) {
        self.latched.store(false, Ordering::Release);
    }
}

impl StopLatch for SharedStopLatch {
    fn is_stopped(&self) -> bool {
        self.latched.load(Ordering::Acquire)
    }

    fn request_stop(&self) {
        self.latched.store(true, Ordering::Release);
    }
}

/// Configuration for the global Stop hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfig {
    /// X11 keysym for the hotkey (default: XK_BACKSPACE = 0xff08).
    pub keysym: u32,
    /// Base modifier bitmask (default: Shift(1) | Control(4) | Mod1/Alt(8) = 13).
    pub modifiers: u16,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            keysym: crate::x11::keys::XK_BACKSPACE,
            // Shift = 0x0001, Control = 0x0004, Mod1/Alt = 0x0008
            modifiers: 0x0001 | 0x0004 | 0x0008,
        }
    }
}

/// Compute the four modifier combinations covering NumLock and CapsLock permutations.
pub fn modifier_combinations(base: u16) -> [u16; 4] {
    const LOCK_MASK: u16 = 0x0002;
    const MOD2_MASK: u16 = 0x0010;
    [
        base,
        base | LOCK_MASK,
        base | MOD2_MASK,
        base | LOCK_MASK | MOD2_MASK,
    ]
}

/// Errors occurring during hotkey registration or management.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// Failed to resolve keycode for the configured keysym.
    #[error("failed to resolve keycode for keysym 0x{keysym:x}")]
    KeycodeNotFound {
        /// The unresolved keysym.
        keysym: u32,
    },
    /// XGrabKey registration failed due to conflict or grab error.
    #[error("hotkey grab conflict or registration failed: keycode {keycode}, modifiers {modifiers:#x}: {reason}")]
    RegistrationFailed {
        /// The keycode that failed to grab.
        keycode: u8,
        /// The modifier mask that failed.
        modifiers: u16,
        /// Underlying failure reason.
        reason: String,
    },
    /// The hotkey listener is inactive or disconnected.
    #[error("hotkey listener inactive or disconnected")]
    ListenerInactive,
}

/// Execute the immediate stop response:
/// 1. Set the stop latch (<= 100ms budget).
/// 2. Issue `release_all_input` via the input ledger (<= 500ms budget).
pub async fn handle_hotkey_trigger(
    stop_latch: &dyn StopLatch,
    ledger: &InputLedger,
    x: &dyn XServer,
) {
    stop_latch.request_stop();
    ledger.release_all(x);
}

/// Manager responsible for XGrabKey registration, lifecycle, and event handling.
pub struct HotkeyManager {
    config: HotkeyConfig,
    resolved_keycode: AtomicU8,
    is_registered: AtomicBool,
    available: AtomicBool,
    last_error: Mutex<Option<String>>,
    stop_latch: Arc<dyn StopLatch>,
}

impl HotkeyManager {
    /// Create a new HotkeyManager with the given configuration and stop latch.
    pub fn new(config: HotkeyConfig, stop_latch: Arc<dyn StopLatch>) -> Self {
        Self {
            config,
            resolved_keycode: AtomicU8::new(0),
            is_registered: AtomicBool::new(false),
            available: AtomicBool::new(false),
            last_error: Mutex::new(None),
            stop_latch,
        }
    }

    /// Access the hotkey configuration.
    pub fn config(&self) -> &HotkeyConfig {
        &self.config
    }

    /// Access the underlying stop latch.
    pub fn stop_latch(&self) -> &Arc<dyn StopLatch> {
        &self.stop_latch
    }

    /// Whether the hotkey was successfully registered and is actively monitoring.
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    /// Update the hotkey availability status.
    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Release);
    }

    /// Get the resolved keycode if registered.
    pub fn resolved_keycode(&self) -> Option<u8> {
        let kc = self.resolved_keycode.load(Ordering::Acquire);
        if kc == 0 {
            None
        } else {
            Some(kc)
        }
    }

    /// Last registration or listener error, if any.
    pub fn last_error(&self) -> Option<String> {
        let guard = match self.last_error.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.clone()
    }

    /// Register the configured hotkey combinations on the root window using `x`.
    pub fn register(&self, x: &dyn XServer) -> Result<u8, HotkeyError> {
        let keycode = match crate::x11::keys::resolve_or_remap_keysym(x, self.config.keysym) {
            Ok(rk) => rk.keycode,
            Err(_) => {
                let err = HotkeyError::KeycodeNotFound {
                    keysym: self.config.keysym,
                };
                self.record_error(err.to_string());
                self.available.store(false, Ordering::Release);
                return Err(err);
            }
        };

        let combos = modifier_combinations(self.config.modifiers);
        let mut grabbed = Vec::new();

        for &mods in &combos {
            if let Err(e) = x.grab_key(keycode, mods) {
                // Roll back any partial grabs
                for &(g_kc, g_mod) in &grabbed {
                    let _ = x.ungrab_key(g_kc, g_mod);
                }
                let err_msg = e.to_string();
                self.record_error(err_msg.clone());
                self.available.store(false, Ordering::Release);
                self.is_registered.store(false, Ordering::Release);
                return Err(HotkeyError::RegistrationFailed {
                    keycode,
                    modifiers: mods,
                    reason: err_msg,
                });
            }
            grabbed.push((keycode, mods));
        }

        self.resolved_keycode.store(keycode, Ordering::Release);
        self.is_registered.store(true, Ordering::Release);
        self.available.store(true, Ordering::Release);
        self.clear_error();
        Ok(keycode)
    }

    /// Ungrab the registered hotkeys.
    pub fn unregister(&self, x: &dyn XServer) {
        if let Some(kc) = self.resolved_keycode() {
            let combos = modifier_combinations(self.config.modifiers);
            for &mods in &combos {
                let _ = x.ungrab_key(kc, mods);
            }
        }
        self.is_registered.store(false, Ordering::Release);
        self.available.store(false, Ordering::Release);
    }

    /// Check whether a KeyPress event matches this hotkey.
    pub fn matches_key_press(&self, keycode: u8, state: u16) -> bool {
        if !self.is_available() {
            return false;
        }
        let target_kc = self.resolved_keycode.load(Ordering::Acquire);
        if target_kc == 0 || target_kc != keycode {
            return false;
        }
        // Base modifiers must all be present in state
        (state & self.config.modifiers) == self.config.modifiers
    }

    /// Handle a potential KeyPress event. Returns true if hotkey matched and triggered stop.
    pub async fn handle_key_press(
        &self,
        keycode: u8,
        state: u16,
        ledger: &InputLedger,
        x: &dyn XServer,
    ) -> bool {
        if self.matches_key_press(keycode, state) {
            handle_hotkey_trigger(self.stop_latch.as_ref(), ledger, x).await;
            true
        } else {
            false
        }
    }

    fn record_error(&self, err: String) {
        let mut guard = match self.last_error.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = Some(err);
    }

    fn clear_error(&self) {
        let mut guard = match self.last_error.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = None;
    }
}

/// Spawn a background task monitoring incoming X11 events for Stop hotkey and physical input.
pub fn spawn_x11_listener(
    x: Arc<dyn XServer>,
    hotkey_mgr: Arc<HotkeyManager>,
    _stop_latch: Arc<dyn StopLatch>,
    ledger: Arc<InputLedger>,
    clock: Arc<dyn crate::x11::activity::ActivityClock>,
    filter: Arc<crate::x11::activity::DeviceFilter>,
    watchdog: crate::x11::watchdog::HeartbeatWatchdog,
) -> Option<tokio::task::JoinHandle<()>> {
    let handle = tokio::runtime::Handle::try_current().ok()?;
    Some(handle.spawn(async move {
        loop {
            watchdog.record_heartbeat();
            match x.poll_event() {
                Ok(Some(crate::x11::server::XEventKind::KeyPress(kc, state))) => {
                    hotkey_mgr
                        .handle_key_press(kc, state, &ledger, x.as_ref())
                        .await;
                }
                Ok(Some(crate::x11::server::XEventKind::RawDeviceInput(dev_id, src_id))) => {
                    filter.process_event_with_source(dev_id, src_id, clock.as_ref());
                }
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(e) => {
                    tracing::warn!("X11 event listener error: {e}");
                    clock.set_available(false);
                    hotkey_mgr.set_available(false);
                    break;
                }
            }
        }
    }))
}

/// Attach and initialize the X11 hotkey and input event listener.
pub fn attach_x11_listener(
    x: Arc<dyn XServer>,
    hotkey_mgr: &Arc<HotkeyManager>,
    stop_latch: &Arc<dyn StopLatch>,
    ledger: &Arc<InputLedger>,
    clock: &Arc<dyn crate::x11::activity::ActivityClock>,
) -> Option<tokio::task::JoinHandle<()>> {
    let _ = hotkey_mgr.register(x.as_ref());
    if let Ok(true) = x.is_xi2_available() {
        let _ = x.select_xi2_events();
        clock.set_available(true);
    }
    let filter = match x.query_xi_devices() {
        Ok(devs) => Arc::new(crate::x11::activity::DeviceFilter::from_devices(&devs)),
        Err(_) => Arc::new(crate::x11::activity::DeviceFilter::default()),
    };
    spawn_x11_listener(
        x,
        hotkey_mgr.clone(),
        stop_latch.clone(),
        ledger.clone(),
        clock.clone(),
        filter,
        crate::x11::watchdog::HeartbeatWatchdog::new(),
    )
}
