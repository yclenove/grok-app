//! Original App Cancel/Stop and the exact abandoned discovery registry handle.
use super::*;
use grok_computer_use_core::{broker::HostTaskDecision, task_consent::HostRequestPhase};
use std::sync::atomic::Ordering;

pub(super) fn is_scenario(scenario: &str) -> bool {
    matches!(
        scenario,
        "choose-picker-cancel"
            | "choose-task-stop"
            | "discovery-picker-cancel"
            | "discovery-task-stop"
            | "discovery-waiter-drop"
    )
}

pub(super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    if evidence["inputRequests"] != 6
        || evidence["rejection"] != "feature_disabled"
        || evidence["outerDropped"] != (scenario == "discovery-waiter-drop")
        || evidence["originalInnerJoined"] != scenario.starts_with("discovery-")
        || [
            "originalDocumentsRetained",
            "threeSharesRetained",
            "noRunCreated",
            "finalAuthorityZero",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original terminal evidence incomplete".into());
    }
    println!("original terminal authority witness: {evidence}");
    Ok(())
}

#[derive(Default)]
pub(super) struct TerminalProbe {
    reservation_entries: Option<Arc<AtomicUsize>>,
    completed: bool,
}

impl TerminalProbe {
    pub(super) fn arm(
        &mut self,
        broker: &crate::computer_use::ComputerUseBroker,
    ) -> Result<Value, String> {
        if self.reservation_entries.is_some() {
            return Err("original terminal reservation witness repeated".into());
        }
        let entries = Arc::new(AtomicUsize::new(0));
        let counter = entries.clone();
        broker.arm_scope_reservation_hook_for_test(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        self.reservation_entries = Some(entries);
        Ok(json!({"armed":true}))
    }

    pub(super) fn reject(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        original: owner_replacement::OriginalOwner<'_>,
        selection: (&str, Vec<String>),
        discovery: &mut DiscoveryProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || self.completed || self.reservation_entries.is_none() {
            return Err("original terminal cut unavailable".into());
        }
        if scenario.ends_with("picker-cancel") {
            if !original.manager.cancel_computer_use_task_picker(
                "view-app",
                original.key,
                selection.0,
            )? {
                return Err("original App picker Cancel did not consume the original slot".into());
            }
        } else if scenario.ends_with("task-stop") {
            original.manager.decide_computer_use_task(
                "view-app",
                original.key,
                HostTaskDecision::Stop,
            )?;
        } else {
            discovery.drop_and_join_original(runtime)?;
        }
        let discovering = scenario.starts_with("discovery-");
        if discovering && scenario != "discovery-waiter-drop" {
            let outcome = discovery.release_and_join(runtime)?;
            if outcome.err().as_deref() != Some("Computer Use scope picker unavailable or retired")
            {
                return Err("original terminal discovery returned an unfenced receipt".into());
            }
        }
        // Exercise the production App entry point, with the original receipt.
        let expected = if scenario.ends_with("picker-cancel") {
            "Computer Use original App picker owner changed"
        } else if discovering {
            "Computer Use picker not ready or retired"
        } else {
            "Computer Use picker selection unavailable or retired"
        };
        let outcome = runtime.block_on(original.manager.approve_computer_use_task_picker(
            "view-app",
            original.key,
            selection.0,
            selection.1,
        ));
        if outcome.err().as_deref() != Some(expected) {
            return Err("original terminal App approval rejection changed".into());
        }
        self.completed = true;
        let mut reply = self.authority(original, approvals, scenario)?;
        reply["rejected"] = json!(true);
        reply["at"] = json!(if discovering {
            "original-owned-discovery"
        } else {
            "original-app-picker"
        });
        reply["exactRejection"] = json!(expected);
        reply["outerDropped"] = json!(scenario == "discovery-waiter-drop");
        reply["originalInnerJoined"] = json!(discovering);
        Ok(reply)
    }

    pub(super) fn authority(
        &self,
        original: owner_replacement::OriginalOwner<'_>,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || !self.completed {
            return Err("original terminal authority witness not complete".into());
        }
        let state = original.manager.computer_use_task_view("view-app")?;
        let phase = if scenario.ends_with("task-stop") {
            HostRequestPhase::Cancelled
        } else {
            HostRequestPhase::Pending
        };
        let entries = self
            .reservation_entries
            .as_ref()
            .ok_or("original terminal entry witness missing")?
            .load(Ordering::SeqCst);
        let broker = original.fixture.broker();
        let discovery = crate::computer_use::task_picker::discovery_owners_for_test();
        let reservation = crate::computer_use::task_picker::reservation_owners_for_test();
        if state
            .request
            .as_ref()
            .is_none_or(|request| request.key != *original.key || request.phase != phase)
            || state.feature_enabled
            || !state.grants.is_empty()
            || !state.executions.is_empty()
            || entries != 0
            || approvals.started() != 0
            || approvals.owners() != 0
            || discovery != 0
            || reservation != 0
            || approvals.settings()["computerUseEnabled"] != false
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
            || !original.fixture.original_transport_is_current()
        {
            return Err("original terminal acquired authority or lost its original owner".into());
        }
        Ok(
            json!({"state":state,"reservationEntries":entries,"approvalCalls":0,
            "ownedWorkers":0,"discoveryWorkers":discovery,"reservationWorkers":reservation,
            "grants":0,"continuations":0,"stored":false}),
        )
    }
}

const CHECKS: &[&str] = &[
    "request-off-no-model-read",
    "real-mcp-first-request-no-authority",
    "original-app-confirm-required",
    "real-unshare-reshare-original-picker",
    "terminal-original-call-fenced-and-joined",
    "terminal-zero-authority-shares-retained",
];

macro_rules! terminal_case {
    ($name:ident, $scenario:literal) => {
        #[test]
        #[ignore = "manual native gate: explicit CFT140+, original App/ACP/source TaskGrant"]
        fn $name() {
            scenario($scenario, CHECKS);
        }
    };
}

terminal_case!(
    existing_task_joint_terminal_choose_picker_cancel,
    "choose-picker-cancel"
);
terminal_case!(
    existing_task_joint_terminal_choose_task_stop,
    "choose-task-stop"
);
terminal_case!(
    existing_task_joint_terminal_discovery_picker_cancel,
    "discovery-picker-cancel"
);
terminal_case!(
    existing_task_joint_terminal_discovery_task_stop,
    "discovery-task-stop"
);
terminal_case!(
    existing_task_joint_terminal_discovery_waiter_drop,
    "discovery-waiter-drop"
);
