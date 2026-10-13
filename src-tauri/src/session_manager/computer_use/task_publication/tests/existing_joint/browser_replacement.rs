//! Full owned Chrome exit, natural Host lease expiry, and a new browser identity.
use super::scenario;

const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "new-task-grant-original-app-publication",
    "new-grant-unselected-member-c-has-no-access",
    "original-grant-fresh-observation-no-input",
    "entire-original-browser-exited-with-original-mcp-alive",
    "original-browser-exit-natural-lease-retires-shares",
    "replacement-browser-new-session-and-document",
    "old-grant-denied-before-and-after-replacement-share",
    "old-run-stop-retains-replacement-browser-shares-and-tabs",
];

#[test]
#[ignore = "manual real browser exit/30-second lease: explicit owned Chrome-for-Testing 140+"]
fn existing_task_joint_browser_reopen_cannot_revive_original_grant() {
    scenario("active-browser-reopen", CHECKS);
}

#[test]
#[ignore = "manual real profile replacement/30-second lease: explicit owned Chrome-for-Testing 140+"]
fn existing_task_joint_profile_replacement_cannot_revive_original_grant() {
    scenario("active-profile-replacement", CHECKS);
}

#[test]
#[ignore = "manual controlled tab-ID collision/real 30-second lease: explicit owned Chrome-for-Testing 140+"]
fn existing_task_joint_profile_tab_id_collision_cannot_revive_original_grant() {
    scenario("active-profile-tab-id-collision", CHECKS);
}
