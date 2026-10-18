//! Shared-tab DOM fusion for one browser window.
//!
//! Viewport frames are tab-viewport physical pixels. They are translated by the
//! accessibility web-area origin before any node is published. A share is used
//! only when the extension id, web-area fingerprint, and web-area frame all
//! match. Otherwise the accessibility tree is returned unchanged and later
//! actions stay on the driver's declared L0 routes.
//!
//! This layer does not read cookies or storage, does not add a driver or Host
//! method, and does not raise the 5s native ceiling. gcu blocks its own input
//! (address bar, dangerous schemes, metadata hosts, loopback, and private
//! networks). It does not claim to stop the browser's own redirects or in-page
//! navigations.

mod merge;
mod router;

pub(crate) use router::dispatch;

use crate::driver::{DriverError, WindowRef};
use crate::model::{Fingerprint, NativeHandle, RawTree, Rect};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Why a DOM read or action could not be used. Payloads are never included.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomFault {
    /// The tab is not shared with this session.
    Unshared,
    /// The frame, viewport, or top-frame identity does not match.
    Frame,
    /// The snapshot or node is no longer current.
    Stale,
    /// The node cannot perform that action.
    NotActionable,
    /// A write was refused, or may already have landed.
    NotSettable {
        /// Whether a write may already have been applied.
        maybe_executed: bool,
    },
    /// The user must take the page. No mutation is attempted.
    Handoff,
    /// The channel is busy and did not start delivery.
    Busy,
    /// The channel is not available. No delivery was started.
    Unavailable,
    /// The channel failed. `maybe_executed` is true only after delivery may have started.
    Driver {
        /// Whether delivery may have started.
        maybe_executed: bool,
    },
}

/// Result of one DOM action. [`DomEffect::Unknown`] must never be replayed.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomEffect {
    /// The channel completed the action.
    Done,
    /// Delivery may have started. Do not automatically retry.
    Unknown,
}

/// One semantic action on a shared tab. Debug output omits written text.
pub enum DomOperation {
    /// Semantic click.
    Click {
        /// Bound share.
        share_id: String,
        /// Snapshot that produced the node.
        snapshot_id: String,
        /// Node id inside that snapshot.
        node_id: String,
    },
    /// Replace a settable value. The value is not included in diagnostics.
    SetValue {
        /// Bound share.
        share_id: String,
        /// Snapshot that produced the node.
        snapshot_id: String,
        /// Node id inside that snapshot.
        node_id: String,
        /// Replacement text, already accepted by policy.
        value: String,
    },
    /// Scroll by physical pixels, each axis at most 2400.
    Scroll {
        /// Bound share.
        share_id: String,
        /// Snapshot that produced the node.
        snapshot_id: String,
        /// Node id inside that snapshot.
        node_id: String,
        /// Horizontal pixels.
        dx: f64,
        /// Vertical pixels.
        dy: f64,
    },
    /// Choose an option value. The value is not included in diagnostics.
    Select {
        /// Bound share.
        share_id: String,
        /// Snapshot that produced the node.
        snapshot_id: String,
        /// Node id inside that snapshot.
        node_id: String,
        /// Option text, already accepted by policy.
        value: String,
    },
}

impl std::fmt::Debug for DomOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Click { .. } => "Click",
            Self::SetValue { .. } => "SetValue",
            Self::Scroll { .. } => "Scroll",
            Self::Select { .. } => "Select",
        };
        f.write_str(kind)
    }
}

/// In-memory channel to a shared tab. Implementations must not read cookies,
/// storage, or any tab other than `share_id`.
///
/// `observe` and `perform` wait on the channel. The caller bounds each call
/// with the 5s native ceiling and cancels it by dropping the future. Neither
/// method may retry [`DomEffect::Unknown`].
#[async_trait]
pub trait DomChannel: Send + Sync {
    /// Extension id this channel authenticated. Nonblocking; no timeout or cancellation.
    fn extension_id(&self) -> &str;

    /// Read the current shared tab as snapshot JSON.
    ///
    /// The value is either the snapshot object or `{kind:snapshot, snapshot}`.
    /// Unknown fields, including cookies and storage, are rejected by the parser.
    /// Blocking: waits on the channel. Timeout: the caller's 5s ceiling.
    /// Cancellation: dropping the future.
    async fn observe(&self, share_id: &str) -> Result<serde_json::Value, DomFault>;

