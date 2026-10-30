//! Private `CGWindowID` lookup for an accessibility element.
//!
//! Public Accessibility does not return a `CGWindowID`. `_AXUIElementGetWindow`
//! is the only mapping this driver accepts. It is off unless
//! `macos-private-spi` is compiled, `GCU_DISABLE_SKYLIGHT` is unset, and
//! `dlsym` finds the symbol. This module does not call SkyLight event-post
//! symbols and does not match a title or a frame.

use gcu_protocol::capabilities::PrivateApiStatus;

/// Capability value for `private_apis["ax_window"]`.
pub(crate) fn ax_window_capability(
    feature_on: bool,
    disabled: bool,
    symbols_present: bool,
) -> PrivateApiStatus {
    if !feature_on || disabled {
        PrivateApiStatus::Disabled
    } else if symbols_present {
        PrivateApiStatus::Available
    } else {
        PrivateApiStatus::Missing
    }
}

/// Runtime status. The environment is read on each call. The symbol is not called.
pub(crate) fn ax_window_status() -> PrivateApiStatus {
    ax_window_capability(
        cfg!(feature = "macos-private-spi"),
        std::env::var_os("GCU_DISABLE_SKYLIGHT").is_some(),
        symbol_present(),
    )
}

fn symbol_present() -> bool {
    #[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
    {
        lookup().is_some()
    }
    #[cfg(not(all(feature = "macos-private-spi", target_os = "macos")))]
    {
        false
    }
}

/// `CGWindowID` for a live accessibility element, or `None` when the gate is shut.
#[cfg(target_os = "macos")]
pub(crate) fn cg_id_from_ax(element: usize) -> Option<u32> {
    if element == 0 || ax_window_status() != PrivateApiStatus::Available {
        return None;
    }
    #[cfg(feature = "macos-private-spi")]
    {
        let function = lookup()?;
        let mut id = 0u32;
        // SAFETY: `element` is a live `AXUIElementRef` retained by the caller.
        // `function` is `_AXUIElementGetWindow`, resolved by `dlsym` and not null.
        // `id` is a writable out-parameter. The call does not release `element`.
        // A non-zero status or a zero id is not a window.
        let status = unsafe { function(element as _, &mut id) };
        if status != 0 || id == 0 {
            None
        } else {
            Some(id)
        }
    }
    #[cfg(not(feature = "macos-private-spi"))]
    {
        None
    }
}

#[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
type GetWindow = unsafe extern "C" fn(element: *mut std::ffi::c_void, id: *mut u32) -> i32;

#[cfg(all(feature = "macos-private-spi", target_os = "macos"))]
fn lookup() -> Option<GetWindow> {
    use std::sync::OnceLock;
    static SLOT: OnceLock<Option<GetWindow>> = OnceLock::new();
    *SLOT.get_or_init(|| {
        // SAFETY: the name is a NUL-terminated literal. `RTLD_DEFAULT` searches
        // already-loaded images. A null result means the symbol is absent and
        // is not called.
        let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"_AXUIElementGetWindow".as_ptr()) };
        if found.is_null() {
            None
        } else {
            // SAFETY: `dlsym` returned a non-null `_AXUIElementGetWindow`.
            // The platform type is `AXError(AXUIElementRef, CGWindowID *)`.
            // The caller passes a live element and a writable `u32`.
            Some(unsafe { std::mem::transmute::<*mut libc::c_void, GetWindow>(found) })
        }
    })
}
