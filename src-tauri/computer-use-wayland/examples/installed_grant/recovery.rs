//! Same-process owned-VM recovery, explicitly requested through the original GTK UI.
//! Not an App/ACP/MCP authorization receipt or an automatic grant replacement.
use super::{run::emit, ui::Command};
use grok_computer_use_core::{
    adapter::{CaptureOptions, ComputerUseAdapter, DispatchRequest},
    session_grants::{AuthorizationTicket, SessionGrants},
};
use grok_computer_use_wayland::{PortalRegistry, PortalSelection};
use serde_json::json;
use std::{sync::mpsc, time::Duration};

pub struct Retired {
    pub ticket: AuthorizationTicket,
    pub selection: PortalSelection,
    pub requests: Vec<DispatchRequest>,
}

pub async fn request(queue: &mpsc::Sender<Command>, old: &Retired) -> Result<(), String> {
    let (armed, arm) = tokio::sync::oneshot::channel();
    let (clicked, click) = tokio::sync::oneshot::channel();
    queue
        .send(Command::ArmRecovery { armed, clicked })
        .map_err(|_| "GTK recovery queue ended")?;
    tokio::time::timeout(Duration::from_secs(2), arm)
        .await
        .map_err(|_| "GTK recovery arm deadline")?
        .map_err(|_| "GTK recovery arm dropped")?;
    emit(
        json!({"event":"RECOVERY_READY", "probePid":std::process::id(),
        "oldRun":old.ticket.run_id, "oldAttempt":old.ticket.attempt_id,
        "oldAuthorizationGeneration":old.ticket.generation,
        "oldOwnersJoined":true, "oldBrokerStopped":true, "automaticRegrant":false}),
    );
    let edges = tokio::time::timeout(Duration::from_secs(90), click)
        .await
        .map_err(|_| "no explicit owned UI recovery request")?
        .map_err(|_| "GTK recovery request dropped")?;
    if edges != [(1, true), (1, false)] {
        return Err(format!(
            "fresh consent intent lacks native GTK click edges: {edges:?}"
        ));
    }
    emit(
        json!({"event":"RECOVERY_REQUESTED", "probePid":std::process::id(),
        "gtkEdges":edges, "explicitUiIntent":true, "automaticRegrant":false}),
    );
    Ok(())
}

pub fn reject_old(
    registry: &PortalRegistry,
    grants: &SessionGrants,
    old: &Retired,
    fresh: &AuthorizationTicket,
    target: &str,
    stage: &str,
) -> Result<(), String> {
    if fresh.run_id == old.ticket.run_id
        || fresh.attempt_id == old.ticket.attempt_id
        || fresh.generation <= old.ticket.generation
        || fresh.same_preparation(&old.ticket)
        || grants.is_current(&old.ticket)
        || !grants.is_current(fresh)
        || !registry.target_alive(target)
        || old.requests.len() != 2
    {
        return Err("fresh authorization does not fence the original attempt".into());
    }
    let mut rejected = Vec::new();
    for old_request in &old.requests {
        let old_target = &old_request.target_id;
        if old_target == target
            || registry.target_alive(old_target)
            || registry
                .claim_target_for_run(&old.ticket.run_id, old_target)
                .is_ok()
            || registry
                .claim_target_for_run(&fresh.run_id, old_target)
                .is_ok()
            || registry
                .capture_for_run_at_generation(
                    &old.ticket.run_id,
                    old_target,
                    2,
                    CaptureOptions::model(true),
                )
                .is_ok()
            || registry
                .capture_for_run_at_generation(
                    &fresh.run_id,
                    old_target,
                    1,
                    CaptureOptions::model(true),
                )
                .is_ok()
            || registry.act(old_request).is_ok()
        {
            return Err("old target/snapshot/action resurrected during fresh grant".into());
        }
        rejected.push(json!({"run":old_request.run_id,"target":old_target,
            "snapshot":old_request.snapshot_id,"action":old_request.action}));
    }
    if registry.cancel(&old.selection).is_ok()
        || registry.forget(&old.selection).is_ok()
        || registry.state(&old.selection).is_ok()
        || !registry.target_alive(target)
    {
        return Err("stale selection was accepted or disrupted fresh grant".into());
    }
    emit(
        json!({"event":"STALE_GRANT_REJECTED", "stage":stage, "probePid":std::process::id(),
        "newRun":fresh.run_id,"newTarget":target,"oldRequests":rejected,
        "oldTicketCurrent":false,"oldSelectionRejected":true,"freshTargetAlive":true}),
    );
    Ok(())
}
