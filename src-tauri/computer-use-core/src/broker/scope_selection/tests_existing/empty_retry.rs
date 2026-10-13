//! Empty retry transfers ownership, never recreates a task or native receipt.
use super::*;
use crate::broker::{HostScopePicker, HostTaskDecision};

const DOCUMENT: &str = "0123456789abcdef0123456789abcdef";

fn empty(f: &Fixture) -> HostScopePicker {
    f.broker
        .tabs()
        .unoffer_connected_tab(
            &f.origin,
            &f.pairing.session_key,
            &connection(&f.pairing),
            2,
            "101",
            7,
        )
        .unwrap();
    let receipt = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap()
        .discover()
        .unwrap();
    assert!(receipt.views().unwrap().is_empty());
    receipt
}

#[test]
fn empty_retry_preserves_original_permit_preparation_and_cancel_owner() {
    let f = Fixture::new();
    let receipt = empty(&f);
    let original_cancel = receipt.cancel_handle();
    let call = receipt.into_empty_retry().unwrap();
    assert!(call.is_current(), "transfer Drop fenced the same permit");
    assert!(call.pairing_challenge().unwrap().is_none());
    reoffer(&f, 3, Some(DOCUMENT)).unwrap();
    assert_eq!(call.existing_share_preparation().unwrap().available_tabs, 1);
    let refreshed = call.discover().unwrap();
    assert_eq!(refreshed.key(), &f.key);
    assert_eq!(refreshed.views().unwrap().len(), 1);
    f.no_execution();
    original_cancel.fence();
    assert!(!refreshed.is_current());
    assert!(refreshed.views().is_err());
    f.no_execution();
}

#[test]
fn dropping_empty_retry_retires_only_original_preparation() {
    let f = Fixture::new();
    let call = empty(&f).into_empty_retry().unwrap();
    drop(call);
    assert!(f.broker.inner.lock().existing_share_preparation.is_none());
    reoffer(&f, 3, Some(DOCUMENT)).unwrap();
    let replacement = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap();
    assert!(replacement.is_current());
    assert_eq!(
        replacement
            .existing_share_preparation()
            .unwrap()
            .available_tabs,
        1
    );
    f.no_execution();
}

#[test]
fn empty_retry_rejects_retired_owner_and_never_refreshes_nonempty_receipt() {
    for fence in ["cancel", "stop", "replace", "ttl"] {
        let f = Fixture::new();
        let receipt = empty(&f);
        let replacement = match fence {
            "cancel" => {
                receipt.cancel_handle().fence();
                None
            }
            "stop" => {
                f.broker
                    .decide_host_task(&f.token, &f.key, HostTaskDecision::Stop)
                    .unwrap();
                None
            }
            "replace" => Some(
                f.broker
                    .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
                    .unwrap(),
            ),
            _ => {
                f.broker.expire_existing_share_preparation_for_test();
                None
            }
        };
        assert!(receipt.into_empty_retry().is_err(), "{fence}");
        if let Some(call) = replacement {
            assert!(call.is_current(), "old retry retired replacement");
            assert!(call.existing_share_preparation().is_ok());
        }
        f.no_execution();
    }
    let f = Fixture::new();
    let receipt = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap()
        .discover()
        .unwrap();
    assert_eq!(receipt.views().unwrap().len(), 1);
    assert!(receipt.into_empty_retry().is_err());
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    f.no_execution();
}
