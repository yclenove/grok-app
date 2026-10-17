//! In-memory approval desk. Disk IO happens only when an Always grant is stored.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::auth::ProcessInstance;
use crate::messages::{Decision, Empty, Identity, Policy, PolicyChanged, RequestApp};
use crate::server::{Peer, RPC_TIMEOUT};
use crate::supervisor::baseline_policy;

use super::disk::{read_policy, write_policy};
use super::text::sanitize_plain;
use super::{
    timeout_secs, Act, BindFault, CardView, Claim, Fault, ImageBinding, Notice, Outcome, Settled,
};

type Hook = Arc<dyn Fn(u64) -> bool + Send + Sync>;

struct Pending {
    session_id: String,
    image: ImageBinding,
    engine: ProcessInstance,
    policy_version: u64,
    publishing: Option<u64>,
    /// Session close or disconnect won while a publish was in flight.
    void: bool,
    tx: Option<oneshot::Sender<Outcome>>,
}

struct Inner {
    policy: Policy,
    disk: Option<PathBuf>,
    engine: Option<ProcessInstance>,
    peer: Option<Peer>,
    hook: Option<Hook>,
    pending: HashMap<u64, Pending>,
    consumed: HashSet<u64>,
    rollback: Option<Policy>,
    events: Option<mpsc::UnboundedSender<Notice>>,
}

/// Shared approval desk. Clone is cheap. Decisions are serialized by the mutex.
#[derive(Clone)]
pub struct Office {
    inner: Arc<Mutex<Inner>>,
}

/// Waiter returned by [`Office::bind`]. Dropping it consumes the request as a timeout.
pub struct Wait {
    office: Office,
    request_id: u64,
    deadline: Instant,
    rx: oneshot::Receiver<Outcome>,
    armed: bool,
}

