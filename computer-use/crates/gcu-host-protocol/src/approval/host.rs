//! Host callbacks that wait for the approval desk instead of denying immediately.
use std::sync::Arc;

use serde_json::Value;

use crate::auth::ProcessInstance;
use crate::messages::{
    AppDecision, ConfirmAction, Confirmed, Decision, Handoff, Request, RequestApp, RpcError,
};
use crate::server::{CallbackFuture, HostCallbacks, Peer};

use super::{BindFault, Office, Outcome};

/// Callbacks for one supervised engine. The office is shared with the UI command.
pub fn host_callbacks(office: Office) -> Arc<dyn HostCallbacks> {
    Arc::new(ApprovalHost { office })
}

struct ApprovalHost {
    office: Office,
}

impl HostCallbacks for ApprovalHost {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        let office = self.office.clone();
        Box::pin(async move { dispatch(office, request).await })
    }

    fn attached(&self, engine: ProcessInstance, peer: Peer) {
        self.office.attach(engine, Some(peer));
    }

    fn disconnected(&self) {
        self.office.detach();
    }
}

async fn dispatch(office: Office, request: Request) -> Result<Value, RpcError> {
    match request.method.as_str() {
        "host.policy.load" => to_json(&office.policy()),
        "host.approval.requestApp" => request_app(office, request).await,
        "host.approval.confirmAction" => confirm_action(office, request).await,
        "host.approval.handoff" => handoff(office, request).await,
        "host.overlay.show"
        | "host.overlay.status"
        | "host.overlay.hide"
        | "host.event.emit"
        | "host.engine.health" => to_json(&crate::messages::Empty {}),
        _ => Err(RpcError::new(-32601, "method unavailable")),
    }
}

async fn request_app(office: Office, request: Request) -> Result<Value, RpcError> {
    let Some(request_id) = request.id else {
        return deny();
    };
    let params = match serde_json::from_value::<RequestApp>(request.params) {
        Ok(params) => params,
        Err(_) => return deny(),
    };
    let bound = office.bind(request_id, &params, wall_ms());
    let wait = match bound {
        Ok((_card, wait)) => wait,
        Err(BindFault::NoEngine | BindFault::Unverified | BindFault::Replay) => return deny(),
    };
    match wait.recv().await {
        Outcome::Decision(decision) => to_json(&AppDecision { decision }),
        Outcome::TimedOut => Err(RpcError::new(-32001, "approval_timeout")),
        Outcome::Confirmed { .. } | Outcome::HandedOff { .. } => deny(),
    }
}

async fn confirm_action(office: Office, request: Request) -> Result<Value, RpcError> {
    let Some(request_id) = request.id else {
        return confirmed(false);
    };
    let Ok(params) = serde_json::from_value::<ConfirmAction>(request.params) else {
        return confirmed(false);
    };
    let Ok((_card, wait)) = office.bind_confirm(request_id, &params, wall_ms()) else {
        return confirmed(false);
    };
    let allowed = matches!(wait.recv().await, Outcome::Confirmed { allowed: true, .. });
    confirmed(allowed)
}

async fn handoff(office: Office, request: Request) -> Result<Value, RpcError> {
    let Some(request_id) = request.id else {
        return to_json(&crate::messages::Empty {});
    };
    let Ok(params) = serde_json::from_value::<Handoff>(request.params) else {
        return to_json(&crate::messages::Empty {});
    };
    let Ok((_card, wait)) = office.bind_handoff(request_id, &params, wall_ms()) else {
        return to_json(&crate::messages::Empty {});
    };
    let _ = wait.recv().await;
    to_json(&crate::messages::Empty {})
}

fn confirmed(allowed: bool) -> Result<Value, RpcError> {
    to_json(&Confirmed { confirmed: allowed })
}

fn deny() -> Result<Value, RpcError> {
    to_json(&AppDecision {
        decision: Decision::Deny,
    })
}

fn to_json(value: &impl serde::Serialize) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|_| RpcError::new(-32603, "approval response"))
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
