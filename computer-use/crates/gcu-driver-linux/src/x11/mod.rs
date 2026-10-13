//! X11 protocol abstraction, window enumeration, and capture.
//!
//! Exposes the [`XServer`] trait with concrete [`RealX`] (backed by `x11rb`)
//! and [`FakeX`] (in-memory for platform-independent testing).

pub mod capture;
pub mod fake;
pub mod real;
pub mod server;
pub mod windows;

pub use capture::{
    capture_window, check_window_visibility, decide_capture_strategy, CaptureStrategy,
    WindowVisibility,
};
pub use fake::{FakeWindow, FakeX};
pub use real::RealX;
pub use server::{zpixmap_to_rgba, RawImage, XError, XRootGeometry, XServer, XWindowGeometry};
pub use windows::{align_windows, list_x11_windows, AtspiWindowCandidate, XWindowCandidate};
