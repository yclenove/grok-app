//! PrintWindow must not bypass a WGC exclusion that the affinity getter cannot read.
use super::*;
use gcu_driver_windows::capture::check_display_affinity;

#[test]
fn capture_protected_or_unknown_affinity_never_enables_print_window() {
    for path in [CapturePath::Wgc, CapturePath::PrintWindow] {
        assert_eq!(check_display_affinity(path, Some(0)), Ok(()));
        for affinity in [1, 0x11] {
            let error = check_display_affinity(path, Some(affinity)).unwrap_err();
            assert!(matches!(error, DriverError::PermissionMissing { .. }));
            assert!(!error.maybe_executed());
        }
    }
    assert_eq!(check_display_affinity(CapturePath::Wgc, None), Ok(()));
    assert_eq!(
        check_display_affinity(CapturePath::PrintWindow, None),
        Err(DriverError::PermissionMissing {
            permission: "window_capture_protection_unknown".into()
        })
    );
}

#[tokio::test]
async fn capture_failed_wgc_with_unqueryable_affinity_refuses_fallback_pixels() {
    let (wgc, print, backend) = sources();
    wgc.set_display_affinity(None);
    print.set_display_affinity(None);
    wgc.set_fault(CaptureFault::CallbackFailed);
    let error = take_capture(
        &backend,
        &source_window(),
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::PermissionMissing {
            permission: "window_capture_protection_unknown".into()
        }
    );
    assert_eq!(backend.stats().fallbacks, 1);
    assert_eq!(backend.stats().published, 0);
    assert_eq!(print.stats().published, 0);
}
