//! Session-scoped Computer Use MCP catalog reconciliation.
//!
//! The ACP catalog is a replacement document, not an append-only side effect.
//! Every caller (extension preferences, Computer Use attach, stop, reconnect)
//! therefore goes through the same per-session desired-state loop. Authority
//! is fenced by `computer_use::sessions` before this module is called.

mod lifecycle;

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
mod tests;
