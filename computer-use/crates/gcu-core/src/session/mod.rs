//! Session lifecycle, serial calls, window locks, and the stop latch.
//!
//! [`Session::begin_turn`] opens a turn. [`Session::turn_ended`] drops turn
//! grants and releases input. It does not clear the stop latch.
//! [`Session::session_closed`] drops session grants, cancels approval waits,
//! and releases input. Always grants are not stored here.
//!
//! [`Session::enqueue`] runs one session's calls one at a time. Stop does not
//! enter that queue: [`Session::handle_stop`] reads the Host latch and issues
//! [`InputRelease::release_all_input`] on its own. The Host holds the latch
//! across turns, reconnects, and engine restarts. [`Session::open`] records
//! [`UserActivity::latched_stop`] so a new engine process still starts stopped.
//! Nothing in this module clears the latch.
//!
//! Approval waits are not inside the session queue, so session close can cancel
//! them without waiting for an action to finish.

mod queue;
mod stop;

pub use queue::{SessionError, WindowGuard, WindowLocks, WINDOW_LOCK_WAIT};
pub use stop::{
    drive_native, InputRelease, NativeTimeout, StopTimings, NATIVE_CALL_TIMEOUT,
    RELEASE_INPUT_BUDGET, STOP_LATCH_BUDGET,
};

use crate::policy::{wait_for_app, wait_for_confirm, AppGrantWait, ConfirmWait, GrantTable};
use gcu_host_api::{
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, ImageRef, ModelReason,
    Risk, SessionCtx, UserActivity,
};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;

/// [`Session::begin_turn`] or [`Session::enqueue`] was called after close.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("session is closed")]
pub struct SessionClosed;

/// One computer-use session.
///
/// The stop latch is read from `activity` on every check. `started_stopped` is
/// only the value observed in [`Session::open`].
pub struct Session {
    id: String,
    activity: Arc<dyn UserActivity>,
    release: Arc<dyn InputRelease>,
    started_stopped: bool,
    closed: AtomicBool,
    turn_open: AtomicBool,
    generation: AtomicU64,
    grants: Mutex<GrantTable>,
    queue: tokio::sync::Mutex<()>,
    cancel: Cancel,
    pub(crate) actions: Mutex<crate::action::SessionActions>,
}

impl Session {
    /// Open a session. The stop latch is read once and stored as the restart bit.
    ///
    /// Blocking: none. The latch read does not wait. Timeout: none.
    /// Cancellation: none. Thread safety: the returned session is `Send + Sync`
    /// behind its internal locks. Engine code must not clear the latch.
    pub fn open(
        id: impl Into<String>,
        activity: Arc<dyn UserActivity>,
        release: Arc<dyn InputRelease>,
    ) -> Self {
        let started_stopped = activity.latched_stop();
        Self {
            id: id.into(),
            activity,
            release,
            started_stopped,
            closed: AtomicBool::new(false),
            turn_open: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            grants: Mutex::new(GrantTable::default()),
            queue: tokio::sync::Mutex::new(()),
            cancel: Cancel::new(),
            actions: Mutex::new(crate::action::SessionActions::default()),
        }
    }

    /// Session id.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// id does not change.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Live activity source used by policy and routing. Nonblocking; no timeout
    /// or cancellation. Engine callers must never clear Stop on this Host source.
    pub fn activity(&self) -> &dyn UserActivity {
        self.activity.as_ref()
    }

    /// Copy the bounded metadata ledger. Memory only; may wait on its mutex,
    /// with no timeout or cancellation. No input payloads are retained.
    pub fn action_ledger(&self) -> crate::action::ActionLedger {
        lock(&self.actions).ledger.clone()
    }

    pub(crate) fn turn_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn cancellation(&self) -> &Cancel {
        &self.cancel
    }

    /// Whether the latch was set when this session was created.
    ///
    /// This stays true after the user later resumes. Use [`Session::is_stopped`]
    /// for the live bit.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// bit does not change.
    #[must_use]
    pub fn started_stopped(&self) -> bool {
        self.started_stopped
    }

