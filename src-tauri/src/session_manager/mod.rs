//! Host session manager: real ACP default; mock only if GROK_APP_ACP=mock.
//!
//! Process policy (I01–I03) — multi-session, no monopoly:
//! - One ACP process per App session (live / background-busy / parked-ready).
//! - Switching chats **never** cancels a busy turn: Streaming / AwaitingPermission
//!   / open tools / deferred prompt_complete demote to `background` (event pump
//!   kept). Only true idle Ready parks warm.
//! - Processes are **not** stolen across App sessions (no same-cwd rebind).
//! - Cap: `maxConcurrentAgents` (default 8, cap 32). Over cap → reclaim idle
//!   parked first; `PROCESS_LIMIT` only when busy slots are full. **Never** kill
//!   background-busy or open-tool turns for capacity.
//! - Idle recycle after `agentIdleMinutes` (default 30); session meta stays.
//! - soft_respawn skips mid-turn live sessions.
//!
//! Streaming performance (I04 / I06):
//! - Mid-stream journal upserts are throttled (≥500ms or paragraph / force).
//! - Pure stream silence: silent heal first (orphan tools / ready-eligible when
//!   the agent RPC already finished), then soft `session://stream_stall`
//!   (Keep waiting / End turn). **Never auto-cancel a user-initiated turn** —
//!   long silence re-prompts only; only the user may End turn.

mod connect;
mod connected_composer;
mod context_control;
mod control;
mod events;
mod events_bg;
mod execution_publication;
mod fork_trim;
mod human_gate_event;
mod human_gate_identity;
mod human_gate_invalidation;
mod human_gate_resolution;
mod journal;
mod main_route;
mod main_runtime;
mod mock_stream;
mod model_control;
mod permission_policy;
mod permission_request;
mod permission_resolution;
mod post_turn_reconcile;
mod prewarm_route;
mod process;
mod process_exit;
mod project_sandbox;
mod retirement;
mod state_event;
mod stream;
mod stream_event;
mod tool_event;
mod turn;
mod turn_event_owner;
mod turn_open;
mod types;
mod watchdog;
mod workspace_context;
pub use human_gate_identity::UiPlanRequest;
pub(crate) use workspace_context::RuntimeWorkspaceContext;
pub use workspace_context::{preview_workspace_runtime, WorkspaceRuntimeReview};

// Multi-session event routing (P0 shared-process load-replay safety).
pub(crate) use stream::StreamEmitPayload;
pub(crate) use stream::{
    has_turn_end_marker_after_last_user, resolve_turn_event_route, SessionRouteHint, TurnEventRoute,
};
#[allow(unused_imports)]
pub(crate) use stream::{
    PendingSessionPersist, PendingStreamJournalFlush, PendingTurnBoundaryPersist,
};

#[cfg(test)]
pub(crate) mod command_spawn_options_tests;
#[cfg(test)]
mod connected_composer_tests;
#[cfg(test)]
mod context_control_tests;
#[cfg(test)]
mod execution_publication_tests;
#[cfg(test)]
mod media_tests;
#[cfg(test)]
mod mock_stream_tests;
#[cfg(test)]
mod model_control_tests;
#[cfg(test)]
pub(crate) mod permission_policy_tests;
#[cfg(test)]
mod prewarm_route_tests;
#[cfg(test)]
mod routing_tests;
#[cfg(test)]
mod stall_tests;
#[cfg(test)]
mod state_event_tests;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU32, AtomicU64},
        Arc, OnceLock,
    },
};

use parking_lot::Mutex;

use crate::session_fsm::SessionState;

use types::*;

pub(crate) use types::{
    extract_tool_input, tool_journal_richer, RewindExecuteResult, RewindPointDto, SessionSnapshot,
    StopReport, UiAskUserRequest, UiPermissionRequest, TOOL_OUTPUT_MAX_PUB, TOOL_OUTPUT_SENTINEL,
};

