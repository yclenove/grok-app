//! X11 protocol abstraction, window enumeration, and capture.
//!
//! Exposes the [`XServer`] trait with concrete [`RealX`] (backed by `x11rb`)
//! and [`FakeX`] (in-memory for platform-independent testing).

pub mod capture;
pub mod fake;
pub mod input;
pub mod keys;
pub mod ledger;
pub mod real;
pub mod server;
pub mod windows;

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
    char_to_keysym, key_to_keysym, modifier_to_keysym, named_key_to_keysym,
    resolve_or_remap_keysym, restore_remapped_keycode, ResolvedKeycode,
};
pub use ledger::InputLedger;
pub use real::RealX;
pub use server::{zpixmap_to_rgba, RawImage, XError, XRootGeometry, XServer, XWindowGeometry};
pub use windows::{align_windows, list_x11_windows, AtspiWindowCandidate, XWindowCandidate};
