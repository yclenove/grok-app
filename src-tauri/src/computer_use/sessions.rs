//! Host chat/run ownership and async target authorization lifecycle.
use super::ComputerUseBroker;
use super::StopCleanupTicket;
pub use grok_computer_use_core::session_grants::AuthorizationTicket;
pub use grok_computer_use_core::session_grants::McpDesiredState;
use grok_computer_use_core::session_grants::SessionGrants;
use parking_lot::Mutex;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::OnceLock;

/// A generation-bound, broker-owned cleanup hand-off.  Keeping the broker
/// Arc with the ticket prevents a later runtime replacement from cleaning a
/// different process's run.
#[derive(Clone)]
pub(crate) struct FencedCleanup {
    session: String,
    broker: Arc<ComputerUseBroker>,
    ticket: StopCleanupTicket,
    catalog_generation: u64,
}

pub(crate) struct FencedRevoke {
    pub matched: bool,
    pub cleanup: Option<FencedCleanup>,
    pub error: Option<String>,
}

pub(crate) struct FencedCancel {
    pub cancelled: bool,
    pub revoke: FencedRevoke,
}

fn grants() -> &'static SessionGrants {
    static GRANTS: OnceLock<SessionGrants> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

fn pending_cleanups() -> &'static Mutex<HashMap<String, FencedCleanup>> {
    static CLEANUPS: OnceLock<Mutex<HashMap<String, FencedCleanup>>> = OnceLock::new();
    CLEANUPS.get_or_init(Default::default)
}

fn same_cleanup(left: &FencedCleanup, right: &FencedCleanup) -> bool {
    left.catalog_generation == right.catalog_generation
        && left.session == right.session
        && left.ticket == right.ticket
        && Arc::ptr_eq(&left.broker, &right.broker)
}

fn remember_cleanup(session: &str, cleanup: FencedCleanup) -> FencedCleanup {
    pending_cleanups()
        .lock()
        .insert(session.to_string(), cleanup.clone());
    cleanup
}

fn clear_cleanup_if_current(session: &str, completed: &FencedCleanup) {
    let mut pending = pending_cleanups().lock();
    if pending
        .get(session)
        .is_some_and(|current| same_cleanup(current, completed))
    {
        pending.remove(session);
    }
}

pub(crate) fn pending_fenced_cleanup(
    session: &str,
    catalog_generation: u64,
) -> Option<FencedCleanup> {
    pending_cleanups()
        .lock()
        .get(session)
        .filter(|cleanup| cleanup.catalog_generation == catalog_generation)
        .cloned()
}

pub fn current(session: &str) -> Option<String> {
    grants().current(session)
}

pub fn tracked_sessions() -> Vec<String> {
    let mut sessions = grants().sessions().into_iter().collect::<BTreeSet<_>>();
    sessions.extend(pending_cleanups().lock().keys().cloned());
    sessions.into_iter().collect()
}

/// Drop process-local grant and cancellation bookkeeping after the App has
/// successfully cleaned up and durably deleted the session.
pub fn forget_session(session: &str) {
    grants().forget_session(session);
    pending_cleanups().lock().remove(session);
}

pub fn mcp_desired(session: &str) -> McpDesiredState {
    grants().mcp_desired(session)
}

pub fn resolve(session: Option<&str>, requested: Option<&str>) -> Result<String, String> {
    let session = session
        .filter(|s| !s.trim().is_empty())
        .ok_or("select a local chat first")?;
    let run = current(session).ok_or("no Computer Use run for this chat")?;
    if requested.is_some_and(|id| id != run) {
        return Err("run does not belong to this chat".into());
    }
    Ok(run)
}

pub fn begin(
    broker: &ComputerUseBroker,
    session: &str,
    requested: Option<&str>,
    attempt_id: &str,
    selector_revision: u64,
) -> Result<AuthorizationTicket, String> {
    grants().begin_attempt(broker, session, requested, attempt_id, selector_revision)
}

#[cfg(test)]
pub fn is_current(ticket: &AuthorizationTicket) -> bool {
    grants().is_current(ticket)
}

pub fn activate(broker: &ComputerUseBroker, ticket: &AuthorizationTicket) -> Result<(), String> {
    grants().publish(broker, ticket, false, || {
        super::set_session_enabled(&ticket.session, true)
    })?;
    if let Err(error) = grants().request_mcp_attach(ticket) {
        // Keep the product flag and token gate fail-closed if the attempt was
        // cancelled between publishing the enabled state and MCP declaration.
        super::set_session_enabled(&ticket.session, false);
        return Err(error);
    }
    Ok(())
}

pub fn complete(broker: &ComputerUseBroker, ticket: &AuthorizationTicket) -> Result<(), String> {
    grants().publish(broker, ticket, true, || {})
}

