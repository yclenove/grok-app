//! Sensitive confirmation and handoff on the shared approval desk.
//!
//! Both lanes use the same pending map, so session close, detach, and a second
//! decision follow the app path. A confirmation grant requires the live policy
//! version, snapshot, and digest to still match. Handoff never grants.

use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::auth::ProcessInstance;
use crate::messages::{App, ConfirmAction, Decision, Handoff, Image, Policy};

use super::super::{
    hex_encode, sanitize_plain, timeout_secs, BindFault, ConfirmAct, ConfirmClaim, ConfirmEnd,
    ConfirmView, CropView, Fault, HandoffAct, HandoffClaim, HandoffEnd, HandoffView, ImageBinding,
    Notice, Outcome, Settled,
};
use super::{emit, finish, lock, Inner, Office, Pending, Wait};

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAX_CROP_BYTES: usize = 256 * 1024;
const MAX_CROP_EDGE: u32 = 4096;

/// Which request a pending id belongs to. App decisions cannot cross lanes.
pub(super) enum Lane {
    /// `host.approval.requestApp`.
    App,
    /// One exact sensitive action. The crop lives on the card, not in the grant.
    Confirm {
        /// Snapshot shown on the card.
        snapshot_id: String,
        /// Digest shown on the card.
        digest: [u8; 32],
    },
    /// User takeover. Not an approval. The reason lives on the card.
    Handoff,
}

/// Outcome for a wait that ends without a grant. Handoff never times out into one.
pub(super) fn release_outcome(lane: &Lane, settled: Settled) -> Outcome {
    match lane {
        Lane::App => match settled {
            Settled::Timeout => Outcome::TimedOut,
            Settled::Grant | Settled::Deny | Settled::Closed => Outcome::Decision(Decision::Deny),
        },
        Lane::Confirm { .. } => Outcome::Confirmed {
            allowed: false,
            end: match settled {
                Settled::Timeout => ConfirmEnd::TimedOut,
                Settled::Closed => ConfirmEnd::Closed,
                Settled::Grant | Settled::Deny => ConfirmEnd::Denied,
            },
        },
        Lane::Handoff => Outcome::HandedOff {
            end: HandoffEnd::Closed,
        },
    }
}

/// Notice for a consumed request. App cards keep the original settled shape.
pub(super) fn settled_notice(
    pending: &Pending,
    request_id: u64,
    outcome: &Outcome,
    settled: Settled,
) -> Notice {
    let session_id = pending.session_id.clone();
    match &pending.lane {
        Lane::App => Notice::Settled {
            request_id,
            session_id,
            outcome: settled,
        },
        Lane::Confirm { .. } => Notice::ConfirmSettled {
            request_id,
            session_id,
            end: match outcome {
                Outcome::Confirmed { end, .. } => *end,
                Outcome::TimedOut => ConfirmEnd::TimedOut,
                Outcome::Decision(_) | Outcome::HandedOff { .. } => ConfirmEnd::Denied,
            },
        },
        Lane::Handoff => Notice::HandoffSettled {
            request_id,
            session_id,
            end: match outcome {
                Outcome::HandedOff { end } => *end,
                Outcome::Decision(_) | Outcome::TimedOut | Outcome::Confirmed { .. } => {
                    HandoffEnd::Closed
                }
            },
        },
    }
}

impl Office {
    /// Bind one sensitive action. An empty snapshot or image is not shown.
    ///
    /// Blocking: none. The returned [`Wait`] waits. Timeout: the policy countdown.
    /// Cancellation: dropping the wait times the request out as a denial.
    /// Thread safety: the desk mutex serializes the insert.
    pub fn bind_confirm(
        &self,
        request_id: u64,
        request: &ConfirmAction,
        opened_ms: u64,
    ) -> Result<(ConfirmView, Wait), BindFault> {
        let mut inner = lock(&self.inner);
        let (engine, image) = prepare_bind(&inner, request_id, &request.app)?;
        if request.summary.snapshot_id.is_empty() {
            return Err(BindFault::Unverified);
        }
        let view = confirm_view(
            request_id,
            request,
            &image,
            &engine,
            &inner.policy,
            opened_ms,
        );
        let (tx, rx) = oneshot::channel();
        inner.pending.insert(
            request_id,
            pending_row(
                request.session_id.clone(),
                image,
                engine,
                view.policy_version,
                Lane::Confirm {
                    snapshot_id: request.summary.snapshot_id.clone(),
                    digest: request.summary.action_digest,
                },
                tx,
            ),
        );
        emit(
            &inner,
            Notice::ConfirmPending {
                card: Box::new(view.clone()),
            },
        );
        let timeout = Duration::from_secs(timeout_secs(view.timeout_secs));
        drop(inner);
        Ok((view, arm(self, request_id, Some(timeout), rx)))
    }