    /// Live stop latch. Reading it does not clear it.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// activity object is `Send + Sync`.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.activity.latched_stop()
    }

    /// Start a turn. Does not bump the turn generation.
    ///
    /// Call [`Session::turn_ended`] before the next turn. A second `begin_turn`
    /// without `turn_ended` keeps the same generation.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: atomic.
    pub fn begin_turn(&self) -> Result<(), SessionClosed> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SessionClosed);
        }
        self.turn_open.store(true, Ordering::Release);
        Ok(())
    }

    /// End the turn: drop turn grants and release input.
    ///
    /// Session grants remain. The stop latch is not cleared. The turn generation
    /// increments only when a turn was open, so a second call does not retire a
    /// grant that was recorded for a later turn.
    ///
    /// Blocking: waits for [`drive_native`] around `release_all_input`.
    /// Timeout: [`NATIVE_CALL_TIMEOUT`]. A timed-out release still leaves the
    /// turn ended. Cancellation: none. This is not cancelled by session close;
    /// close has its own release. Thread safety: the grant mutex is recovered
    /// if poisoned.
    pub async fn turn_ended(&self) {
        if self.turn_open.swap(false, Ordering::AcqRel) {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        lock(&self.grants).turn_ended();
        lock(&self.actions).snapshots.begin_turn();
        let _ = drive_native(self.release.release_all_input()).await;
    }

    /// Close the session: drop grants, cancel approval waits, then release input.
    ///
    /// Idempotent. The stop latch is not cleared. Cancellation of in-flight
    /// approvals happens before the release await.
    ///
    /// Blocking: waits for the release call. Timeout: [`NATIVE_CALL_TIMEOUT`]
    /// on that release. Cancellation: this method is the cancel signal for
    /// approval waits. Thread safety: atomic close flag, then the grant mutex.
    pub async fn session_closed(&self) {
        self.closed.store(true, Ordering::Release);
        self.turn_open.store(false, Ordering::Release);
        lock(&self.grants).session_closed();
        self.cancel.cancel();
        *lock(&self.actions) = crate::action::SessionActions::default();
        let _ = drive_native(self.release.release_all_input()).await;
    }

    /// Whether [`Session::session_closed`] has run.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: atomic.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Snapshot of turn and session grants.
    ///
    /// Blocking: may wait on the grant mutex. Timeout: none. Cancellation: none.
    /// Thread safety: poisoned mutexes are recovered.
    #[must_use]
    pub fn grants(&self) -> GrantTable {
        lock(&self.grants).clone()
    }

    /// Run `work` after earlier calls on this session finish.
    ///
    /// Does not consult the stop latch. Stop must not call this.
    ///
    /// Blocking: waits for the session queue, then for `work`. Timeout: none
    /// on the queue. Cancellation: a closed session returns [`SessionClosed`]
    /// and does not start `work`. Dropping this future while it is queued
    /// leaves the queue free once the future is dropped. Thread safety: `work`
    /// is `Send`.
    pub async fn enqueue<T, F>(&self, work: F) -> Result<T, SessionClosed>
    where
        T: Send,
        F: Future<Output = T> + Send,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(SessionClosed);
        }
        let _ticket = self.queue.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(SessionClosed);
        }
        Ok(work.await)
    }

    /// Serialize on this session, then on `window_id` across sessions.
    ///
    /// Blocking: waits for both locks. Timeout: the window wait is
    /// [`WINDOW_LOCK_WAIT`], then [`SessionError::Busy`]. The session queue
    /// itself has no timeout. Cancellation: a closed session returns
    /// [`SessionError::Closed`] and does not take the window lock. Dropping
    /// the future releases whichever lock it holds. Thread safety: `work` is
    /// `Send`. Not reentrant.
    pub async fn enqueue_window<T, F>(
        &self,
        locks: &WindowLocks,
        window_id: &str,
        work: F,
    ) -> Result<T, SessionError>
    where
        T: Send,
        F: Future<Output = T> + Send,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(SessionError::Closed);
        }
        let _ticket = self.queue.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(SessionError::Closed);
        }
        let _window = locks.acquire(window_id).await?;
        Ok(work.await)
    }

    /// Observe the stop latch and, when it is set, issue `release_all_input`.
    ///
    /// Does not take the session queue and does not clear the latch.
    /// `release_issued` is recorded before the release future is awaited, so a
    /// slow driver does not consume the 500 ms budget. The await itself is
    /// capped by [`NATIVE_CALL_TIMEOUT`].
    ///
    /// Blocking: when latched, waits up to the native timeout for release.
    /// Timeout: [`NATIVE_CALL_TIMEOUT`] on the release. The timings still
    /// return. Cancellation: none. Thread safety: does not share the action queue.
    pub async fn handle_stop(&self) -> StopTimings {
        let started = Instant::now();
        let latched = self.activity.latched_stop();
        let latch = started.elapsed();
        if !latched {
            return StopTimings {
                latch,
                release_issued: latch,
            };
        }
        let release_issued = started.elapsed();
        let release = self.release.clone();
        let _ = drive_native(async move { release.release_all_input().await }).await;
        StopTimings {
            latch,
            release_issued,
        }
    }

    /// Ask the user to authorize `app`, with timeout and session-close cancel.
    ///
    /// A closed session returns [`ApprovalDecision::Deny`] without calling
    /// `provider`. Turn grants are stored only when this turn is still the
    /// turn that started the wait. Session grants are stored until close.
    /// Always, deny, and timeout are not stored. The Host writes always grants.
    ///
    /// Blocking: waits for the user, `timeout`, or session close.
    /// Timeout: `timeout` returns [`ApprovalDecision::Timeout`]. A zero timeout
    /// returns timeout without polling the provider when close is not already
    /// set. Cancellation: [`Session::session_closed`] returns deny and dismisses
    /// the provider token. Thread safety: one grant mutex; callers on other
    /// sessions use their own.
    pub async fn request_app(
        &self,
        provider: &dyn ApprovalProvider,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        timeout: std::time::Duration,
    ) -> ApprovalDecision {
        if self.closed.load(Ordering::Acquire) {
            return ApprovalDecision::Deny;
        }
        let session = SessionCtx {
            session_id: self.id.clone(),
        };
        wait_for_app(AppGrantWait {
            provider,
            session: &session,
            app,
            reason,
            risk,
            timeout,
            session_cancel: &self.cancel,
            turn_generation: self.generation.load(Ordering::Acquire),
            generation: &self.generation,
            closed: &self.closed,
            turn_open: &self.turn_open,
            grants: &self.grants,
        })
        .await
    }

    /// Ask the user to confirm one action summary.
    ///
    /// Blocking: waits for the user, `timeout`, or session close.
    /// Timeout: returns `false`. Cancellation: session close returns `false`
    /// and dismisses the provider token. A closed session does not call
    /// `provider`. Thread safety: does not record a grant.
    pub async fn request_confirm(
        &self,
        provider: &dyn ApprovalProvider,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        timeout: std::time::Duration,
    ) -> bool {
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        let session = SessionCtx {
            session_id: self.id.clone(),
        };
        wait_for_confirm(ConfirmWait {
            provider,
            session: &session,
            app,
            summary,
            crop,
            timeout,
            session_cancel: &self.cancel,
            closed: &self.closed,
        })
        .await
    }

    /// Record a decision the caller already has.
    ///
    /// [`ApprovalDecision::Turn`] is stored only while a turn is open.
    /// [`ApprovalDecision::Session`] is stored until close. Always, deny, and
    /// timeout are ignored. A closed session stores nothing.
    ///
    /// Blocking: may wait on the grant mutex. Timeout: none. Cancellation:
    /// none. Thread safety: poisoned mutexes are recovered.
    pub fn record_decision(&self, decision: ApprovalDecision, app: &AppInfo) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let mut grants = lock(&self.grants);
        match decision {
            ApprovalDecision::Turn if self.turn_open.load(Ordering::Acquire) => {
                grants.record_turn(app);
            }
            ApprovalDecision::Session => grants.record_session(app),
            ApprovalDecision::Turn
            | ApprovalDecision::Always
            | ApprovalDecision::Deny
            | ApprovalDecision::Timeout => {}
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