    /// Deliver one action. `Unknown` means the outcome is uncertain.
    ///
    /// Blocking: waits on the channel. Timeout: the caller's 5s ceiling.
    /// Cancellation: dropping the future. A dropped call is not replayed.
    async fn perform(&self, op: &DomOperation) -> Result<DomEffect, DomFault>;
}

/// A user-shared tab bound to one session, app, and window.
///
/// Binding stores memory only. It does not read the tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    /// Wire share id.
    pub share_id: String,
    /// Session that owns the share.
    pub session_id: String,
    /// Driver app id. Not required to be a wire identifier.
    pub app_id: String,
    /// Driver window id.
    pub window_id: String,
    /// Extension id that must match the channel.
    pub extension_id: String,
    /// Accessibility web-area fingerprint captured for this share.
    pub web_area_fingerprint: Fingerprint,
    /// Accessibility web-area frame in window physical pixels.
    pub web_area_frame: Rect,
    /// Browser document id. Not compared to snapshot JSON.
    pub document_id: String,
}

/// A share was refused because an identifier or frame was unusable.
///
/// No page text is included. Blocking: none. Timeout: none. Cancellation: none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareRejected;

/// Process-local DOM handle. The snapshot node id is not part of identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomToken {
    share_id: String,
    fingerprint: u64,
}

/// Marker replaced with a stable [`DomToken`] after fingerprints are computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingDom {
    node_id: String,
}

pub(crate) struct ResolvedDom {
    share_id: String,
    snapshot_id: String,
    node_id: String,
}

struct Active {
    share_id: String,
    snapshot_id: String,
    nodes: HashMap<u64, String>,
    handoff: bool,
}

struct State {
    shares: HashMap<(String, String, String), Share>,
    active: HashMap<(String, String, String), Active>,
    handles: HashMap<(String, u64), NativeHandle>,
}

/// Share table and DOM channel used by [`crate::action::ActionRouter`].
///
/// Construction and bind/revoke touch memory only. [`Hub::align`] waits on the
/// channel; it does not start its own timer. The router bounds that wait at 5s
/// and cancels it on Stop, session close, timeout, or drop.
pub struct Hub {
    channel: Arc<dyn DomChannel>,
    state: Mutex<State>,
}

/// Alias used by callers that attach a hub to the action router.
pub type BrowserHub = Hub;