    /// Consume one confirmation. A mismatched allow becomes stale and grants nothing.
    ///
    /// Identity mismatches do not consume the card. Deny and timeout consume it
    /// when the session, image, engine, and shown policy version match, even if
    /// the digest has since changed. Blocking: none. Timeout: none.
    /// Cancellation: none. Thread safety: the desk mutex.
    pub fn decide_confirm(&self, claim: &ConfirmClaim) -> Result<ConfirmEnd, Fault> {
        let mut inner = lock(&self.inner);
        let (snapshot_id, digest, bound_version) = confirm_lane(&inner, claim)?;
        let end = match claim.act {
            ConfirmAct::Allow
                if snapshot_id == claim.snapshot_id
                    && digest == claim.action_digest
                    && bound_version == inner.policy.version =>
            {
                ConfirmEnd::Allowed
            }
            ConfirmAct::Allow => ConfirmEnd::Stale,
            ConfirmAct::Deny => ConfirmEnd::Denied,
            ConfirmAct::Timeout => ConfirmEnd::TimedOut,
        };
        let allowed = end == ConfirmEnd::Allowed;
        let settled = if allowed {
            Settled::Grant
        } else if end == ConfirmEnd::TimedOut {
            Settled::Timeout
        } else {
            Settled::Deny
        };
        finish(
            &mut inner,
            claim.request_id,
            Outcome::Confirmed { allowed, end },
            settled,
        );
        Ok(end)
    }

    /// Bind one handoff. There is no countdown and the result is never a grant.
    ///
    /// Blocking: none. The returned [`Wait`] has no deadline. Timeout: none.
    /// Cancellation: dropping the wait closes the card. Thread safety: the desk mutex.
    pub fn bind_handoff(
        &self,
        request_id: u64,
        request: &Handoff,
        opened_ms: u64,
    ) -> Result<(HandoffView, Wait), BindFault> {
        let mut inner = lock(&self.inner);
        let (engine, image) = prepare_bind(&inner, request_id, &request.app)?;
        let view = handoff_view(
            request_id,
            request,
            &image,
            &engine,
            inner.policy.version,
            opened_ms,
        );
        let (tx, rx) = oneshot::channel();
        inner.pending.insert(
            request_id,
            pending_row(
                request.session_id.clone(),
                image,
                engine,
                view.policy_version,
                Lane::Handoff,
                tx,
            ),
        );
        emit(
            &inner,
            Notice::HandoffPending {
                card: Box::new(view.clone()),
            },
        );
        drop(inner);
        Ok((view, arm(self, request_id, None, rx)))
    }

    /// Mark a handoff handled. This does not change policy and does not clear Stop.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the desk mutex.
    pub fn decide_handoff(&self, claim: &HandoffClaim) -> Result<HandoffEnd, Fault> {
        let mut inner = lock(&self.inner);
        handoff_lane(&inner, claim)?;
        let HandoffAct::Handled = claim.act;
        let end = HandoffEnd::Handled;
        finish(
            &mut inner,
            claim.request_id,
            Outcome::HandedOff { end },
            Settled::Deny,
        );
        Ok(end)
    }
}

fn prepare_bind(
    inner: &Inner,
    request_id: u64,
    app: &App,
) -> Result<(ProcessInstance, ImageBinding), BindFault> {
    if inner.consumed.contains(&request_id) || inner.pending.contains_key(&request_id) {
        return Err(BindFault::Replay);
    }
    let Some(engine) = inner.engine.clone() else {
        return Err(BindFault::NoEngine);
    };
    let image = ImageBinding::from_app(app);
    if image.key.is_empty() || image.image_path.is_empty() {
        return Err(BindFault::Unverified);
    }
    Ok((engine, image))
}

fn confirm_lane(inner: &Inner, claim: &ConfirmClaim) -> Result<(String, [u8; 32], u64), Fault> {
    let pending = live(inner, claim.request_id)?;
    if pending.session_id != claim.session_id
        || pending.image != claim.image
        || pending.engine != claim.engine
        || pending.policy_version != claim.policy_version
    {
        return Err(Fault::Identity);
    }
    match &pending.lane {
        Lane::Confirm {
            snapshot_id,
            digest,
        } => Ok((snapshot_id.clone(), *digest, pending.policy_version)),
        Lane::App | Lane::Handoff => Err(Fault::Identity),
    }
}

