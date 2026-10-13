//! Managed-browser support is feature-specific, not the App deployment target.
//! Read the native product version; never trust an environment/model override.

pub const MACOS_MINIMUM_MAJOR: u16 = 14;
pub const MACOS_MINIMUM_ERROR: &str =
    "managed_browser_macos_requires_14: the managed browser requires macOS 14 or later; the App still supports macOS 11 or later";
pub const VERSION_UNAVAILABLE_ERROR: &str =
    "managed_browser_os_version_unavailable: could not verify the native macOS version; no managed browser was started";

pub fn check_macos_product_version(version: &str) -> Result<(), &'static str> {
    if version.is_empty() || version.len() > 32 {
        return Err(VERSION_UNAVAILABLE_ERROR);
    }
    let parts = version
        .split('.')
        .map(|part| {
            if part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return None;
            }
            part.parse::<u16>().ok()
        })
        .collect::<Option<Vec<_>>>()
        .filter(|parts| !parts.is_empty() && parts.len() <= 3)
        .ok_or(VERSION_UNAVAILABLE_ERROR)?;
    if parts[0] < MACOS_MINIMUM_MAJOR {
        return Err(MACOS_MINIMUM_ERROR);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn ensure_supported() -> Result<(), &'static str> {
    use std::ffi::CStr;
    let mut buffer = [0_u8; 64];
    let mut length = buffer.len();
    // SAFETY: native read-only sysctl, writable bounded storage and size_t;
    // no new-value buffer, shell, process, user profile or permission operation.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || length == 0 || length > buffer.len() {
        return Err(VERSION_UNAVAILABLE_ERROR);
    }
    let version = CStr::from_bytes_with_nul(&buffer[..length])
        .ok()
        .and_then(|value| value.to_str().ok())
        .ok_or(VERSION_UNAVAILABLE_ERROR)?;
    check_macos_product_version(version)
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_supported() -> Result<(), &'static str> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_app_supported_older_macs_for_managed_browser_only() {
        for version in ["11.0", "11.7.10", "12.7", "13.0", "13.99.99"] {
            assert_eq!(
                check_macos_product_version(version),
                Err(MACOS_MINIMUM_ERROR)
            );
        }
    }

    #[test]
    fn accepts_boundary_and_newer_product_versions_not_darwin_kernel_numbers() {
        for version in ["14", "14.0", "14.0.0", "14.7.1", "15.6", "26.0", "27.1"] {
            assert_eq!(check_macos_product_version(version), Ok(()));
        }
    }

    #[test]
    fn unreadable_or_ambiguous_version_fails_closed_not_as_a_repair_error() {
        for version in [
            "", "unknown", "14.x", "14.", ".14", "14..1", "14.0.0.0", " 14.0", "14.0\n", "14.0\0",
            "014.0", "14.00", "+14.0", "65536.0", "14.65536",
        ] {
            assert_eq!(
                check_macos_product_version(version),
                Err(VERSION_UNAVAILABLE_ERROR)
            );
        }
    }

    #[test]
    fn app_release_minimum_stays_11_and_policy_does_not_raise_it() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/build-release-config.mjs"),
        )
        .unwrap();
        assert!(script.contains("minimumSystemVersion: \"11.0\""));
        assert_eq!(MACOS_MINIMUM_MAJOR, 14);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_platforms_do_not_query_or_inherit_the_macos_floor() {
        assert_eq!(ensure_supported(), Ok(()));
    }
}
