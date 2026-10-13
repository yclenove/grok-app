//! Actual terminal gestures before the unchanged original authority publication.
use super::*;
use grok_computer_use_core::{broker::HostTaskDecision, task_consent::HostRequestPhase};

pub(super) fn is_scenario(scenario: &str) -> bool {
    matches!(
        scenario,
        "publication-picker-cancel" | "publication-task-stop" | "publication-waiter-drop"
    )
}

#[derive(Default)]
pub(super) struct PublicationTerminalProbe {
    completed: bool,
}

impl PublicationTerminalProbe {
    pub(super) fn reject(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        original: owner_replacement::OriginalOwner<'_>,
        picker: &str,
        publication: &mut PublicationProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || self.completed {
            return Err("original publication terminal misplaced".into());
        }
        let dropped = scenario == "publication-waiter-drop";
        if scenario.ends_with("picker-cancel") {
            if !original.manager.cancel_computer_use_task_picker(
                "view-app",
                original.key,
                picker,
            )? {
                return Err("original App publication Cancel did not consume its picker".into());
            }
        } else if scenario.ends_with("task-stop") {
            original.manager.decide_computer_use_task(
                "view-app",
                original.key,
                HostTaskDecision::Stop,
            )?;
        }
        let (run, outcome) = if dropped {
            publication.drop_and_join_original(runtime, approvals)?
        } else {
            publication.release_and_join(runtime)?
        };
        if outcome.err().as_deref() != Some("Computer Use approval retired before publication") {
            return Err(
                "original publication terminal did not return its exact fenced rejection".into(),
            );
        }
        if run != publication.original_run()? {
            return Err("original publication terminal followed a replacement run".into());
        }
        self.completed = true;
        let mut reply = self.snapshot(original, publication, approvals, scenario)?;
        reply["rejected"] = json!(true);
        reply["at"] = json!("original-publication");
        reply["exactRejection"] = json!("Computer Use approval retired before publication");
        reply["outerDropped"] = json!(dropped);
        reply["originalInnerJoined"] = json!(true);
        Ok(reply)
    }

    fn snapshot(
        &self,
        original: owner_replacement::OriginalOwner<'_>,
        publication: &PublicationProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        if !is_scenario(scenario) || !self.completed {
            return Err("original publication terminal not complete".into());
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
            || publication.entry_count() != 1
            || approvals.started() != 1
            || approvals.owners() != 0
            || crate::computer_use::task_picker::discovery_owners_for_test() != 0
            || crate::computer_use::task_picker::reservation_owners_for_test() != 0
            || approvals.settings()["computerUseEnabled"] != false
            || broker.executable_grant_count("view-app") != 0
            || broker.continuation_count("view-app") != 0
            || !original.fixture.original_transport_is_current()
        {
            return Err(
                "original publication terminal retained authority or changed its owner".into(),
            );
        }
        let run = publication.original_run()?;
        let stopped =
            broker.stop_state(run).map_err(|error| error.to_string())? == StopState::Stopped;
        Ok(
            json!({"state":state,"runId":run,"originalStopped":stopped,"publicationEntries":1,
            "approvalCalls":1,"ownedWorkers":0,"discoveryWorkers":0,"reservationWorkers":0,
            "grants":0,"continuations":0,"stored":false}),
        )
    }

    pub(super) fn authority(
        &self,
        original: owner_replacement::OriginalOwner<'_>,
        publication: &PublicationProbe,
        approvals: &FixtureApprovals,
        scenario: &str,
    ) -> Result<Value, String> {
        let reply = self.snapshot(original, publication, approvals, scenario)?;
        if reply["originalStopped"] != true {
            return Err("original publication execution has not actually stopped".into());
        }
        Ok(reply)
    }
}

pub(super) fn validate_evidence(scenario: &str, evidence: &Value) -> Result<(), String> {
    if evidence["inputRequests"] != 6
        || evidence["rejection"] != "feature_disabled"
        || evidence["outerDropped"] != (scenario == "publication-waiter-drop")
        || [
            "originalInnerJoined",
            "originalRunStopped",
            "originalExecutionPollObserved",
            "forbiddenObservationPollWaited",
            "originalDocumentsRetained",
            "threeSharesRetained",
            "finalAuthorityZero",
        ]
        .iter()
        .any(|field| evidence[field] != true)
    {
        return Err("original publication terminal evidence incomplete".into());
    }
    println!("original publication terminal authority witness: {evidence}");
    Ok(())
}

pub(super) fn validate_connection(connection: &Value) -> Result<(), String> {
    if connection["pairingConfirmed"] != 1
        || connection["http"].as_array().is_none_or(Vec::is_empty)
    {
        return Err("original passive connection evidence did not observe the real pair".into());
    }
    println!("original passive publication connection witness: {connection}");
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

macro_rules! publication_terminal_case {
    ($name:ident, $scenario:literal) => {
        #[test]
        #[ignore = "manual native gate: explicit CFT140+, original App/ACP/source TaskGrant"]
        fn $name() {
            scenario($scenario, CHECKS);
        }
    };
}
publication_terminal_case!(
    existing_task_joint_publication_terminal_picker_cancel,
    "publication-picker-cancel"
);
publication_terminal_case!(
    existing_task_joint_publication_terminal_task_stop,
    "publication-task-stop"
);
publication_terminal_case!(
    existing_task_joint_publication_terminal_waiter_drop,
    "publication-waiter-drop"
);