impl Office {
    /// Load `path` when it contains a policy. Otherwise start from the baseline.
    /// A corrupt file grants nothing. `None` keeps the policy in memory only.
    pub fn open(path: Option<PathBuf>) -> Self {
        let policy = path
            .as_ref()
            .and_then(|path| read_policy(path))
            .unwrap_or_else(baseline_policy);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                policy,
                disk: path,
                engine: None,
                peer: None,
                hook: None,
                pending: HashMap::new(),
                consumed: HashSet::new(),
                rollback: None,
                events: None,
            })),
        }
    }

    /// Remember the authenticated engine. Replacing it denies every pending card.
    pub fn attach(&self, engine: ProcessInstance, peer: Option<Peer>) {
        let mut inner = lock(&self.inner);
        if inner.engine.as_ref() != Some(&engine) {
            deny_all(&mut inner, Settled::Closed);
        }
        inner.engine = Some(engine);
        inner.peer = peer;
    }

    /// Test double for `engine.policy.changed`. When set, the live peer is not called.
    pub fn set_publish_hook(&self, hook: Hook) {
        lock(&self.inner).hook = Some(hook);
    }

    /// Current policy, including grants written by a consumed Always.
    pub fn policy(&self) -> Policy {
        lock(&self.inner).policy.clone()
    }

    /// UI notices. A second call replaces the previous subscriber.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<Notice> {
        let (tx, rx) = mpsc::unbounded_channel();
        lock(&self.inner).events = Some(tx);
        rx
    }

    /// Bind one real request to the attached engine and the current policy version.
    ///
    /// The returned [`Wait`] resolves on a decision, a timeout, or session close.
    /// Dropping it times the request out. No other method creates a pending card.
    pub fn bind(
        &self,
        request_id: u64,
        request: &RequestApp,
        opened_ms: u64,
    ) -> Result<(CardView, Wait), BindFault> {
        let mut inner = lock(&self.inner);
        if inner.consumed.contains(&request_id) || inner.pending.contains_key(&request_id) {
            return Err(BindFault::Replay);
        }
        let Some(engine) = inner.engine.clone() else {
            return Err(BindFault::NoEngine);
        };
        let image = ImageBinding::from_app(&request.app);
        if image.key.is_empty() || image.image_path.is_empty() {
            return Err(BindFault::Unverified);
        }
        let (tx, rx) = oneshot::channel();
        let card = card_view(
            request_id,
            request,
            &image,
            &engine,
            &inner.policy,
            opened_ms,
        );
        inner.pending.insert(
            request_id,
            Pending {
                session_id: request.session_id.clone(),
                image,
                engine,
                policy_version: card.policy_version,
                publishing: None,
                void: false,
                tx: Some(tx),
            },
        );
        emit(
            &inner,
            Notice::Pending {
                card: Box::new(card.clone()),
            },
        );
        let deadline = Instant::now() + Duration::from_secs(timeout_secs(card.timeout_secs));
        drop(inner);
        Ok((
            card,
            Wait {
                office: self.clone(),
                request_id,
                deadline,
                rx,
                armed: true,
            },
        ))
    }

    /// Consume `claim` at most once. Always is stored before it is published.
    ///
    /// A failed publish rolls the grant back and leaves the card pending.
    /// Cancellation: this method waits on the publisher; it does not time out
    /// itself. The publisher uses the ordinary five-second RPC budget.
    pub async fn decide(&self, claim: Claim) -> Result<(), Fault> {
        let prepared = {
            let mut inner = lock(&self.inner);
            prepare(&mut inner, &claim)?
        };
        let Prepared::Publish {
            request_id,
            version,
        } = prepared
        else {
            return Ok(());
        };
        if !self.still_publishing(request_id, version) {
            self.abort_publish(request_id, version);
            return Err(Fault::Failed);
        }
        let published = self.publish(version).await;
        if !self.still_publishing(request_id, version) {
            if published {
                self.revert_published(request_id, version).await;
            } else {
                self.abort_publish(request_id, version);
            }
            return Err(Fault::Failed);
        }
        if !published {
            self.abort_publish(request_id, version);
            return Err(Fault::Failed);
        }
        self.commit_publish(request_id, version);
        Ok(())
    }

    /// Deny every pending card for `session_id`. Other sessions stay.
    pub fn close_session(&self, session_id: &str) {
        let mut inner = lock(&self.inner);
        let ids: Vec<u64> = inner
            .pending
            .iter()
            .filter(|(_, pending)| pending.session_id == session_id)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            void_or_finish(&mut inner, id, Settled::Deny);
        }
    }

    /// Deny every pending card and forget the engine. Used on disconnect.
    ///
    /// Nonblocking. Does not wait for an in-flight publish; that publish rolls
    /// the grant back when it finishes.
    pub fn detach(&self) {
        let mut inner = lock(&self.inner);
        deny_all(&mut inner, Settled::Closed);
        inner.engine = None;
        inner.peer = None;
    }

    /// Deny every pending card and keep the attached engine. Feature-off uses this.
    ///
    /// Nonblocking, same publish rollback rule as [`Self::detach`].
    pub fn deny_pending(&self) {
        let mut inner = lock(&self.inner);
        deny_all(&mut inner, Settled::Deny);
    }

    fn still_publishing(&self, request_id: u64, version: u64) -> bool {
        lock(&self.inner)
            .pending
            .get(&request_id)
            .is_some_and(|pending| pending.publishing == Some(version) && !pending.void)
    }

    fn abort_publish(&self, request_id: u64, version: u64) {
        let mut inner = lock(&self.inner);
        rollback(&mut inner, version);
        clear_publish(&mut inner, request_id);
    }

    fn commit_publish(&self, request_id: u64, version: u64) {
        let mut inner = lock(&self.inner);
        let live = inner
            .pending
            .get(&request_id)
            .is_some_and(|pending| pending.publishing == Some(version));
        if !live {
            rollback(&mut inner, version);
            return;
        }
        inner.rollback = None;
        finish(
            &mut inner,
            request_id,
            Outcome::Decision(Decision::Always),
            Settled::Grant,
        );
    }

    async fn revert_published(&self, request_id: u64, version: u64) {
        let next = {
            let mut inner = lock(&self.inner);
            let next = if inner.policy.version == version {
                inner.rollback.take().map(|mut old| {
                    old.version = version.saturating_add(1);
                    let _ = store(&inner.disk, &old);
                    let next = old.version;
                    inner.policy = old;
                    next
                })
            } else {
                None
            };
            clear_publish(&mut inner, request_id);
            next
        };
        if let Some(next) = next {
            let _ = self.publish(next).await;
        }
    }

    async fn publish(&self, version: u64) -> bool {
        let (hook, peer) = {
            let inner = lock(&self.inner);
            (inner.hook.clone(), inner.peer.clone())
        };
        if let Some(hook) = hook {
            // A test hook may block until the test closes the session. Running
            // it on the blocking pool keeps the runtime free to take that path.
            return tokio::task::spawn_blocking(move || hook(version))
                .await
                .unwrap_or(false);
        }
        let Some(peer) = peer else {
            return false;
        };
        peer.call::<_, Empty>(
            "engine.policy.changed",
            &PolicyChanged { version },
            RPC_TIMEOUT,
        )
        .await
        .is_ok()
    }

    fn expire(&self, request_id: u64) {
        let mut inner = lock(&self.inner);
        if inner
            .pending
            .get(&request_id)
            .is_some_and(|pending| pending.publishing.is_none())
        {
            finish(&mut inner, request_id, Outcome::TimedOut, Settled::Timeout);
        }
    }
}

