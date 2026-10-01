//! Session-scoped Computer Use MCP catalog reconciliation.
//!
//! The ACP catalog is a replacement document, not an append-only side effect.
//! Every caller (extension preferences, Computer Use attach, stop, reconnect)
//! therefore goes through the same per-session desired-state loop. Authority
//! is fenced by `computer_use::sessions` before this module is called.

use super::*;
use crate::acp_client::AcpClient;
use crate::computer_use::sessions;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const MCP_RECONCILE_TIMEOUT: Duration = Duration::from_secs(15);
const MCP_RECONCILE_MAX_ATTEMPTS: usize = 8;

type McpBaseBuilder = Arc<dyn Fn(Option<&str>) -> Value + Send + Sync>;
type McpEntryBuilder = Arc<dyn Fn(&str, &str) -> Result<Value, String> + Send + Sync>;

#[derive(Clone)]
struct McpReconcileDeps {
    timeout: Duration,
    build_base: McpBaseBuilder,
    build_entry: McpEntryBuilder,
}

impl Default for McpReconcileDeps {
    fn default() -> Self {
        Self {
            timeout: MCP_RECONCILE_TIMEOUT,
            build_base: Arc::new(crate::extensions::build_session_mcp_servers),
            build_entry: Arc::new(crate::computer_use::mcp_acp_entry),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpReconcileIntent {
    LatestDesired,
    CleanupOnly,
}

/// Redacted state exposed to diagnostics. It intentionally contains no MCP
/// command, environment, URL, token, or catalog contents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct McpCatalogStatus {
    pub desired_generation: u64,
    pub applied_generation: Option<u64>,
    /// Whether the latest desired catalog still contains the Computer Use
    /// entry. This distinguishes an attach that is pending from a detach that
    /// failed and needs retry.
    pub desired_present: bool,
    pub pending: bool,
    /// Native/browser cleanup is independent from the ACP catalog update.
    /// A successful desired-absent replacement must not hide a failed surface
    /// release from diagnostics or the explicit Retry action.
    pub resource_cleanup_pending: bool,
    pub last_error: Option<String>,
}

/// Redacted result of the bounded App-exit catalog barrier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct McpShutdownReport {
    pub attempted: usize,
    pub settled: usize,
    pub errors: Vec<String>,
}

/// The synchronous portion of a Computer Use Stop.  `desired` identifies the
/// exact absent catalog that was published by the fence; `cleanup` is bound to
/// the same Broker/run generation and may be completed on a blocking worker.
pub(crate) struct FencedComputerUseStop {
    pub desired: sessions::McpDesiredState,
    pub cleanup: Option<sessions::FencedCleanup>,
    pub fence_error: Option<String>,
}

/// Exact ACP incarnation detached by a Stop during the connection handshake.
/// Keeping this separate from `FencedComputerUseStop` prevents process
/// termination from being lost when there is no Computer Use run to clean up.
pub(crate) struct FencedAcpTermination {
    app_session_id: String,
    process_id: String,
    acp: Arc<AcpClient>,
}

impl FencedAcpTermination {
    pub(super) fn capture(session: &mut LiveSession) -> Option<Self> {
        session.acp.take().map(|acp| Self {
            app_session_id: session.app_session_id.clone(),
            process_id: session.process_id.clone(),
            acp,
        })
    }
}

impl FencedComputerUseStop {
    fn is_noop(&self) -> bool {
        self.desired.generation == 0 && self.cleanup.is_none() && self.fence_error.is_none()
    }
}

impl McpShutdownReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.settled == self.attempted
    }
}

#[derive(Clone)]
struct McpEndpoint {
    acp: Arc<AcpClient>,
    agent_session_id: String,
    process_id: String,
    cwd: Option<String>,
}

fn safe_error(error: impl AsRef<str>) -> String {
    let lower = error.as_ref().to_ascii_lowercase();
    if lower.contains("timed out") {
        "Computer Use MCP catalog update timed out".into()
    } else if lower.contains("not connected")
        || lower.contains("connection closed")
        || lower.contains("disconnected")
    {
        "Computer Use agent is not connected".into()
    } else if lower.contains("changed during reconciliation") {
        "Computer Use MCP catalog changed during reconciliation".into()
    } else {
        // ACP errors may contain the rejected catalog, command, or environment.
        // Keep the UI ledger useful without retaining any transport payload.
        "Computer Use MCP catalog update failed".into()
    }
}

fn safe_resource_cleanup_error(error: impl AsRef<str>) -> String {
    let lower = error.as_ref().to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "Computer Use surface cleanup timed out".into()
    } else {
        "Computer Use surface cleanup failed".into()
    }
}

impl SessionManager {
    /// Snapshot App session ids that can own an ACP MCP catalog. Do not use
    /// `agent_session_id` here: a shared ACP may host several App sessions and
    /// the grant registry is keyed by the App id.
    pub(crate) fn mcp_session_ids(&self) -> Vec<String> {
        let mut ids = std::collections::BTreeSet::new();
        if let Some(session) = self.inner.lock().as_ref() {
            if session.acp.as_ref().is_some_and(|client| client.is_alive()) {
                ids.insert(session.app_session_id.clone());
            }
        }
        ids.extend(
            self.background
                .lock()
                .iter()
                .filter(|(_, session)| session.acp.as_ref().is_some_and(|client| client.is_alive()))
                .map(|(id, _)| id.clone()),
        );
        ids.extend(
            self.parked
                .lock()
                .iter()
                .filter(|(_, session)| session.acp.is_alive())
                .map(|(id, _)| id.clone()),
        );
        ids.into_iter().collect()
    }

