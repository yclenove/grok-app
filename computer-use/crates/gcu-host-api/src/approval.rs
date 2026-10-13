//! Approval requests. The Host displays them. The engine waits.

use crate::types::{ActionSummary, AppInfo, ImageRef, ModelReason, Risk, SessionCtx};
use async_trait::async_trait;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// How the user resolved an app-authorization request.
///
/// [`ApprovalDecision::Turn`] is the per-turn grant. Older drafts called it `Once`.
/// [`ApprovalDecision::Always`] means the user asked for a persistent grant. The
/// Host writes that grant. The engine does not: there is no `grant_always` on
/// [`crate::PolicyStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApprovalDecision {
    /// Grant the current turn only. The engine drops it when the turn ends.
    Turn,
    /// Grant until this session ends.
    Session,
    /// The user chose a persistent grant. The Host stores it.
    Always,
    /// The user denied the request, or the request was cancelled.
    Deny,
    /// The Host's wait ended with no decision.
    Timeout,
}

/// Why the engine needs the user to take the window.
///
/// This is engine-produced. It is not a [`ModelReason`] and must not be labeled
/// as model-provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffReason {
    /// The app is waiting for a login.
    Login,
    /// The app is showing a captcha or challenge.
    Captcha,
    /// A system prompt is in front of the target (UAC, polkit, SecurityAgent).
    SystemPrompt,
    /// Engine-written plain text that is not one of the cases above.
    Other(String),
}

/// Shared cancellation flag for one approval wait.
///
/// Clones share the flag. Dropping a clone does not cancel the others, and
/// dropping the last clone does not cancel either. Session close must call
/// [`Cancel::cancel`] on the token it handed to the pending request.
///
/// This is not Tokio's `CancellationToken`. `gcu-host-api` cannot depend on
/// tokio (architecture §2), so the flag and the waiter list use `std`.
#[derive(Clone)]
pub struct Cancel {
    inner: Arc<CancelInner>,
}

struct CancelInner {
    cancelled: AtomicBool,
    wakers: std::sync::Mutex<Vec<Waker>>,
}

impl fmt::Debug for Cancel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Default for Cancel {
    fn default() -> Self {
        Self::new()
    }
}

impl Cancel {
    /// Create a token that is not cancelled.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// token is `Send + Sync`.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancelInner {
                cancelled: AtomicBool::new(false),
                wakers: std::sync::Mutex::new(Vec::new()),
            }),
        }
    }

    /// Signal cancellation and wake every waiter.
    ///
    /// Idempotent. A second call does not clear the flag.
    ///
    /// Blocking: none. It does not wait for the approval future to observe the
    /// flag. Timeout: none. Cancellation: this is the signal. Thread safety:
    /// safe to call from any thread that holds a clone.
    pub fn cancel(&self) {
        let wakers = {
            let mut wakers = crate::lock(&self.inner.wakers);
            self.inner.cancelled.store(true, Ordering::Release);
            std::mem::take(&mut *wakers)
        };
        for waker in wakers {
            waker.wake();
        }
    }

    /// Whether [`Cancel::cancel`] has been called.
    ///
    /// Blocking: none. Timeout: none. Cancellation: this only reads the flag.
    /// Thread safety: lock-free.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Resolve when the token is cancelled.
    ///
    /// If it is already cancelled, the future is ready on the first poll.
    ///
    /// Blocking: the future waits until [`Cancel::cancel`]. Timeout: none.
    /// Cancellation: this future is the wait. Thread safety: the future is
    /// `Send`. Poll it from one task.
    #[must_use = "the future does nothing unless awaited"]
    pub fn cancelled(&self) -> Cancelled {
        Cancelled {
            inner: Arc::clone(&self.inner),
        }
    }

    pub(crate) fn register(&self, waker: &Waker) {
        let mut wakers = crate::lock(&self.inner.wakers);
        if self.is_cancelled() {
            drop(wakers);
            waker.wake_by_ref();
            return;
        }
        crate::register_waker(&mut wakers, waker);
    }
}

/// Future from [`Cancel::cancelled`].
pub struct Cancelled {
    inner: Arc<CancelInner>,
}

impl fmt::Debug for Cancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Cancelled")
            .field("cancelled", &self.inner.cancelled.load(Ordering::Acquire))
            .finish()
    }
}

impl Future for Cancelled {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let cancel = Cancel {
            inner: Arc::clone(&self.inner),
        };
        if cancel.is_cancelled() {
            return Poll::Ready(());
        }
        cancel.register(context.waker());
        if cancel.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// User-facing authorization. Implemented by the Host, called by the engine.
///
/// `request_app` and `confirm_action` must return when their [`Cancel`] token
/// is signalled. Session close cancels every token for that session. A
/// cancelled `request_app` returns [`ApprovalDecision::Deny`]. A cancelled
/// `confirm_action` returns `false`.
#[async_trait]
pub trait ApprovalProvider: Send + Sync {
    /// Ask the user to authorize `app`.
    ///
    /// `reason`, when present, is model-written plain text. Label it as
    /// model-provided. Do not treat it as an OS fact. `app.identity` is the
    /// OS-verified identity.
    ///
    /// Blocking: awaits a user decision, the Host timeout, or `cancel`.
    /// Timeout: the Host waits [`crate::PolicyConfig::timeout_secs`] (default
    /// [`crate::DEFAULT_APPROVAL_TIMEOUT_SECS`] seconds) and returns
    /// [`ApprovalDecision::Timeout`]. This method takes no duration.
    /// Cancellation: signal `cancel`, including on session close. The pending
    /// UI is dismissed and this returns [`ApprovalDecision::Deny`] without
    /// waiting out the timeout. Cancellation beats a late click. Dropping the
    /// future does not dismiss Host UI; the caller must cancel the token.
    /// Thread safety: `Send + Sync`. Different sessions may call concurrently.
    /// The engine serializes one session.
    async fn request_app(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: Option<&ModelReason>,
        risk: Risk,
        cancel: &Cancel,
    ) -> ApprovalDecision;

    /// Ask the user to confirm one sensitive action.
    ///
    /// `summary.snapshot_id` and `summary.action_digest` bind the dialog to the
    /// action the engine will run. `crop` is an in-memory image. Copy the bytes
    /// if the UI keeps them after this call returns. There is no file path.
    ///
    /// Returns `true` only when the user confirms that exact summary. Returns
    /// `false` when the user denies, the Host times out, or `cancel` fires.
    /// `false` means the action must not run.
    ///
    /// Blocking: awaits the user, the Host timeout, or `cancel`.
    /// Timeout: the same Host timeout as [`ApprovalProvider::request_app`].
    /// On timeout, return `false`.
    /// Cancellation: session close cancels the token. Return `false` promptly.
    /// Dropping the future does not dismiss the UI.
    /// Thread safety: `Send + Sync`. Different sessions may call concurrently.
    async fn confirm_action(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        cancel: &Cancel,
    ) -> bool;

    /// Tell the user they must take over the app.
    ///
    /// `reason` is produced by the engine, not the model.
    ///
    /// Blocking: may await until the user dismisses the handoff or `cancel` fires.
    /// Timeout: none. This is not an approval and does not time out into
    /// [`ApprovalDecision::Timeout`].
    /// Cancellation: session close cancels the token. Dismiss the UI and return.
    /// Dropping the future does not dismiss the UI.
    /// Thread safety: `Send + Sync`.
    async fn handoff(
        &self,
        session: &SessionCtx,
        app: &AppInfo,
        reason: HandoffReason,
        cancel: &Cancel,
    );
}
