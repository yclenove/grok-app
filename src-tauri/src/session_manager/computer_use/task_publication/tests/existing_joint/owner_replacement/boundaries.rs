//! Original App receipt rejection and actual owned reserve retirement.
use super::*;

impl OwnerReplacementProbe {
    pub(in super::super) fn reject_choose(
        &self,
        runtime: &tokio::runtime::Runtime,
        original: OriginalOwner<'_>,
        selection: (&str, Vec<String>),
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !self.replaced || stage(scenario) != Some(ReplacementStage::Choose) {
            return Err("original choose owner rejection unavailable".into());
        }
        // The production App check is the subject: do not bypass it by calling
        // the ACP-only fixture constructor with a newly captured endpoint.
        let outcome = runtime.block_on(original.manager.approve_computer_use_task_picker(
            "view-app",
            original.key,
            selection.0,
            selection.1,
        ));
        if outcome.as_ref().err().map(String::as_str) != Some(rejection(scenario)?) {
            return Err("original App picker did not reject the exact retired owner".into());
        }
        let entries = self
            .choose_reservation_entries
            .as_ref()
            .ok_or("original choose reservation witness missing")?
            .load(std::sync::atomic::Ordering::SeqCst);
        let state = original.fixture.original_host_task_view()?;
        let broker = original.fixture.broker();
        if state.request.as_ref().map(|request| &request.key) != Some(original.key)
            || state.feature_enabled
            || !state.grants.is_empty()
            || !state.executions.is_empty()
            || entries != 0
            || approvals.started() != 0
            || approvals.owners() != 0
            || crate::computer_use::task_picker::reservation_owners_for_test() != 0
            || approvals.settings()["computerUseEnabled"] != false
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
        {
            return Err("old choose owner started work or acquired authority".into());
        }
        println!("original App choose rejection: exact_owner_rejection=true, core_reservation_entries={entries}, approval_calls=0, grants=0, continuations=0");
        Ok(
            json!({"rejected":true,"at":"original-app-picker","state":state,
            "ownedWorkers":0,"reservationWorkers":0,"reservationEntries":entries,
            "approvalCalls":0,"continuations":0,"stored":false,
            "ownerWitness":self.witness(original.fixture)?}),
        )
    }
}

const CHOOSE_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-owner-replaced-with-zero-authority",
    "choose-owner-replacement-rejects-original-picker",
    "original-task-zero-authority-user-shares-retained",
];

const RESERVATION_CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "original-owner-replaced-with-zero-authority",
    "reservation-owner-replacement-rejects-original-worker",
    "original-task-zero-authority-user-shares-retained",
];

macro_rules! original_owner_case {
    ($name:ident, $scenario:literal, $checks:ident) => {
        #[test]
        #[ignore = "manual native gate: explicit CFT140+, original App/ACP/source/new TaskGrant"]
        fn $name() {
            scenario($scenario, $checks);
        }
    };
}

original_owner_case!(
    existing_task_joint_boundary_owner_choose_process_rejects_original_app_picker,
    "choose-app-process-replaced",
    CHOOSE_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_choose_agent_rejects_original_app_picker,
    "choose-agent-replaced",
    CHOOSE_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_choose_acp_rejects_original_app_picker,
    "choose-acp-replaced",
    CHOOSE_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_choose_transport_rejects_original_app_picker,
    "choose-transport-replaced",
    CHOOSE_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_reservation_process_rejects_original_pin,
    "reservation-app-process-replaced",
    RESERVATION_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_reservation_agent_rejects_original_pin,
    "reservation-agent-replaced",
    RESERVATION_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_reservation_acp_rejects_original_pin,
    "reservation-acp-replaced",
    RESERVATION_CHECKS
);
original_owner_case!(
    existing_task_joint_boundary_owner_reservation_transport_rejects_before_constructor,
    "reservation-transport-replaced",
    RESERVATION_CHECKS
);
