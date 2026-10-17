//! Trusted settings edits. No method here can add an Always identity.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::{sanitize_plain, Fault, Settled};
use super::{deny_all, lock, store, Inner, Office};
use crate::messages::{
    ConfirmationLevel, DoctorReport, Empty, ForegroundPolicy, Identity, Image, Policy,
};
use crate::server::RPC_TIMEOUT;

/// Local App file, separate from the engine's read-only Policy schema.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub policy: Policy,
    #[serde(default)]
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub computer_use_enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Grant {
    pub identity: Identity,
    pub name: String,
    pub icon: Option<Image>,
    pub granted_ms: Option<u64>,
}

/// Display-only persistent approval. `id` identifies the exact stored OS image.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    /// Opaque identity digest; cannot be used to create a grant.
    pub id: String,
    /// OS name, sanitized as plain text.
    pub name: String,
    /// Canonical path helps distinguish apps with the same display name.
    pub image_path: String,
    /// Missing for legacy grants; never invent a grant time.
    pub granted_ms: Option<u64>,
    /// Bounded PNG for display only.
    pub icon: Option<Image>,
}

pub(super) fn snapshot(inner: &Inner) -> Snapshot {
    Snapshot {
        policy: inner.policy.clone(),
        grants: inner.grants.clone(),
        computer_use_enabled: inner.enabled,
    }
}

pub(super) fn restore(inner: &mut Inner, state: Snapshot) {
    inner.policy = state.policy;
    inner.grants = state.grants;
    inner.enabled = state.computer_use_enabled;
}

pub(super) fn grant(request: &crate::messages::RequestApp) -> Grant {
    Grant {
        identity: request.app.identity.clone(),
        name: sanitize_plain(&request.app.name),
        icon: request.app.icon.clone().filter(safe_icon),
        granted_ms: None,
    }
}

fn safe_icon(icon: &Image) -> bool {
    icon.mime == "image/png"
        && (1..=256).contains(&icon.width)
        && (1..=256).contains(&icon.height)
        && icon.bytes.len() <= 64 * 1024
        && icon.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn identity_id(identity: &Identity) -> String {
    // Identity has only infallibly serializable fields. Hash the full identity,
    // including build digest; a replaced app cannot revoke a different row.
    let bytes = serde_json::to_vec(identity).unwrap_or_default();
    super::super::hex_encode(&Sha256::digest(bytes))
}

impl Office {
    /// List current grants. Memory only; no native work, timeout or cancellation.
    pub fn approvals(&self) -> Vec<Approval> {
        let inner = lock(&self.inner);
        inner
            .policy
            .always
            .iter()
            .map(|identity| {
                let id = identity_id(identity);
                let saved = inner.grants.iter().find(|g| identity_id(&g.identity) == id);
                Approval {
                    id,
                    name: saved
                        .map(|g| sanitize_plain(&g.name))
                        .unwrap_or_else(|| sanitize_plain(&identity.key)),
                    image_path: sanitize_plain(&identity.image_path),
                    granted_ms: saved.and_then(|g| g.granted_ms),
                    icon: saved.and_then(|g| g.icon.clone()).filter(safe_icon),
                }
            })
            .collect()
    }

    /// Persisted feature preference. Memory only. Missing/corrupt files default off.
    pub fn enabled(&self) -> bool {
        lock(&self.inner).enabled
    }

    /// Save a switch after the supervisor accepted it. Synchronous atomic file IO;
    /// call on a blocking worker. No timeout/cancellation while replacing the file.
    pub fn save_enabled(&self, enabled: bool) -> Result<(), Fault> {
        let _edit = self.edits.try_lock().map_err(|_| Fault::Failed)?;
        let mut inner = lock(&self.inner);
        let mut next = snapshot(&inner);
        next.computer_use_enabled = enabled;
        if !store(&inner.disk, &next) {
            return Err(Fault::Failed);
        }
        restore(&mut inner, next);
        Ok(())
    }

    /// Revoke one exact identity, persist, then await the engine reload (five seconds).
    /// Failed publication closes the peer so cached authority cannot be used. The
    /// removal remains persisted. Dropping the future also closes an unpublished peer.
    pub async fn revoke(&self, id: &str) -> Result<(), Fault> {
        let id = id.to_owned();
        self.edit(move |state| {
            if !state.policy.always.iter().any(|i| identity_id(i) == id) {
                return Err(Fault::Absent);
            }
            state.policy.always.retain(|i| identity_id(i) != id);
            state.grants.retain(|g| identity_id(&g.identity) != id);
            Ok(())
        })
        .await
    }

    /// Update only the two user-selectable policy fields. Never changes grants.
    /// Atomic disk IO precedes a five-second reload. Cancellation fails closed.
    pub async fn set_preferences(
        &self,
        confirmation: ConfirmationLevel,
        foreground: ForegroundPolicy,
    ) -> Result<(), Fault> {
        self.edit(move |state| {
            state.policy.sensitive_action_confirmation = confirmation;
            state.policy.allow_foreground = foreground;
            Ok(())
        })
        .await
    }

    async fn edit(
        &self,
        apply: impl FnOnce(&mut Snapshot) -> Result<(), Fault> + Send + 'static,
    ) -> Result<(), Fault> {
        let edit = self
            .edits
            .clone()
            .try_lock_owned()
            .map_err(|_| Fault::Failed)?;
        let mut guard = ReloadGuard {
            office: self.clone(),
            completed: false,
        };
        let office = self.clone();
        // The worker owns the edit lock through the atomic replace even if the
        // caller cancels. The reload guard closes the stale peer on cancellation.
        let prepared = tokio::task::spawn_blocking(move || {
            let mut inner = lock(&office.inner);
            let mut next = snapshot(&inner);
            apply(&mut next)?;
            next.policy.version = next.policy.version.checked_add(1).ok_or(Fault::Failed)?;
            if !store(&inner.disk, &next) {
                return Err(Fault::Failed);
            }
            restore(&mut inner, next);
            deny_all(&mut inner, Settled::Deny);
            Ok((inner.policy.version, inner.engine.is_some(), edit))
        })
        .await;
        let (version, live, _edit) = match prepared {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(error)) => {
                guard.completed = true;
                return Err(error);
            }
            Err(_) => return Err(Fault::Failed),
        };
        if live && !self.publish(version).await {
            return Err(Fault::Failed);
        }
        guard.completed = true;
        Ok(())
    }

    /// Forward engine.system.doctor on the authenticated peer. Five-second budget;
    /// disconnected or failed probes are errors, never a successful empty report.
    pub async fn doctor(&self) -> Result<DoctorReport, Fault> {
        let peer = lock(&self.inner).peer.clone().ok_or(Fault::Failed)?;
        peer.call("engine.system.doctor", &Empty {}, RPC_TIMEOUT)
            .await
            .map_err(|_| Fault::Failed)
    }
}

struct ReloadGuard {
    office: Office,
    completed: bool,
}

impl Drop for ReloadGuard {
    fn drop(&mut self) {
        if !self.completed {
            let peer = lock(&self.office.inner).peer.clone();
            if let Some(peer) = peer {
                peer.close();
            }
            self.office.detach();
        }
    }
}