fn clear(session: &str) {
    if let Some(endpoint) = super::ipc::endpoint() {
        endpoint.revoke_session(session);
    }
    super::set_session_enabled(session, false);
}

#[cfg(test)]
pub fn fail_checked(
    broker: &ComputerUseBroker,
    ticket: &AuthorizationTicket,
) -> Result<(), String> {
    grants().fail_checked(broker, ticket, || clear(&ticket.session))
}

pub fn revoke(session: &str) {
    if let Some(broker) = super::global_broker() {
        grants().revoke(&broker, session, || clear(session));
    } else {
        clear(session);
    }
}

/// Fence authority and revoke the local IPC credential without performing
/// adapter/browser cleanup.  The returned hand-off may be completed from a
/// blocking worker after the UI has acknowledged Stop.
pub fn fence_revoke_checked(session: &str) -> FencedRevoke {
    if let Some(broker) = super::global_broker() {
        fence_revoke_checked_with_broker(broker, session)
    } else {
        clear(session);
        let desired = grants().mcp_desired(session);
        FencedRevoke {
            matched: desired.generation != 0,
            cleanup: pending_fenced_cleanup(session, desired.generation),
            error: None,
        }
    }
}

pub(crate) fn fence_revoke_checked_with_broker(
    broker: Arc<ComputerUseBroker>,
    session: &str,
) -> FencedRevoke {
    let fenced = grants().fence_revoke_checked(&broker, session, || clear(session));
    retain_fenced_cleanup(broker, session, fenced)
}

fn retain_fenced_cleanup(
    broker: Arc<ComputerUseBroker>,
    session: &str,
    fenced: grok_computer_use_core::session_grants::RevokeFence,
) -> FencedRevoke {
    let desired = grants().mcp_desired(session);
    let cleanup = match fenced.cleanup {
        Some(ticket) => Some(remember_cleanup(
            session,
            FencedCleanup {
                session: session.to_string(),
                broker,
                ticket,
                catalog_generation: desired.generation,
            },
        )),
        None => pending_fenced_cleanup(session, desired.generation),
    };
    FencedRevoke {
        matched: fenced.matched,
        cleanup,
        error: fenced.error,
    }
}

#[cfg(test)]
pub(crate) fn fence_revoke_checked_for_test(
    broker: Arc<ComputerUseBroker>,
    session: &str,
) -> FencedRevoke {
    fence_revoke_checked_with_broker(broker, session)
}

pub(crate) fn fence_failed_authorization(
    broker: Arc<ComputerUseBroker>,
    ticket: &AuthorizationTicket,
) -> FencedRevoke {
    let fenced = grants().fence_fail_checked(&broker, ticket, || clear(&ticket.session));
    retain_fenced_cleanup(broker, &ticket.session, fenced)
}

pub(crate) fn finish_fenced_cleanup_blocking(cleanup: FencedCleanup) -> Result<(), String> {
    cleanup
        .broker
        .finish_stop_cleanup(&cleanup.ticket)
        .map_err(|error| error.to_string())?;
    if cleanup
        .broker
        .stop_cleanup_pending(cleanup.ticket.run_id())
        .map_err(|error| error.to_string())?
    {
        Err("Computer Use surface cleanup is already in progress".into())
    } else {
        clear_cleanup_if_current(&cleanup.session, &cleanup);
        Ok(())
    }
}

pub fn revoke_checked(session: &str) -> Result<(), String> {
    let fenced = fence_revoke_checked(session);
    let mut errors = fenced.error.into_iter().collect::<Vec<_>>();
    if let Some(cleanup) = fenced.cleanup {
        if let Err(error) = finish_fenced_cleanup_blocking(cleanup) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(crate) fn fence_cancel_attempt_checked(
    broker: Arc<ComputerUseBroker>,
    session: &str,
    attempt_id: &str,
    selector_revision: u64,
) -> Result<FencedCancel, String> {
    let fenced = grants().fence_cancel_attempt_checked(
        &broker,
        session,
        attempt_id,
        selector_revision,
        || clear(session),
    )?;
    Ok(FencedCancel {
        cancelled: fenced.cancelled,
        revoke: retain_fenced_cleanup(broker, session, fenced.revoke),
    })
}

/// Fence every known session without performing adapter, browser-worker, or
/// filesystem cleanup. Pending cleanup hand-offs remain in the process-local
/// ledger for the lifecycle coordinator or final blocking shutdown phase.
pub fn fence_all_checked() -> Result<(), String> {
    let mut errors = Vec::new();
    for session in tracked_sessions() {
        if let Some(error) = fence_revoke_checked(&session).error {
            errors.push(format!("{session}: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn revoke_all_checked() -> Result<(), String> {
    let mut errors = Vec::new();
    for session in tracked_sessions() {
        if let Err(error) = revoke_checked(&session) {
            errors.push(format!("{session}: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