    fn mcp_catalog_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.mcp_catalog_locks.lock();
        Arc::clone(
            locks
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn computer_use_cleanup_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.computer_use_cleanup_locks.lock();
        Arc::clone(
            locks
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /// Advance the base catalog clock without allowing a wrapping generation.
    pub(super) fn note_mcp_base_changed(&self) -> u64 {
        loop {
            let current = self.mcp_catalog_revision.load(Ordering::SeqCst);
            let next = current
                .checked_add(1)
                .expect("Computer Use MCP catalog generation exhausted");
            if self
                .mcp_catalog_revision
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return next;
            }
        }
    }

    fn set_mcp_status(&self, session_id: &str, status: McpCatalogStatus) {
        self.mcp_catalog_status
            .lock()
            .insert(session_id.to_string(), status);
    }

    pub(crate) fn mcp_catalog_status(&self, session_id: &str) -> Option<McpCatalogStatus> {
        self.mcp_catalog_status.lock().get(session_id).cloned()
    }

    fn record_fenced_cleanup(
        &self,
        session_id: &str,
        desired: &sessions::McpDesiredState,
        resource_cleanup_pending: bool,
    ) {
        if desired.generation == 0 {
            return;
        }
        let settled = self.mcp_catalog_status(session_id).is_some_and(|status| {
            !status.pending
                && !status.desired_present
                && status.applied_generation == Some(desired.generation)
        });
        if !settled {
            self.mark_mcp_pending(session_id, desired);
        }
        if let Some(status) = self.mcp_catalog_status.lock().get_mut(session_id) {
            if status.desired_generation == desired.generation && !status.desired_present {
                status.resource_cleanup_pending = resource_cleanup_pending;
            }
        }
    }

    fn mark_resource_cleanup_result(
        &self,
        session_id: &str,
        desired_generation: u64,
        error: Option<&str>,
    ) {
        if let Some(status) = self.mcp_catalog_status.lock().get_mut(session_id) {
            if status.desired_generation != desired_generation || status.desired_present {
                return;
            }
            status.resource_cleanup_pending = error.is_some();
            if let Some(error) = error {
                status.last_error = Some(safe_resource_cleanup_error(error));
            } else if !status.pending {
                status.last_error = None;
            }
        }
    }

    /// Synchronous authority fence for Stop.  This intentionally performs no
    /// ACP, adapter, browser-worker, or filesystem work.  Errors are retained
    /// in the plan so callers can still acknowledge the local Stop and expose
    /// a retryable cleanup state.
    pub(crate) fn fence_computer_use_stop(&self, session_id: &str) -> FencedComputerUseStop {
        let fenced = sessions::fence_revoke_checked(session_id);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    /// Variant for lifecycle coordinators that already captured the exact
    /// process Broker. Cleanup remains bound to that Broker even if a test or
    /// recovery path replaces the global runtime slot later.
    pub(crate) fn fence_computer_use_stop_with_broker(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        session_id: &str,
    ) -> FencedComputerUseStop {
        let fenced = sessions::fence_revoke_checked_with_broker(broker, session_id);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    /// Fence compensation for one exact failed authorization. A stale failure
    /// cannot stop a newer attempt because the grant layer matches the full
    /// Host ticket before publishing desired-absent.
    pub(crate) fn fence_failed_computer_use_authorization(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        ticket: &sessions::AuthorizationTicket,
    ) -> FencedComputerUseStop {
        let session_id = ticket.session.as_str();
        let fenced = sessions::fence_failed_authorization(broker, ticket);
        self.fenced_computer_use_plan(session_id, fenced)
    }

    pub(crate) fn fence_cancelled_computer_use_authorization(
        &self,
        broker: Arc<crate::computer_use::ComputerUseBroker>,
        session_id: &str,
        attempt_id: &str,
        selector_revision: u64,
    ) -> Result<(bool, Option<FencedComputerUseStop>), String> {
        let fenced = sessions::fence_cancel_attempt_checked(
            broker,
            session_id,
            attempt_id,
            selector_revision,
        )?;
        if !fenced.cancelled {
            return Ok((false, None));
        }
        Ok((
            true,
            Some(self.fenced_computer_use_plan(session_id, fenced.revoke)),
        ))
    }

    fn fenced_computer_use_plan(
        &self,
        session_id: &str,
        fenced: sessions::FencedRevoke,
    ) -> FencedComputerUseStop {
        let sessions::FencedRevoke {
            matched,
            cleanup,
            error: fence_error,
        } = fenced;
        if !matched && cleanup.is_none() && fence_error.is_none() {
            return FencedComputerUseStop {
                desired: sessions::McpDesiredState {
                    generation: 0,
                    run_id: None,
                    attempt_generation: None,
                },
                cleanup: None,
                fence_error: None,
            };
        }
        let desired = sessions::mcp_desired(session_id);
        // Broker fencing can only fail before producing a cleanup ticket (for
        // example, an already-missing run). That is a terminal consistency
        // error, not a retryable resource operation. Never advertise a Retry
        // action that has no generation-bound hand-off to execute.
        let resource_cleanup_pending = cleanup.is_some()
            || sessions::pending_fenced_cleanup(session_id, desired.generation).is_some();
        self.record_fenced_cleanup(session_id, &desired, resource_cleanup_pending);
        FencedComputerUseStop {
            desired,
            cleanup,
            fence_error,
        }
    }

    /// Finish one fenced Stop after local acknowledgement.  The ACP catalog
    /// is reconciled only if the exact absent generation is still current; a
    /// newer authorization is never consumed by an older cleanup.  Surface
    /// cleanup runs behind `spawn_blocking` because managed workers and native
    /// adapters may perform bounded blocking I/O.
    pub(crate) async fn finish_fenced_computer_use_stop(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
    ) -> Result<(), String> {
        if plan.is_noop() {
            return Ok(());
        }
        let mut errors = Vec::new();
        if let Some(error) = plan.fence_error.as_ref() {
            errors.push(error.clone());
        }

        let current = sessions::mcp_desired(session_id);
        let detach = if current == plan.desired && plan.desired.run_id.is_none() {
            self.reconcile_session_mcp_scoped(
                session_id,
                McpReconcileDeps::default(),
                McpReconcileIntent::CleanupOnly,
            )
            .await
        } else if current != plan.desired {
            if current.run_id.is_some() {
                Err("Computer Use cleanup was superseded by a newer authorization".into())
            } else {
                // A newer absent generation owns the catalog now.  Do not
                // let this older Stop overwrite its status or send a stale
                // replacement.
                Ok(())
            }
        } else {
            Ok(())
        };
        if let Err(error) = detach {
            errors.push(format!("Computer Use cleanup pending: {error}"));
        }

        if let Some(cleanup) = plan.cleanup {
            let cleanup_lock = self.computer_use_cleanup_lock(session_id);
            let _serial = cleanup_lock.lock().await;
            let result = tauri::async_runtime::spawn_blocking(move || {
                sessions::finish_fenced_cleanup_blocking(cleanup)
            })
            .await
            .map_err(|error| error.to_string())?;
            if let Err(error) = result {
                self.mark_resource_cleanup_result(
                    session_id,
                    plan.desired.generation,
                    Some(&error),
                );
                errors.push(error);
            } else {
                self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
            }
        } else if plan.fence_error.is_none() {
            self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    async fn finish_fenced_computer_use_stop_task(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
        acp_termination: Option<FencedAcpTermination>,
    ) -> Result<(), String> {
        if let Some(termination) = acp_termination {
            debug_assert_eq!(termination.app_session_id, session_id);
            if self.has_other_process_tenant(&termination.process_id, &termination.app_session_id) {
                tracing::info!(
                    session = %termination.app_session_id,
                    process = %termination.process_id,
                    "handshake Stop detached a shared ACP tenant"
                );
            } else {
                // Termination is independent from catalog/surface cleanup. In
                // particular, a no-op plan or a failed cleanup must not leave
                // the cancelled handshake process alive.
                Self::kill_acp_bounded(&termination.acp).await;
                tracing::info!(
                    session = %termination.app_session_id,
                    process = %termination.process_id,
                    "handshake Stop terminated the captured ACP"
                );
            }
        }

        self.finish_fenced_computer_use_stop(session_id, plan).await
    }

    /// Schedule the slow half of a fenced Stop without holding up the Host
    /// command or session event loop. A handshake termination carries its
    /// exact detached ACP incarnation; delayed work never looks up the
    /// session's current endpoint.
    pub(crate) fn spawn_fenced_computer_use_stop(
        self: &Arc<Self>,
        session_id: String,
        plan: FencedComputerUseStop,
        acp_termination: Option<FencedAcpTermination>,
    ) {
        if plan.is_noop() && acp_termination.is_none() {
            return;
        }
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = manager
                .finish_fenced_computer_use_stop_task(&session_id, plan, acp_termination)
                .await;
            if let Err(error) = &result {
                tracing::warn!(
                    session = %session_id,
                    "Computer Use cleanup after local Stop is pending: {error}"
                );
            }
        });
    }

    /// Fence every Computer Use tenant owned by one exact ACP incarnation.
    /// This is deliberately synchronous and must run before the session maps
    /// discard the dead endpoint. The returned plans contain only the
    /// generation-bound surface cleanup hand-offs; catalog transport is known
    /// to be gone and is therefore marked settled without an ACP request.
    pub(crate) fn fence_computer_use_for_process_exit(
        &self,
        process_id: &str,
    ) -> Vec<(String, FencedComputerUseStop)> {
        self.fence_computer_use_for_process_exit_with(
            process_id,
            crate::computer_use::global_broker(),
        )
    }

    fn fence_computer_use_for_process_exit_with(
        &self,
        process_id: &str,
        broker: Option<Arc<crate::computer_use::ComputerUseBroker>>,
    ) -> Vec<(String, FencedComputerUseStop)> {
        let mut fenced_sessions = BTreeSet::new();
        let mut plans = Vec::new();
        {
            let live = self.inner.lock();
            if let Some(session) = live
                .as_ref()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        {
            let background = self.background.lock();
            for session in background
                .values()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        {
            let parked = self.parked.lock();
            for session in parked
                .values()
                .filter(|session| session.process_id == process_id)
            {
                self.fence_process_exit_session(
                    &session.app_session_id,
                    broker.as_ref(),
                    &mut fenced_sessions,
                    &mut plans,
                );
            }
        }
        plans
    }

    fn fence_process_exit_session(
        &self,
        session_id: &str,
        broker: Option<&Arc<crate::computer_use::ComputerUseBroker>>,
        fenced_sessions: &mut BTreeSet<String>,
        plans: &mut Vec<(String, FencedComputerUseStop)>,
    ) {
        if !fenced_sessions.insert(session_id.to_string()) {
            return;
        }
        let desired = sessions::mcp_desired(session_id);
        if desired.run_id.is_none()
            && desired.generation != 0
            && self.mcp_catalog_status(session_id).is_some_and(|status| {
                !status.pending
                    && !status.desired_present
                    && status.applied_generation == Some(desired.generation)
            })
        {
            // A duplicate ProcessExited notification must not replay a
            // retained surface cleanup hand-off.
            return;
        }
        let plan = match broker {
            Some(broker) => {
                self.fence_computer_use_stop_with_broker(Arc::clone(broker), session_id)
            }
            None => self.fence_computer_use_stop(session_id),
        };
        if plan.is_noop() {
            return;
        }
        self.mark_mcp_transport_gone(session_id, &plan);
        plans.push((session_id.to_string(), plan));
    }

    /// A dead ACP cannot receive a catalog replacement. Keep the desired
    /// absent generation authoritative so a future reconnect starts from the
    /// base catalog, while retaining any adapter/browser cleanup bit.
    fn mark_mcp_transport_gone(&self, session_id: &str, plan: &FencedComputerUseStop) {
        if plan.desired.generation == 0 {
            return;
        }
        let previous = self.mcp_catalog_status(session_id).unwrap_or_default();
        self.set_mcp_status(
            session_id,
            McpCatalogStatus {
                desired_generation: plan.desired.generation,
                applied_generation: Some(plan.desired.generation),
                desired_present: false,
                pending: false,
                resource_cleanup_pending: previous.resource_cleanup_pending
                    || plan.cleanup.is_some(),
                last_error: plan.fence_error.as_deref().map(safe_error),
            },
        );
    }

    async fn finish_process_exit_cleanup(
        &self,
        session_id: &str,
        plan: FencedComputerUseStop,
    ) -> Result<(), String> {
        let mut errors = plan.fence_error.into_iter().collect::<Vec<_>>();
        if let Some(cleanup) = plan.cleanup {
            let cleanup_lock = self.computer_use_cleanup_lock(session_id);
            let _serial = cleanup_lock.lock().await;
            let result = tauri::async_runtime::spawn_blocking(move || {
                sessions::finish_fenced_cleanup_blocking(cleanup)
            })
            .await
            .map_err(|error| error.to_string())?;
            if let Err(error) = result {
                self.mark_resource_cleanup_result(
                    session_id,
                    plan.desired.generation,
                    Some(&error),
                );
                errors.push(error);
            } else {
                self.mark_resource_cleanup_result(session_id, plan.desired.generation, None);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Finish only generation-bound adapter/browser cleanup after an ACP exit.
    /// No task in this path may attempt to address the dead ACP endpoint.
    pub(crate) fn spawn_process_exit_computer_use_cleanup(
        self: &Arc<Self>,
        cleanups: Vec<(String, FencedComputerUseStop)>,
    ) {
        for (session_id, plan) in cleanups {
            let manager = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = manager.finish_process_exit_cleanup(&session_id, plan).await {
                    tracing::warn!(
                        session = %session_id,
                        "Computer Use cleanup after ACP exit is pending: {error}"
                    );
                }
            });
        }
    }

    /// A fresh ACP endpoint needs the active CU entry, while a previously
    /// failed desired-absent update must be retried before its cleanup ledger
    /// can be cleared. A settled absent catalog is already supplied by the
    /// normal session-open base catalog and needs no extra update.
    pub(crate) fn computer_use_reconcile_needed_after_connect(&self, session_id: &str) -> bool {
        let desired = sessions::mcp_desired(session_id);
        desired.run_id.is_some()
            || self.mcp_catalog_status(session_id).is_some_and(|status| {
                status.pending || status.applied_generation != Some(desired.generation)
            })
    }

    /// Locate a session in live, background, or parked ownership. A parked
    /// agent is still a valid ACP endpoint for detach; it must not be treated
    /// as the currently focused chat by any other command.
    fn mcp_endpoint(&self, session_id: &str) -> Option<McpEndpoint> {
        if let Some(session) = self.inner.lock().as_ref() {
            if session.app_session_id == session_id {
                if let (Some(acp), Some(agent_session_id)) = (
                    session.acp.clone().filter(|client| client.is_alive()),
                    session.meta.agent_session_id.clone(),
                ) {
                    return Some(McpEndpoint {
                        acp,
                        agent_session_id,
                        process_id: session.process_id.clone(),
                        cwd: session.project_path.clone(),
                    });
                }
            }
        }
        if let Some(session) = self.background.lock().get(session_id) {
            if let (Some(acp), Some(agent_session_id)) = (
                session.acp.clone().filter(|client| client.is_alive()),
                session.meta.agent_session_id.clone(),
            ) {
                return Some(McpEndpoint {
                    acp,
                    agent_session_id,
                    process_id: session.process_id.clone(),
                    cwd: session.project_path.clone(),
                });
            }
        }
        if let Some(session) = self.parked.lock().get(session_id) {
            if session.acp.is_alive() {
                if let Some(agent_session_id) = session.meta.agent_session_id.clone() {
                    return Some(McpEndpoint {
                        acp: session.acp.clone(),
                        agent_session_id,
                        process_id: session.process_id.clone(),
                        cwd: session.project_path.clone(),
                    });
                }
            }
        }
        None
    }

    fn endpoint_is_current(&self, session_id: &str, expected: &McpEndpoint) -> bool {
        self.mcp_endpoint(session_id).is_some_and(|current| {
            current.agent_session_id == expected.agent_session_id
                && current.process_id == expected.process_id
                && Arc::ptr_eq(&current.acp, &expected.acp)
        })
    }

    fn strip_computer_use_entries(mut base: Value) -> Result<Value, String> {
        let entries = base.as_array_mut().ok_or("invalid MCP catalog")?;
        entries.retain(|entry| {
            entry.get("name").and_then(Value::as_str) != Some(crate::computer_use::MCP_SERVER_NAME)
        });
        Ok(base)
    }

    async fn build_catalog(
        session_id: &str,
        cwd: Option<String>,
        desired: &sessions::McpDesiredState,
        deps: &McpReconcileDeps,
    ) -> Result<Value, String> {
        let sid = session_id.to_string();
        let run_id = desired.run_id.clone();
        let build_base = Arc::clone(&deps.build_base);
        let base = tauri::async_runtime::spawn_blocking(move || build_base(cwd.as_deref()))
            .await
            .map_err(|error| error.to_string())?;
        let mut catalog = Self::strip_computer_use_entries(base)?;
        if let Some(run_id) = run_id {
            let entry = match (deps.build_entry)(&sid, &run_id) {
                Ok(entry) => entry,
                Err(error) => return Err(error),
            };
            catalog
                .as_array_mut()
                .ok_or("invalid MCP catalog")?
                .push(entry);
        }
        Ok(catalog)
    }

    fn mark_mcp_pending(&self, session_id: &str, desired: &sessions::McpDesiredState) {
        let previous = self.mcp_catalog_status(session_id).unwrap_or_default();
        self.set_mcp_status(
            session_id,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: previous.applied_generation,
                desired_present: desired.run_id.is_some(),
                pending: true,
                resource_cleanup_pending: previous.resource_cleanup_pending,
                last_error: None,
            },
        );
    }

    fn mark_mcp_error(&self, session_id: &str, desired: &sessions::McpDesiredState, error: &str) {
        let previous = self.mcp_catalog_status(session_id).unwrap_or_default();
        self.set_mcp_status(
            session_id,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: previous.applied_generation,
                desired_present: desired.run_id.is_some(),
                pending: true,
                resource_cleanup_pending: previous.resource_cleanup_pending,
                last_error: Some(safe_error(error)),
            },
        );
    }

    fn mark_mcp_applied(&self, session_id: &str, desired: &sessions::McpDesiredState) {
        let resource_cleanup_pending = self
            .mcp_catalog_status(session_id)
            .is_some_and(|status| status.resource_cleanup_pending);
        self.set_mcp_status(
            session_id,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: Some(desired.generation),
                desired_present: desired.run_id.is_some(),
                pending: false,
                resource_cleanup_pending,
                last_error: None,
            },
        );
    }

    /// Reconcile the current desired catalog for one App session.
    pub async fn reconcile_session_mcp(&self, session_id: &str) -> Result<(), String> {
        self.reconcile_session_mcp_with(session_id, McpReconcileDeps::default())
            .await
    }

    async fn reconcile_session_mcp_with(
        &self,
        session_id: &str,
        deps: McpReconcileDeps,
    ) -> Result<(), String> {
        self.reconcile_session_mcp_scoped(session_id, deps, McpReconcileIntent::LatestDesired)
            .await
    }

    async fn reconcile_session_mcp_scoped(
        &self,
        session_id: &str,
        deps: McpReconcileDeps,
        intent: McpReconcileIntent,
    ) -> Result<(), String> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err("select a local chat first".into());
        }
        let lock = self.mcp_catalog_lock(session_id);
        let _serial = lock.lock().await;

        for _attempt in 0..MCP_RECONCILE_MAX_ATTEMPTS {
            let desired = sessions::mcp_desired(session_id);
            if intent == McpReconcileIntent::CleanupOnly && desired.run_id.is_some() {
                return Err(
                    "Computer Use cleanup retry was superseded by a newer authorization".into(),
                );
            }
            if intent == McpReconcileIntent::CleanupOnly
                && self.mcp_catalog_status(session_id).is_some_and(|status| {
                    !status.pending
                        && !status.desired_present
                        && status.applied_generation == Some(desired.generation)
                })
            {
                return Ok(());
            }
            let base_revision = self.mcp_catalog_revision.load(Ordering::SeqCst);
            let endpoint = self.mcp_endpoint(session_id);

            // A stopped session may already have dropped its ACP process. There
            // is then no remote catalog left to detach; present desired state,
            // however, is an actionable lifecycle error.
            let Some(endpoint) = endpoint else {
                if desired.run_id.is_none() {
                    self.mark_mcp_applied(session_id, &desired);
                    return Ok(());
                }
                let error = "Computer Use agent is not connected";
                self.mark_mcp_error(session_id, &desired, error);
                return Err(error.into());
            };

            let catalog = match Self::build_catalog(
                session_id,
                endpoint.cwd.clone(),
                &desired,
                &deps,
            )
            .await
            {
                Ok(catalog) => catalog,
                Err(error) => {
                    // A stop can revoke the token while the base catalog is
                    // being built. Re-read ownership before surfacing an error.
                    if sessions::mcp_desired(session_id) != desired
                        || self.mcp_catalog_revision.load(Ordering::SeqCst) != base_revision
                    {
                        continue;
                    }
                    self.mark_mcp_error(session_id, &desired, &error);
                    return Err(error);
                }
            };

            // Do not send a stale replacement merely because another caller
            // changed desired state while the blocking builder was running.
            if sessions::mcp_desired(session_id) != desired
                || self.mcp_catalog_revision.load(Ordering::SeqCst) != base_revision
                || !self.endpoint_is_current(session_id, &endpoint)
            {
                continue;
            }

            self.mark_mcp_pending(session_id, &desired);
            let update = tokio::time::timeout(
                deps.timeout,
                endpoint
                    .acp
                    .update_mcp_servers(&endpoint.agent_session_id, catalog),
            )
            .await;

            // A timeout may mean ACP applied the replacement before its reply
            // was lost. Always compare the authoritative desired state first;
            // a retry is an idempotent full-catalog replacement, never an
            // action replay.
            let latest_desired = sessions::mcp_desired(session_id);
            let latest_base = self.mcp_catalog_revision.load(Ordering::SeqCst);
            let endpoint_current = self.endpoint_is_current(session_id, &endpoint);
            if latest_desired != desired || latest_base != base_revision || !endpoint_current {
                continue;
            }

            match update {
                Ok(Ok(_)) => {
                    self.mark_mcp_applied(session_id, &desired);
                    return Ok(());
                }
                Ok(Err(error)) => {
                    let error = safe_error(error);
                    self.mark_mcp_error(session_id, &desired, &error);
                    return Err(error);
                }
                Err(_) => {
                    let error = "Computer Use MCP catalog update timed out";
                    self.mark_mcp_error(session_id, &desired, error);
                    return Err(error.into());
                }
            }
        }

        let desired_generation = sessions::mcp_desired(session_id).generation;
        let error = "Computer Use MCP catalog changed during reconciliation";
        let desired = sessions::mcp_desired(session_id);
        debug_assert_eq!(desired_generation, desired.generation);
        self.mark_mcp_error(session_id, &desired, error);
        Err(error.into())
    }

    /// Retry only an already-recorded desired-absent cleanup. This never
    /// creates a run, selects a target, or follows a newer desired-present
    /// state that races with the retry.
    pub async fn retry_computer_use_cleanup(&self, session_id: &str) -> Result<(), String> {
        self.retry_computer_use_cleanup_with(session_id, McpReconcileDeps::default())
            .await
    }

    async fn retry_computer_use_cleanup_with(
        &self,
        session_id: &str,
        deps: McpReconcileDeps,
    ) -> Result<(), String> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err("select a local chat first".into());
        }
        let desired = sessions::mcp_desired(session_id);
        let status = self
            .mcp_catalog_status(session_id)
            .ok_or("No Computer Use cleanup is pending")?;
        if status.desired_generation != desired.generation {
            return Err("Computer Use cleanup was superseded by a newer lifecycle state".into());
        }
        if desired.run_id.is_some() || status.desired_present {
            return Err("Computer Use cleanup retry is only available after Stop".into());
        }
        if !status.pending
            && !status.resource_cleanup_pending
            && status.applied_generation != Some(desired.generation)
        {
            return Err("No Computer Use cleanup is pending".into());
        }
        if status.pending || status.applied_generation != Some(desired.generation) {
            self.reconcile_session_mcp_scoped(session_id, deps, McpReconcileIntent::CleanupOnly)
                .await?;
        }

