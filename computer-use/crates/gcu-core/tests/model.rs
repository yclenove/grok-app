//! Portable model and driver contract regression tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{driver::Cancel, fingerprint::*, model::*};

#[test]
fn fingerprint_identity_fields_change_hash_and_ignore_value() {
    let base = fingerprint(
        Role::Button,
        Some("Send"),
        Some("send"),
        Fingerprint(0),
        None,
    );
    for other in [
        fingerprint(Role::Link, Some("Send"), Some("send"), Fingerprint(0), None),
        fingerprint(
            Role::Button,
            Some("Save"),
            Some("send"),
            Fingerprint(0),
            None,
        ),
        fingerprint(
            Role::Button,
            Some("Send"),
            Some("save"),
            Fingerprint(0),
            None,
        ),
        fingerprint(
            Role::Button,
            Some("Send"),
            Some("send"),
            Fingerprint(1),
            None,
        ),
    ] {
        assert_ne!(base, other);
    }
    assert_ne!(
        fingerprint(Role::Button, None, None, Fingerprint(0), None),
        fingerprint(Role::Button, Some(""), None, Fingerprint(0), None)
    );
}

#[test]
fn fingerprint_frames_quantize_at_documented_pixel_boundary() {
    let rect = |x| {
        Some(Rect {
            x,
            y: 0.0,
            w: 100.0,
            h: 20.0,
        })
    };
    let hash = |x| fingerprint(Role::Button, None, None, Fingerprint(0), rect(x));
    assert_eq!(hash(0.0), hash(FRAME_QUANTUM_PX - 0.01));
    assert_ne!(hash(0.0), hash(FRAME_QUANTUM_PX));
    assert_ne!(hash(-0.01), hash(0.0));
}

#[test]
fn native_handle_downcast_preserves_shared_identity() {
    let handle = NativeHandle::new(String::from("private"));
    assert_eq!(handle.downcast_ref::<String>().unwrap(), "private");
    assert!(handle.downcast_ref::<u32>().is_none());
    assert!(handle.ptr_eq(&handle.clone()));
    assert!(!format!("{handle:?}").contains("private"));
}

#[tokio::test]
async fn cancellation_before_wait_is_observed_by_all_clones() {
    let cancel = Cancel::default();
    let clone = cancel.clone();
    cancel.cancel();
    clone.cancelled().await;
    assert!(clone.is_cancelled());
}

#[tokio::test]
async fn cancellation_during_wait_wakes_all_waiters() {
    let cancel = Cancel::default();
    let other = cancel.clone();
    let waiter = tokio::spawn(async move {
        other.cancelled().await;
    });
    tokio::task::yield_now().await;
    cancel.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn fingerprint_length_delimiting_and_ancestor_paths_are_deterministic() {
    let zero = Fingerprint(0);
    assert_ne!(
        fingerprint(Role::Button, Some("ab"), Some("c"), zero, None),
        fingerprint(Role::Button, Some("a"), Some("bc"), zero, None)
    );
    assert_ne!(
        ancestor_path(zero, Role::Group, None, None, 0),
        ancestor_path(zero, Role::Group, None, None, 1)
    );
    // Known identity vector: Button ordinal 8, two missing fields, zero path, no frame.
    assert_eq!(
        fingerprint(Role::Button, None, None, zero, None),
        Fingerprint(0xfe83cddcf3822a1d)
    );
}

#[test]
fn driver_errors_classify_uncertainty_without_guessing() {
    use gcu_core::driver::DriverError;
    for error in [
        DriverError::WindowGone,
        DriverError::StaleElement,
        DriverError::CapabilityUnavailable,
        DriverError::InvalidRequest,
        DriverError::Timeout {
            maybe_executed: false,
        },
        DriverError::Crashed {
            maybe_executed: false,
        },
    ] {
        assert!(!error.maybe_executed());
    }
    for error in [
        DriverError::Timeout {
            maybe_executed: true,
        },
        DriverError::Crashed {
            maybe_executed: true,
        },
        DriverError::Cancelled {
            maybe_executed: true,
        },
        DriverError::Native {
            message: "metadata".into(),
            maybe_executed: true,
        },
    ] {
        assert!(error.maybe_executed());
    }
}

#[test]
fn states_values_and_roles_serde_roundtrip_without_native_handles() {
    let states = States::ENABLED | States::PASSWORD | States::EDITABLE;
    let json = serde_json::to_string(&states).unwrap();
    assert_eq!(serde_json::from_str::<States>(&json).unwrap(), states);
    assert_eq!(
        serde_json::to_string(&Role::CheckBox).unwrap(),
        "\"check_box\""
    );
    let value = NodeValue::Range {
        value: 5.0,
        min: 0.0,
        max: 10.0,
    };
    assert_eq!(
        serde_json::from_str::<NodeValue>(&serde_json::to_string(&value).unwrap()).unwrap(),
        value
    );
}
