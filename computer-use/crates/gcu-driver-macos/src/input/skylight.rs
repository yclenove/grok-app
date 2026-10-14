//! SkyLight symbol probe. Feature `macos-private-spi`, default on.
//!
//! ADR 0006 tried `SLEventPostToPid` and `SLPSPostEventRecordTo` (including
//! focus-without-raise) against Chromium on macOS 27. None of them delivered a
//! background click. This module only reports whether those symbols resolve.
//! It does not call them and it does not make a Chromium target background-capable.
//! `GCU_DISABLE_SKYLIGHT` forces [`PrivateApiStatus::Disabled`] without a panic.

use gcu_protocol::capabilities::PrivateApiStatus;

/// `SLEventPostToPid`.
#[cfg_attr(
    not(all(feature = "macos-private-spi", target_os = "macos")),
    allow(dead_code)
)]
pub const SL_EVENT_POST_TO_PID: &str = "SLEventPostToPid";
/// `SLPSPostEventRecordTo`.
#[cfg_attr(
    not(all(feature = "macos-private-spi", target_os = "macos")),
    allow(dead_code)
)]
pub const SLPS_POST_EVENT_RECORD_TO: &str = "SLPSPostEventRecordTo";

/// Map a probe to the capability-table value.
pub fn classify(feature_on: bool, disabled: bool, symbols_present: bool) -> PrivateApiStatus {
    if !feature_on || disabled {
        PrivateApiStatus::Disabled
    } else if symbols_present {
        PrivateApiStatus::Available
    } else {
        PrivateApiStatus::Missing
    }
}

/// Runtime status. On non-macOS hosts the symbols are missing when the feature
/// is compiled, and disabled when it is not.
pub fn status() -> PrivateApiStatus {
    let disabled = std::env::var_os("GCU_DISABLE_SKYLIGHT").is_some();
    classify(
        cfg!(feature = "macos-private-spi"),
        disabled,
        symbols_present(),
    )
}

fn symbols_present() -> bool {
    #[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
    {
        native_symbols()
    }
    #[cfg(not(all(feature = "macos-private-spi", target_os = "macos")))]
    {
        false
    }
}

#[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
fn native_symbols() -> bool {
    // SAFETY: the path is a NUL-terminated literal. A null handle means the
    // framework is absent; it is not dereferenced. RTLD_LAZY | RTLD_LOCAL
    // does not run static initializers beyond the system's own rules and does
    // not replace already-loaded symbols.
    let handle = unsafe {
        libc::dlopen(
            c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        )
    };
    if handle.is_null() {
        return false;
    }
    let present = symbol(handle, SL_EVENT_POST_TO_PID) && symbol(handle, SLPS_POST_EVENT_RECORD_TO);
    // SAFETY: `handle` came from dlopen in this function and is not null.
    // We do not call the looked-up symbols. dlclose drops this reference only.
    unsafe { libc::dlclose(handle) };
    present
}

#[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
fn symbol(handle: *mut libc::c_void, name: &str) -> bool {
    let Ok(c_name) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: `handle` is a live dlopen handle and `c_name` is NUL-terminated.
    // A null result means the symbol is absent. The pointer is not called.
    let found = unsafe { libc::dlsym(handle, c_name.as_ptr()) };
    !found.is_null()
}