        if status.resource_cleanup_pending {
            let cleanup_lock = self.computer_use_cleanup_lock(session_id);
            let _serial = cleanup_lock.lock().await;
            let latest_desired = sessions::mcp_desired(session_id);
            let latest_status = self
                .mcp_catalog_status(session_id)
                .ok_or("No Computer Use cleanup is pending")?;
            if !latest_status.resource_cleanup_pending {
                return Ok(());
            }
            if latest_desired != desired
                || latest_status.desired_generation != desired.generation
                || latest_status.desired_present
            {
                return Err(
                    "Computer Use cleanup was superseded by a newer lifecycle state".into(),
                );
            }
            let cleanup = sessions::pending_fenced_cleanup(session_id, desired.generation)
                .ok_or("Computer Use surface cleanup hand-off is unavailable")?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                sessions::finish_fenced_cleanup_blocking(cleanup)
            })
            .await
            .map_err(|error| error.to_string())?;
            match result {
                Ok(()) => self.mark_resource_cleanup_result(session_id, desired.generation, None),
                Err(error) => {
                    self.mark_resource_cleanup_result(session_id, desired.generation, Some(&error));
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// Attach only after the Host grant has declared desired=present. The
    /// connect lock keeps a concurrent session switch from swapping the ACP
    /// endpoint while the initial authorization is checked.
    pub async fn attach_computer_use(&self, session_id: &str, run_id: &str) -> Result<(), String> {
        let _connect = self
            .try_lock_connect(Duration::from_secs(10))
            .await
            .ok_or("chat is connecting")?;
        let desired = sessions::mcp_desired(session_id);
        if desired.run_id.as_deref() != Some(run_id) {
            return Err("Computer Use authorization is no longer current".into());
        }
        let (acp, agent_id) = self
            .with_session_mut(session_id, |session| {
                if Self::live_session_is_busy(session) {
                    return Err("wait for the current turn to finish");
                }
                let acp = session
                    .acp
                    .clone()
                    .filter(|a| a.is_alive())
                    .ok_or("connect this chat first")?;
                let agent_id = session
                    .meta
                    .agent_session_id
                    .clone()
                    .ok_or("chat has no agent session")?;
                Ok((acp, agent_id))
            })
            .ok_or("chat is not connected")??;
        if !acp.supports_local_computer_use().await {
            return Err("Computer Use requires a local interactive agent".into());
        }
        crate::computer_use::refuse_if_not_local(session_id)?;
        // Ensure the endpoint checked above is still the one being updated;
        // the reconciler repeats this check after every ACP response.
        if self
            .mcp_endpoint(session_id)
            .is_none_or(|endpoint| endpoint.agent_session_id != agent_id)
        {
            return Err("Computer Use agent session changed; authorize again".into());
        }
        self.reconcile_session_mcp(session_id).await
    }

    /// Desired-state detach used by Stop, feature-off, and cleanup paths.
    pub async fn detach_computer_use(&self, session_id: &str) -> Result<(), String> {
        self.reconcile_session_mcp(session_id).await
    }

    /// Revoke Host authority first, then replace the ACP catalog with the
    /// generic (Computer-Use-absent) catalog. Both operations are attempted so
    /// a failed stop request cannot skip MCP cleanup.
    pub async fn revoke_and_detach_computer_use(&self, session_id: &str) -> Result<(), String> {
        let plan = self.fence_computer_use_stop(session_id);
        self.finish_fenced_computer_use_stop(session_id, plan).await
    }

    /// Revoke and detach every live ACP-backed Computer Use session before a
    /// process-wide recycle (account/provider/data-root changes). This must
    /// run while the ownership maps still contain their endpoints: a shared
    /// ACP may continue serving other App chats after one tenant is drained.
    pub async fn revoke_and_detach_all_computer_use(&self) -> Vec<String> {
        let mut session_ids = self.mcp_session_ids().into_iter().collect::<BTreeSet<_>>();
        session_ids.extend(self.mcp_catalog_status.lock().keys().cloned());
        session_ids.extend(sessions::tracked_sessions());
        let mut errors = Vec::new();
        for session_id in session_ids {
            if let Err(error) = self.revoke_and_detach_computer_use(&session_id).await {
                errors.push(format!("{session_id}: {error}"));
            }
        }
        errors
    }

    /// Fence every process-local grant, then give all known ACP catalogs one
    /// bounded opportunity to converge to Computer-Use-absent before App exit.
    /// The timeout leaves each unfinished session marked pending; callers must
    /// still finish local resource shutdown and allow the process to exit.
    pub async fn shutdown_computer_use_catalogs(&self, budget: Duration) -> McpShutdownReport {
        let mut errors = Vec::new();
        if let Err(error) = sessions::fence_all_checked() {
            errors.push(format!("authority: {}", safe_error(error)));
        }

        let mut ids = BTreeSet::new();
        ids.extend(self.mcp_session_ids());
        ids.extend(self.mcp_catalog_status.lock().keys().cloned());
        ids.extend(sessions::tracked_sessions());
        let attempted = ids.len();
        let mut settled = 0usize;
        let mut completed = BTreeSet::new();
        let mut pending = FuturesUnordered::new();
        for session_id in ids.iter().cloned() {
            pending.push(async move {
                let result = self.detach_computer_use(&session_id).await;
                (session_id, result)
            });
        }

        let deadline = tokio::time::Instant::now() + budget;
        while !pending.is_empty() {
            match tokio::time::timeout_at(deadline, pending.next()).await {
                Ok(Some((session_id, Ok(())))) => {
                    completed.insert(session_id);
                    settled += 1;
                }
                Ok(Some((session_id, Err(error)))) => {
                    completed.insert(session_id.clone());
                    errors.push(format!("{session_id}: {}", safe_error(error)));
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        for session_id in ids.difference(&completed) {
            let desired = sessions::mcp_desired(session_id);
            let error = "Computer Use App-exit catalog cleanup timed out";
            self.mark_mcp_error(session_id, &desired, error);
            errors.push(format!("{session_id}: {error}"));
        }

        McpShutdownReport {
            attempted,
            settled,
            errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_client::AcpEvent;
    use crate::computer_use::ComputerUseBroker;
    use crate::journal_throttle::JournalWriteThrottle;
    use crate::permission::{PermissionPolicy, SessionAllowCache};
    use crate::session_fsm::SessionFsm;
    use crate::store::SessionMeta;
    use grok_computer_use_core::adapter::{
        AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, SurfaceKind,
        TargetInfo,
    };
    use grok_computer_use_core::broker::{BrokerOptions, StopState};
    use grok_computer_use_core::protocol::Observation;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::{mpsc, oneshot};

    const MCP_TEST_TARGET: &str = "mcp-test:window:1";

    #[derive(Default)]
    struct McpTestAdapter {
        aborts: AtomicU64,
        abort_blocked: AtomicBool,
        abort_failures: AtomicU64,
        dispatches: AtomicU64,
        releases: AtomicU64,
    }

    impl McpTestAdapter {
        fn fail_next_aborts(&self, count: u64) {
            self.abort_failures.store(count, Ordering::SeqCst);
        }

        fn set_abort_blocked(&self, blocked: bool) {
            self.abort_blocked.store(blocked, Ordering::SeqCst);
        }
    }

    impl ComputerUseAdapter for McpTestAdapter {
        fn backend_id(&self) -> &'static str {
            "mcp-test"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::for_surface(SurfaceKind::Desktop, self.backend_id())
        }

        fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
            Ok(vec![TargetInfo {
                target_id: MCP_TEST_TARGET.into(),
                title: "MCP reconciliation fixture".into(),
                app_name: "mcp-test".into(),
                kind: "window".into(),
                pid: Some(std::process::id()),
                backend: self.backend_id().into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "mcp-test-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "MCP reconciliation test target".into(),
            }])
        }

        fn target_alive(&self, target_id: &str) -> bool {
            target_id == MCP_TEST_TARGET
        }

        fn observe(&self, _target_id: &str) -> Result<Observation, String> {
            Err("unused in MCP reconciler test".into())
        }

        fn act(&self, _request: &DispatchRequest) -> Result<AdapterActResult, String> {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            Err("unused in MCP reconciler test".into())
        }

        fn abort(&self, _run_id: &str, _generation: u64) -> Result<(), String> {
            self.aborts.fetch_add(1, Ordering::SeqCst);
            while self.abort_blocked.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            if self
                .abort_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err("injected surface cleanup failure".into());
            }
            Ok(())
        }

        fn is_idle(&self, _run_id: &str) -> bool {
            true
        }

        fn start_periodic_preview(&self, _target_id: &str) {}

        fn stop_periodic_preview(&self) {}

        fn periodic_preview_active(&self) -> bool {
            false
        }

        fn release_target_for_run(&self, _run_id: &str, target_id: &str) {
            if target_id == MCP_TEST_TARGET {
                self.releases.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    #[derive(Debug)]
    enum StubReply {
        Ok,
        Error(&'static str),
    }

    struct StubRequest {
        method: String,
        params: Value,
        reply: oneshot::Sender<StubReply>,
    }

    struct TcpStub {
        client: Arc<AcpClient>,
        requests: mpsc::UnboundedReceiver<StubRequest>,
        server: tokio::task::JoinHandle<()>,
        events: tokio::task::JoinHandle<()>,
    }

    impl TcpStub {
        async fn connect() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind ACP stub");
            let addr = listener.local_addr().expect("ACP stub address");
            let (request_tx, requests) = mpsc::unbounded_channel();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept ACP client");
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                loop {
                    line.clear();
                    let Ok(read) = reader.read_line(&mut line).await else {
                        break;
                    };
                    if read == 0 {
                        break;
                    }
                    let message: Value = serde_json::from_str(line.trim()).expect("ACP JSON-RPC");
                    let id = message
                        .get("id")
                        .and_then(Value::as_u64)
                        .expect("request id");
                    let method = message
                        .get("method")
                        .and_then(Value::as_str)
                        .expect("request method")
                        .to_string();
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    let (reply, receive_reply) = oneshot::channel();
                    if request_tx
                        .send(StubRequest {
                            method,
                            params,
                            reply,
                        })
                        .is_err()
                    {
                        break;
                    }
                    let response = match receive_reply.await {
                        Ok(StubReply::Ok) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "applied": true }
                        }),
                        Ok(StubReply::Error(message)) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32000, "message": message }
                        }),
                        Err(_) => break,
                    };
                    let mut encoded = serde_json::to_vec(&response).expect("encode response");
                    encoded.push(b'\n');
                    if write.write_all(&encoded).await.is_err() {
                        break;
                    }
                }
            });
            let (client, mut event_rx) = AcpClient::connect_tcp(
                &addr.to_string(),
                std::env::temp_dir().join("grok-cu-acp-stub"),
            )
            .await
            .expect("connect ACP stub");
            let events = tokio::spawn(async move {
                while let Some((_session, event)) = event_rx.recv().await {
                    if matches!(event, AcpEvent::ProcessExited { .. }) {
                        break;
                    }
                }
            });
            Self {
                client,
                requests,
                server,
                events,
            }
        }

        async fn next(&mut self) -> StubRequest {
            tokio::time::timeout(Duration::from_secs(2), self.requests.recv())
                .await
                .expect("ACP request timeout")
                .expect("ACP request channel closed")
        }

        async fn assert_no_request(&mut self) {
            match tokio::time::timeout(Duration::from_millis(120), self.requests.recv()).await {
                Err(_) | Ok(None) => {}
                Ok(Some(_)) => panic!("unexpected ACP catalog request"),
            }
        }

        async fn shutdown(self) {
            self.client.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(2), self.server).await;
            let _ = tokio::time::timeout(Duration::from_secs(2), self.events).await;
        }
    }

    fn base_catalog(version: u64) -> Value {
        json!([{
            "name": format!("base-{version}"),
            "command": "base-command",
            "args": [],
            "env": []
        }])
    }

    fn deps(base_version: Arc<AtomicU64>, timeout: Duration) -> McpReconcileDeps {
        McpReconcileDeps {
            timeout,
            build_base: Arc::new(move |_| base_catalog(base_version.load(Ordering::SeqCst))),
            build_entry: Arc::new(|session, run| {
                Ok(json!({
                    "name": crate::computer_use::MCP_SERVER_NAME,
                    "command": "private-runtime",
                    "args": ["computer-use-mcp"],
                    "env": [
                        { "name": "GROK_APP_CU_SESSION", "value": session },
                        { "name": "GROK_APP_CU_RUN", "value": run }
                    ]
                }))
            }),
        }
    }

    fn ready_live(
        app_session_id: &str,
        agent_session_id: &str,
        process_id: &str,
        acp: Arc<AcpClient>,
    ) -> LiveSession {
        let mut fsm = SessionFsm::new();
        let _ = fsm.start_connect();
        let _ = fsm.handshake_ok();
        let now = std::time::Instant::now();
        LiveSession {
            app_session_id: app_session_id.into(),
            process_id: process_id.into(),
            meta: SessionMeta {
                id: app_session_id.into(),
                project_id: None,
                title: "MCP TCP test".into(),
                agent_session_id: Some(agent_session_id.into()),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                model_id: None,
                archived: false,
                pinned: false,
                effort: None,
                mode: None,
                permission_policy: None,
                json_schema: None,
                scheduled: false,
                worktree_path: None,
                worktree_branch: None,
                is_worktree_session: false,
                plugin_dirs: Vec::new(),
                extra_rules: None,
                max_agent_turns: None,
                system_prompt_override: None,
                fork_agent_session: false,
                fork_rewind_prompt_index: None,
                no_ask_user: None,
            },
            fsm,
            backend: "tcp-acp-stub".into(),
            acp: Some(acp),
            mock_stream: None,
            streaming_message_id: None,
            active_turn_id: None,
            stream_message_id_locked: false,
            stream_buf: String::new(),
            stream_thought: String::new(),
            stream_last_was_assistant: false,
            stream_attachments: Vec::new(),
            model_id: None,
            effort: None,
            product_mode: None,
            project_path: None,
            allow_cache: SessionAllowCache::default(),
            policy: PermissionPolicy::default(),
            provider_retry_attempt: 0,
            provider_retry_aborted: false,
            needs_history_bootstrap: false,
            pending_plan_rpc_id: None,
            pending_permission_rpc_id: None,
            pending_permission_options: None,
            pending_permission_tool_name: None,
            pending_permission_ui: None,
            pending_ask_user_rpc_id: None,
            pending_ask_user_ui: None,
            last_activity: now,
            last_stream_progress: now,
            last_stall_emit: None,
            stall_soft_emits: 0,
            journal_throttle: JournalWriteThrottle::with_default_interval(),
            open_tool_ids: HashSet::new(),
            open_tool_seen_at: HashMap::new(),
            terminal_tool_ids: HashSet::new(),
            deferred_prompt_complete: None,
            tools_this_turn: 0,
            saw_model_output: false,
            prompt_in_flight: false,
            sent_prompt_this_visit: false,
            pending_stream_emit: None,
            stream_emit_flush_gen: 0,
            last_tool_heartbeat_emit: None,
        }
    }

    fn manager_with_live(
        session: &str,
        agent: &str,
        client: Arc<AcpClient>,
    ) -> Arc<SessionManager> {
        let manager = Arc::new(SessionManager::new());
        *manager.inner.lock() = Some(ready_live(session, agent, "process-shared", client));
        manager
    }

    fn desired_present(
        session: &str,
        attempt: &str,
    ) -> (ComputerUseBroker, sessions::AuthorizationTicket) {
        let adapter = Arc::new(McpTestAdapter::default());
        let broker = ComputerUseBroker::new(
            adapter,
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-mcp-reconcile-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        );
        let ticket = sessions::begin(&broker, session, None, attempt, 1).expect("begin attempt");
        broker
            .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
            .expect("authorize fixture");
        sessions::activate(&broker, &ticket).expect("request MCP attach");
        (broker, ticket)
    }

    fn catalogs(request: &StubRequest) -> &[Value] {
        assert_eq!(request.method, "_x.ai/session/update_mcp_servers");
        request
            .params
            .get("mcpServers")
            .and_then(Value::as_array)
            .expect("MCP catalog array")
    }

    fn assert_catalog(request: &StubRequest, base: &str, cu_count: usize) {
        let rows = catalogs(request);
        assert!(rows.iter().any(|row| row.get("name") == Some(&json!(base))));
        assert_eq!(
            rows.iter()
                .filter(|row| {
                    row.get("name").and_then(Value::as_str)
                        == Some(crate::computer_use::MCP_SERVER_NAME)
                })
                .count(),
            cu_count
        );
    }

    fn noop_stop_plan() -> FencedComputerUseStop {
        FencedComputerUseStop {
            desired: sessions::McpDesiredState {
                generation: 0,
                run_id: None,
                attempt_generation: None,
            },
            cleanup: None,
            fence_error: None,
        }
    }

    fn activate_test_run(
        broker: &ComputerUseBroker,
        session: &str,
        attempt: &str,
    ) -> sessions::AuthorizationTicket {
        let ticket = sessions::begin(broker, session, None, attempt, 1).expect("begin test run");
        broker
            .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
            .expect("authorize test target");
        sessions::activate(broker, &ticket).expect("activate test run");
        sessions::complete(broker, &ticket).expect("complete test run");
        ticket
    }

    async fn call_ipc(url: &str, token: &str, name: &str, arguments: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{url}/cu/tool"))
            .bearer_auth(token)
            .json(&json!({"name": name, "arguments": arguments}))
            .send()
            .await
            .expect("loopback request");
        let status = response.status().as_u16();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }

    fn detach_handshake_acp(manager: &SessionManager, session: &str) -> FencedAcpTermination {
        manager
            .with_session_mut(session, FencedAcpTermination::capture)
            .flatten()
            .expect("handshake ACP is captured exactly once")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_ipc_revokes_stop_and_process_exit_bearers_before_cleanup() {
        let suffix = uuid::Uuid::new_v4().to_string();
        let adapter = Arc::new(McpTestAdapter::default());
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(format!("cu-ipc-exit-{suffix}.lease")),
                ..BrokerOptions::default()
            },
        ));
        let manager = Arc::new(SessionManager::new());
        crate::computer_use::ipc::register_session_manager(&manager);
        let endpoint = crate::computer_use::ipc::ensure(Arc::clone(&broker))
            .expect("production Computer Use IPC endpoint");

        // Model Stop must close the credential before its deliberately blocked
        // adapter cleanup completes. A repeated Host Stop shares the same
        // generation-bound cleanup ticket and cannot dispatch it twice.
        let model_session = format!("model-stop-{suffix}");
        let model_ticket = activate_test_run(&broker, &model_session, "model-stop-attempt");
        let model_token = endpoint
            .issue_session(&model_session, &model_ticket.run_id)
            .expect("issue model bearer");
        assert_eq!(
            call_ipc(&endpoint.url, &model_token, "computer_status", json!({}))
                .await
                .0,
            200
        );

        adapter.set_abort_blocked(true);
        let (stop_status, stop_body) =
            call_ipc(&endpoint.url, &model_token, "computer_stop", json!({})).await;
        assert_eq!(stop_status, 200, "{stop_body}");
        assert_eq!(
            broker.stop_state(&model_ticket.run_id).unwrap(),
            StopState::StopRequested
        );
        assert!(sessions::mcp_desired(&model_session).run_id.is_none());
        assert_eq!(
            call_ipc(&endpoint.url, &model_token, "computer_status", json!({}))
                .await
                .0,
            401,
            "the stopped bearer must be invalid before Stop returns"
        );
        let stopped_generation = sessions::mcp_desired(&model_session).generation;
        let repeated =
            manager.fence_computer_use_stop_with_broker(Arc::clone(&broker), &model_session);
        assert_eq!(repeated.desired.generation, stopped_generation);
        manager.spawn_fenced_computer_use_stop(model_session.clone(), repeated, None);

        adapter.set_abort_blocked(false);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = manager.mcp_catalog_status(&model_session);
                if adapter.aborts.load(Ordering::SeqCst) == 1
                    && adapter.releases.load(Ordering::SeqCst) == 1
                    && status.as_ref().is_some_and(|status| {
                        !status.pending
                            && !status.desired_present
                            && !status.resource_cleanup_pending
                            && status.applied_generation == Some(status.desired_generation)
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("model Stop cleanup converged");
        assert_eq!(
            sessions::mcp_desired(&model_session).generation,
            stopped_generation
        );
        sessions::forget_session(&model_session);

        // ProcessExited first fences every App session on the exact ACP
        // incarnation. The ACP transport is still alive in this fixture only
        // so the test can prove that no catalog request is needed on this path.
        let live_id = format!("exit-live-{suffix}");
        let background_id = format!("exit-background-{suffix}");
        let parked_id = format!("exit-parked-{suffix}");
        let live_ticket = activate_test_run(&broker, &live_id, "exit-live-attempt");
        let background_ticket =
            activate_test_run(&broker, &background_id, "exit-background-attempt");
        let parked_ticket = activate_test_run(&broker, &parked_id, "exit-parked-attempt");
        let process_tokens = [
            (
                live_id.clone(),
                live_ticket.run_id.clone(),
                endpoint
                    .issue_session(&live_id, &live_ticket.run_id)
                    .expect("issue live bearer"),
            ),
            (
                background_id.clone(),
                background_ticket.run_id.clone(),
                endpoint
                    .issue_session(&background_id, &background_ticket.run_id)
                    .expect("issue background bearer"),
            ),
            (
                parked_id.clone(),
                parked_ticket.run_id.clone(),
                endpoint
                    .issue_session(&parked_id, &parked_ticket.run_id)
                    .expect("issue parked bearer"),
            ),
        ];
        for (_, _, token) in &process_tokens {
            assert_eq!(
                call_ipc(&endpoint.url, token, "computer_status", json!({}))
                    .await
                    .0,
                200
            );
        }

        let stub = TcpStub::connect().await;
        *manager.inner.lock() = Some(ready_live(
            &live_id,
            "agent-exit-live",
            "process-exit-ipc",
            Arc::clone(&stub.client),
        ));
        manager.background.lock().insert(
            background_id.clone(),
            ready_live(
                &background_id,
                "agent-exit-background",
                "process-exit-ipc",
                Arc::clone(&stub.client),
            ),
        );
        let parked_live = ready_live(
            &parked_id,
            "agent-exit-parked",
            "process-exit-ipc",
            Arc::clone(&stub.client),
        );
        manager.parked.lock().insert(
            parked_id.clone(),
            ParkedAgent {
                process_id: parked_live.process_id,
                app_session_id: parked_live.app_session_id,
                meta: parked_live.meta,
                acp: parked_live.acp.expect("parked ACP"),
                last_activity: parked_live.last_activity,
                model_id: parked_live.model_id,
                effort: parked_live.effort,
                product_mode: parked_live.product_mode,
                project_path: parked_live.project_path,
                policy: parked_live.policy,
                sandbox_profile: None,
                needs_history_bootstrap: parked_live.needs_history_bootstrap,
                backend: parked_live.backend,
            },
        );

        let plans = manager.fence_computer_use_for_process_exit_with(
            "process-exit-ipc",
            Some(Arc::clone(&broker)),
        );
        assert_eq!(plans.len(), 3, "all shared-process tenants must be fenced");
        assert!(
            manager
                .fence_computer_use_for_process_exit_with(
                    "process-exit-ipc",
                    Some(Arc::clone(&broker)),
                )
                .is_empty()
        );

        for (_, _, token) in &process_tokens {
            assert_eq!(
                call_ipc(&endpoint.url, token, "computer_status", json!({}))
                    .await
                    .0,
                401,
                "ProcessExited must revoke each old bearer before cleanup"
            );
            assert_eq!(
                call_ipc(&endpoint.url, token, "computer_act", json!({}))
                    .await
                    .0,
                401,
                "an old action credential must fail at the HTTP boundary"
            );
        }
        assert_eq!(
            adapter.dispatches.load(Ordering::SeqCst),
            0,
            "no post-exit action reached the adapter"
        );

        let baseline_aborts = adapter.aborts.load(Ordering::SeqCst);
        let baseline_releases = adapter.releases.load(Ordering::SeqCst);
        adapter.fail_next_aborts(1);
        let mut failed_cleanup = Vec::new();
        for (session, plan) in plans {
            if manager
                .finish_process_exit_cleanup(&session, plan)
                .await
                .is_err()
            {
                failed_cleanup.push(session);
            }
        }
        assert_eq!(failed_cleanup.len(), 1, "one injected cleanup must fail");
        for session in &failed_cleanup {
            let status = manager
                .mcp_catalog_status(session)
                .expect("failed cleanup status");
            assert!(!status.pending && !status.desired_present);
            assert!(status.resource_cleanup_pending);
            manager
                .retry_computer_use_cleanup(session)
                .await
                .expect("same-generation process-exit cleanup retry");
        }
        for (session, _, _) in &process_tokens {
            let status = manager
                .mcp_catalog_status(session)
                .expect("settled process-exit status");
            assert!(!status.pending);
            assert!(!status.desired_present);
            assert!(!status.resource_cleanup_pending);
            sessions::forget_session(session);
        }
        assert_eq!(
            adapter.aborts.load(Ordering::SeqCst) - baseline_aborts,
            4,
            "three cleanups plus one retry"
        );
        assert_eq!(
            adapter.releases.load(Ordering::SeqCst) - baseline_releases,
            3,
            "each process-owned target is released once"
        );
        *manager.inner.lock() = None;
        manager.background.lock().clear();
        manager.parked.lock().clear();
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn handshake_stop_kills_captured_acp_even_without_computer_use_run() {
        let session = format!("handshake-noop-{}", uuid::Uuid::new_v4());
        let stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-handshake", Arc::clone(&stub.client));
        let termination = detach_handshake_acp(&manager, &session);
        assert!(manager
            .inner
            .lock()
            .as_ref()
            .is_some_and(|live| live.acp.is_none()));

        manager
            .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
            .await
            .expect("no-op Computer Use cleanup must not block process termination");
        assert!(
            !stub.client.is_alive(),
            "captured handshake ACP must be dead"
        );
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn handshake_stop_kills_acp_when_computer_use_cleanup_fails() {
        let session = format!("handshake-cleanup-error-{}", uuid::Uuid::new_v4());
        let stub = TcpStub::connect().await;
        let manager =
            manager_with_live(&session, "agent-handshake-error", Arc::clone(&stub.client));
        let termination = detach_handshake_acp(&manager, &session);
        let desired = sessions::McpDesiredState {
            generation: 1,
            run_id: None,
            attempt_generation: None,
        };
        let plan = FencedComputerUseStop {
            desired,
            cleanup: None,
            fence_error: Some("injected cleanup failure".into()),
        };

        let error = manager
            .finish_fenced_computer_use_stop_task(&session, plan, Some(termination))
            .await
            .expect_err("injected cleanup failure must remain observable");
        assert!(error.contains("injected cleanup failure"));
        assert!(
            !stub.client.is_alive(),
            "cleanup failure must not keep the cancelled handshake ACP alive"
        );
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn delayed_handshake_stop_cannot_kill_reconnected_acp() {
        let session = format!("handshake-reconnect-{}", uuid::Uuid::new_v4());
        let old_stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-old", Arc::clone(&old_stub.client));
        let termination = detach_handshake_acp(&manager, &session);

        let new_stub = TcpStub::connect().await;
        *manager.inner.lock() = Some(ready_live(
            &session,
            "agent-new",
            "process-new",
            Arc::clone(&new_stub.client),
        ));
        manager
            .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
            .await
            .expect("old handshake termination must complete");

        assert!(!old_stub.client.is_alive(), "old ACP must be terminated");
        assert!(
            new_stub.client.is_alive(),
            "replacement ACP must remain alive"
        );
        assert!(manager.inner.lock().as_ref().is_some_and(|live| {
            live.process_id == "process-new"
                && live
                    .acp
                    .as_ref()
                    .is_some_and(|acp| Arc::ptr_eq(acp, &new_stub.client))
        }));
        new_stub.shutdown().await;
        old_stub.shutdown().await;
    }

    #[tokio::test]
    async fn handshake_stop_detaches_shared_tenant_without_killing_co_tenant() {
        let session = format!("handshake-shared-{}", uuid::Uuid::new_v4());
        let co_tenant = format!("handshake-shared-peer-{}", uuid::Uuid::new_v4());
        let stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-shared", Arc::clone(&stub.client));
        manager.background.lock().insert(
            co_tenant.clone(),
            ready_live(
                &co_tenant,
                "agent-shared-peer",
                "process-shared",
                Arc::clone(&stub.client),
            ),
        );
        let termination = detach_handshake_acp(&manager, &session);
        manager
            .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
            .await
            .expect("shared handshake detach must complete");

        assert!(
            stub.client.is_alive(),
            "a live co-tenant must keep the shared ACP alive"
        );
        assert!(manager.background.lock().contains_key(&co_tenant));
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn process_exit_fences_live_background_and_parked_tenants_once() {
        let suffix = uuid::Uuid::new_v4().to_string();
        let live_id = format!("exit-live-{suffix}");
        let background_id = format!("exit-background-{suffix}");
        let parked_id = format!("exit-parked-{suffix}");
        let adapter = Arc::new(McpTestAdapter::default());
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(format!("cu-exit-{suffix}.lease")),
                ..BrokerOptions::default()
            },
        ));
        let live_ticket = activate_test_run(&broker, &live_id, "exit-live-attempt");
        let background_ticket =
            activate_test_run(&broker, &background_id, "exit-background-attempt");
        let parked_ticket = activate_test_run(&broker, &parked_id, "exit-parked-attempt");
        let stub = TcpStub::connect().await;
        let manager = manager_with_live(&live_id, "agent-exit-live", Arc::clone(&stub.client));
        manager.background.lock().insert(
            background_id.clone(),
            ready_live(
                &background_id,
                "agent-exit-background",
                "process-shared",
                Arc::clone(&stub.client),
            ),
        );
        let parked_live = ready_live(
            &parked_id,
            "agent-exit-parked",
            "process-shared",
            Arc::clone(&stub.client),
        );
        manager.parked.lock().insert(
            parked_id.clone(),
            ParkedAgent {
                process_id: parked_live.process_id,
                app_session_id: parked_live.app_session_id,
                meta: parked_live.meta,
                acp: parked_live.acp.expect("parked ACP"),
                last_activity: parked_live.last_activity,
                model_id: parked_live.model_id,
                effort: parked_live.effort,
                product_mode: parked_live.product_mode,
                project_path: parked_live.project_path,
                policy: parked_live.policy,
                sandbox_profile: None,
                needs_history_bootstrap: parked_live.needs_history_bootstrap,
                backend: parked_live.backend,
            },
        );

        let plans = manager
            .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
        assert_eq!(plans.len(), 3, "every process tenant must be fenced");
        for (session, plan) in &plans {
            assert!(plan.desired.run_id.is_none(), "{session} remains absent");
            assert_eq!(
                plan.desired.generation,
                manager
                    .mcp_catalog_status(session)
                    .expect("transport-gone status")
                    .desired_generation
            );
        }
        assert!(broker.authorized_target(&live_ticket.run_id).is_err());
        assert!(broker.authorized_target(&background_ticket.run_id).is_err());
        assert!(broker.authorized_target(&parked_ticket.run_id).is_err());

        let duplicate = manager
            .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
        assert!(
            duplicate.is_empty(),
            "duplicate exit must not create a second fence"
        );
        for (session, plan) in plans {
            manager
                .finish_process_exit_cleanup(&session, plan)
                .await
                .expect("surface cleanup after process exit");
            sessions::forget_session(&session);
        }
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn stale_process_exit_does_not_fence_replacement_session() {
        let session = format!("exit-replacement-{}", uuid::Uuid::new_v4());
        let adapter = Arc::new(McpTestAdapter::default());
        let broker = Arc::new(ComputerUseBroker::new(
            adapter,
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-replacement-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let old_ticket = activate_test_run(&broker, &session, "exit-old-attempt");
        let old_stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-exit-old", Arc::clone(&old_stub.client));
        let old_fenced = sessions::fence_revoke_checked_for_test(Arc::clone(&broker), &session);
        if let Some(cleanup) = old_fenced.cleanup {
            sessions::finish_fenced_cleanup_blocking(cleanup)
                .expect("old run cleanup before reconnect");
        }
        let new_stub = TcpStub::connect().await;
        *manager.inner.lock() = Some(ready_live(
            &session,
            "agent-exit-new",
            "process-exit-new",
            Arc::clone(&new_stub.client),
        ));
        let new_ticket = activate_test_run(&broker, &session, "exit-new-attempt");

        let plans = manager
            .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
        assert!(
            plans.is_empty(),
            "old exit must not match replacement process"
        );
        assert!(broker.authorized_target(&new_ticket.run_id).is_ok());
        assert!(broker.authorized_target(&old_ticket.run_id).is_err());

        sessions::fail_checked(&broker, &new_ticket).expect("cleanup replacement run");
        sessions::forget_session(&session);
        new_stub.shutdown().await;
        old_stub.shutdown().await;
    }

    #[test]
    fn stale_authorization_failure_is_host_noop_for_newer_present_catalog() {
        let session = format!("stale-auth-failure-{}", uuid::Uuid::new_v4());
        let (broker, first) = desired_present(&session, "attempt-a");
        let broker = Arc::new(broker);
        sessions::complete(broker.as_ref(), &first).expect("complete first authorization");
        sessions::fail_checked(broker.as_ref(), &first).expect("retire first authorization");

        let second = sessions::begin(
            broker.as_ref(),
            &session,
            Some(&first.run_id),
            "attempt-b",
            1,
        )
        .expect("begin newer authorization");
        broker
            .authorize_target(&second.run_id, MCP_TEST_TARGET)
            .expect("authorize newer target");
        sessions::activate(broker.as_ref(), &second).expect("request newer MCP attach");
        sessions::complete(broker.as_ref(), &second).expect("complete newer authorization");

        let manager = SessionManager::new();
        let desired = sessions::mcp_desired(&session);
        let applied = McpCatalogStatus {
            desired_generation: desired.generation,
            applied_generation: Some(desired.generation),
            desired_present: true,
            pending: false,
            resource_cleanup_pending: false,
            last_error: None,
        };
        manager.set_mcp_status(&session, applied.clone());

        let plan = manager.fence_failed_computer_use_authorization(Arc::clone(&broker), &first);
        assert!(
            plan.is_noop(),
            "a stale failure must not schedule Host cleanup"
        );
        assert_eq!(manager.mcp_catalog_status(&session), Some(applied));
        assert!(sessions::is_current(&second));

        sessions::fail_checked(broker.as_ref(), &second).expect("clean up newer authorization");
        sessions::forget_session(&session);
    }

    #[tokio::test]
    async fn stop_during_mcp_update_converges_to_absent() {
        let session = format!("stop-during-update-{}", uuid::Uuid::new_v4());
        let (broker, ticket) = desired_present(&session, "attempt-a");
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-a", Arc::clone(&stub.client));
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
        let reconcile = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let first = stub.next().await;
        assert_catalog(&first, "base-1", 1);
        sessions::fail_checked(&broker, &ticket).expect("revoke while update is blocked");
        first.reply.send(StubReply::Ok).expect("reply first attach");
        let second = stub.next().await;
        assert_catalog(&second, "base-1", 0);
        second.reply.send(StubReply::Ok).expect("reply detach");
        reconcile.await.expect("join reconcile").expect("converge");
        let status = manager
            .mcp_catalog_status(&session)
            .expect("catalog status");
        assert!(!status.pending);
        assert!(!status.desired_present);
        assert_eq!(status.applied_generation, Some(status.desired_generation));
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn applied_but_timed_out_update_remains_pending_then_retries_idempotently() {
        let session = format!("timeout-applied-{}", uuid::Uuid::new_v4());
        let (_broker, _ticket) = desired_present(&session, "attempt-timeout");
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-timeout", Arc::clone(&stub.client));
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_millis(80));
        let first_run = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let first = stub.next().await;
        assert_catalog(&first, "base-1", 1);
        let result = first_run.await.expect("join timeout reconcile");
        assert!(result.is_err(), "ambiguous timeout must not claim applied");
        let status = manager
            .mcp_catalog_status(&session)
            .expect("timeout status");
        assert!(status.pending);
        assert!(status
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("timed out")));

        // The server may have applied the full replacement before losing its
        // reply. A late success is ignored; an explicit retry sends the same
        // desired replacement and is therefore safe and idempotent.
        first.reply.send(StubReply::Ok).expect("late ACP response");
        tokio::task::yield_now().await;
        let retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let second = stub.next().await;
        assert_catalog(&second, "base-1", 1);
        second.reply.send(StubReply::Ok).expect("retry response");
        retry.await.expect("join retry").expect("retry converges");
        assert!(!manager.mcp_catalog_status(&session).unwrap().pending);
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn failed_detach_is_cleanup_pending_until_retry_succeeds() {
        let session = format!("detach-retry-{}", uuid::Uuid::new_v4());
        let (broker, ticket) = desired_present(&session, "attempt-detach");
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-detach", Arc::clone(&stub.client));
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));

        let attach = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let attached = stub.next().await;
        assert_catalog(&attached, "base-1", 1);
        attached.reply.send(StubReply::Ok).unwrap();
        attach.await.unwrap().unwrap();

        sessions::fail_checked(&broker, &ticket).expect("revoke desired state");
        let detach = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let failed = stub.next().await;
        assert_catalog(&failed, "base-1", 0);
        failed
            .reply
            .send(StubReply::Error("injected detach failure"))
            .unwrap();
        assert!(detach.await.unwrap().is_err());
        let failed_status = manager.mcp_catalog_status(&session).unwrap();
        assert!(failed_status.pending && !failed_status.desired_present);

        let retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let recovered = stub.next().await;
        assert_catalog(&recovered, "base-1", 0);
        recovered.reply.send(StubReply::Ok).unwrap();
        retry.await.unwrap().unwrap();
        let recovered_status = manager.mcp_catalog_status(&session).unwrap();
        assert!(!recovered_status.pending && !recovered_status.desired_present);
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn resource_cleanup_retry_is_generation_bound_and_releases_once() {
        let session = format!("resource-cleanup-{}", uuid::Uuid::new_v4());
        let adapter = Arc::new(McpTestAdapter::default());
        adapter.fail_next_aborts(1);
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(format!(
                    "cu-resource-cleanup-{}.lease",
                    uuid::Uuid::new_v4()
                )),
                ..BrokerOptions::default()
            },
        ));
        let ticket = sessions::begin(
            broker.as_ref(),
            &session,
            None,
            "resource-cleanup-attempt",
            1,
        )
        .expect("begin cleanup fixture");
        broker
            .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
            .expect("authorize cleanup fixture");
        sessions::activate(broker.as_ref(), &ticket).expect("publish desired-present");
        sessions::complete(broker.as_ref(), &ticket).expect("complete authorization");

        let fenced = sessions::fence_revoke_checked_for_test(Arc::clone(&broker), &session);
        assert_eq!(fenced.error, None);
        let cleanup = fenced.cleanup.expect("generation-bound cleanup hand-off");
        let desired = sessions::mcp_desired(&session);
        assert!(desired.run_id.is_none());
        let manager = Arc::new(SessionManager::new());
        manager.record_fenced_cleanup(&session, &desired, true);
        let plan = FencedComputerUseStop {
            desired: desired.clone(),
            cleanup: Some(cleanup),
            fence_error: None,
        };

        let error = manager
            .finish_fenced_computer_use_stop(&session, plan)
            .await
            .expect_err("first resource cleanup must fail");
        assert!(error.contains("surface cancellation"), "{error}");
        let failed = manager.mcp_catalog_status(&session).unwrap();
        assert!(!failed.pending);
        assert!(failed.resource_cleanup_pending);
        assert_eq!(failed.applied_generation, Some(desired.generation));
        assert_eq!(
            failed.last_error.as_deref(),
            Some("Computer Use surface cleanup failed")
        );
        assert_eq!(adapter.aborts.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.releases.load(Ordering::SeqCst), 0);

        let retry_a = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            tokio::spawn(async move { manager.retry_computer_use_cleanup(&session).await })
        };
        let retry_b = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            tokio::spawn(async move { manager.retry_computer_use_cleanup(&session).await })
        };
        retry_a
            .await
            .expect("join first retry")
            .expect("first same-generation resource retry converges");
        retry_b
            .await
            .expect("join duplicate retry")
            .expect("duplicate same-generation retry is idempotent");
        let settled = manager.mcp_catalog_status(&session).unwrap();
        assert!(!settled.pending);
        assert!(!settled.resource_cleanup_pending);
        assert_eq!(settled.applied_generation, Some(desired.generation));
        assert_eq!(sessions::mcp_desired(&session), desired);
        assert_eq!(adapter.aborts.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.releases.load(Ordering::SeqCst), 1);

        manager
            .retry_computer_use_cleanup(&session)
            .await
            .expect("settled retry is idempotent");
        manager.record_fenced_cleanup(&session, &desired, false);
        assert!(
            !manager
                .mcp_catalog_status(&session)
                .unwrap()
                .resource_cleanup_pending
        );
        assert_eq!(adapter.aborts.load(Ordering::SeqCst), 2);
        assert_eq!(adapter.releases.load(Ordering::SeqCst), 1);
        sessions::forget_session(&session);
    }

    #[tokio::test]
    async fn cleanup_retry_converges_pending_absent_and_repeated_calls_are_idempotent() {
        let session = format!("cleanup-command-{}", uuid::Uuid::new_v4());
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-cleanup", Arc::clone(&stub.client));
        let desired = sessions::mcp_desired(&session);
        manager.set_mcp_status(
            &session,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: None,
                desired_present: false,
                pending: true,
                resource_cleanup_pending: false,
                last_error: Some("Computer Use MCP catalog update failed".into()),
            },
        );
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
        let retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move {
                manager
                    .retry_computer_use_cleanup_with(&session, deps)
                    .await
            })
        };
        let request = stub.next().await;
        assert_catalog(&request, "base-1", 0);
        request.reply.send(StubReply::Ok).unwrap();
        retry.await.unwrap().unwrap();

        let settled = manager.mcp_catalog_status(&session).unwrap();
        assert!(!settled.pending && !settled.desired_present);
        assert_eq!(settled.applied_generation, Some(desired.generation));
        manager
            .retry_computer_use_cleanup_with(&session, deps)
            .await
            .expect("a repeated cleanup retry is an idempotent acknowledgement");
        stub.assert_no_request().await;
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn cleanup_retry_failure_stays_pending_redacted_and_retryable() {
        let session = format!("cleanup-failure-{}", uuid::Uuid::new_v4());
        let mut stub = TcpStub::connect().await;
        let manager =
            manager_with_live(&session, "agent-cleanup-failure", Arc::clone(&stub.client));
        let desired = sessions::mcp_desired(&session);
        manager.set_mcp_status(
            &session,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: None,
                desired_present: false,
                pending: true,
                resource_cleanup_pending: false,
                last_error: Some("Computer Use MCP catalog update failed".into()),
            },
        );
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));

        let failed_retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move {
                manager
                    .retry_computer_use_cleanup_with(&session, deps)
                    .await
            })
        };
        let failed = stub.next().await;
        assert_catalog(&failed, "base-1", 0);
        failed
            .reply
            .send(StubReply::Error(
                "injected raw ACP failure Authorization: Bearer TOPSECRET",
            ))
            .unwrap();
        let error = failed_retry.await.unwrap().unwrap_err();
        assert_eq!(error, "Computer Use MCP catalog update failed");
        let failed_status = manager.mcp_catalog_status(&session).unwrap();
        assert!(failed_status.pending);
        assert!(!failed_status.desired_present);
        assert_eq!(
            failed_status.last_error.as_deref(),
            Some("Computer Use MCP catalog update failed")
        );
        assert!(!format!("{failed_status:?}").contains("TOPSECRET"));

        let recovered_retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move {
                manager
                    .retry_computer_use_cleanup_with(&session, deps)
                    .await
            })
        };
        let recovered = stub.next().await;
        assert_catalog(&recovered, "base-1", 0);
        recovered.reply.send(StubReply::Ok).unwrap();
        recovered_retry.await.unwrap().unwrap();
        let settled = manager.mcp_catalog_status(&session).unwrap();
        assert!(!settled.pending);
        assert!(!settled.desired_present);
        assert_eq!(settled.applied_generation, Some(desired.generation));
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn cleanup_retry_rejects_desired_present_without_sending_absent() {
        let session = format!("cleanup-present-{}", uuid::Uuid::new_v4());
        let mut stub = TcpStub::connect().await;
        let manager =
            manager_with_live(&session, "agent-cleanup-present", Arc::clone(&stub.client));
        let (broker, ticket) = desired_present(&session, "attempt-present");
        let desired = sessions::mcp_desired(&session);
        manager.set_mcp_status(
            &session,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: None,
                desired_present: true,
                pending: true,
                resource_cleanup_pending: false,
                last_error: None,
            },
        );

        let error = manager
            .retry_computer_use_cleanup_with(
                &session,
                deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1)),
            )
            .await
            .expect_err("cleanup retry must not follow desired-present state");
        assert!(error.contains("only available after Stop"), "{error}");
        stub.assert_no_request().await;
        sessions::fail_checked(&broker, &ticket).expect("clean up desired-present grant");
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn cleanup_retry_never_follows_a_newer_desired_present_state() {
        let session = format!("cleanup-superseded-{}", uuid::Uuid::new_v4());
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-superseded", Arc::clone(&stub.client));
        let desired = sessions::mcp_desired(&session);
        manager.set_mcp_status(
            &session,
            McpCatalogStatus {
                desired_generation: desired.generation,
                applied_generation: None,
                desired_present: false,
                pending: true,
                resource_cleanup_pending: false,
                last_error: Some("Computer Use MCP catalog update failed".into()),
            },
        );
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
        let retry = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move {
                manager
                    .retry_computer_use_cleanup_with(&session, deps)
                    .await
            })
        };
        let absent = stub.next().await;
        assert_catalog(&absent, "base-1", 0);
        let (broker, ticket) = desired_present(&session, "attempt-after-retry");
        absent.reply.send(StubReply::Ok).unwrap();

        let error = retry.await.unwrap().unwrap_err();
        assert!(error.contains("superseded"), "{error}");
        stub.assert_no_request().await;
        sessions::fail_checked(&broker, &ticket).expect("clean up newer grant");
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn late_old_detach_cannot_overwrite_new_attach() {
        let session = format!("detach-then-attach-{}", uuid::Uuid::new_v4());
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-race", Arc::clone(&stub.client));
        let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
        let old_detach = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let first = stub.next().await;
        assert_catalog(&first, "base-1", 0);
        let (_broker, ticket) = desired_present(&session, "attempt-new");
        first.reply.send(StubReply::Ok).unwrap();
        let second = stub.next().await;
        assert_catalog(&second, "base-1", 1);
        assert!(catalogs(&second).iter().any(|row| {
            row.get("env")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|env| env.get("value") == Some(&json!(ticket.run_id)))
        }));
        second.reply.send(StubReply::Ok).unwrap();
        old_detach.await.unwrap().unwrap();
        let status = manager.mcp_catalog_status(&session).unwrap();
        assert!(!status.pending && status.desired_present);
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn base_extension_revision_race_preserves_latest_base_and_cu_entry() {
        let session = format!("base-race-{}", uuid::Uuid::new_v4());
        let (_broker, _ticket) = desired_present(&session, "attempt-base");
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "agent-base", Arc::clone(&stub.client));
        let base_version = Arc::new(AtomicU64::new(1));
        let deps = deps(Arc::clone(&base_version), Duration::from_secs(1));
        let reconcile = {
            let manager = Arc::clone(&manager);
            let session = session.clone();
            let deps = deps.clone();
            tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
        };
        let stale = stub.next().await;
        assert_catalog(&stale, "base-1", 1);
        base_version.store(2, Ordering::SeqCst);
        manager.note_mcp_base_changed();
        stale.reply.send(StubReply::Ok).unwrap();
        let latest = stub.next().await;
        assert_catalog(&latest, "base-2", 1);
        assert!(!catalogs(&latest)
            .iter()
            .any(|row| row.get("name") == Some(&json!("base-1"))));
        latest.reply.send(StubReply::Ok).unwrap();
        reconcile.await.unwrap().unwrap();
        stub.shutdown().await;
    }

    #[tokio::test]
    async fn live_background_and_parked_sessions_all_receive_catalog_updates() {
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live("live-mcp", "agent-live", Arc::clone(&stub.client));
        manager.background.lock().insert(
            "background-mcp".into(),
            ready_live(
                "background-mcp",
                "agent-background",
                "process-shared",
                Arc::clone(&stub.client),
            ),
        );
        let parked_live = ready_live(
            "parked-mcp",
            "agent-parked",
            "process-shared",
            Arc::clone(&stub.client),
        );
        manager.parked.lock().insert(
            "parked-mcp".into(),
            ParkedAgent {
                process_id: parked_live.process_id,
                app_session_id: parked_live.app_session_id,
                meta: parked_live.meta,
                acp: parked_live.acp.expect("parked ACP"),
                last_activity: parked_live.last_activity,
                model_id: parked_live.model_id,
                effort: parked_live.effort,
                product_mode: parked_live.product_mode,
                project_path: parked_live.project_path,
                policy: parked_live.policy,
                sandbox_profile: None,
                needs_history_bootstrap: false,
                backend: parked_live.backend,
            },
        );
        assert_eq!(
            manager.mcp_session_ids(),
            vec!["background-mcp", "live-mcp", "parked-mcp"]
        );
        let deps = deps(Arc::new(AtomicU64::new(3)), Duration::from_secs(1));
        let updates = {
            let manager = Arc::clone(&manager);
            tokio::spawn(async move {
                let mut tasks = Vec::new();
                for session in ["live-mcp", "background-mcp", "parked-mcp"] {
                    let manager = Arc::clone(&manager);
                    let deps = deps.clone();
                    tasks.push(tokio::spawn(async move {
                        manager.reconcile_session_mcp_with(session, deps).await
                    }));
                }
                for task in tasks {
                    task.await.expect("join session update")?;
                }
                Ok::<(), String>(())
            })
        };
        let mut agents = std::collections::BTreeSet::new();
        for _ in 0..3 {
            let request = stub.next().await;
            assert_catalog(&request, "base-3", 0);
            agents.insert(
                request
                    .params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .expect("agent session id")
                    .to_string(),
            );
            request.reply.send(StubReply::Ok).unwrap();
        }
        updates.await.unwrap().unwrap();
        assert_eq!(
            agents,
            ["agent-background", "agent-live", "agent-parked"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        stub.shutdown().await;
    }

    #[test]
    fn reconnect_retries_a_pending_desired_absent_catalog() {
        let manager = SessionManager::new();
        manager.set_mcp_status(
            "cleanup-pending",
            McpCatalogStatus {
                desired_generation: 0,
                applied_generation: None,
                desired_present: false,
                pending: true,
                resource_cleanup_pending: false,
                last_error: Some("injected detach failure".into()),
            },
        );
        assert!(manager.computer_use_reconcile_needed_after_connect("cleanup-pending"));

        manager.set_mcp_status(
            "cleanup-pending",
            McpCatalogStatus {
                desired_generation: 0,
                applied_generation: Some(0),
                desired_present: false,
                pending: false,
                resource_cleanup_pending: false,
                last_error: None,
            },
        );
        assert!(!manager.computer_use_reconcile_needed_after_connect("cleanup-pending"));
    }

    #[test]
    fn mcp_errors_exposed_to_status_never_retain_transport_payloads() {
        let secret = "cu-secret-sentinel-0123456789";
        let raw = format!(
            "ACP rejected catalog env GROK_COMPUTER_USE_TOKEN={secret}; Authorization: Bearer {secret}"
        );
        let public = safe_error(raw);
        assert_eq!(public, "Computer Use MCP catalog update failed");
        assert!(!public.contains(secret));
        assert_eq!(
            safe_error("request timed out after 15 seconds"),
            "Computer Use MCP catalog update timed out"
        );
    }
}
