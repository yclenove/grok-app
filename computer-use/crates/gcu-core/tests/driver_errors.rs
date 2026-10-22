//! `DriverError` to protocol error mapping and `maybe_executed` semantics.

use gcu_core::driver::{DiscoveryStatus, DriverError};
use gcu_protocol::ErrorCode;

#[test]
fn every_driver_error_maps_to_a_protocol_code() {
    let cases = [
        (
            DriverError::Timeout {
                maybe_executed: true,
            },
            ErrorCode::Timeout,
        ),
        (
            DriverError::Crashed {
                maybe_executed: false,
            },
            ErrorCode::DriverError,
        ),
        (
            DriverError::Cancelled {
                maybe_executed: false,
            },
            ErrorCode::StoppedByUser,
        ),
        (DriverError::AppNotFound, ErrorCode::AppNotFound),
        (DriverError::AmbiguousApp, ErrorCode::AmbiguousApp),
        (DriverError::WindowGone, ErrorCode::WindowGone),
        (DriverError::StaleElement, ErrorCode::StaleElement),
        (
            DriverError::CapabilityUnavailable,
            ErrorCode::CapabilityUnavailable,
        ),
        (
            DriverError::ElementNotActionable,
            ErrorCode::ElementNotActionable,
        ),
        (
            DriverError::NotSettable {
                maybe_executed: false,
            },
            ErrorCode::NotSettable,
        ),
        (
            DriverError::NotSettable {
                maybe_executed: true,
            },
            ErrorCode::NotSettable,
        ),
        (DriverError::OutOfBounds, ErrorCode::OutOfBounds),
        (
            DriverError::InvalidRequest,
            ErrorCode::CapabilityUnavailable,
        ),
        (DriverError::ElevationRequired, ErrorCode::ElevationRequired),
        (
            DriverError::PermissionMissing {
                permission: "accessibility".into(),
            },
            ErrorCode::PermissionMissing,
        ),
        (DriverError::SecureDesktop, ErrorCode::HandoffRequired),
        (DriverError::WindowMinimized, ErrorCode::WindowMinimized),
        (
            DriverError::Native {
                message: "boom".into(),
                maybe_executed: true,
            },
            ErrorCode::DriverError,
        ),
        (DriverError::ScopeDenied, ErrorCode::AppDenied),
    ];
    for (error, code) in cases {
        assert_eq!(error.error_code(), code, "{error}");
    }
}

#[test]
fn gate_errors_prove_zero_effects() {
    for error in [
        DriverError::ElevationRequired,
        DriverError::PermissionMissing {
            permission: "screen_recording".into(),
        },
        DriverError::SecureDesktop,
        DriverError::WindowMinimized,
        DriverError::ScopeDenied,
    ] {
        assert!(
            !error.maybe_executed(),
            "{error} must not claim a possible effect"
        );
    }
    assert!(!DriverError::NotSettable {
        maybe_executed: false
    }
    .maybe_executed());
    assert!(DriverError::NotSettable {
        maybe_executed: true
    }
    .maybe_executed());
}

#[test]
fn discovery_defaults_to_complete() {
    assert!(!DiscoveryStatus::default().indexing);
}