impl Hub {
    /// Hold `channel` without reading a tab or waiting.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn new(channel: Arc<dyn DomChannel>) -> Self {
        Self {
            channel,
            state: Mutex::new(State {
                shares: HashMap::new(),
                active: HashMap::new(),
                handles: HashMap::new(),
            }),
        }
    }

    /// Remember one share for `(session, app, window)`, replacing any previous one.
    ///
    /// Does not read the tab. Blocking: none. Timeout: none. Cancellation: none.
    pub fn bind(&self, share: Share) -> Result<(), ShareRejected> {
        if !wire_id(&share.share_id)
            || !wire_id(&share.session_id)
            || !wire_id(&share.extension_id)
            || !wire_id(&share.document_id)
            || !host_id(&share.app_id)
            || !host_id(&share.window_id)
            || !usable_frame(share.web_area_frame)
        {
            return Err(ShareRejected);
        }
        let key = key(&share.session_id, &share.app_id, &share.window_id);
        let mut state = self.lock();
        if let Some(previous) = state.shares.insert(key.clone(), share) {
            drop_share(&mut state, &previous.share_id);
        }
        state.active.remove(&key);
        Ok(())
    }

    /// Drop the share for one window. Does not read the tab or wait.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn revoke(&self, session_id: &str, app_id: &str, window_id: &str) {
        let mut state = self.lock();
        let key = key(session_id, app_id, window_id);
        if let Some(share) = state.shares.remove(&key) {
            drop_share(&mut state, &share.share_id);
        }
        state.active.remove(&key);
    }

    /// Drop every share owned by `session_id`. Memory only; no wait or cancellation.
    pub fn close_session(&self, session_id: &str) {
        let mut state = self.lock();
        let stale: Vec<_> = state
            .shares
            .iter()
            .filter(|(key, _)| key.0 == session_id)
            .map(|(key, share)| (key.clone(), share.share_id.clone()))
            .collect();
        for (key, share_id) in stale {
            state.shares.remove(&key);
            state.active.remove(&key);
            drop_share(&mut state, &share_id);
        }
    }

    /// Replace a matching web-area subtree, or return `raw` unchanged.
    ///
    /// A mismatch clears the active DOM map and does not fail the observation.
    /// Blocking: waits on [`DomChannel::observe`] only after the share, extension,
    /// fingerprint, and frame match. Timeout: none here; the caller enforces 5s.
    /// Cancellation: dropping this future drops the channel read.
    pub async fn align(&self, session_id: &str, window: &WindowRef, raw: RawTree) -> RawTree {
        let key = key(session_id, &window.app.info.id, &window.id);
        let Some(share) = self.lock().shares.get(&key).cloned() else {
            self.clear_active(&key);
            return raw;
        };
        if self.channel.extension_id() != share.extension_id {
            self.clear_active(&key);
            return raw;
        }
        let Some(origin) = merge::accepted_origin(
            merge::locate(raw.root.as_ref(), share.web_area_fingerprint),
            share.web_area_frame,
        ) else {
            self.clear_active(&key);
            return raw;
        };
        let value = match self.channel.observe(&share.share_id).await {
            Ok(value) => value,
            Err(_) => {
                self.clear_active(&key);
                return raw;
            }
        };
        let Some(parsed) = merge::parse(&value) else {
            self.clear_active(&key);
            return raw;
        };
        if !merge::size_matches(parsed.width, parsed.height, origin) {
            self.clear_active(&key);
            return raw;
        }
        let Some(mut merged) = merge::splice(&raw, share.web_area_fingerprint, origin, &parsed)
        else {
            self.clear_active(&key);
            return raw;
        };
        if !self.install(&key, &share.share_id, &parsed, &mut merged) {
            self.clear_active(&key);
            return raw;
        }
        merged
    }

    fn install(
        &self,
        key: &(String, String, String),
        share_id: &str,
        parsed: &merge::Parsed,
        raw: &mut RawTree,
    ) -> bool {
        let Some(root) = raw.root.as_mut() else {
            return false;
        };
        let found = merge::collect(root);
        if !found.ok {
            return false;
        }
        let mut state = self.lock();
        for (fingerprint, node_id) in &found.nodes {
            let handle = state
                .handles
                .entry((share_id.to_owned(), *fingerprint))
                .or_insert_with(|| {
                    NativeHandle::new(DomToken {
                        share_id: share_id.to_owned(),
                        fingerprint: *fingerprint,
                    })
                })
                .clone();
            if !merge::install_handle(root, *fingerprint, node_id, handle) {
                return false;
            }
        }
        state.active.insert(
            key.clone(),
            Active {
                share_id: share_id.to_owned(),
                snapshot_id: parsed.snapshot_id.clone(),
                nodes: found.nodes.into_iter().collect(),
                handoff: parsed.handoff,
            },
        );
        true
    }

    pub(crate) fn resolve(&self, token: &DomToken) -> Result<ResolvedDom, DriverError> {
        let state = self.lock();
        let Some(active) = state.active.values().find(|active| {
            active.share_id == token.share_id && active.nodes.contains_key(&token.fingerprint)
        }) else {
            return Err(DriverError::StaleElement);
        };
        if active.handoff {
            return Err(DriverError::SecureDesktop);
        }
        let Some(node_id) = active.nodes.get(&token.fingerprint).cloned() else {
            return Err(DriverError::StaleElement);
        };
        Ok(ResolvedDom {
            share_id: active.share_id.clone(),
            snapshot_id: active.snapshot_id.clone(),
            node_id,
        })
    }

    fn clear_active(&self, key: &(String, String, String)) {
        self.lock().active.remove(key);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn key(session: &str, app: &str, window: &str) -> (String, String, String) {
    (session.to_owned(), app.to_owned(), window.to_owned())
}

fn drop_share(state: &mut State, share_id: &str) {
    state.handles.retain(|(id, _), _| id != share_id);
    state.active.retain(|_, active| active.share_id != share_id);
}

fn wire_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn host_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.contains('\0')
}

fn usable_frame(frame: Rect) -> bool {
    [frame.x, frame.y, frame.w, frame.h]
        .iter()
        .all(|value| value.is_finite())
        && frame.w > 0.0
        && frame.h > 0.0
}
