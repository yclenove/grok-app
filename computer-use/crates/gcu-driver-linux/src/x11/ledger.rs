//! Input accounting ledger tracking pressed keys and pointer buttons.
//!
//! Enforces press-before-send accounting to guarantee all held inputs
//! are reliably released upon cancellation, errors, stop requests, or crashes.

use crate::x11::server::XServer;
use std::sync::Mutex;
use tracing::{debug, warn};

/// Individual pressed input recorded by the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldInput {
    /// Keyboard keycode.
    Key(u8),
    /// Mouse button index.
    Button(u8),
}

/// Ledger tracking all AI-initiated key and button down states that have not yet been released.
#[derive(Debug, Default)]
pub struct InputLedger {
    held: Mutex<Vec<HeldInput>>,
}

impl InputLedger {
    /// Create a new empty input ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a key down event into the ledger BEFORE sending the hardware or protocol event.
    pub fn record_key_down(&self, keycode: u8) {
        let mut held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let item = HeldInput::Key(keycode);
        if !held.contains(&item) {
            held.push(item);
            debug!(keycode, "recorded key press in ledger");
        }
    }

    /// Record a key up event, removing the key from the ledger after sending.
    pub fn record_key_up(&self, keycode: u8) {
        let mut held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let item = HeldInput::Key(keycode);
        held.retain(|&k| k != item);
        debug!(keycode, "cleared key release from ledger");
    }

    /// Record a mouse button down event into the ledger BEFORE sending the protocol event.
    pub fn record_button_down(&self, button: u8) {
        let mut held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let item = HeldInput::Button(button);
        if !held.contains(&item) {
            held.push(item);
            debug!(button, "recorded button press in ledger");
        }
    }

    /// Record a mouse button up event, removing the button from the ledger after sending.
    pub fn record_button_up(&self, button: u8) {
        let mut held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let item = HeldInput::Button(button);
        held.retain(|&b| b != item);
        debug!(button, "cleared button release from ledger");
    }

    /// Check whether any keys or buttons are currently recorded as held down.
    pub fn is_empty(&self) -> bool {
        let held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        held.is_empty()
    }

    /// Return snapshots of held `(keys, buttons)`.
    pub fn held_inputs(&self) -> (Vec<u8>, Vec<u8>) {
        let held = match self.held.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut keys = Vec::new();
        let mut buttons = Vec::new();
        for item in held.iter() {
            match item {
                HeldInput::Key(k) => keys.push(*k),
                HeldInput::Button(b) => buttons.push(*b),
            }
        }
        (keys, buttons)
    }

    /// Release all held keys and buttons through the provided `XServer` in reverse order.
    ///
    /// This method is idempotent: repeated calls when the ledger is empty are no-ops.
    pub fn release_all(&self, x: &dyn XServer) {
        let items_to_release = {
            let mut held = match self.held.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            std::mem::take(&mut *held)
        };

        if items_to_release.is_empty() {
            return;
        }

        debug!(
            count = items_to_release.len(),
            "releasing all held inputs via ledger in reverse chronological order"
        );

        // Release in exact reverse chronological order of actuation.
        for item in items_to_release.into_iter().rev() {
            match item {
                HeldInput::Key(key) => {
                    if let Err(e) = x.xtest_fake_key(key, false) {
                        warn!(key, "failed to release held key during ledger cleanup: {e}");
                    }
                }
                HeldInput::Button(btn) => {
                    if let Err(e) = x.xtest_fake_button(btn, false) {
                        warn!(
                            btn,
                            "failed to release held button during ledger cleanup: {e}"
                        );
                    }
                }
            }
        }

        let _ = x.flush();
    }
}
