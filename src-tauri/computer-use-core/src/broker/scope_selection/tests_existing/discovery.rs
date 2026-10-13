//! Actual discover snapshot/recheck boundary; registry gestures are synthetic.
use super::*;
use crate::task_consent::HostRequestPhase;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn existing_discovery_unshare_after_original_snapshot_rejects_receipt_without_authority() {
    let f = Fixture::new();
    let entered = Arc::new(AtomicBool::new(false));
    let observed = entered.clone();
    let broker = f.broker.clone();
    let pairing = f.pairing.clone();
    let origin = f.origin.clone();
    f.broker
        .arm_scope_discovery_hook_for_test(Box::new(move |count| {
            assert_eq!(count, 1, "hook did not follow the original snapshot");
            observed.store(true, Ordering::SeqCst);
            assert!(!broker.feature_enabled());
            assert_eq!(broker.executable_grant_count("existing-app"), 0);
            assert_eq!(broker.continuation_count("existing-app"), 0);
            assert!(broker
                .tabs()
                .unoffer_connected_tab(
                    &origin,
                    &pairing.session_key,
                    &connection(&pairing),
                    2,
                    "101",
                    7
                )
                .unwrap());
        }));
    let call = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap();
    let result = call.discover();
    assert!(entered.load(Ordering::SeqCst));
    assert!(
        matches!(result, Err(BrokerError::Adapter(ref message)) if message == &BrokerError::TargetUnauthorized.to_string()),
        "must reject the original proof, not expire or panic: {:?}",
        result.as_ref().err()
    );
    assert_eq!(
        f.broker
            .host_task_view(&f.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    f.no_execution();
}
