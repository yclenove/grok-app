//! App-side session open and revoke. The HTTP server stays in the engine.

use super::{now_ms, ComputerUseHost, Inner};
use gcu_host_protocol::inject::{http_entry, should_inject, InjectFacts, Issued};
use gcu_host_protocol::supervisor::{Error, Signal};

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
            let wake = host.inner.lock().await.book.live_count().saturating_add(1);
            if host.set_sessions(wake).await.is_err() {
                return None;
            }
            if matches!(open_and_issue(host, session_id).await, Opened::Ready) {
                return cached(host, session_id).await;
            }
            let live = host.inner.lock().await.book.live_count();
            let _ = host.set_sessions(live).await;
            None
        }
    }
}

/// Revoke one session. A session this host never opened is a no-op.
pub(super) async fn revoke(host: &ComputerUseHost, session_id: &str) {
    // Session close denies a pending card even when this host never issued a token.
    host.office().close_session(session_id);
    let mut inner = host.inner.lock().await;
    if inner.book.current(session_id).is_none() {
        return;
    }
    if let Some(control) = inner.control.as_ref() {
        if let Err(error) = control.close_endpoint(session_id).await {
            tracing::debug!(error = %error, "computer use session close failed");
        }
    }
    inner.book.revoke(session_id);
    let count = inner.book.live_count();
    drop(inner);
    if let Err(error) = host.set_sessions(count).await {
        tracing::debug!(error = %error, "computer use session count update failed");
    }
}

/// Close every live endpoint before the feature switch shuts the engine down.
/// The caller already holds the host mutex.
pub(super) async fn revoke_all_locked(inner: &mut Inner) {
    let ids = inner.book.live_ids();
    if let Some(control) = inner.control.as_ref() {
        for id in &ids {
            if let Err(error) = control.close_endpoint(id).await {
                tracing::debug!(error = %error, "computer use session close failed");
            }
        }
    }
    if let Some(control) = inner.control.as_mut() {
        if let Err(error) = control.handle(Signal::Sessions(0), now_ms()).await {
            tracing::debug!(error = %error, "computer use session count update failed");
        }
    }
    inner.book.revoke_all();
}

async fn cached(host: &ComputerUseHost, session_id: &str) -> Option<serde_json::Value> {
    let inner = host.inner.lock().await;
    inner
        .book
        .current(session_id)
        .map(|issued| http_entry(&issued.mcp_url, &issued.token))
}

async fn open_and_issue(host: &ComputerUseHost, session_id: &str) -> Opened {
    let mut inner = host.inner.lock().await;
    if inner.book.current(session_id).is_some() {
        return Opened::Ready;
    }
    let Some(control) = inner.control.as_ref() else {
        return Opened::Failed;
    };
    let opened = match control.open_local_endpoint(session_id).await {
        Ok(opened) => opened,
        Err(Error::Disconnected) => return Opened::Down,
        Err(error) => {
            tracing::debug!(error = %error, "computer use session open failed");
            return Opened::Failed;
        }
    };
    inner.book.issue(session_id, Issued::from(&opened));
    let count = inner.book.live_count();
    drop(inner);
    if let Err(error) = host.set_sessions(count).await {
        tracing::debug!(error = %error, "computer use session count update failed");
    }
    Opened::Ready
}
