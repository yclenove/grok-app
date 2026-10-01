//! Cleanup-only proofs are independent of pairing keys. They never grant page access.
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::execution::ActionCancellation;
use crate::pairing::PairingConnection;

pub const ACTION_COMPLETION_PROTOCOL: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserExitCleanup {
    pub browser_id: String,
    pub request_id: String,
    pub document_id: String,
    pub phase: String,
}
pub const COMPLETION_REQUEST_BYTES: usize = 4096;
const MAX_PENDING: usize = 8;
pub(crate) const COMPLETION_TOMBSTONE_LIMIT: usize = 64;
const FINISHED_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CompletionBinding {
    pub protocol: u32,
    pub request_id: String,
    pub connection: PairingConnection,
    pub session: String,
    pub run_id: String,
    pub tab_id: String,
    pub document_id: String,
    pub document_generation: u64,
    pub grant_generation: u64,
    pub snapshot_id: String,
}

impl CompletionBinding {
    fn valid(&self) -> bool {
        let uuid = |value: &str| value.len() == 36 && uuid::Uuid::parse_str(value).is_ok();
        let text = |value: &str, limit| {
            !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
        };
        let generation = |value| (1..=crate::protocol::JS_MAX_SAFE_INTEGER).contains(&value);
        self.protocol == ACTION_COMPLETION_PROTOCOL
            && uuid(&self.request_id)
            && uuid(&self.snapshot_id)
            && uuid(&self.connection.instance_id)
            && uuid(&self.connection.connection_nonce)
            && generation(self.connection.generation)
            && generation(self.document_generation)
            && generation(self.grant_generation)
            && text(&self.session, 256)
            && text(&self.run_id, 256)
            && text(&self.tab_id, 128)
            && text(&self.document_id, 128)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompletionKey(String);

impl std::fmt::Debug for CompletionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CompletionProof {
    pub binding: CompletionBinding,
    pub completion_key: CompletionKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletionPhase {
    Offered,
    Claimed,
    Settled,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionStatus {
    pub phase: CompletionPhase,
    pub cancel_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletionRetirement {
    Pending,
    Settled,
    Absent,
}

struct Authentication {
    binding: CompletionBinding,
    tag: [u8; 32],
}

impl Authentication {
    fn proof_mac(authority: &str, binding: &CompletionBinding) -> Result<Hmac<Sha256>, String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(authority.as_bytes())
            .map_err(|_| "invalid completion authority")?;
        mac.update(b"grok-cu-completion-proof-v2\0");
        mac.update(&serde_json::to_vec(binding).map_err(|_| "invalid completion binding")?);
        Ok(mac)
    }

    fn authenticate(authority: &str, proof: &CompletionProof) -> Result<(), String> {
        if !proof.binding.valid()
            || proof.completion_key.0.len() != 64
            || !proof
                .completion_key
                .0
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("invalid completion proof".into());
        }
        let key = hex::decode(&proof.completion_key.0).map_err(|_| "invalid completion proof")?;
        Self::proof_mac(authority, &proof.binding)?
            .verify_slice(&key)
            .map_err(|_| "invalid completion proof".into())
    }

    fn mac(proof: &CompletionProof) -> Result<Hmac<Sha256>, String> {
        if proof.completion_key.0.len() != 64
            || !proof
                .completion_key
                .0
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("invalid completion proof".into());
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(proof.completion_key.0.as_bytes())
            .map_err(|_| "invalid completion proof")?;
        mac.update(b"grok-cu-completion-v2\0");
        mac.update(&serde_json::to_vec(&proof.binding).map_err(|_| "invalid completion binding")?);
        Ok(mac)
    }

    fn issue(
        binding: CompletionBinding,
        authority: &str,
    ) -> Result<(Self, CompletionProof), String> {
        if !binding.valid() {
            return Err("invalid completion binding".into());
        }
        let completion_key = hex::encode(
            Self::proof_mac(authority, &binding)?
                .finalize()
                .into_bytes(),
        );
        let proof = CompletionProof {
            binding: binding.clone(),
            completion_key: CompletionKey(completion_key),
        };
        let tag = Self::mac(&proof)?.finalize().into_bytes().into();
        Ok((Self { binding, tag }, proof))
    }

    fn verify(&self, authority: &str, proof: &CompletionProof) -> Result<(), String> {
        Self::authenticate(authority, proof)?;
        if self.binding != proof.binding {
            return Err("completion identity mismatch".into());
        }
        Self::mac(proof)?
            .verify_slice(&self.tag)
            .map_err(|_| "invalid completion proof".into())
    }
}

struct Pending {
    binding: CompletionBinding,
    authentication: Option<Authentication>,
    deadline: Instant,
    cancellation: ActionCancellation,
    claimed: bool,
    bound_claim: bool,
    browser_session_id: Option<String>,
}

pub(super) struct CompletionRegistry {
    pending: Vec<Pending>,
    finished: VecDeque<(Authentication, Instant)>,
    authority: String,
}

impl Default for CompletionRegistry {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            finished: VecDeque::new(),
            authority: super::csprng_bearer_token(),
        }
    }
}

impl CompletionRegistry {
    pub(super) fn len(&self) -> usize {
        self.pending.len()
    }

