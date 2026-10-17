//! App-side session open and revoke. The HTTP server stays in the engine.

use super::{now_ms, ComputerUseHost, Inner};
use gcu_host_protocol::inject::{http_entry, should_inject, InjectFacts};
use gcu_host_protocol::supervisor::Signal;

enum Opened {
    Ready,
    Down,
    Failed,
}

/// Issue or reuse the local endpoint. SSH, scheduled, remote IM, and a disabled
/// feature return None and do not call the engine.
pub(super) async fn prepare(
    host: &ComputerUseHost,
    session_id: &str,
    facts: InjectFacts,
) -> Option<serde_json::Value> {
    if !should_inject(host.status().await.enabled, facts) {
        return None;
    }
    if let Some(entry) = cached(host, session_id).await {
        return Some(entry);
    }
    match open_and_issue(host, session_id).await {
        Opened::Ready => cached(host, session_id).await,
        Opened::Failed => None,
        Opened::Down => {
            let wake = host.lifecycle.live_count().saturating_add(1);
            if host.set_sessions(wake).await.is_err() {
                return None;
            }
            if matches!(open_and_issue(host, session_id).await, Opened::Ready) {
                return cached(host, session_id).await;
            }
            let live = host.lifecycle.live_count();
            let _ = host.set_sessions(live).await;
            None
        }
    }
}

/// Revoke one session. A session this host never opened is a no-op.
pub(super) async fn revoke(host: &ComputerUseHost, session_id: &str) {
    if let Err(error) = host.lifecycle.close(session_id).await {
        tracing::debug!(%error, "computer use session close failed");
    }
    let mut inner = host.inner.lock().await;
    if let Some(control) = inner.control.as_mut() {
        let count = host.lifecycle.live_count();
        if let Err(error) = control.handle(Signal::Sessions(count), now_ms()).await {
            tracing::debug!(%error, "computer use session count update failed");
        }
    }
}

/// The caller already holds the host mutex. Never clears the Stop latch.
pub(super) async fn revoke_all_locked(
    inner: &mut Inner,
    lifecycle: &gcu_host_protocol::lifecycle::Lifecycle,
) {
    if let Err(error) = lifecycle.close_all().await {
        tracing::debug!(%error, "computer use sessions close failed");
    }
    if let Some(control) = inner.control.as_mut() {
        if let Err(error) = control.handle(Signal::Sessions(0), now_ms()).await {
            tracing::debug!(%error, "computer use session count update failed");
        }
    }
}

async fn cached(host: &ComputerUseHost, session_id: &str) -> Option<serde_json::Value> {
    host.lifecycle
        .current(session_id)
        .map(|issued| http_entry(&issued.mcp_url, &issued.token))
}

async fn open_and_issue(host: &ComputerUseHost, session_id: &str) -> Opened {
    let inner = host.inner.lock().await;
    if host.lifecycle.current(session_id).is_some() {
        return Opened::Ready;
    }
    let Some(_control) = inner.control.as_ref() else {
        return Opened::Failed;
    };
    match host.lifecycle.open(session_id).await {
        Ok(_) => {}
        Err(gcu_host_protocol::Error::Disconnected) => return Opened::Down,
        Err(error) => {
            tracing::debug!(error = %error, "computer use session open failed");
            return Opened::Failed;
        }
    };
    let count = host.lifecycle.live_count();
    drop(inner);
    if let Err(error) = host.set_sessions(count).await {
        tracing::debug!(error = %error, "computer use session count update failed");
    }
    Opened::Ready
}
