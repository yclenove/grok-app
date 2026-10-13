//! Actual App Cancel/Stop or Drop while the original owned reservation is entered.
use super::*;
use grok_computer_use_core::{broker::HostTaskDecision, task_consent::HostRequestPhase};

pub(super) fn is_scenario(scenario: &str) -> bool {
    matches!(
        scenario,
        "reservation-picker-cancel" | "reservation-task-stop" | "reservation-waiter-drop"
    )
}

#[derive(Default)]
pub(super) struct ReservationTerminalProbe {
    completed: bool,
}

impl ReservationTerminalProbe {
    pub(super) fn reject(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        original: owner_replacement::OriginalOwner<'_>,
        picker: &str,
        reservation: &mut ReservationProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || self.completed {
            return Err("original reservation terminal misplaced".into());
        }
        if scenario.ends_with("picker-cancel") {
            if !original.manager.cancel_computer_use_task_picker(
                "view-app",
                original.key,
                picker,
            )? {
                return Err("original App reservation Cancel did not consume its picker".into());
            }
        } else if scenario.ends_with("task-stop") {
            original.manager.decide_computer_use_task(
                "view-app",
                original.key,
                HostTaskDecision::Stop,
            )?;
        } else {
            reservation.drop_and_join_original(runtime)?;
        }
        if scenario != "reservation-waiter-drop" {
            let result = reservation.release_and_join(runtime)?;
            if result.err().as_deref()
                != Some("Computer Use selected window instance retired or changed")
            {
                return Err("original reservation terminal returned an unfenced approval".into());
            }
        }
        self.completed = true;
        let mut reply = self.authority(original, reservation, approvals, scenario)?;
        reply["rejected"] = json!(true);
        reply["at"] = json!("original-owned-reservation");
        reply["exactRejection"] = json!("Computer Use selected window instance retired or changed");
        reply["outerDropped"] = json!(scenario == "reservation-waiter-drop");
        reply["originalInnerJoined"] = json!(true);
        Ok(reply)
    }

    pub(super) fn authority(
        &self,
        original: owner_replacement::OriginalOwner<'_>,
        reservation: &ReservationProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || !self.completed {
            return Err("original reservation terminal not complete".into());
        }
        let broker = original.fixture.broker();
        let state = original.manager.computer_use_task_view("view-app")?;
        let phase = if scenario.ends_with("task-stop") {
            HostRequestPhase::Cancelled
        } else {
            HostRequestPhase::Pending
        };
        if state
            .request
            .as_ref()
            .is_none_or(|request| request.key != *original.key || request.phase != phase)
            || state.feature_enabled
            || !state.grants.is_empty()
            || !state.executions.is_empty()
            || reservation.entry_count() != 1
            || approvals.started() != 0
            || approvals.owners() != 0
            || crate::computer_use::task_picker::discovery_owners_for_test() != 0
            || crate::computer_use::task_picker::reservation_owners_for_test() != 0
            || approvals.settings()["computerUseEnabled"] != false
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
            || !original.fixture.original_transport_is_current()
        {
            return Err(
                "original reservation terminal acquired authority or lost its owner".into(),
            );
        }
        Ok(
            json!({"state":state,"reservationEntries":reservation.entry_count(),"approvalCalls":0,"ownedWorkers":0,
            "discoveryWorkers":0,"reservationWorkers":0,"grants":0,"continuations":0,"stored":false}),
        )
    }
}

pub(super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    if evidence["inputRequests"] != 6
        || evidence["rejection"] != "feature_disabled"
        || evidence["outerDropped"] != (scenario == "reservation-waiter-drop")
        || evidence["originalInnerJoined"] != true
        || [
            "originalDocumentsRetained",
            "threeSharesRetained",
            "noRunCreated",
            "finalAuthorityZero",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original reservation terminal evidence incomplete".into());
    }
    println!("original reservation terminal authority witness: {evidence}");
    Ok(())
}

const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "terminal-original-call-fenced-and-joined",
    "terminal-zero-authority-shares-retained",
];

macro_rules! reservation_terminal_case {
    ($name:ident, $scenario:literal) => {
        #[test]
        #[ignore = "manual native gate: explicit CFT140+, original App/ACP/source TaskGrant"]
        fn $name() {
            scenario($scenario, CHECKS);
        }
    };
}
reservation_terminal_case!(
    existing_task_joint_reservation_terminal_picker_cancel,
    "reservation-picker-cancel"
);
reservation_terminal_case!(
    existing_task_joint_reservation_terminal_task_stop,
    "reservation-task-stop"
);
reservation_terminal_case!(
    existing_task_joint_reservation_terminal_waiter_drop,
    "reservation-waiter-drop"
);
