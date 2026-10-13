//! Already-enabled Host: real re-Pair/Share cannot rebind a pending new TaskGrant.
use super::scenario;

const CHECKS: &[&str] = &[
    "request-enabled-no-model-authority",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "replacement-real-pair-and-shares-no-authority",
    "original-connection-repair-rejects-old-authority",
    "original-task-zero-authority-new-shares-retained",
];

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_repair_before_choice_rejects_original_candidates() {
    scenario("choose-connection-repair", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_repair_during_reservation_rejects_original_worker() {
    scenario("reservation-connection-repair", CHECKS);
}

#[test]
#[ignore = "manual native gate: explicit Chrome-for-Testing 140+, never the Chromium 130 managed seed"]
fn existing_task_joint_connection_repair_before_publication_rejects_original_commit() {
    scenario("publication-connection-repair", CHECKS);
}
