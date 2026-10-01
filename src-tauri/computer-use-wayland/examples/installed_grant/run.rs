use super::{recovery::Retired, ui::Command};
use grok_computer_use_core::{
    adapter::{ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest},
    broker::{BrokerOptions, ComputerUseBroker, StopState},
    execution::ActionCancellation,
    native_parent::{retain_parent_dispatch, NativeParent},
    owned::HostOwnedAdapter,
    protocol::{ActionKind, ActionTarget},
    session_grants::{AuthorizationTicket, SessionGrants},
};
use grok_computer_use_wayland::{
    PortalInputPolicy, PortalOptions, PortalRegistry, PortalSelection, PortalSelectionState,
    SourceKind,
};
use serde_json::json;
use std::{
    io::Write,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

struct ExplicitOwnedHost;
impl PortalInputPolicy for ExplicitOwnedHost {
    fn input_available(&self) -> bool {
        true
    }
    fn user_input_active(&self) -> bool {
        false
    }
}
pub(super) fn emit(value: serde_json::Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}
async fn snapshot(queue: &mpsc::Sender<Command>) -> Result<(bool, Vec<(u32, bool)>), String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    queue
        .send(Command::Snapshot(send))
        .map_err(|_| "GTK queue ended")?;
    tokio::time::timeout(Duration::from_secs(2), receive)
        .await
        .map_err(|_| "GTK snapshot deadline")?
        .map_err(|_| "GTK snapshot dropped".into())
}
async fn closed(
    registry: &PortalRegistry,
    selection: &PortalSelection,
) -> Result<PortalSelectionState, String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = registry.state(selection)?;
        match state {
            PortalSelectionState::Closed { .. } => return Ok(state),
            PortalSelectionState::Failed { error } => return Err(error),
            _ if Instant::now() >= deadline => {
                return Err(format!("original owners not joined: {state:?}"))
            }
            _ => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
}