enum Prepared {
    Done,
    Publish { request_id: u64, version: u64 },
}

fn prepare(inner: &mut Inner, claim: &Claim) -> Result<Prepared, Fault> {
    if inner.consumed.contains(&claim.request_id) {
        return Err(Fault::Replay);
    }
    let Some(pending) = inner.pending.get(&claim.request_id) else {
        return Err(Fault::Absent);
    };
    if pending.publishing.is_some() || pending.void {
        return Err(Fault::Replay);
    }
    if pending.session_id != claim.session_id
        || pending.image != claim.image
        || pending.engine != claim.engine
        || pending.policy_version != claim.policy_version
    {
        return Err(Fault::Identity);
    }
    let request_id = claim.request_id;
    match claim.act {
        Act::Timeout => {
            finish(inner, request_id, Outcome::TimedOut, Settled::Timeout);
            Ok(Prepared::Done)
        }
        Act::Turn => done(inner, request_id, Decision::Turn),
        Act::Session => done(inner, request_id, Decision::Session),
        Act::Deny => done(inner, request_id, Decision::Deny),
        Act::Always if !inner.policy.always_allow_enabled => {
            done(inner, request_id, Decision::Deny)
        }
        Act::Always => begin_always(inner, request_id),
    }
}

fn done(inner: &mut Inner, request_id: u64, decision: Decision) -> Result<Prepared, Fault> {
    let settled = if decision == Decision::Deny {
        Settled::Deny
    } else {
        Settled::Grant
    };
    finish(inner, request_id, Outcome::Decision(decision), settled);
    Ok(Prepared::Done)
}

fn begin_always(inner: &mut Inner, request_id: u64) -> Result<Prepared, Fault> {
    let Some(image) = inner
        .pending
        .get(&request_id)
        .map(|pending| pending.image.clone())
    else {
        return Err(Fault::Absent);
    };
    let old = inner.policy.clone();
    let version = old.version.checked_add(1).ok_or(Fault::Failed)?;
    let mut next = old.clone();
    next.version = version;
    push_identity(&mut next, &image);
    if !store(&inner.disk, &next) {
        return Err(Fault::Failed);
    }
    inner.policy = next;
    inner.rollback = Some(old);
    if let Some(pending) = inner.pending.get_mut(&request_id) {
        pending.publishing = Some(version);
    }
    Ok(Prepared::Publish {
        request_id,
        version,
    })
}

fn rollback(inner: &mut Inner, version: u64) {
    if inner.policy.version != version {
        return;
    }
    let Some(old) = inner.rollback.take() else {
        return;
    };
    if store(&inner.disk, &old) {
        inner.policy = old;
    }
}

