//! Pure validation of the original peer's terminal report; no Host operations.
use super::*;

pub(super) fn validate(
    value: &Value,
    scenario: &str,
    checks: &[&str],
    active_lifecycle: bool,
    rejected: bool,
) -> Result<(), String> {
    let dormant_bootstrap = scenario == "startup-dormant";
    if value["checks"] != json!(checks)
        || value["scenario"] != scenario
        || value["legacyGrant"] != false
        || value["syntheticHostDecisions"] != true
        || value["syntheticAcpPeer"] != true
        || value["syntheticRequest"] != false
        || value["mcpRequests"] != if dormant_bootstrap { 0 } else { 1 }
        || (scenario != "success" && !dormant_bootstrap && !active_lifecycle && !rejected)
    {
        return Err("native joint matrix or evidence identity changed".into());
    }
    if preapproval_lease::is_scenario(scenario) {
        preapproval_lease::validate_evidence(&value["preapprovalLeaseEvidence"])?;
    }
    if terminals::is_scenario(scenario) {
        terminals::validate_evidence(scenario, &value["terminalEvidence"])?;
    }
    if reservation_terminals::is_scenario(scenario) {
        reservation_terminals::validate_evidence(scenario, &value["terminalEvidence"])?;
    }
    if publication_terminals::is_scenario(scenario) {
        publication_terminals::validate_evidence(scenario, &value["terminalEvidence"])?;
        publication_terminals::validate_connection(&value["connectionEvidence"])?;
    }
    if scenario == "active-chat-stop-queued" {
        chat_stop::validate_evidence(&value["chatStopEvidence"])?;
    }
    if scenario == "active-host-resume-concurrent" {
        recovery::validate_evidence(&value["recoveryRaceEvidence"])?;
    }
    if recovery::is_owner_scenario(scenario) {
        recovery::validate_owner_evidence(scenario, &value["recoveryOwnerEvidence"])?;
    }
    if late_actions::is_scenario(scenario) {
        late_actions::validate_evidence(scenario, &value["lateActionEvidence"])?;
    }
    if capture::is_scenario(scenario) {
        capture::validate_evidence(scenario, &value["captureEvidence"])?;
    }
    if matches!(
        scenario,
        "active-host-resume-pause" | "active-host-resume-chat-stop"
    ) {
        recovery::validate_terminal_evidence(scenario, &value["recoveryTerminalEvidence"])?;
    }
    if dormant_bootstrap {
        let evidence = &value["bootstrapEvidence"];
        for property in [
            "registrationMatched",
            "nativeStopped",
            "noRunningWorkerBeforePopup",
            "sourcePopupOpened",
            "nativeRunningAfterPopup",
            "originalVersionAndRegistrationRetained",
            "popupUnpaired",
            "browserStartupNotInvented",
            "noRequestOrAuthority",
        ] {
            if evidence[property] != true {
                return Err("native bootstrap physical witness missing".into());
            }
        }
        println!("native dormant source bootstrap witness: {evidence}");
    }
    Ok(())
}

pub(super) fn report_success(scenario: &str, checks: usize, dormant_bootstrap: bool) {
    if dormant_bootstrap {
        println!("existing native source bootstrap: {checks} checks; actual original registration/version stop and real popup wake; zero request/Pair/Share/authority; no installed App or Grok evidence");
    } else if capture::is_late_scenario(scenario) || scenario == "active-capture-success" {
        println!("existing new TaskGrant joint {scenario}: {checks} checks; actual first session MCP request/source MV3/authenticated HTTP/original App ACP/native PNG; owned CDP action driver; synthetic Host/ACP peer; no human toolbar, installed App or Grok evidence");
    } else {
        println!("existing new TaskGrant joint {scenario}: {checks} checks; actual first session MCP request/source MV3/authenticated HTTP/original App ACP; synthetic Host/ACP peer; no toolbar grant, installed App or Grok evidence");
    }
}