pub struct SessionManager {
    /// Currently focused live session (UI-bound for send).
    pub(super) inner: Mutex<Option<LiveSession>>,
    /// Busy sessions still receiving ACP events (streaming / permission).
    /// Keyed by app session id. Enables multi-session parallel streaming.
    pub(super) background: Mutex<HashMap<String, LiveSession>>,
    /// Warm Ready agents for other App sessions (keyed by app session id).
    pub(super) parked: Mutex<HashMap<String, ParkedAgent>>,
    /// Serialize synchronous cross-map ownership handoff with owner lookups.
    /// Acquire before a pool-map guard; never hold across await or user I/O.
    pub(super) runtime_transfer: Mutex<()>,
    /// In-flight approval receipts only; their guards remove entries on drop.
    permission_responses: Mutex<HashSet<permission_resolution::PermissionResponseKey>>,
    human_responses: Mutex<HashSet<human_gate_identity::HumanResponseKey>>,
    /// Process prewarmed while the user is composing a new chat (no session yet).
    pub(super) prewarm: Mutex<PrewarmState>,
    /// Generation token for cancellable prewarm tasks. A task that started
    /// before recycle/route change must not publish a stale Ready child after
    /// a newer prewarm has taken ownership of the slot.
    pub(super) prewarm_epoch: AtomicU64,
    /// Tool identity learned from in_progress `tool_call` notifications
    /// (terminal `tool_call_update` payloads are status-only). Keyed by
    /// app session id → tool call id. See `remember_tool_identity`.
    pub(super) tool_identities: std::sync::Mutex<
        std::collections::HashMap<String, std::collections::HashMap<String, ToolIdentity>>,
    >,
    /// Serialize connect / park / unpark so openSession prefetch cannot race first send.
    pub(super) connect_lock: tokio::sync::Mutex<()>,
    /// Latest-wins generation for `session_connect`. A timed-out or superseded
    /// waiter must not enter `connect_inner` after it finally acquires the lock.
    pub(super) connect_epoch: AtomicU64,
    /// ACP children spawned inside `connect_inner` that are not yet (or no
    /// longer) the live/parked slot owner. Wall-clock abort sweeps this list
    /// with `kill_acp_bounded` so a cancelled handshake cannot leak workers.
    pub(super) pending_children: Mutex<Vec<PendingAcpChild>>,
    /// Diagnostic snapshot of the current `connect_lock` holder.
    pub(super) connect_holder: Mutex<Option<ConnectLockHolderInfo>>,
    /// Consecutive idle-recycle ticks that failed to acquire `connect_lock`.
    pub(super) connect_lock_busy_ticks: AtomicU32,
    /// Per-session locks that serialize a prompt's user-journal append with
    /// post-turn reconciliation. Retry sleeps never hold these locks, and one
    /// chat never delays another chat's send.
    pub(super) post_turn_journal_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Soft-respawn requested while the session was mid-turn (YOLO off,
    /// effort change, proxy, …). Flushed when the turn becomes idle so
    /// the next process picks up spawn flags (P0-5 / #598).
    pub(super) pending_soft_respawn: Mutex<HashMap<String, String>>,
    /// Context window chosen on the new-chat composer, before `session/new`.
    /// Applied on the first empty session, then cleared.
    pub(super) pending_context_window: Mutex<Option<u64>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

static NATIVE: OnceLock<Arc<SessionManager>> = OnceLock::new();

impl SessionManager {
    /// Process-wide manager `session_send` / `lib.rs` install into Tauri state.
    pub fn native() -> Arc<Self> {
        NATIVE.get_or_init(|| Arc::new(Self::new())).clone()
    }

    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            background: Mutex::new(HashMap::new()),
            parked: Mutex::new(HashMap::new()),
            runtime_transfer: Mutex::new(()),
            permission_responses: Mutex::new(HashSet::new()),
            human_responses: Mutex::new(HashSet::new()),
            prewarm: Mutex::new(PrewarmState::None),
            prewarm_epoch: AtomicU64::new(0),
            tool_identities: std::sync::Mutex::new(std::collections::HashMap::new()),
            connect_lock: tokio::sync::Mutex::new(()),
            connect_epoch: AtomicU64::new(0),
            pending_children: Mutex::new(Vec::new()),
            connect_holder: Mutex::new(None),
            connect_lock_busy_ticks: AtomicU32::new(0),
            post_turn_journal_locks: Mutex::new(HashMap::new()),
            pending_soft_respawn: Mutex::new(HashMap::new()),
            pending_context_window: Mutex::new(None),
        }
    }

    /// Drop bookkeeping for a chat that no longer exists in the store.
    pub fn forget_deleted_session(&self, session_id: &str) {
        self.pending_soft_respawn.lock().remove(session_id);
        let kids: Vec<PendingAcpChild> = {
            let mut list = self.pending_children.lock();
            let mut taken = Vec::new();
            list.retain(|c| {
                if c.session_id.as_deref() == Some(session_id) {
                    taken.push(c.clone());
                    false
                } else {
                    true
                }
            });
            taken
        };
        if !kids.is_empty() {
            tokio::spawn(async move {
                for child in kids {
                    SessionManager::kill_acp_bounded(&child.acp).await;
                }
            });
        }
    }

    pub(super) fn invalidate_prewarm_epoch(&self) {
        self.prewarm_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub(super) fn post_turn_journal_lock(
        &self,
        app_session_id: &str,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.post_turn_journal_locks.lock();
        Arc::clone(
            locks
                .entry(app_session_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /// True when any live or background session is mid-turn (streaming / tools / connect).
    /// Used by the host automation scheduler to avoid stealing agent slots.
    pub fn any_turn_busy(&self) -> bool {
        {
            let guard = self.inner.lock();
            if let Some(s) = guard.as_ref() {
                if s.prompt_in_flight
                    || matches!(
                        s.fsm.state(),
                        SessionState::Streaming
                            | SessionState::AwaitingPermission
                            | SessionState::Connecting
                    )
                    || !s.open_tool_ids.is_empty()
                {
                    return true;
                }
            }
        }
        let bg = self.background.lock();
        for s in bg.values() {
            if s.prompt_in_flight
                || matches!(
                    s.fsm.state(),
                    SessionState::Streaming
                        | SessionState::AwaitingPermission
                        | SessionState::Connecting
                )
                || !s.open_tool_ids.is_empty()
            {
                return true;
            }
        }
        false
    }
}