    pub(super) fn bindings(&self) -> impl Iterator<Item = &CompletionBinding> {
        self.pending.iter().map(|entry| &entry.binding)
    }

    #[cfg(test)]
    pub(super) fn offer(
        &mut self,
        binding: CompletionBinding,
        cancellation: ActionCancellation,
    ) -> Result<CompletionProof, String> {
        self.offer_with_browser(binding, cancellation, None)
    }

    pub(super) fn offer_with_browser(
        &mut self,
        binding: CompletionBinding,
        cancellation: ActionCancellation,
        browser_session_id: Option<String>,
    ) -> Result<CompletionProof, String> {
        let request_id = binding.request_id.clone();
        self.reserve_with_browser(binding, cancellation, false, browser_session_id)?;
        self.deliver(&request_id)
    }

    /// Queue occupancy without keeping a plaintext receipt key in the Host.
    #[cfg(test)]
    pub(super) fn reserve(
        &mut self,
        binding: CompletionBinding,
        cancellation: ActionCancellation,
        bound_claim: bool,
    ) -> Result<(), String> {
        self.reserve_with_browser(binding, cancellation, bound_claim, None)
    }

    pub(super) fn reserve_with_browser(
        &mut self,
        binding: CompletionBinding,
        cancellation: ActionCancellation,
        bound_claim: bool,
        browser_session_id: Option<String>,
    ) -> Result<(), String> {
        cancellation.check()?;
        self.sweep(&HashSet::new());
        if !binding.valid()
            || self.pending.len() >= MAX_PENDING
            || self.has_tab(&binding.tab_id)
            || self
                .bindings()
                .any(|old| old.request_id == binding.request_id)
            || self
                .finished
                .iter()
                .any(|(old, _)| old.binding.request_id == binding.request_id)
        {
            return Err("completion offer unavailable".into());
        }
        self.pending.push(Pending {
            binding,
            authentication: None,
            deadline: Instant::now() + Duration::from_secs(10),
            cancellation,
            claimed: false,
            bound_claim,
            browser_session_id,
        });
        Ok(())
    }

    pub(super) fn deliver(&mut self, request_id: &str) -> Result<CompletionProof, String> {
        self.sweep(&HashSet::new());
        let entry = self
            .pending
            .iter_mut()
            .find(|entry| entry.binding.request_id == request_id)
            .ok_or("completion offer unavailable")?;
        if entry.authentication.is_some() {
            return Err("completion offer already delivered".into());
        }
        entry.cancellation.check()?;
        let (authentication, proof) =
            Authentication::issue(entry.binding.clone(), &self.authority)?;
        entry.authentication = Some(authentication);
        Ok(proof)
    }

    fn remember(&mut self, authentication: Authentication) {
        if self.finished.len() >= COMPLETION_TOMBSTONE_LIMIT {
            self.finished.pop_front();
        }
        self.finished.push_back((authentication, Instant::now()));
    }

    pub(super) fn sweep(&mut self, revoked: &HashSet<String>) {
        let now = Instant::now();
        self.finished
            .retain(|(_, time)| now.duration_since(*time) < FINISHED_TTL);
        for index in (0..self.pending.len()).rev() {
            let pending = &self.pending[index];
            if now >= pending.deadline || revoked.contains(&pending.binding.request_id) {
                pending.cancellation.cancel();
            }
            if pending.cancellation.check().is_err() && !pending.claimed {
                let removed = self.pending.remove(index);
                if let Some(authentication) = removed.authentication {
                    self.remember(authentication);
                }
            }
        }
    }

    pub(super) fn cancel_run(&mut self, run: Option<&str>) {
        for entry in &self.pending {
            if run.is_none_or(|run| entry.binding.run_id == run) {
                entry.cancellation.cancel();
            }
        }
        self.sweep(&HashSet::new());
    }

