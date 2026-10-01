//! Host authorization attempts are valid only while their ticket is current.
use crate::broker::{ComputerUseBroker, StopCleanupTicket, StopState};
use crate::execution::ActionCancellation;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const CANCEL_TOMBSTONE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CANCELLED_ATTEMPTS_PER_SESSION: usize = 64;
const MAX_CANCELLED_ATTEMPTS_GLOBAL: usize = 256;

#[derive(Clone)]
pub struct AuthorizationTicket {
    pub session: String,
    pub run_id: String,
    pub attempt_id: String,
    pub selector_revision: u64,
    pub generation: u64,
    nonce: String,
    preparation: Arc<Preparation>,
}

#[derive(Default)]
struct Preparation {
    cancellation: ActionCancellation,
    completed: AtomicBool,
}

impl AuthorizationTicket {
    /// Native Host resources must belong to this exact preparation, not merely
    /// an equal run string or a replayed UI attempt. No nonce is exposed.
    pub fn same_preparation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.preparation, &other.preparation)
            && self.nonce == other.nonce
            && self.session == other.session
            && self.run_id == other.run_id
            && self.attempt_id == other.attempt_id
            && self.selector_revision == other.selector_revision
            && self.generation == other.generation
    }

    /// Sticky, read-only cancellation for Host preparation (native consent,
    /// parent export, queued UI binding). This is not model input authority.
    pub fn check_preparation(&self) -> Result<(), String> {
        if self.preparation.completed.load(Ordering::SeqCst) {
            return Err("target authorization preparation already completed".into());
        }
        self.preparation
            .cancellation
            .check()
            .map_err(|_| "target authorization was cancelled".into())
    }

    /// Revocation only: committing preparation does not revoke its native grant.
    pub fn is_preparation_cancelled(&self) -> bool {
        self.preparation.cancellation.check().is_err()
    }

    pub async fn preparation_cancelled(&self) {
        self.preparation.cancellation.cancelled().await;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpDesiredState {
    pub generation: u64,
    pub run_id: Option<String>,
    pub attempt_generation: Option<u64>,
}

/// Result of the synchronous half of a revoke/failure transition.
///
/// `cleanup` must be retained even when `error` is present. For example, an
/// exhausted catalog generation must not discard the Broker ticket that owns
/// the already-fenced surface cleanup.
#[derive(Debug)]
pub struct RevokeFence {
    /// Whether this fence matched the lifecycle state it intended to revoke.
    /// A stale authorization failure must remain a true Host-level no-op.
    pub matched: bool,
    pub cleanup: Option<StopCleanupTicket>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct CancelFence {
    pub cancelled: bool,
    pub revoke: RevokeFence,
}

impl RevokeFence {
    fn from_parts(
        fence: Result<Option<StopCleanupTicket>, crate::error::BrokerError>,
        error: Option<String>,
        matched: bool,
    ) -> Self {
        match fence {
            Ok(cleanup) => Self {
                matched,
                cleanup,
                error,
            },
            Err(fence_error) => Self {
                matched,
                cleanup: None,
                error: Some(match error {
                    Some(error) => format!("{fence_error}; {error}"),
                    None => fence_error.to_string(),
                }),
            },
        }
    }

    fn finish(self, broker: &ComputerUseBroker) -> Result<(), String> {
        let mut errors = self.error.into_iter().collect::<Vec<_>>();
        if let Some(ticket) = self.cleanup {
            if let Err(error) = broker.finish_stop_cleanup(&ticket) {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[derive(Clone)]
struct Slot {
    run_id: String,
    attempt_id: String,
    selector_revision: u64,
    generation: u64,
    nonce: String,
    pending: bool,
    revoked: bool,
    mcp_desired: bool,
    catalog_generation: u64,
    preparation: Arc<Preparation>,
}

impl Slot {
    fn cancel_preparation(&self) {
        // A completed attempt no longer owns preparation. Replacing it must
        // not revoke a previously accepted target if reauthorization fails.
        if self.pending {
            self.preparation.cancellation.cancel();
        }
    }
}

#[derive(Clone, Debug)]
struct CancelTombstone {
    attempt_id: String,
    selector_revision: u64,
    created_at: Instant,
    expires_at: Instant,
}

#[derive(Default)]
struct CancelBucket {
    tombstones: VecDeque<CancelTombstone>,
    /// A cancellation that could not be represented within the bucket cap.
    /// While set, every new authorization in this session fails closed.
    saturated_until: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationCapacityScope {
    Session,
    Global,
}

impl std::fmt::Display for CancellationCapacityScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session => formatter.write_str(
                "computer_use_cancel_capacity_session: authorization is blocked until cancellation state expires",
            ),
            Self::Global => formatter.write_str(
                "computer_use_cancel_capacity_global: authorization is blocked until cancellation state expires",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancellationTombstoneStatus {
    pub session_count: usize,
    pub total_count: usize,
    pub blocked: Option<CancellationCapacityScope>,
}

trait MonotonicClock: Send + Sync {
    fn now(&self) -> Instant;
}

struct SystemMonotonicClock;

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone, Copy)]
struct TombstoneLimits {
    ttl: Duration,
    per_session: usize,
    global: usize,
}

impl Default for TombstoneLimits {
    fn default() -> Self {
        Self {
            ttl: CANCEL_TOMBSTONE_TTL,
            per_session: MAX_CANCELLED_ATTEMPTS_PER_SESSION,
            global: MAX_CANCELLED_ATTEMPTS_GLOBAL,
        }
    }
}

#[derive(Default)]
struct GrantState {
    slots: HashMap<String, Slot>,
    /// Per-session tombstones close the cancel-before-begin IPC race without
    /// allowing one noisy session to evict another session's cancellation.
    cancelled_attempts: HashMap<String, CancelBucket>,
    /// Once the process-wide cap is exceeded, authorization stays fail-closed
    /// until the coarse guard expires. Existing tombstones are never evicted.
    global_saturated_until: Option<Instant>,
    next_generation: u64,
    next_catalog_generation: u64,
}

impl GrantState {
    fn purge_expired(&mut self, now: Instant) {
        self.cancelled_attempts.retain(|_, bucket| {
            bucket.tombstones.retain(|tombstone| {
                debug_assert!(tombstone.created_at <= tombstone.expires_at);
                tombstone.expires_at > now
            });
            if bucket
                .saturated_until
                .is_some_and(|expires_at| expires_at <= now)
            {
                bucket.saturated_until = None;
            }
            !bucket.tombstones.is_empty() || bucket.saturated_until.is_some()
        });
        if self
            .global_saturated_until
            .is_some_and(|expires_at| expires_at <= now)
        {
            self.global_saturated_until = None;
        }
    }

    fn total_cancelled_attempts(&self) -> usize {
        self.cancelled_attempts
            .values()
            .map(|bucket| bucket.tombstones.len())
            .sum()
    }

    fn cancellation_block(
        &self,
        session: &str,
        limits: TombstoneLimits,
    ) -> Option<CancellationCapacityScope> {
        if self.global_saturated_until.is_some() || self.total_cancelled_attempts() >= limits.global
        {
            return Some(CancellationCapacityScope::Global);
        }
        self.cancelled_attempts
            .get(session)
            .filter(|bucket| {
                bucket.saturated_until.is_some() || bucket.tombstones.len() >= limits.per_session
            })
            .map(|_| CancellationCapacityScope::Session)
    }

    fn consume_cancelled_attempt(
        &mut self,
        session: &str,
        attempt_id: &str,
        selector_revision: u64,
    ) -> bool {
        let mut consumed = false;
        let mut remove_bucket = false;
        if let Some(bucket) = self.cancelled_attempts.get_mut(session) {
            if let Some(index) = bucket.tombstones.iter().position(|tombstone| {
                tombstone.attempt_id == attempt_id
                    && tombstone.selector_revision == selector_revision
            }) {
                bucket.tombstones.remove(index);
                consumed = true;
            }
            remove_bucket = bucket.tombstones.is_empty() && bucket.saturated_until.is_none();
        }
        if remove_bucket {
            self.cancelled_attempts.remove(session);
        }
        consumed
    }

    fn remember_cancelled_attempt(
        &mut self,
        session: &str,
        attempt_id: &str,
        selector_revision: u64,
        now: Instant,
        limits: TombstoneLimits,
    ) -> Result<bool, CancellationCapacityScope> {
        if self.cancelled_attempts.get(session).is_some_and(|bucket| {
            bucket.tombstones.iter().any(|tombstone| {
                tombstone.attempt_id == attempt_id
                    && tombstone.selector_revision == selector_revision
            })
        }) {
            return Ok(false);
        }

        let expires_at = now
            .checked_add(limits.ttl)
            .expect("Computer Use cancellation TTL must fit Instant");
        if let Some(scope) = self.cancellation_block(session, limits) {
            match scope {
                CancellationCapacityScope::Session => {
                    self.cancelled_attempts
                        .entry(session.to_string())
                        .or_default()
                        .saturated_until = Some(expires_at);
                }
                CancellationCapacityScope::Global => {
                    self.global_saturated_until = Some(expires_at);
                }
            }
            return Err(scope);
        }

        self.cancelled_attempts
            .entry(session.to_string())
            .or_default()
            .tombstones
            .push_back(CancelTombstone {
                attempt_id: attempt_id.to_string(),
                selector_revision,
                created_at: now,
                expires_at,
            });
        Ok(true)
    }

    fn cancellation_status(
        &self,
        session: &str,
        limits: TombstoneLimits,
    ) -> CancellationTombstoneStatus {
        CancellationTombstoneStatus {
            session_count: self
                .cancelled_attempts
                .get(session)
                .map(|bucket| bucket.tombstones.len())
                .unwrap_or_default(),
            total_count: self.total_cancelled_attempts(),
            blocked: self.cancellation_block(session, limits),
        }
    }
}

pub struct SessionGrants {
    state: Mutex<GrantState>,
    clock: std::sync::Arc<dyn MonotonicClock>,
    limits: TombstoneLimits,
}

impl Default for SessionGrants {
    fn default() -> Self {
        Self {
            state: Mutex::new(GrantState::default()),
            clock: std::sync::Arc::new(SystemMonotonicClock),
            limits: TombstoneLimits::default(),
        }
    }
}

impl SessionGrants {
    #[cfg(test)]
    fn with_clock_and_limits(
        clock: std::sync::Arc<dyn MonotonicClock>,
        ttl: Duration,
        per_session: usize,
        global: usize,
    ) -> Self {
        assert!(ttl > Duration::ZERO);
        assert!(per_session > 0);
        assert!(global >= per_session);
        Self {
            state: Mutex::new(GrantState::default()),
            clock,
            limits: TombstoneLimits {
                ttl,
                per_session,
                global,
            },
        }
    }

    #[cfg(test)]
    fn exhaust_catalog_generation(&self) {
        self.state.lock().next_catalog_generation = u64::MAX;
    }

    pub fn current(&self, session: &str) -> Option<String> {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        state.slots.get(session).map(|slot| slot.run_id.clone())
    }

    pub fn sessions(&self) -> Vec<String> {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        state.slots.keys().cloned().collect()
    }

    pub fn mcp_desired(&self, session: &str) -> McpDesiredState {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        state
            .slots
            .get(session)
            .map(|slot| McpDesiredState {
                generation: slot.catalog_generation,
                run_id: slot.mcp_desired.then(|| slot.run_id.clone()),
                attempt_generation: slot.mcp_desired.then_some(slot.generation),
            })
            .unwrap_or(McpDesiredState {
                generation: 0,
                run_id: None,
                attempt_generation: None,
            })
    }

    pub fn cancellation_status(&self, session: &str) -> CancellationTombstoneStatus {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        state.cancellation_status(session, self.limits)
    }

    pub fn begin(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        requested: Option<&str>,
    ) -> Result<AuthorizationTicket, String> {
        self.begin_attempt(
            broker,
            session,
            requested,
            &format!("legacy:{}", uuid::Uuid::new_v4()),
            1,
        )
    }

    pub fn begin_attempt(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        requested: Option<&str>,
        attempt_id: &str,
        selector_revision: u64,
    ) -> Result<AuthorizationTicket, String> {
        if session.trim().is_empty() {
            return Err("select a local chat first".into());
        }
        let attempt_id = attempt_id.trim();
        if attempt_id.is_empty() || selector_revision == 0 {
            return Err("invalid Computer Use authorization attempt".into());
        }
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        if state.consume_cancelled_attempt(session, attempt_id, selector_revision) {
            return Err("target authorization was cancelled".into());
        }
        if let Some(scope) = state.cancellation_block(session, self.limits) {
            return Err(scope.to_string());
        }
        let previous = state.slots.get(session).cloned();
        if requested.is_some_and(|id| previous.as_ref().is_none_or(|s| s.run_id != id)) {
            return Err("run does not belong to this chat".into());
        }
        if previous.as_ref().is_some_and(|s| s.pending) {
            return Err("target authorization is already in progress".into());
        }
        let run_id = match previous {
            Some(slot) => match broker.stop_state(&slot.run_id).map_err(|e| e.to_string())? {
                StopState::Running => slot.run_id.clone(),
                StopState::StopRequested => return Err("previous task is still stopping".into()),
                StopState::Stopped => uuid::Uuid::new_v4().to_string(),
            },
            None => uuid::Uuid::new_v4().to_string(),
        };
        let generation = state
            .next_generation
            .checked_add(1)
            .ok_or("Computer Use authorization generation exhausted")?;
        let catalog_generation = state
            .next_catalog_generation
            .checked_add(1)
            .ok_or("Computer Use MCP catalog generation exhausted")?;
        broker
            .open_run(session, &run_id)
            .map_err(|e| e.to_string())?;
        state.next_generation = generation;
        state.next_catalog_generation = catalog_generation;
        let nonce = uuid::Uuid::new_v4().to_string();
        let preparation = Arc::new(Preparation::default());
        state.slots.insert(
            session.into(),
            Slot {
                run_id: run_id.clone(),
                attempt_id: attempt_id.to_string(),
                selector_revision,
                generation,
                nonce: nonce.clone(),
                pending: true,
                revoked: false,
                mcp_desired: false,
                catalog_generation,
                preparation: preparation.clone(),
            },
        );
        Ok(AuthorizationTicket {
            session: session.into(),
            run_id,
            attempt_id: attempt_id.to_string(),
            selector_revision,
            generation,
            nonce,
            preparation,
        })
    }

    pub fn is_current(&self, ticket: &AuthorizationTicket) -> bool {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        state.slots.get(&ticket.session).is_some_and(|slot| {
            slot.run_id == ticket.run_id
                && slot.attempt_id == ticket.attempt_id
                && slot.selector_revision == ticket.selector_revision
                && slot.generation == ticket.generation
                && slot.nonce == ticket.nonce
                && !slot.revoked
        })
    }

    pub fn request_mcp_attach(&self, ticket: &AuthorizationTicket) -> Result<u64, String> {
        let mut state = self.state.lock();
        let already_requested = state
            .slots
            .get(&ticket.session)
            .filter(|slot| {
                slot.run_id == ticket.run_id
                    && slot.attempt_id == ticket.attempt_id
                    && slot.selector_revision == ticket.selector_revision
                    && slot.generation == ticket.generation
                    && slot.nonce == ticket.nonce
                    && slot.pending
                    && !slot.revoked
            })
            .is_some_and(|slot| slot.mcp_desired);
        if already_requested {
            return Ok(state
                .slots
                .get(&ticket.session)
                .map(|slot| slot.catalog_generation)
                .unwrap_or_default());
        }
        let generation = state
            .next_catalog_generation
            .checked_add(1)
            .ok_or("Computer Use MCP catalog generation exhausted")?;
        let slot = state
            .slots
            .get_mut(&ticket.session)
            .filter(|slot| {
                slot.run_id == ticket.run_id
                    && slot.attempt_id == ticket.attempt_id
                    && slot.selector_revision == ticket.selector_revision
                    && slot.generation == ticket.generation
                    && slot.nonce == ticket.nonce
                    && slot.pending
            })
            .ok_or("target authorization was cancelled")?;
        slot.mcp_desired = true;
        slot.catalog_generation = generation;
        state.next_catalog_generation = generation;
        Ok(generation)
    }

    /// Callback publishes only Host state; never await or query native UI here.
    pub fn publish(
        &self,
        broker: &ComputerUseBroker,
        ticket: &AuthorizationTicket,
        finish: bool,
        publish: impl FnOnce(),
    ) -> Result<(), String> {
        match broker.stop_state(&ticket.run_id) {
            Ok(StopState::Running) => {}
            _ => return Err("target authorization was cancelled".into()),
        }
        if !broker.feature_enabled() {
            return Err("target authorization was cancelled".into());
        }
        let mut state = self.state.lock();
        let slot = state
            .slots
            .get_mut(&ticket.session)
            .filter(|s| {
                s.nonce == ticket.nonce
                    && s.run_id == ticket.run_id
                    && s.attempt_id == ticket.attempt_id
                    && s.selector_revision == ticket.selector_revision
                    && s.generation == ticket.generation
                    && s.pending
            })
            .ok_or("target authorization was cancelled")?;
        publish();
        if finish {
            slot.preparation.completed.store(true, Ordering::SeqCst);
            slot.pending = false;
        }
        Ok(())
    }

    /// Revoke the session's authority and return a generation-bound cleanup
    /// hand-off.  This method performs only local work: the Broker fence,
    /// credential revocation callback, and desired-absent publication.  It
    /// never calls an adapter or browser worker.
    pub fn fence_revoke_checked(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        clear: impl FnOnce(),
    ) -> RevokeFence {
        let mut state = self.state.lock();
        // Repeated stop/revoke calls are idempotent and must not create
        // unbounded catalog generations once the slot is already absent.
        let slot_snapshot = state.slots.get(session).cloned();
        let Some(slot_snapshot) = slot_snapshot else {
            clear();
            return RevokeFence {
                matched: false,
                cleanup: None,
                error: None,
            };
        };
        let should_advance = !slot_snapshot.revoked;
        let (catalog_generation, generation_error) = if should_advance {
            match state.next_catalog_generation.checked_add(1) {
                Some(generation) => (Some(generation), None),
                None => (
                    None,
                    Some("Computer Use MCP catalog generation exhausted".to_string()),
                ),
            }
        } else {
            (None, None)
        };

        // This is deliberately before desired-absent publication.  `clear`
        // is the local IPC/feature-flag revocation callback; it must not do
        // network or adapter work and must not re-enter SessionGrants.
        let fence = broker.fence_stop(&slot_snapshot.run_id);
        clear();

        if let Some(slot) = state.slots.get_mut(session) {
            if should_advance {
                slot.cancel_preparation();
                slot.nonce = uuid::Uuid::new_v4().to_string();
                slot.pending = false;
                slot.revoked = true;
                slot.mcp_desired = false;
            }
            if let Some(generation) = catalog_generation {
                slot.catalog_generation = generation;
                state.next_catalog_generation = generation;
            }
        }

        RevokeFence::from_parts(fence, generation_error, true)
    }

    /// Compatibility composite for synchronous callers.  New async Host
    /// paths should retain the returned ticket and finish it on a blocking
    /// boundary after local Stop has been acknowledged.
    pub fn revoke_checked(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        clear: impl FnOnce(),
    ) -> Result<(), String> {
        self.fence_revoke_checked(broker, session, clear)
            .finish(broker)
    }

    pub fn revoke(&self, broker: &ComputerUseBroker, session: &str, clear: impl FnOnce()) {
        let _ = self.revoke_checked(broker, session, clear);
    }

    /// Cancel one UI authorization attempt without allowing a late cleanup to
    /// revoke a newer attempt for the same chat. The tombstone also handles a
    /// cancellation command that overtakes authorize at the Host boundary.
    pub fn fence_cancel_attempt_checked(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        attempt_id: &str,
        selector_revision: u64,
        clear: impl FnOnce(),
    ) -> Result<CancelFence, String> {
        let session = session.trim();
        let attempt_id = attempt_id.trim();
        if session.is_empty() || attempt_id.is_empty() || selector_revision == 0 {
            return Err("invalid Computer Use authorization attempt".into());
        }

        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        let capacity_error = state
            .remember_cancelled_attempt(session, attempt_id, selector_revision, now, self.limits)
            .err();
        let catalog_generation = state.next_catalog_generation.checked_add(1);
        let run_id = state
            .slots
            .get_mut(session)
            .filter(|slot| {
                slot.attempt_id == attempt_id
                    && slot.selector_revision == selector_revision
                    && !slot.revoked
            })
            .map(|slot| {
                slot.cancel_preparation();
                slot.nonce = uuid::Uuid::new_v4().to_string();
                slot.pending = false;
                slot.revoked = true;
                slot.mcp_desired = false;
                if let Some(generation) = catalog_generation {
                    slot.catalog_generation = generation;
                }
                slot.run_id.clone()
            });
        if run_id.is_some() {
            if let Some(generation) = catalog_generation {
                state.next_catalog_generation = generation;
            }
        }
        let Some(run_id) = run_id else {
            return capacity_error.map_or_else(
                || {
                    Ok(CancelFence {
                        cancelled: false,
                        revoke: RevokeFence {
                            matched: false,
                            cleanup: None,
                            error: None,
                        },
                    })
                },
                |scope| Err(scope.to_string()),
            );
        };
        // `begin_attempt` takes the same state lock before opening a run.
        // Keep the old run's Broker fence and credential clear inside it so a
        // successor cannot be published between the two operations.
        let fence = broker.fence_stop(&run_id);
        clear();
        drop(state);
        let mut errors = capacity_error
            .map(|scope| scope.to_string())
            .into_iter()
            .collect::<Vec<_>>();
        if catalog_generation.is_none() {
            errors.push("Computer Use MCP catalog generation exhausted".into());
        }
        Ok(CancelFence {
            cancelled: true,
            revoke: RevokeFence::from_parts(
                fence,
                (!errors.is_empty()).then(|| errors.join("; ")),
                true,
            ),
        })
    }

    pub fn cancel_attempt_checked(
        &self,
        broker: &ComputerUseBroker,
        session: &str,
        attempt_id: &str,
        selector_revision: u64,
        clear: impl FnOnce(),
    ) -> Result<bool, String> {
        let fenced = self.fence_cancel_attempt_checked(
            broker,
            session,
            attempt_id,
            selector_revision,
            clear,
        )?;
        fenced.revoke.finish(broker)?;
        Ok(fenced.cancelled)
    }

    /// Fence a failed authorization without performing surface cleanup. A
    /// late failure does not revoke a newer target picked by the user.
    pub fn fence_fail_checked(
        &self,
        broker: &ComputerUseBroker,
        ticket: &AuthorizationTicket,
        clear: impl FnOnce(),
    ) -> RevokeFence {
        let mut state = self.state.lock();
        let catalog_generation = state.next_catalog_generation.checked_add(1);
        let run_id = state
            .slots
            .get_mut(&ticket.session)
            .filter(|s| {
                s.nonce == ticket.nonce
                    && s.run_id == ticket.run_id
                    && s.attempt_id == ticket.attempt_id
                    && s.selector_revision == ticket.selector_revision
                    && s.generation == ticket.generation
                    && !s.revoked
            })
            .map(|slot| {
                slot.cancel_preparation();
                slot.nonce = uuid::Uuid::new_v4().to_string();
                slot.pending = false;
                slot.revoked = true;
                slot.mcp_desired = false;
                if let Some(generation) = catalog_generation {
                    slot.catalog_generation = generation;
                }
                slot.run_id.clone()
            });
        if run_id.is_some() {
            if let Some(generation) = catalog_generation {
                state.next_catalog_generation = generation;
            }
        }
        if let Some(run_id) = run_id {
            // Failure cleanup is scoped to this ticket; do not let a later
            // authorization enter before the old credential is cleared.
            let fence = broker.fence_stop(&run_id);
            clear();
            drop(state);
            RevokeFence::from_parts(
                fence,
                catalog_generation
                    .is_none()
                    .then(|| "Computer Use MCP catalog generation exhausted".to_string()),
                true,
            )
        } else {
            RevokeFence {
                matched: false,
                cleanup: None,
                error: None,
            }
        }
    }

    /// A late failure must not revoke a newer target picked by the user.
    pub fn fail_checked(
        &self,
        broker: &ComputerUseBroker,
        ticket: &AuthorizationTicket,
        clear: impl FnOnce(),
    ) -> Result<(), String> {
        self.fence_fail_checked(broker, ticket, clear)
            .finish(broker)
    }

    pub fn fail(
        &self,
        broker: &ComputerUseBroker,
        ticket: &AuthorizationTicket,
        clear: impl FnOnce(),
    ) {
        let _ = self.fail_checked(broker, ticket, clear);
    }

    /// Remove process-local bookkeeping only after the App has confirmed both
    /// external cleanup and durable session deletion. This never stops a run;
    /// callers must complete that lifecycle before forgetting its fence.
    pub fn forget_session(&self, session: &str) {
        let now = self.clock.now();
        let mut state = self.state.lock();
        state.purge_expired(now);
        if let Some(slot) = state.slots.remove(session) {
            slot.cancel_preparation();
        }
        state.cancelled_attempts.remove(session);
    }
}

impl Drop for SessionGrants {
    fn drop(&mut self) {
        for slot in self.state.get_mut().slots.values() {
            slot.cancel_preparation();
        }
    }
}

#[cfg(test)]
#[path = "session_grants/preparation_tests.rs"]
mod preparation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{broker::BrokerOptions, fake::FakeAdapter};
    use std::sync::mpsc;
    use std::sync::Arc;

    struct FakeClock {
        now: Mutex<Instant>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock();
            *now = now
                .checked_add(duration)
                .expect("test clock duration must fit Instant");
        }
    }

    impl MonotonicClock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock()
        }
    }

    fn grants_with_limits(
        ttl: Duration,
        per_session: usize,
        global: usize,
    ) -> (Arc<FakeClock>, SessionGrants) {
        let clock = Arc::new(FakeClock::new());
        let grants = SessionGrants::with_clock_and_limits(
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
            ttl,
            per_session,
            global,
        );
        (clock, grants)
    }

    fn broker() -> ComputerUseBroker {
        ComputerUseBroker::new(
            Arc::new(FakeAdapter::new()),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
                ..BrokerOptions::default()
            },
        )
    }

    #[test]
    fn stopped_authorization_cannot_publish_late_enable() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants.begin(&broker, "chat", None).unwrap();
        grants.revoke(&broker, "chat", || {});
        assert!(grants
            .publish(&broker, &ticket, true, || panic!("late enable"))
            .is_err());
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::Stopped
        );
    }

    #[test]
    fn old_failure_cannot_stop_a_new_attempt() {
        let broker = broker();
        let grants = SessionGrants::default();
        let first = grants.begin(&broker, "chat", None).unwrap();
        assert!(grants.begin(&broker, "chat", None).is_err());
        grants.revoke(&broker, "chat", || {});
        let second = grants.begin(&broker, "chat", Some(&first.run_id)).unwrap();
        grants.fail(&broker, &first, || panic!("cleared new credential"));
        grants.publish(&broker, &second, true, || {}).unwrap();
        assert_ne!(first.run_id, second.run_id);
        assert_eq!(
            broker.stop_state(&second.run_id).unwrap(),
            StopState::Running
        );
    }

    #[test]
    fn run_reference_cannot_cross_chats_or_replace_pending_authorization() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants.begin(&broker, "a", None).unwrap();
        assert!(grants.begin(&broker, "b", Some(&ticket.run_id)).is_err());
        grants.publish(&broker, &ticket, false, || {}).unwrap();
        assert!(grants.begin(&broker, "a", None).is_err());
        grants.publish(&broker, &ticket, true, || {}).unwrap();
        let second = grants.begin(&broker, "a", None).unwrap();
        assert!(grants
            .publish(&broker, &ticket, true, || panic!("old ticket"))
            .is_err());
        assert_eq!(second.run_id, ticket.run_id);
    }

    #[test]
    fn late_publish_after_stop_cannot_enable() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants.begin(&broker, "chat", None).unwrap();
        broker.request_stop(&ticket.run_id).unwrap();
        assert!(grants
            .publish(&broker, &ticket, true, || panic!("late enable after stop"))
            .is_err());
        assert!(broker
            .authorize_target(&ticket.run_id, &FakeAdapter::new().fixture_id())
            .is_err());
    }

    #[test]
    fn deny_invalidates_ticket_before_authorize() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants.begin(&broker, "chat", None).unwrap();
        grants.fail(&broker, &ticket, || {});
        assert!(grants
            .publish(&broker, &ticket, true, || panic!("denied ticket"))
            .is_err());
    }

    #[test]
    fn host_attempt_identity_is_stored_and_fenced() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "ui-attempt-a", 7)
            .unwrap();
        assert_eq!(ticket.attempt_id, "ui-attempt-a");
        assert_eq!(ticket.selector_revision, 7);
        assert!(ticket.generation > 0);
        assert!(grants.is_current(&ticket));

        let mut wrong_attempt = ticket.clone();
        wrong_attempt.attempt_id = "ui-attempt-b".into();
        assert!(!grants.is_current(&wrong_attempt));

        let mut wrong_revision = ticket.clone();
        wrong_revision.selector_revision += 1;
        assert!(!grants.is_current(&wrong_revision));

        let mut wrong_generation = ticket.clone();
        wrong_generation.generation += 1;
        assert!(!grants.is_current(&wrong_generation));

        grants.publish(&broker, &ticket, true, || {}).unwrap();
        assert!(grants.is_current(&ticket));
        grants.revoke(&broker, "chat", || {});
        assert!(!grants.is_current(&ticket));
    }

    #[test]
    fn selector_revision_is_correlation_not_the_host_clock() {
        let broker = broker();
        let grants = SessionGrants::default();
        let first = grants
            .begin_attempt(&broker, "chat", None, "mounted-a", 1)
            .unwrap();
        grants.revoke(&broker, "chat", || {});
        let second = grants
            .begin_attempt(&broker, "chat", Some(&first.run_id), "mounted-b", 1)
            .unwrap();
        assert!(second.generation > first.generation);
        assert!(!grants.is_current(&first));
        assert!(grants.is_current(&second));
    }

    #[test]
    fn invalid_attempt_metadata_is_rejected_before_opening_a_run() {
        let broker = broker();
        let grants = SessionGrants::default();
        assert!(grants.begin_attempt(&broker, "chat", None, "", 1).is_err());
        assert!(grants
            .begin_attempt(&broker, "chat", None, "attempt", 0)
            .is_err());
        assert!(grants.current("chat").is_none());
    }

    #[test]
    fn mcp_desired_state_transitions_are_fenced_and_monotonic() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "attach-a", 1)
            .unwrap();

        let absent = grants.mcp_desired("chat");
        assert!(absent.generation > 0);
        assert_eq!(absent.run_id, None);
        assert_eq!(absent.attempt_generation, None);

        let present_generation = grants.request_mcp_attach(&ticket).unwrap();
        let present = grants.mcp_desired("chat");
        assert_eq!(present.generation, present_generation);
        assert_eq!(present.run_id.as_deref(), Some(ticket.run_id.as_str()));
        assert_eq!(present.attempt_generation, Some(ticket.generation));

        // A retry of the same attach is idempotent.
        assert_eq!(
            grants.request_mcp_attach(&ticket).unwrap(),
            present_generation
        );
        assert_eq!(grants.mcp_desired("chat"), present);

        grants.publish(&broker, &ticket, true, || {}).unwrap();
        assert_eq!(grants.mcp_desired("chat"), present);

        grants.revoke_checked(&broker, "chat", || {}).unwrap();
        let revoked = grants.mcp_desired("chat");
        assert!(revoked.generation > present.generation);
        assert_eq!(revoked.run_id, None);
        assert_eq!(revoked.attempt_generation, None);

        // Repeated revoke is a no-op from the catalog's point of view.
        grants.revoke_checked(&broker, "chat", || {}).unwrap();
        assert_eq!(grants.mcp_desired("chat"), revoked);
    }

    #[test]
    fn fenced_revoke_publishes_absent_and_revokes_before_surface_cleanup() {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_abort_blocked(true);
        let broker = ComputerUseBroker::new(
            fake.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
                ..BrokerOptions::default()
            },
        );
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "stop-fence", 1)
            .unwrap();
        broker
            .authorize_target(&ticket.run_id, &fake.fixture_id())
            .unwrap();
        grants.request_mcp_attach(&ticket).unwrap();
        grants.publish(&broker, &ticket, true, || {}).unwrap();

        let cleared = std::sync::atomic::AtomicBool::new(false);
        let fenced = grants.fence_revoke_checked(&broker, "chat", || {
            cleared.store(true, std::sync::atomic::Ordering::SeqCst)
        });
        assert_eq!(fenced.error, None);
        let cleanup = fenced.cleanup.expect("cleanup ticket");
        assert!(cleared.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(grants.mcp_desired("chat").run_id, None);
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::StopRequested
        );
        assert_eq!(fake.abort_called(), 0, "the fence must not clean surfaces");

        fake.set_abort_blocked(false);
        broker.finish_stop_cleanup(&cleanup).unwrap();
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::Stopped
        );
        assert_eq!(fake.abort_called(), 1);
        assert_eq!(fake.release_called(), 1);
    }

    #[test]
    fn catalog_generation_error_retains_the_already_fenced_cleanup_ticket() {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(
            fake.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
                ..BrokerOptions::default()
            },
        );
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "generation-error", 1)
            .unwrap();
        broker
            .authorize_target(&ticket.run_id, &fake.fixture_id())
            .unwrap();
        grants.request_mcp_attach(&ticket).unwrap();
        grants.publish(&broker, &ticket, true, || {}).unwrap();
        grants.exhaust_catalog_generation();

        let fenced = grants.fence_revoke_checked(&broker, "chat", || {});
        assert!(fenced
            .error
            .as_deref()
            .is_some_and(|error| error.contains("generation exhausted")));
        assert!(
            fenced.cleanup.is_some(),
            "cleanup ownership must not be lost"
        );
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::StopRequested
        );
        fenced
            .finish(&broker)
            .expect_err("generation error remains visible");
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::Stopped
        );
        assert_eq!(fake.abort_called(), 1);
        assert_eq!(fake.release_called(), 1);
        assert!(grants.mcp_desired("chat").run_id.is_none());
    }

    #[test]
    fn missing_broker_run_is_terminal_and_does_not_invent_a_cleanup_ticket() {
        let owner = broker();
        let unrelated = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&owner, "chat", None, "missing-run", 1)
            .unwrap();
        grants.request_mcp_attach(&ticket).unwrap();
        let cleared = std::sync::atomic::AtomicBool::new(false);

        let fenced = grants.fence_revoke_checked(&unrelated, "chat", || {
            cleared.store(true, std::sync::atomic::Ordering::SeqCst)
        });
        assert!(cleared.load(std::sync::atomic::Ordering::SeqCst));
        assert!(fenced.cleanup.is_none());
        assert!(fenced
            .error
            .as_deref()
            .is_some_and(|error| error.contains("run not found")));
        assert!(grants.mcp_desired("chat").run_id.is_none());
    }

    #[test]
    fn stale_ticket_cannot_request_mcp_after_new_attempt() {
        let broker = broker();
        let grants = SessionGrants::default();
        let first = grants
            .begin_attempt(&broker, "chat", None, "attach-a", 1)
            .unwrap();
        grants.revoke_checked(&broker, "chat", || {}).unwrap();
        let second = grants
            .begin_attempt(&broker, "chat", Some(&first.run_id), "attach-b", 1)
            .unwrap();

        assert!(grants.request_mcp_attach(&first).is_err());
        assert!(grants.mcp_desired("chat").run_id.is_none());
        let generation = grants.request_mcp_attach(&second).unwrap();
        assert_eq!(grants.mcp_desired("chat").generation, generation);
    }

    #[test]
    fn failed_attempt_publishes_absent_catalog_state() {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "attach-a", 1)
            .unwrap();
        let before = grants.mcp_desired("chat").generation;
        grants.request_mcp_attach(&ticket).unwrap();
        grants.fail_checked(&broker, &ticket, || {}).unwrap();
        let failed = grants.mcp_desired("chat");
        assert!(failed.generation > before);
        assert_eq!(failed.run_id, None);
        assert_eq!(failed.attempt_generation, None);
        assert!(grants.request_mcp_attach(&ticket).is_err());
    }

    #[test]
    fn cancel_before_begin_tombstones_the_exact_attempt() {
        let broker = broker();
        let grants = SessionGrants::default();
        assert!(!grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap());
        let error = grants
            .begin_attempt(&broker, "chat", None, "ui-a", 1)
            .err()
            .expect("the exact cancelled attempt must be rejected");
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(grants.cancellation_status("chat").session_count, 0);
        assert!(grants
            .begin_attempt(&broker, "chat", None, "ui-b", 1)
            .is_ok());
    }

    #[test]
    fn tombstone_is_live_just_before_expiry_and_consumed_on_match() {
        let ttl = Duration::from_secs(10);
        let (clock, grants) = grants_with_limits(ttl, 2, 4);
        let broker = broker();
        grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap();
        clock.advance(ttl - Duration::from_nanos(1));

        let error = grants
            .begin_attempt(&broker, "chat", None, "ui-a", 1)
            .err()
            .expect("a tombstone remains active until its exact deadline");
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(grants.cancellation_status("chat").session_count, 0);
    }

    #[test]
    fn duplicate_cancel_does_not_grow_or_extend_ttl() {
        let ttl = Duration::from_secs(10);
        let (clock, grants) = grants_with_limits(ttl, 2, 4);
        let broker = broker();
        grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap();
        clock.advance(Duration::from_secs(9));
        grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap();
        assert_eq!(grants.cancellation_status("chat").session_count, 1);

        clock.advance(Duration::from_secs(1));
        let expired = grants.cancellation_status("chat");
        assert_eq!(expired.session_count, 0);
        assert_eq!(expired.blocked, None);
        assert!(grants
            .begin_attempt(&broker, "chat", None, "ui-a", 1)
            .is_ok());
    }

    #[test]
    fn selector_revision_is_part_of_tombstone_identity() {
        let broker = broker();
        let grants = SessionGrants::default();
        grants
            .cancel_attempt_checked(&broker, "chat", "same-attempt", 1, || {})
            .unwrap();

        let current = grants
            .begin_attempt(&broker, "chat", None, "same-attempt", 2)
            .expect("a distinct selector revision must not consume the tombstone");
        assert!(grants.is_current(&current));
        let error = grants
            .begin_attempt(&broker, "chat", None, "same-attempt", 1)
            .err()
            .expect("the exact old revision remains cancelled");
        assert!(error.contains("cancelled"), "{error}");
        assert!(grants.is_current(&current));
        assert_eq!(grants.cancellation_status("chat").session_count, 0);
    }

    #[test]
    fn per_session_capacity_fails_closed_without_opening_a_run() {
        let ttl = Duration::from_secs(10);
        let (clock, grants) = grants_with_limits(ttl, 2, 8);
        let broker = broker();
        for attempt in ["ui-a", "ui-b"] {
            grants
                .cancel_attempt_checked(&broker, "chat", attempt, 1, || {})
                .unwrap();
        }
        assert_eq!(
            grants.cancellation_status("chat"),
            CancellationTombstoneStatus {
                session_count: 2,
                total_count: 2,
                blocked: Some(CancellationCapacityScope::Session),
            }
        );

        let runs_before = broker.test_run_count();
        let begin_error = grants
            .begin_attempt(&broker, "chat", None, "ui-c", 1)
            .err()
            .expect("a saturated session must reject authorization");
        assert!(begin_error.contains("capacity_session"), "{begin_error}");
        assert_eq!(broker.test_run_count(), runs_before);
        let cancel_error = grants
            .cancel_attempt_checked(&broker, "chat", "ui-c", 1, || {})
            .expect_err("an unrecordable cancellation must be observable");
        assert!(cancel_error.contains("capacity_session"), "{cancel_error}");
        assert_eq!(grants.cancellation_status("chat").session_count, 2);

        clock.advance(ttl);
        assert_eq!(grants.cancellation_status("chat").blocked, None);
        assert!(grants
            .begin_attempt(&broker, "chat", None, "ui-c", 1)
            .is_ok());
    }

    #[test]
    fn global_capacity_never_evicts_another_session_tombstone() {
        let ttl = Duration::from_secs(10);
        let (clock, grants) = grants_with_limits(ttl, 2, 3);
        let broker = broker();
        grants
            .cancel_attempt_checked(&broker, "protected", "keep-me", 1, || {})
            .unwrap();
        for attempt in ["fill-a", "fill-b"] {
            grants
                .cancel_attempt_checked(&broker, "noisy", attempt, 1, || {})
                .unwrap();
        }
        assert_eq!(
            grants.cancellation_status("new-session").blocked,
            Some(CancellationCapacityScope::Global)
        );
        let overflow = grants
            .cancel_attempt_checked(&broker, "overflow", "late-cancel", 1, || {})
            .expect_err("the global cap must be observable");
        assert!(overflow.contains("capacity_global"), "{overflow}");
        assert_eq!(grants.cancellation_status("protected").session_count, 1);

        let protected = grants
            .begin_attempt(&broker, "protected", None, "keep-me", 1)
            .err()
            .expect("the protected cancellation must not be evicted");
        assert!(protected.contains("cancelled"), "{protected}");
        let runs_before = broker.test_run_count();
        let globally_blocked = grants
            .begin_attempt(&broker, "another", None, "new", 1)
            .err()
            .expect("overflow guard must remain fail closed");
        assert!(globally_blocked.contains("capacity_global"));
        assert_eq!(broker.test_run_count(), runs_before);

        clock.advance(ttl);
        assert_eq!(grants.cancellation_status("another").blocked, None);
        assert!(grants
            .begin_attempt(&broker, "another", None, "new", 1)
            .is_ok());
    }

    #[test]
    fn a_new_registry_does_not_restore_process_local_tombstones() {
        let ttl = Duration::from_secs(10);
        let (clock, first) = grants_with_limits(ttl, 2, 4);
        let broker = broker();
        first
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap();
        assert_eq!(first.cancellation_status("chat").session_count, 1);

        let restarted = SessionGrants::with_clock_and_limits(
            Arc::clone(&clock) as Arc<dyn MonotonicClock>,
            ttl,
            2,
            4,
        );
        assert_eq!(restarted.cancellation_status("chat").session_count, 0);
        assert!(restarted
            .begin_attempt(&broker, "chat", None, "ui-a", 1)
            .is_ok());
    }

    #[test]
    fn forget_session_removes_only_its_slot_and_tombstones() {
        let broker = broker();
        let grants = SessionGrants::default();
        for session in ["gone", "keep"] {
            grants
                .cancel_attempt_checked(&broker, session, "cancelled", 1, || {})
                .unwrap();
            grants
                .begin_attempt(&broker, session, None, "active", 1)
                .unwrap();
        }

        grants.forget_session("gone");
        grants.forget_session("gone");
        assert_eq!(grants.current("gone"), None);
        assert_eq!(grants.cancellation_status("gone").session_count, 0);
        assert!(grants.current("keep").is_some());
        assert_eq!(grants.cancellation_status("keep").session_count, 1);
        assert_eq!(grants.sessions(), vec!["keep".to_string()]);
    }

    #[test]
    fn exact_cancel_revokes_completed_attempt_but_not_its_successor() {
        let broker = broker();
        let grants = SessionGrants::default();
        let first = grants
            .begin_attempt(&broker, "chat", None, "ui-a", 1)
            .unwrap();
        grants.publish(&broker, &first, true, || {}).unwrap();
        assert!(grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap());
        assert_eq!(
            broker.stop_state(&first.run_id).unwrap(),
            StopState::Stopped
        );

        let second = grants
            .begin_attempt(&broker, "chat", Some(&first.run_id), "ui-b", 1)
            .unwrap();
        assert!(!grants
            .cancel_attempt_checked(&broker, "chat", "ui-a", 1, || {})
            .unwrap());
        assert_eq!(
            broker.stop_state(&second.run_id).unwrap(),
            StopState::Running
        );
        assert!(grants.is_current(&second));
    }

    #[test]
    fn cancellation_and_failure_serialize_credential_clear_with_next_authorization() {
        for fail in [false, true] {
            let broker = Arc::new(broker());
            let grants = Arc::new(SessionGrants::default());
            let first = grants
                .begin_attempt(&broker, "chat", None, "ui-a", 1)
                .unwrap();
            grants.publish(&broker, &first, true, || {}).unwrap();

            let (clear_entered_tx, clear_entered_rx) = mpsc::channel();
            let (release_clear_tx, release_clear_rx) = mpsc::channel();
            let revoke_grants = Arc::clone(&grants);
            let revoke_broker = Arc::clone(&broker);
            let revoke_ticket = first.clone();
            let revoke = std::thread::spawn(move || {
                let clear = || {
                    clear_entered_tx.send(()).unwrap();
                    release_clear_rx
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap();
                    assert!(
                        revoke_grants.state.try_lock().is_none(),
                        "credential clear must be serialized with the next begin"
                    );
                };
                if fail {
                    revoke_grants.fence_fail_checked(&revoke_broker, &revoke_ticket, clear)
                } else {
                    revoke_grants
                        .fence_cancel_attempt_checked(&revoke_broker, "chat", "ui-a", 1, clear)
                        .unwrap()
                        .revoke
                }
            });
            clear_entered_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();

            let (begin_started_tx, begin_started_rx) = mpsc::channel();
            let (begin_done_tx, begin_done_rx) = mpsc::channel();
            let next_grants = Arc::clone(&grants);
            let next_broker = Arc::clone(&broker);
            let old_run = first.run_id.clone();
            let begin = std::thread::spawn(move || {
                begin_started_tx.send(()).unwrap();
                begin_done_tx
                    .send(next_grants.begin_attempt(
                        &next_broker,
                        "chat",
                        Some(&old_run),
                        "ui-b",
                        1,
                    ))
                    .unwrap();
            });
            begin_started_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert_eq!(
                begin_done_rx.recv_timeout(Duration::from_millis(100)).err(),
                Some(mpsc::RecvTimeoutError::Timeout),
                "a new authorization must wait until old credentials are cleared"
            );
            release_clear_tx.send(()).unwrap();
            let fenced = revoke.join().unwrap();
            begin.join().unwrap();
            assert!(begin_done_rx.recv().unwrap().is_err());
            if let Some(cleanup) = fenced.cleanup {
                broker.finish_stop_cleanup(&cleanup).unwrap();
            }
            let next = grants
                .begin_attempt(&broker, "chat", Some(&first.run_id), "ui-b", 1)
                .unwrap();
            assert!(grants.is_current(&next));
        }
    }

    #[test]
    fn cancellation_fence_acknowledges_before_blocked_cleanup_and_releases_once() {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_abort_blocked(true);
        let broker = ComputerUseBroker::new(
            fake.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
                ..BrokerOptions::default()
            },
        );
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "cancel-fence", 1)
            .unwrap();
        broker
            .authorize_target(&ticket.run_id, &fake.fixture_id())
            .unwrap();
        grants.request_mcp_attach(&ticket).unwrap();
        grants.publish(&broker, &ticket, true, || {}).unwrap();

        let cleared = std::sync::atomic::AtomicBool::new(false);
        let fenced = grants
            .fence_cancel_attempt_checked(&broker, "chat", "cancel-fence", 1, || {
                cleared.store(true, std::sync::atomic::Ordering::SeqCst)
            })
            .unwrap();
        assert!(fenced.cancelled);
        assert_eq!(fenced.revoke.error, None);
        let cleanup = fenced.revoke.cleanup.expect("cleanup ticket");
        assert!(cleared.load(std::sync::atomic::Ordering::SeqCst));
        assert!(grants.mcp_desired("chat").run_id.is_none());
        assert!(!grants.is_current(&ticket));
        assert_eq!(
            broker.stop_state(&ticket.run_id).unwrap(),
            StopState::StopRequested
        );
        assert_eq!(fake.abort_called(), 0, "the fence must not clean surfaces");
        assert_eq!(fake.release_called(), 0);

        fake.set_abort_blocked(false);
        assert_eq!(
            broker.finish_stop_cleanup(&cleanup).unwrap(),
            StopState::Stopped
        );
        assert_eq!(fake.abort_called(), 1);
        assert_eq!(fake.release_called(), 1);

        assert_eq!(
            broker.finish_stop_cleanup(&cleanup).unwrap(),
            StopState::Stopped
        );
        assert_eq!(fake.abort_called(), 1);
        assert_eq!(fake.release_called(), 1);
    }

    #[test]
    fn yolo_never_issues_a_grant() {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(
            fake.clone(),
            BrokerOptions {
                feature_enabled: true,
                lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
                ..BrokerOptions::default()
            },
        );
        broker.open_run("chat", "run").unwrap();
        let mut req = crate::protocol::ActionRequest {
            version: crate::protocol::PROTOCOL_VERSION,
            action_id: "a1".into(),
            run_id: "run".into(),
            target_id: fake.fixture_id(),
            target_generation: 1,
            snapshot_id: "s".into(),
            geometry_revision: 1,
            action: crate::protocol::ActionKind::Click,
            target: crate::protocol::ActionTarget::Element {
                element_ref: "n1".into(),
            },
            parameters: serde_json::json!({"yolo": true}),
        };
        let out = broker.act(req.clone());
        assert!(!out.executed);
        assert!(fake.executions().is_empty());
        req.parameters = serde_json::json!({});
        let out = broker.act(req);
        assert_eq!(out.kind, crate::protocol::OutcomeKind::Rejected);
        assert!(fake.executions().is_empty());
    }
}
