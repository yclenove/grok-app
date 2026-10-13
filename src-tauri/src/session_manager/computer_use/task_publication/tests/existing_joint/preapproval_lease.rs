//! Default-off natural lease races; raw witnesses cannot drive maintenance.
use super::*;

const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-production-lease-retired-before-release",
    "original-lease-expiry-rejects-old-authority",
    "original-task-zero-authority-tabs-retained",
];

pub(super) fn is_scenario(scenario: &str) -> bool {
    matches!(
        scenario,
        "choose-lease-expiry" | "reservation-lease-expiry" | "publication-lease-expiry"
    )
}

pub(super) fn attach_lease(value: &mut Value, broker: &crate::computer_use::ComputerUseBroker) {
    let (stored, remaining, shares) = broker.tabs().stored_pairing_lease_for_test();
    value["leaseStored"] = json!(stored);
    value["leaseRemainingMs"] = json!(remaining.as_millis());
    value["leaseStoredShares"] = json!(shares);
    value["leaseMs"] = json!(grok_computer_use_core::pairing::CONNECTION_LEASE.as_millis());
}

pub(super) fn validate_evidence(value: &Value) -> Result<(), String> {
    let entry_window = value["entryWindowRemainingMs"].as_u64();
    let entered_remaining = value["enteredRemainingMs"].as_u64();
    let elapsed = value["pauseToRetirementMs"].as_u64();
    let initial = value["pausedRemainingMs"].as_u64();
    if !entry_window.is_some_and(|ms| (8000..=10000).contains(&ms))
        || !entered_remaining.is_some_and(|ms| (1..=10000).contains(&ms))
        || !initial.is_some_and(|ms| (1..=30000).contains(&ms))
        || !elapsed
            .zip(initial)
            .is_some_and(|(elapsed, initial)| elapsed >= initial && elapsed <= 32000)
        || value["enteredStored"] != true
        || value["retiredStored"] != false
        || value["retiredRemainingMs"] != 0
        || value["retiredShares"] != 0
        || value["retiredBeforeResume"] != true
        || value["disconnectRequestsBeforePause"] != 0
        || value["leaseMs"] != 30000
    {
        return Err("natural preapproval lease timing or ownership witness missing".into());
    }
    Ok(())
}

#[test]
#[ignore = "manual real 30-second lease: explicit Chrome-for-Testing 140+, unchanged product clock"]
fn existing_task_joint_lease_expiry_before_choice_rejects_original_candidates() {
    scenario("choose-lease-expiry", CHECKS);
}

#[test]
#[ignore = "manual real 30-second lease: explicit Chrome-for-Testing 140+, unchanged product clock"]
fn existing_task_joint_lease_expiry_during_reservation_rejects_original_worker() {
    scenario("reservation-lease-expiry", CHECKS);
}

#[test]
#[ignore = "manual real 30-second lease: explicit Chrome-for-Testing 140+, unchanged product clock"]
fn existing_task_joint_lease_expiry_before_publication_rejects_original_commit() {
    scenario("publication-lease-expiry", CHECKS);
}
