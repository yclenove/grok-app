//! Host callbacks for a supervised engine.
//!
//! The closed callback answers policy loads and denies every approval. It is
//! the production default until the App approval card is attached. The scripted
//! callback is for tests that supply one real decision per request.

use crate::{
    messages::{AppDecision, Confirmed, Decision, Policy, Request, RpcError},
    server::{CallbackFuture, HostCallbacks},
    Error,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// One engine request waiting for a test decision. Dropping it does not approve.
pub struct Observed {
    /// Authenticated request.
    pub request: Request,
    answer: Option<oneshot::Sender<Result<Value, RpcError>>>,
}

impl Observed {
    /// Send one response. A second call is ignored. Serialization failure denies.
    pub fn reply<T: Serialize>(mut self, value: T) {
        let Some(answer) = self.answer.take() else {
            return;
        };
        let result =
            serde_json::to_value(value).map_err(|_| RpcError::new(-32603, "invalid test response"));
        let _ = answer.send(result);
    }
}

struct Scripted {
    sender: mpsc::Sender<Observed>,
    policy: Arc<Mutex<Policy>>,
}

impl HostCallbacks for Scripted {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        let sender = self.sender.clone();
        let policy = self.policy.clone();
        Box::pin(async move {
            if request.method == "host.policy.load" {
                return serde_json::to_value(lock_policy(&policy))
                    .map_err(|_| RpcError::new(-32603, "policy"));
            }
            let (tx, rx) = oneshot::channel();
            let notification = request.id.is_none();
            sender
                .try_send(Observed {
                    request,
                    answer: if notification { None } else { Some(tx) },
                })
                .map_err(|_| RpcError::new(-32002, "test Host busy"))?;
            if notification {
                Ok(serde_json::json!({}))
            } else {
                rx.await
                    .unwrap_or_else(|_| Err(RpcError::new(-32000, "test Host disconnected")))
            }
        })
    }
    fn disconnected(&self) {}
}

fn lock_policy(policy: &Mutex<Policy>) -> Policy {
    policy
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

/// Test host that records requests and waits for [`Observed::reply`].
pub struct ScriptedHost {
    policy: Arc<Mutex<Policy>>,
    sender: mpsc::Sender<Observed>,
    requests: mpsc::Receiver<Observed>,
}

impl ScriptedHost {
    /// Queue of 32. A full queue fails the request instead of granting it.
    pub fn new(policy: Policy) -> Self {
        let (sender, requests) = mpsc::channel(32);
        Self {
            policy: Arc::new(Mutex::new(policy)),
            sender,
            requests,
        }
    }

    /// Callbacks to pass into [`super::session::EngineSession::spawn`].
    pub fn callbacks(&self) -> Arc<dyn HostCallbacks> {
        Arc::new(Scripted {
            sender: self.sender.clone(),
            policy: self.policy.clone(),
        })
    }

    /// Next request, or [`Error::Timeout`] after five seconds.
    ///
    /// Unexpected requests are not auto-approved. Notifications other than the
    /// requested method are discarded. A different request fails the wait.
    pub async fn next_method(&mut self, method: &str) -> Result<Observed, Error> {
        loop {
            let event = tokio::time::timeout(crate::server::RPC_TIMEOUT, self.requests.recv())
                .await
                .map_err(|_| Error::Timeout)?
                .ok_or(Error::Disconnected)?;
            if event.request.method == method {
                return Ok(event);
            }
            if event.request.id.is_some() {
                return Err(Error::Invalid);
            }
        }
    }
}

struct Closed {
    policy: Mutex<Policy>,
}

impl HostCallbacks for Closed {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        Box::pin(async move {
            match request.method.as_str() {
                "host.policy.load" => serde_json::to_value(lock_policy(&self.policy))
                    .map_err(|_| RpcError::new(-32603, "policy")),
                "host.approval.requestApp" => serde_json::to_value(AppDecision {
                    decision: Decision::Deny,
                })
                .map_err(|_| RpcError::new(-32603, "decision")),
                "host.approval.confirmAction" => {
                    serde_json::to_value(Confirmed { confirmed: false })
                        .map_err(|_| RpcError::new(-32603, "decision"))
                }
                "host.approval.handoff"
                | "host.overlay.show"
                | "host.overlay.status"
                | "host.overlay.hide"
                | "host.event.emit"
                | "host.engine.health" => Ok(serde_json::json!({})),
                _ => Err(RpcError::new(-32601, "method unavailable")),
            }
        })
    }
    fn disconnected(&self) {}
}

/// Production callbacks. Approvals deny, confirmation is false, policy is the snapshot.
pub fn closed_callbacks(policy: Policy) -> Arc<dyn HostCallbacks> {
    Arc::new(Closed {
        policy: Mutex::new(policy),
    })
}
