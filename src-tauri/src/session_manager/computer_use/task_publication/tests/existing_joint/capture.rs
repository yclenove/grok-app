//! Real native screenshot permission failure, independent of semantic recovery.
use super::*;
mod success;

pub(super) fn is_scenario(value: &str) -> bool {
    matches!(
        value,
        "active-capture-permission-denied" | "active-capture-success"
    ) || is_late_scenario(value)
}

pub(super) fn is_late_scenario(value: &str) -> bool {
    matches!(
        value,
        "active-late-capture-pause"
            | "active-late-capture-chat-stop"
            | "active-late-capture-early-continue"
            | "active-late-capture-early-stop"
    )
}

// Fixed capture scenarios share only original Host/ACP interruption and recovery.
pub(super) fn control_command(command: &str) -> Option<&'static str> {
    match command {
        "late-capture-held" => Some("late-action-held"),
        "late-capture-interrupt" => Some("late-action-interrupt"),
        "late-capture-state" => Some("late-action-state"),
        "late-capture-finish" => Some("late-action-finish"),
        "late-capture-replay" => Some("late-action-replay"),
        "late-capture-resume" => Some("late-action-resume"),
        _ => None,
    }
}

pub(super) fn validate_evidence(scenario: &str, value: &Value) -> Result<(), String> {
    if scenario == "active-capture-success" {
        return success::validate_evidence(value);
    }
    if is_late_scenario(scenario) {
        return validate_late_evidence(scenario, value);
    }
    for property in [
        "nativePermissionDenied",
        "captureJoined",
        "zeroImagePublication",
        "noAutomaticSemanticFallback",
        "originalAuthorityRetained",
        "explicitSemanticSucceeded",
        "originalDocumentsRetained",
        "threeSharesRetained",
        "originalStopped",
    ] {
        if value[property] != true {
            return Err("original native capture permission witness missing".into());
        }
    }
    for (property, count) in [
        ("reads", 3),
        ("inputs", 0),
        ("captures", 1),
        ("denials", 1),
        ("settled", 1),
        ("decodes", 0),
        ("results", 3),
        ("pairing", 1),
        ("disconnects", 0),
    ] {
        if value[property] != count {
            return Err("original native capture count changed".into());
        }
    }
    println!("original native capture permission witness: {value}");
    Ok(())
}

fn validate_late_evidence(scenario: &str, value: &Value) -> Result<(), String> {
    let chat_stop = scenario.ends_with("chat-stop");
    let early = scenario.contains("-early-");
    if early {
        for property in [
            "earlyContinueWhileHeld",
            "duplicateSharedReservation",
            "zeroNewWorkWhileHeld",
            "zeroPeerWireWhileHeld",
            "originalWorkerJoined",
            "originalWorkerRetired",
            "originalPauseWaiterJoined",
        ] {
            if value[property] != true {
                return Err("early Continue ownership witness missing".into());
            }
        }
        let stopped = scenario.ends_with("early-stop");
        if value["waitingStop"] != stopped
            || value["freshActions"] != usize::from(!stopped)
            || value["freshRecoveryReads"] != usize::from(!stopped)
            || value["continuationsDelivered"] != usize::from(!stopped)
            || (stopped && value["stopFencedBeforeRelease"] != true)
            || (!stopped && value["successorReadsAfterOriginalJoin"] != true)
        {
            return Err("early Continue publication or Stop witness missing".into());
        }
    }
    if value["interruption"] != if chat_stop { "chat-stop" } else { "pause" }
        || value["actionDriver"] != "owned-cdp-test-driver"
        || value["browserVersion"] != "Chrome/154.0.8037.92"
        || value["hostIdleBeforeRelease"] != false
        || value["hostCleanupPendingBeforeRelease"] != true
        || value["hostIdleAfterJoin"] != true
        || value["originalMcpError"] != true
        || value["originalMcpImage"] != false
        || value["originalMcpOldText"] != false
        || value["final"]["decodes"] != 0
        || value["final"]["observationReturned"] != false
        || value["final"]["completionImage"] != false
        || (!chat_stop && !scenario.ends_with("early-stop") && value["newExplicitContinue"] != true)
    {
        return Err("original late capture lifecycle witness missing".into());
    }
    for property in [
        "ownedActionGranted",
        "originalWaiterJoined",
        "oldDeliveryNotRevived",
        "originalDocumentsRetained",
        "threeSharesRetained",
        "zeroPeerWire",
        "originalStopped",
    ] {
        if value[property] != true {
            return Err("original late capture authority or owner changed".into());
        }
    }
    for property in [
        "documentMatched",
        "geometryMatched",
        "requestBound",
        "observePending",
        "ownerPending",
        "scopeMatched",
        "claimed",
        "originalWrappersRetained",
    ] {
        if value["held"][property] != true {
            return Err("original capture native cut not proved".into());
        }
    }
    for property in [
        "nativeInvokes",
        "nativeSettled",
        "matched",
        "captures",
        "settled",
    ] {
        if value["held"][property] != 1 || value["final"][property] != 1 {
            return Err("original native capture count changed".into());
        }
    }
    if value["held"]["decodes"] != 0
        || value["held"]["denials"] != 0
        || value["final"]["observeJoined"] != true
        || value["final"]["ownerJoined"] != true
        || value["final"]["ownerPending"] != false
        || value["final"]["settleInvokes"] != 1
        || value["final"]["settleJoined"] != true
        || value["final"]["settlePending"] != false
        || value["final"]["statusPending"] != 0
        || value["final"]["statusInvokes"].as_u64().unwrap_or(0) == 0
        || value["final"]["statusJoined"] != value["final"]["statusInvokes"]
        || value["final"]["returned"] != true
        || value["counts"]["inputs"] != 0
        || value["counts"]["pairing"] != 1
        || value["counts"]["disconnects"] != 0
    {
        return Err("original capture did not physically settle once".into());
    }
    println!("original late capture validated witness: {value}");
    Ok(())
}