fn handoff_lane(inner: &Inner, claim: &HandoffClaim) -> Result<(), Fault> {
    let pending = live(inner, claim.request_id)?;
    if pending.session_id != claim.session_id
        || pending.image != claim.image
        || pending.engine != claim.engine
        || pending.policy_version != claim.policy_version
    {
        return Err(Fault::Identity);
    }
    match pending.lane {
        Lane::Handoff => Ok(()),
        Lane::App | Lane::Confirm { .. } => Err(Fault::Identity),
    }
}

fn live(inner: &Inner, request_id: u64) -> Result<&Pending, Fault> {
    if inner.consumed.contains(&request_id) {
        return Err(Fault::Replay);
    }
    let Some(pending) = inner.pending.get(&request_id) else {
        return Err(Fault::Absent);
    };
    if pending.publishing.is_some() || pending.void {
        return Err(Fault::Replay);
    }
    Ok(pending)
}

fn pending_row(
    session_id: String,
    image: ImageBinding,
    engine: ProcessInstance,
    policy_version: u64,
    lane: Lane,
    tx: oneshot::Sender<Outcome>,
) -> Pending {
    Pending {
        session_id,
        image,
        engine,
        policy_version,
        lane,
        publishing: None,
        void: false,
        tx: Some(tx),
    }
}

fn arm(
    office: &Office,
    request_id: u64,
    timeout: Option<Duration>,
    rx: oneshot::Receiver<Outcome>,
) -> Wait {
    Wait {
        office: office.clone(),
        request_id,
        deadline: timeout.map(|duration| Instant::now() + duration),
        rx,
        armed: true,
    }
}

fn confirm_view(
    request_id: u64,
    request: &ConfirmAction,
    image: &ImageBinding,
    engine: &ProcessInstance,
    policy: &Policy,
    opened_ms: u64,
) -> ConfirmView {
    ConfirmView {
        request_id,
        session_id: request.session_id.clone(),
        engine_pid: engine.pid,
        engine_started: engine.started,
        policy_version: policy.version,
        image: image.clone(),
        name: sanitize_plain(&request.app.name),
        summary: sanitize_plain(&request.summary.text),
        reason: String::new(),
        snapshot_id: request.summary.snapshot_id.clone(),
        action_digest: hex_encode(&request.summary.action_digest),
        crop: classify_crop(request.crop.as_ref()),
        timeout_secs: timeout_secs(policy.timeout_secs),
        opened_ms,
        has_icon: request.app.icon.is_some(),
    }
}

fn handoff_view(
    request_id: u64,
    request: &Handoff,
    image: &ImageBinding,
    engine: &ProcessInstance,
    policy_version: u64,
    opened_ms: u64,
) -> HandoffView {
    let (code, detail) = split_reason(&request.reason);
    HandoffView {
        request_id,
        session_id: request.session_id.clone(),
        engine_pid: engine.pid,
        engine_started: engine.started,
        policy_version,
        image: image.clone(),
        name: sanitize_plain(&request.app.name),
        code,
        detail,
        opened_ms,
        has_icon: request.app.icon.is_some(),
    }
}

fn split_reason(reason: &str) -> (String, String) {
    match reason {
        "login" => ("login".to_owned(), String::new()),
        "captcha" => ("captcha".to_owned(), String::new()),
        "system_prompt" => ("system_prompt".to_owned(), String::new()),
        other => ("other".to_owned(), sanitize_plain(other)),
    }
}

/// Keep a small real PNG. Anything else is a gap, and its bytes are not stored.
fn classify_crop(crop: Option<&Image>) -> CropView {
    let Some(image) = crop else {
        return CropView::Missing;
    };
    let safe = image.mime == "image/png"
        && image.width > 0
        && image.height > 0
        && image.width <= MAX_CROP_EDGE
        && image.height <= MAX_CROP_EDGE
        && image.bytes.len() >= PNG_MAGIC.len()
        && image.bytes.len() <= MAX_CROP_BYTES
        && image.bytes.starts_with(PNG_MAGIC);
    if !safe {
        return CropView::Unlocated;
    }
    CropView::Redacted {
        mime: image.mime.clone(),
        width: image.width,
        height: image.height,
        bytes: image.bytes.clone(),
    }
}