pub async fn accept(queue: mpsc::Sender<Command>) -> Result<(), String> {
    let registry = Arc::new(PortalRegistry::new(Arc::new(ExplicitOwnedHost)));
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(HostOwnedAdapter::new(registry.clone())),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::path::Path::new("/run/user/1000")
                .join(format!("grok-owned-grant-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let grants = Arc::new(SessionGrants::default());
    let recovery = std::env::var("GROK_CU_RECOVERY_ACCEPTANCE").as_deref() == Ok("1");
    if recovery && std::env::var("GROK_CU_POINTER_ACCEPTANCE").as_deref() != Ok("1") {
        return Err("recovery requires both native pointer effects".into());
    }
    let first = round(&registry, &broker, &grants, &queue, 1, None).await?;
    if recovery {
        super::recovery::request(&queue, &first).await?;
        let second = round(&registry, &broker, &grants, &queue, 2, Some(&first)).await?;
        emit(
            json!({"event":"RECOVERY_COMPLETE", "probePid":std::process::id(),
            "oldRun":first.ticket.run_id,"newRun":second.ticket.run_id,
            "sameProcessRegistryBrokerGrantsGtk":true,"freshRecoveryVerified":true,
            "bothOriginalOwnersJoined":true,"automaticRegrant":false,
            "appAcpMcpVerified":false,"hostAuthorizationCommitted":false,
            "stockGnomeSupported":false,"atomicStopVerified":false}),
        );
    }
    Ok(())
}

async fn round(
    registry: &PortalRegistry,
    broker: &Arc<ComputerUseBroker>,
    grants: &Arc<SessionGrants>,
    queue: &mpsc::Sender<Command>,
    cycle: u64,
    previous: Option<&Retired>,
) -> Result<Retired, String> {
    emit(json!({"event":"CYCLE_BEGIN","cycle":cycle,"probePid":std::process::id()}));
    let ticket = grants.begin(broker, "owned-installed-grant", None)?;
    let queued = ticket.clone();
    let ui = queue.clone();
    let selection = registry.select_parented_gnome_for_authorization(
        PortalOptions::new(ticket.run_id.clone(), SourceKind::Monitor),
        1,
        1,
        &ticket,
        move || async move {
            let (reply, delivered) = tokio::sync::oneshot::channel();
            ui.send(Command::Export(queued, reply))
                .map_err(|_| "GTK export queue ended")?;
            retain_parent_dispatch(delivered)
                .await
                .map(|p| Box::new(p) as Box<dyn NativeParent>)
        },
    );
    let selection = match selection {
        Ok(selection) => selection,
        Err(error) => {
            revoke(broker, grants).await?;
            return Err(error);
        }
    };
    emit(
        json!({"event":"PICKER_PENDING", "probePid":std::process::id(), "run":ticket.run_id,
        "attempt":ticket.attempt_id,"authorizationGeneration":ticket.generation,"cycle":cycle}),
    );
    let result = exercise(
        registry, &selection, &ticket, grants, queue, cycle, previous,
    )
    .await;
    // Always retire the exact selection, even on denial, assertion, or timeout.
    // No fallback portal, replacement owner, or speculative second consent.
    let cleanup = async {
        registry.cancel(&selection)?;
        let terminal = closed(registry, &selection).await?;
        registry.forget(&selection)?;
        Ok::<_, String>(terminal)
    }
    .await;
    // Always fence the same Host attempt, including denial and exercise failure.
    // Keep cleanup errors; a fresh attempt may not hide unfinished owners.
    let fenced = revoke(broker, grants).await;
    let terminal = match (cleanup, fenced) {
        (Ok(state), Ok(())) => state,
        (cleanup, fenced) => {
            return Err(format!(
                "owned cleanup failed: {cleanup:?}; {fenced:?}; exercise={:?}",
                result.as_ref().err()
            ))
        }
    };
    if grants.is_current(&ticket)
        || broker
            .stop_state(&ticket.run_id)
            .map_err(|e| e.to_string())?
            != StopState::Stopped
    {
        return Err("original Host attempt not fenced/stopped".into());
    }
    let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
    emit(
        json!({"event":"ORIGINAL_OWNERS_JOINED", "state":format!("{terminal:?}"), "result":outcome,
        "cycle":cycle,"run":ticket.run_id,"hostAttemptFenced":true,"brokerStopped":true,
        "appAcpMcpVerified":false, "stockGnomeSupported":false, "atomicStopVerified":false}),
    );
    let requests = result?;
    emit(json!({"event":"CYCLE_END","cycle":cycle,"probePid":std::process::id()}));
    Ok(Retired {
        ticket,
        selection,
        requests,
    })
}

async fn revoke(
    broker: &Arc<ComputerUseBroker>,
    grants: &Arc<SessionGrants>,
) -> Result<(), String> {
    let broker = broker.clone();
    let grants = grants.clone();
    tokio::task::spawn_blocking(move || {
        grants.revoke_checked(&broker, "owned-installed-grant", || {})
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn exercise(
    registry: &PortalRegistry,
    selection: &PortalSelection,
    ticket: &AuthorizationTicket,
    grants: &SessionGrants,
    queue: &mpsc::Sender<Command>,
    cycle: u64,
    previous: Option<&Retired>,
) -> Result<Vec<DispatchRequest>, String> {
    let run = ticket.run_id.as_str();
    let deadline = Instant::now() + Duration::from_secs(95);
    let target = loop {
        match registry.state(selection)? {
            PortalSelectionState::Ready { target_id } => break target_id,
            PortalSelectionState::Pending if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            other => return Err(format!("consent failed: {other:?}")),
        }
    };
    if let Some(old) = previous {
        super::recovery::reject_old(registry, grants, old, ticket, &target, "before-fresh-input")?;
    }
    registry.claim_target_for_run(run, &target)?;
    let frame_deadline = Instant::now() + Duration::from_secs(8);
    let observation = loop {
        if let Ok(observation) =
            registry.capture_for_run_at_generation(run, &target, 1, CaptureOptions::model(true))
        {
            break observation;
        }
        if Instant::now() >= frame_deadline {
            return Err("no actual portal pixels".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let focus_deadline = Instant::now() + Duration::from_secs(3);
    while !snapshot(queue).await?.0 {
        if Instant::now() >= focus_deadline {
            return Err("owned GTK parent not focused after consent".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let before = snapshot(queue).await?.1;
    let request = DispatchRequest {
        managed_request: None,
        cancellation: ActionCancellation::default(),
        run_id: run.into(),
        action_id: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        target_id: target.clone(),
        target_generation: observation.target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Key,
        target: ActionTarget::Coord { x: 0.5, y: 0.5 },
        parameters: json!({"key":"enter"}),
        scope: ActionScope::Directed,
    };
    let submission = registry.act(&request)?;
    if !submission.applied {
        return Err(format!("Enter not submitted: {submission:?}"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let received = loop {
        let events = snapshot(queue).await?.1;
        if events.len() >= before.len() + 2 {
            break events;
        }
        if Instant::now() >= deadline {
            return Err("native GTK Enter receipt missing".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    if !received.starts_with(&before) || received[before.len()..] != [(65293, true), (65293, false)]
    {
        return Err(format!("unexpected native receipt: {received:?}"));
    }
    let mut requests = vec![request];
    if std::env::var("GROK_CU_POINTER_ACCEPTANCE").as_deref() == Ok("1") {
        requests.push(super::pointer::accept(registry, run, &target, queue, cycle - 1).await?);
    }
    if let Some(old) = previous {
        super::recovery::reject_old(registry, grants, old, ticket, &target, "after-fresh-input")?;
    }
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;
    let dbus = zbus::fdo::DBusProxy::new(&connection)
        .await
        .map_err(|e| e.to_string())?;
    let owner = dbus
        .get_name_owner(
            "org.gnome.Shell"
                .try_into()
                .map_err(|e: zbus::names::Error| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    let helper = zbus::Proxy::new(
        &connection,
        owner.as_str(),
        "/org/grok/ComputerUse/NativePolicy",
        "org.grok.ComputerUse.NativePolicy1",
    )
    .await
    .map_err(|e| e.to_string())?;
    let baseline: (u32, String, u32, bool) = helper
        .call("GetState", &())
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let settled: (u32, String, u32, bool) = helper
        .call("GetState", &())
        .await
        .map_err(|e| e.to_string())?;
    if baseline != settled || baseline.3 || !registry.target_alive(&target) {
        return Err("EI input falsely revoked original grant".into());
    }
    emit(json!({"event":"PORTAL_OBSERVATION", "observation":observation}));
    emit(
        json!({"event":"GRANT_READY", "target":target, "snapshot":observation.snapshot_id,
        "pixels":[observation.image.width,observation.image.height], "gtkReceipt":received[before.len()..],
        "gtkHistoryBefore":before,"gtkHistoryAfter":received,"helperBaseline":baseline,"cycle":cycle,
        "realPortalConsent":true, "productionRegistry":true, "experimentalObserver":true}),
    );
    // External controller may now deliver one actual physical key edge.
    // No further model/act/cancel call triggers retirement during this window.
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match registry.state(selection)? {
            PortalSelectionState::Ready { .. } if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            PortalSelectionState::Closing | PortalSelectionState::Closed { .. } => break,
            state => return Err(format!("no automatic policy revocation: {state:?}")),
        }
    }
    let terminal = closed(registry, selection).await?;
    let after: (u32, String, u32, bool) = helper
        .call("GetState", &())
        .await
        .map_err(|e| e.to_string())?;
    if after.0 != baseline.0 || after.1 != baseline.1 || after.2 <= baseline.2 || after.3 {
        return Err(format!(
            "not same-epoch physical takeover evidence: {baseline:?} -> {after:?}"
        ));
    }
    if registry.target_alive(&target)
        || registry.claim_target_for_run(run, &target).is_ok()
        || registry
            .capture_for_run_at_generation(run, &target, 2, CaptureOptions::model(true))
            .is_ok()
        || requests.iter().any(|request| registry.act(request).is_ok())
    {
        return Err("retired original target resurrected".into());
    }
    emit(
        json!({"event":"AUTOMATIC_REVOCATION", "terminal":format!("{terminal:?}"),
        "oldTargetRejected":true, "externalStopRequired":false, "freshRecoveryVerified":false, "helperAfter":after}),
    );
    Ok(requests)
}
