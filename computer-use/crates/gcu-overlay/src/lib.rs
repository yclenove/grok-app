//! Standalone Host software cursor and status overlay.
//!
//! Open [`DesktopOverlay`] on the desktop/main thread; give its [`Overlay`]
//! handle to the Host as an `OverlayProvider`. Pump it independently of action
//! and approval queues. This crate never grants approval or registers MCP tools.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod animation;
pub mod availability;
mod controller;
mod copy;
mod desktop;
#[cfg(target_os = "macos")]
#[path = "macos/ffi.rs"]
mod native;
#[cfg(windows)]
#[path = "windows/ffi.rs"]
mod native;
#[cfg(target_os = "linux")]
#[path = "x11.rs"]
mod native;
mod scene;

pub use controller::{Clock, Config, MonotonicClock, Overlay};
pub use copy::{Catalog, CopyKey};
pub use desktop::{Controls, DesktopOverlay, Error};
pub use gcu_protocol::Rect;
pub use scene::{Button, ButtonAction, Cursor, Scene, State};

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