fn void_or_finish(inner: &mut Inner, request_id: u64, settled: Settled) {
    let publishing = inner
        .pending
        .get(&request_id)
        .is_some_and(|pending| pending.publishing.is_some());
    if !publishing {
        let outcome = if settled == Settled::Timeout {
            Outcome::TimedOut
        } else {
            Outcome::Decision(Decision::Deny)
        };
        finish(inner, request_id, outcome, settled);
        return;
    }
    let Some(pending) = inner.pending.get_mut(&request_id) else {
        return;
    };
    pending.void = true;
    let session_id = pending.session_id.clone();
    if let Some(tx) = pending.tx.take() {
        let _ = tx.send(Outcome::Decision(Decision::Deny));
    }
    emit(
        inner,
        Notice::Settled {
            request_id,
            session_id,
            outcome: settled,
        },
    );
}

fn clear_publish(inner: &mut Inner, request_id: u64) {
    let void = inner
        .pending
        .get(&request_id)
        .is_some_and(|pending| pending.void);
    if void {
        inner.pending.remove(&request_id);
        inner.consumed.insert(request_id);
        return;
    }
    if let Some(pending) = inner.pending.get_mut(&request_id) {
        pending.publishing = None;
    }
}

fn finish(inner: &mut Inner, request_id: u64, outcome: Outcome, settled: Settled) {
    let Some(mut pending) = inner.pending.remove(&request_id) else {
        return;
    };
    inner.consumed.insert(request_id);
    if let Some(tx) = pending.tx.take() {
        let _ = tx.send(outcome);
    }
    emit(
        inner,
        Notice::Settled {
            request_id,
            session_id: pending.session_id,
            outcome: settled,
        },
    );
}

fn deny_all(inner: &mut Inner, settled: Settled) {
    let ids: Vec<u64> = inner.pending.keys().copied().collect();
    for id in ids {
        void_or_finish(inner, id, settled);
    }
}

fn push_identity(policy: &mut Policy, image: &ImageBinding) {
    let identity = image.to_identity();
    if policy
        .always
        .iter()
        .any(|saved| same_identity(saved, &identity))
    {
        return;
    }
    policy.always.push(identity);
}

fn same_identity(saved: &Identity, identity: &Identity) -> bool {
    saved.platform == identity.platform
        && saved.key == identity.key
        && saved.team_id == identity.team_id
        && saved.publisher == identity.publisher
        && saved.sha256 == identity.sha256
        && saved.signature == identity.signature
        && saved.image_path == identity.image_path
}

fn store(path: &Option<PathBuf>, policy: &Policy) -> bool {
    match path {
        Some(path) => write_policy(path, policy),
        None => true,
    }
}

fn emit(inner: &Inner, notice: Notice) {
    if let Some(tx) = &inner.events {
        let _ = tx.send(notice);
    }
}

fn card_view(
    request_id: u64,
    request: &RequestApp,
    image: &ImageBinding,
    engine: &ProcessInstance,
    policy: &Policy,
    opened_ms: u64,
) -> CardView {
    CardView {
        request_id,
        session_id: request.session_id.clone(),
        engine_pid: engine.pid,
        engine_started: engine.started,
        policy_version: policy.version,
        image: image.clone(),
        name: sanitize_plain(&request.app.name),
        reason: request
            .reason
            .as_deref()
            .map(sanitize_plain)
            .unwrap_or_default(),
        risk: request.risk,
        timeout_secs: timeout_secs(policy.timeout_secs),
        opened_ms,
        has_icon: request.app.icon.is_some(),
        always_enabled: policy.always_allow_enabled,
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Wait {
    /// Wait until the user, the countdown, or session close settles the request.
    ///
    /// The countdown uses the monotonic clock captured at bind. Cancelling this
    /// future times the request out unless it was already settled.
    pub async fn recv(mut self) -> Outcome {
        let outcome = tokio::select! {
            biased;
            result = &mut self.rx => result.unwrap_or(Outcome::Decision(Decision::Deny)),
            _ = tokio::time::sleep_until(self.deadline) => {
                self.office.expire(self.request_id);
                Outcome::TimedOut
            }
        };
        // Disarm only after the select finishes. Cancelling this future must
        // still time the request out via Drop.
        self.armed = false;
        outcome
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        if self.armed {
            self.office.expire(self.request_id);
        }
    }
}
