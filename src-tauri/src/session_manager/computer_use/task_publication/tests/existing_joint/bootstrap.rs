//! Native source startup only; no request, Pair/Share or authority is manufactured.
use super::scenario;

#[test]
#[ignore = "manual exact source worker stop/popup wake: explicit owned Chrome-for-Testing 140+"]
fn existing_task_joint_dormant_source_worker_wakes_from_real_popup_without_authority() {
    scenario(
        "startup-dormant",
        &[
            "native-source-registration-and-install-match",
            "native-original-worker-physically-stopped",
            "old-start-order-has-no-running-source-worker",
            "real-source-popup-wakes-worker-without-authority",
        ],
    );
}