fn late_checks(chat_stop: bool) -> Vec<&'static str> {
    let mut checks = CHECKS[..6].to_vec();
    checks.extend([
        "original-grant-fresh-observation-no-input",
        "original-capture-owned-action-grant",
        "original-capture-native-return-held",
        "original-capture-interrupted-without-authority",
        "original-capture-late-return-rejected-and-joined",
        "original-capture-old-delivery-and-shares-retained",
    ]);
    if !chat_stop {
        checks.push("original-capture-new-explicit-continue");
    }
    checks.push("original-capture-final-stop-keeps-shares");
    checks
}

fn early_checks(stopped: bool) -> Vec<&'static str> {
    let mut checks = CHECKS[..6].to_vec();
    checks.extend([
        "original-grant-fresh-observation-no-input",
        "original-capture-owned-action-grant",
        "original-capture-native-return-held",
        "original-capture-interrupted-without-authority",
        "early-continue-waits-for-original-capture",
        "duplicate-early-continue-shares-reservation",
    ]);
    if stopped {
        checks.push("stop-during-original-capture-recovery-wait");
    }
    checks.extend([
        "original-capture-late-return-rejected-and-joined",
        "early-continue-fresh-publication-or-terminal-stop",
        "original-capture-final-stop-keeps-shares",
    ]);
    checks
}

#[test]
#[ignore = "manual native gate: pinned CFT154 original PNG, one early Continue and duplicate"]
fn existing_task_joint_late_capture_early_continue() {
    scenario("active-late-capture-early-continue", &early_checks(false));
}

#[test]
#[ignore = "manual native gate: pinned CFT154 original PNG and Stop during early Continue wait"]
fn existing_task_joint_late_capture_early_stop() {
    scenario("active-late-capture-early-stop", &early_checks(true));
}

#[test]
#[ignore = "manual native gate: pinned CFT154 owned CDP action and original App screenshot/Pause"]
fn existing_task_joint_late_capture_pause() {
    scenario("active-late-capture-pause", &late_checks(false));
}

#[test]
#[ignore = "manual native gate: pinned CFT154 owned CDP action and original App screenshot/Chat Stop"]
fn existing_task_joint_late_capture_chat_stop() {
    scenario("active-late-capture-chat-stop", &late_checks(true));
}

#[test]
#[ignore = "manual native gate: original Chrome capture rejection, never fake pixels or expanded permissions"]
fn existing_task_joint_capture_permission_denied_keeps_original_authority() {
    let mut checks = CHECKS[..6].to_vec();
    checks.extend([
        "original-grant-fresh-observation-no-input",
        "original-native-capture-permission-denied-no-publication",
        "explicit-semantic-observation-reuses-original-authority",
        "capture-denied-stop-keeps-original-documents-and-shares",
    ]);
    scenario("active-capture-permission-denied", &checks);
}

pub(super) use success::preview;