    /// Release one old browser owner only when its startup id, request, and
    /// document all match a cleanup record. The label is cleanup or unknown.
    /// Nothing is settled as applied or verified, and a mismatch releases nothing.
    pub(super) fn converge_browser_exit(
        &mut self,
        previous_id: &str,
        cleanups: &[BrowserExitCleanup],
    ) -> Vec<(String, &'static str)> {
        let mut released = Vec::new();
        for cleanup in cleanups {
            let result = match cleanup.phase.as_str() {
                "physicallySettled" => "cleanup",
                "prepared" => "unknown",
                _ => continue,
            };
            if cleanup.browser_id != previous_id {
                continue;
            }
            let Some(index) = self.pending.iter().position(|entry| {
                entry.browser_session_id.as_deref() == Some(previous_id)
                    && entry.binding.request_id == cleanup.request_id
                    && entry.binding.document_id == cleanup.document_id
            }) else {
                continue;
            };
            let entry = self.pending.remove(index);
            entry.cancellation.cancel();
            released.push((entry.binding.request_id, result));
        }
        released
    }

    pub(super) fn has_tab(&self, tab: &str) -> bool {
        self.pending.iter().any(|entry| entry.binding.tab_id == tab)
    }

    pub(super) fn idle(&self, run: &str) -> bool {
        !self.pending.iter().any(|entry| entry.binding.run_id == run)
    }

    pub(super) fn claim(&mut self, proof: &CompletionProof) -> Result<(), String> {
        self.claim_kind(proof, false)
    }

    pub(super) fn claim_bound(&mut self, proof: &CompletionProof) -> Result<(), String> {
        self.claim_kind(proof, true)
    }

    fn claim_kind(&mut self, proof: &CompletionProof, bound_claim: bool) -> Result<(), String> {
        self.sweep(&HashSet::new());
        let entry = self
            .pending
            .iter_mut()
            .find(|entry| entry.binding.request_id == proof.binding.request_id)
            .ok_or("completion offer unavailable")?;
        if entry.bound_claim != bound_claim {
            return Err("completion claim kind mismatch".into());
        }
        entry
            .authentication
            .as_ref()
            .ok_or("completion not delivered")?
            .verify(&self.authority, proof)?;
        entry.cancellation.check()?;
        if entry.claimed {
            return Err("completion offer already claimed".into());
        }
        entry.claimed = true;
        Ok(())
    }

    pub(super) fn status(&mut self, proof: &CompletionProof) -> Result<CompletionStatus, String> {
        self.sweep(&HashSet::new());
        if let Some(entry) = self
            .pending
            .iter()
            .find(|entry| entry.binding.request_id == proof.binding.request_id)
        {
            entry
                .authentication
                .as_ref()
                .ok_or("completion not delivered")?
                .verify(&self.authority, proof)?;
            return Ok(CompletionStatus {
                phase: if entry.claimed {
                    CompletionPhase::Claimed
                } else {
                    CompletionPhase::Offered
                },
                cancel_requested: entry.cancellation.check().is_err(),
            });
        }
        let (authentication, _) = self
            .finished
            .iter()
            .find(|(authentication, _)| {
                authentication.binding.request_id == proof.binding.request_id
            })
            .ok_or("completion offer unavailable")?;
        authentication.verify(&self.authority, proof)?;
        Ok(CompletionStatus {
            phase: CompletionPhase::Settled,
            cancel_requested: true,
        })
    }

    pub(super) fn settle(&mut self, proof: &CompletionProof) -> Result<(), String> {
        let status = self.status(proof)?;
        if status.phase == CompletionPhase::Settled {
            return Ok(());
        }
        let index = self
            .pending
            .iter()
            .position(|entry| entry.binding.request_id == proof.binding.request_id)
            .ok_or("completion offer unavailable")?;
        let removed = self.pending.remove(index);
        if let Some(authentication) = removed.authentication {
            self.remember(authentication);
        }
        Ok(())
    }

    /// Read-only terminal query for a locally proven physical completion. It never
    /// claims or releases a live Host entry. `Absent` is authenticated by this
    /// process's authority and therefore cannot cross an App restart.
    pub(super) fn retirement(
        &mut self,
        proof: &CompletionProof,
    ) -> Result<CompletionRetirement, String> {
        Authentication::authenticate(&self.authority, proof)?;
        self.sweep(&HashSet::new());
        if let Some(entry) = self
            .pending
            .iter()
            .find(|entry| entry.binding.request_id == proof.binding.request_id)
        {
            entry
                .authentication
                .as_ref()
                .ok_or("completion not delivered")?
                .verify(&self.authority, proof)?;
            return Ok(CompletionRetirement::Pending);
        }
        if let Some((authentication, _)) = self.finished.iter().find(|(authentication, _)| {
            authentication.binding.request_id == proof.binding.request_id
        }) {
            authentication.verify(&self.authority, proof)?;
            return Ok(CompletionRetirement::Settled);
        }
        Ok(CompletionRetirement::Absent)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn expire_tombstones_for_test(&mut self) {
        for (_, time) in &mut self.finished {
            *time = Instant::now() - FINISHED_TTL;
        }
        self.sweep(&HashSet::new());
    }
}

#[cfg(test)]
mod tests;
