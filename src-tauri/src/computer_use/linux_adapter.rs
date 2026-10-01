//! Linux desktop surface. The production X11 implementation is shared with
//! the owned native probe; no separate fixture-only input path lives here.

#![cfg(target_os = "linux")]

#[cfg(feature = "computer-use-probe")]
pub use grok_computer_use_x11::run_native_selftest;
pub use grok_computer_use_x11::LinuxAdapter;
