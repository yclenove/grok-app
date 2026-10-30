//! ADR 0017 §4 disposable-environment contract. No real provider is installed.
//!
//! Provider and verifier implementations belong to the trusted supervisor,
//! outside the guest/client. Neither CLI flags nor model output supply evidence.
//! The fake implementation exists only in unit tests. This module never starts
//! a credentialed process, configures a firewall, or provisions a VM.

use serde::{Deserialize, Serialize};

/// The five mandatory rows of ADR 0017 §4. Every row needs both kinds of evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Guarantee {
    /// Disposable OS user, desktop, image, private directories and helper/Stop store.
    DisposableDesktop,
    /// Filesystem, IPC, debugger and supervisor-control isolation.
    FileIpcIsolation,
    /// External default-deny gateway with only the exact model endpoint allowed.
    EndpointOnlyEgress,
    /// Short-lived evaluation credentials only; no ambient identities or agents.
    NoAmbientCredentials,
    /// Native identity/permissions/Stop, scope ACK and whole-tree lifetime barrier.
    NativeControlAndExit,
}

impl Guarantee {
    /// Complete mandatory set, in report order.
    pub const ALL: [Self; 5] = [
        Self::DisposableDesktop,
        Self::FileIpcIsolation,
        Self::EndpointOnlyEgress,
        Self::NoAmbientCredentials,
        Self::NativeControlAndExit,
    ];
}

/// Mandatory machine probes. `Passed` means the expected safe condition held;
/// for negative probes it means access was denied, not that the connection worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Probe {
    /// Instance/boot/UID/desktop/image identity, sessions, processes and mounts.
    DesktopInventory,
    /// Fresh private directories and separate helper and durable external Stop store.
    PrivateStorage,
    /// Sandbox active; no debug/ptrace, control handles or supervisor write access.
    Sandbox,
    /// Controlled host file sentinel cannot be read.
    HostFileDenied,
    /// Host agent, display and helper IPC cannot be reached.
    HostIpcDenied,
    /// Gateway policy digest/default-deny counters; guest cannot modify it.
    GatewayPolicy,
    /// Controlled LAN probe denied.
    LanDenied,
    /// Controlled metadata-service probe denied.
    MetadataDenied,
    /// Controlled arbitrary public destination probe denied.
    PublicDenied,
    /// Controlled direct DNS probe denied; resolution belongs to the gateway.
    DnsDenied,
    /// Controlled IPv6 probe denied.
    Ipv6Denied,
    /// Controlled QUIC/UDP probe denied.
    QuicDenied,
    /// Exact endpoint pinned; no arbitrary proxy, redirects or extra refresh endpoint.
    ModelEndpoint,
    /// Environment key allow-list, auth permissions/expiry, and no agent socket.
    CredentialMetadata,
    /// Native signature/instance, permission and Stop checks, without a model.
    NativeReadiness,
    /// External tree/Job/VM owner tested on timeout, cancellation and parent death.
    LifetimeBarrier,
}

impl Probe {
    /// All required probes. Missing and duplicate results are unknown.
    pub const ALL: [Self; 16] = [
        Self::DesktopInventory,
        Self::PrivateStorage,
        Self::Sandbox,
        Self::HostFileDenied,
        Self::HostIpcDenied,
        Self::GatewayPolicy,
        Self::LanDenied,
        Self::MetadataDenied,
        Self::PublicDenied,
        Self::DnsDenied,
        Self::Ipv6Denied,
        Self::QuicDenied,
        Self::ModelEndpoint,
        Self::CredentialMetadata,
        Self::NativeReadiness,
        Self::LifetimeBarrier,
    ];

    fn guarantee(self) -> Guarantee {
        match self {
            Self::DesktopInventory | Self::PrivateStorage => Guarantee::DisposableDesktop,
            Self::Sandbox | Self::HostFileDenied | Self::HostIpcDenied => {
                Guarantee::FileIpcIsolation
            }
            Self::GatewayPolicy
            | Self::LanDenied
            | Self::MetadataDenied
            | Self::PublicDenied
            | Self::DnsDenied
            | Self::Ipv6Denied
            | Self::QuicDenied
            | Self::ModelEndpoint => Guarantee::EndpointOnlyEgress,
            Self::CredentialMetadata => Guarantee::NoAmbientCredentials,
            Self::NativeReadiness | Self::LifetimeBarrier => Guarantee::NativeControlAndExit,
        }
    }
}

/// Machine observation; unavailable evidence never means success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The expected safe condition was observed.
    Passed,
    /// A controlled check observed a violation.
    Failed,
    /// Not measured, incomplete, timed out, missing or ambiguous.
    Unknown,
}

/// Evidence provenance, distinct from the result of a machine check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    /// Established by the trusted provider's machine probe.
    Checked,
    /// Established by a verified, bound, unexpired external attestation.
    Attested,
    /// No acceptable evidence.
    Unknown,
}

/// Trusted supervisor's expected identity. A different attempt needs new evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Unique run/attempt id, not a task name reused across repeats.
    pub attempt: String,
    /// Exact VM/device instance id.
    pub instance: String,
    /// Boot identity.
    pub boot: String,
    /// Disposable OS UID and desktop/session identity.
    pub desktop_user: String,
    /// Controlled base-image digest.
    pub image_digest: String,
    /// Externally enforced sandbox/gateway policy digest.
    pub policy_digest: String,
    /// Expected native engine's scope ACK (run, generation, scope digest).
    pub scope_ack: gcu_host_protocol::scope::Ack,
}

impl Binding {
    fn complete(&self) -> bool {
        [
            &self.attempt,
            &self.instance,
            &self.boot,
            &self.desktop_user,
            &self.image_digest,
            &self.policy_digest,
            &self.scope_ack.scope_digest,
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
            && self.scope_ack.run_id == self.attempt
            && self.scope_ack.generation > 0
    }
}

/// External proof. Raw bytes and identities are deliberately absent from reports.
#[derive(Debug, Clone)]
pub struct Attestation {
    /// Exact subject covered by the signed/owner-approved statement.
    pub binding: Binding,
    /// Which §4 external guarantees the signed statement covers.
    pub guarantees: Vec<Guarantee>,
    /// Exclusive expiry, in UTC Unix seconds.
    pub expires_at: u64,
    /// Opaque proof for an out-of-band configured verifier. Never a CLI checkbox.
    pub proof: Vec<u8>,
}

/// Must verify authenticity against pinned external trust and the entire statement,
/// including binding, claims and expiry. No production implementation ships here.
pub trait AttestationVerifier {
    /// False on invalid signature, unknown authority, parse failure or unavailable trust.
    fn authentic(&self, attestation: &Attestation) -> bool;
}

/// Evidence gathered without starting any credentialed client, including preflight.
#[derive(Debug, Clone, Default)]
pub struct Prepared {
    /// None means no environment was allocated; partial allocations need a binding.
    pub binding: Option<Binding>,
    /// One result per required probe; no model statements.
    pub checks: Vec<(Probe, Check)>,
    /// Required external evidence for every guarantee row.
    pub attestations: Vec<Attestation>,
    /// ACK received on the trusted private native control connection.
    pub scope_ack: Option<gcu_host_protocol::scope::Ack>,
}

/// Why the boundary is being torn down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupCause {
    /// Attempt completed (including client failure/timeout).
    Finished,
    /// Contract gate refused to start a client.
    Refused,
    /// Future/owner cancelled or unwound before an explicit finish.
    Cancelled,
    /// External supervisor observed owner-control EOF/death.
    ParentDeath,
}

/// Provider's teardown evidence. Kill/reap the entire tree before removing storage.
/// Removing a local directory or killing just the client leader is insufficient.
#[derive(Debug, Clone)]
pub struct CleanupProof {
    /// Same binding as preparation, including partial/failed preparations.
    pub binding: Option<Binding>,
    /// External barrier confirms every owned process/VM is gone.
    pub whole_tree: Check,
    /// All attempt storage and private auth material destroyed.
    pub storage: Check,
    /// All control/data channels and forwarding revoked.
    pub channels: Check,
}

/// Disposable boundary owner. Install external parent-death cleanup BEFORE
/// allocation. `prepare` failures still return evidence and must be cleaned up.
/// Calls must be bounded by the external supervisor; panic/drop is not a receipt.
pub trait DisposableProvider {
    /// Reset and probe one attempt. No credentialed process may be started here.
    fn prepare(&mut self) -> Prepared;
    /// Idempotently revoke, kill/reap the whole tree, then erase storage.
    /// None, failed or unknown evidence stops the run as `not_run`.
    fn teardown(&mut self, cause: CleanupCause) -> Option<CleanupProof>;
}

/// Fixed refusal labels; provider strings and proof payloads are never rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// No trusted provider allocated a disposable environment.
    ProviderUnavailable,
    /// Missing, incomplete or stale attempt/environment identity.
    BindingMismatch,
    /// A machine probe failed; attestation cannot override it.
    MachineCheckFailed,
    /// Required machine evidence missing/duplicated/incomplete.
    CriticalUnknown,
    /// Required external statement missing.
    MissingAttestation,
    /// External proof untrusted, expired, or bound to another subject.
    UntrustedAttestation,
    /// Missing or mismatched private scope ACK.
    MissingScopeAck,
    /// Missing, mismatched, failed or unknown whole-tree cleanup receipt.
    CleanupFailed,
}

/// One complete §4 row, preserving machine results and external provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuaranteeRecord {
    /// Required guarantee.
    pub guarantee: Guarantee,
    /// Checked only when every mandatory probe in this row passed.
    pub machine: EvidenceState,
    /// Attested only after external authenticity, identity and expiry verification.
    pub external: EvidenceState,
    /// Individual normalized machine observations, including unknown/missing probes.
    pub checks: Vec<(Probe, Check)>,
}

/// Sanitized cleanup receipt. No local paths, credentials or raw attestations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReceipt {
    /// Teardown trigger.
    pub cause: CleanupCause,
    /// Receipt belongs to the exact prepared subject.
    pub binding_matches: bool,
    /// Whole process/VM tree was destroyed.
    pub whole_tree: Check,
    /// Storage/credentials were erased.
    pub storage: Check,
    /// IPC/forwarding was revoked.
    pub channels: Check,
}

impl CleanupReceipt {
    /// All cleanup conditions were positively confirmed.
    pub fn complete(&self) -> bool {
        self.binding_matches
            && [self.whole_tree, self.storage, self.channels]
                .iter()
                .all(|check| *check == Check::Passed)
    }
}

/// Contract report. Passing this contract alone never enables real evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentReport {
    /// Always all five §4 rows, even when no provider exists.
    pub guarantees: Vec<GuaranteeRecord>,
    /// Private native ACK evidence.
    pub scope_ack: EvidenceState,
    /// Any refusal makes this and remaining attempts `not_run`.
    pub refusal: Option<Refusal>,
    /// Explicit teardown evidence; None is never successful cleanup.
    pub cleanup: Option<CleanupReceipt>,
}

impl EnvironmentReport {
    /// True only for a fully evidenced attempt with an explicit finished receipt.
    /// Also validates deserialized reports: a missing refusal label is not proof.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.refusal.is_none()
            && self.scope_ack == EvidenceState::Checked
            && self.guarantees.len() == Guarantee::ALL.len()
            && Guarantee::ALL.into_iter().all(|guarantee| {
                let mut rows = self
                    .guarantees
                    .iter()
                    .filter(|row| row.guarantee == guarantee);
                let Some(row) = rows.next() else {
                    return false;
                };
                rows.next().is_none()
                    && row.machine == EvidenceState::Checked
                    && row.external == EvidenceState::Attested
                    && row.checks.len()
                        == Probe::ALL
                            .into_iter()
                            .filter(|probe| probe.guarantee() == guarantee)
                            .count()
                    && Probe::ALL
                        .into_iter()
                        .filter(|probe| probe.guarantee() == guarantee)
                        .all(|probe| {
                            row.checks
                                .iter()
                                .filter(|check| **check == (probe, Check::Passed))
                                .count()
                                == 1
                        })
            })
            && self.cleanup.as_ref().is_some_and(|receipt| {
                receipt.cause == CleanupCause::Finished && receipt.complete()
            })
    }
}

fn evaluate(
    prepared: &Prepared,
    expected: Option<&Binding>,
    now: u64,
    verifier: &impl AttestationVerifier,
) -> EnvironmentReport {
    let mut refusal = match (&prepared.binding, expected) {
        (None, _) => Some(Refusal::ProviderUnavailable),
        (Some(actual), Some(expected)) if actual == expected && actual.complete() => None,
        _ => Some(Refusal::BindingMismatch),
    };
    let mut guarantees = Vec::new();
    for guarantee in Guarantee::ALL {
        let checks: Vec<_> = Probe::ALL
            .into_iter()
            .filter(|probe| probe.guarantee() == guarantee)
            .map(|probe| {
                let mut found = prepared.checks.iter().filter(|(key, _)| *key == probe);
                let check = found.next().map_or(Check::Unknown, |(_, check)| *check);
                (
                    probe,
                    if found.next().is_some() {
                        Check::Unknown
                    } else {
                        check
                    },
                )
            })
            .collect();
        let machine = if checks.iter().all(|(_, check)| *check == Check::Passed) {
            EvidenceState::Checked
        } else {
            refusal.get_or_insert(if checks.iter().any(|(_, check)| *check == Check::Failed) {
                Refusal::MachineCheckFailed
            } else {
                Refusal::CriticalUnknown
            });
            EvidenceState::Unknown
        };
        let claims: Vec<_> = prepared
            .attestations
            .iter()
            .filter(|attestation| attestation.guarantees.contains(&guarantee))
            .collect();
        let external = if !claims.is_empty()
            && claims.iter().all(|attestation| {
                Some(&attestation.binding) == expected
                    && attestation.binding.complete()
                    && attestation.expires_at > now
                    && verifier.authentic(attestation)
            }) {
            EvidenceState::Attested
        } else {
            refusal.get_or_insert(if claims.is_empty() {
                Refusal::MissingAttestation
            } else {
                Refusal::UntrustedAttestation
            });
            EvidenceState::Unknown
        };
        guarantees.push(GuaranteeRecord {
            guarantee,
            machine,
            external,
            checks,
        });
    }
    let scope_ack = if prepared.scope_ack.as_ref().is_some_and(|ack| {
        expected.is_some_and(|binding| binding.complete() && &binding.scope_ack == ack)
    }) {
        EvidenceState::Checked
    } else {
        refusal.get_or_insert(Refusal::MissingScopeAck);
        EvidenceState::Unknown
    };
    EnvironmentReport {
        guarantees,
        scope_ack,
        refusal,
        cleanup: None,
    }
}

struct Teardown<'a, P: DisposableProvider> {
    provider: &'a mut P,
    closed: bool,
}

impl<P: DisposableProvider> Drop for Teardown<'_, P> {
    fn drop(&mut self) {
        if !self.closed {
            // Best effort on unwind only. Parent death requires an external owner.
            let _ = self.provider.teardown(CleanupCause::Cancelled);
        }
    }
}

/// Shared ordering used by the stopped real-run path and fake contract tests.
/// The callback represents *all* client work, including tool preflight. A value
/// is withheld on cleanup failure, so it cannot turn into a scored pass.
pub(crate) fn supervise<T>(
    provider: &mut impl DisposableProvider,
    expected: Option<&Binding>,
    now: u64,
    verifier: &impl AttestationVerifier,
    client_work: impl FnOnce() -> T,
) -> (Option<T>, EnvironmentReport) {
    let mut held = Teardown {
        provider,
        closed: false,
    };
    let prepared = held.provider.prepare();
    let mut report = evaluate(&prepared, expected, now, verifier);
    let mut value = None;
    let cause = if report.refusal.is_some() {
        CleanupCause::Refused
    } else {
        value = Some(client_work());
        CleanupCause::Finished
    };
    report.cleanup = held.provider.teardown(cause).map(|proof| CleanupReceipt {
        cause,
        binding_matches: proof.binding == prepared.binding,
        whole_tree: proof.whole_tree,
        storage: proof.storage,
        channels: proof.channels,
    });
    held.closed = true;
    if !report
        .cleanup
        .as_ref()
        .is_some_and(CleanupReceipt::complete)
    {
        report.refusal = Some(Refusal::CleanupFailed);
        value = None;
    }
    (value, report)
}

struct Unavailable;
impl DisposableProvider for Unavailable {
    fn prepare(&mut self) -> Prepared {
        Prepared::default()
    }
    fn teardown(&mut self, _: CleanupCause) -> Option<CleanupProof> {
        // No allocation or process ever existed. This is not VM cleanup evidence.
        Some(CleanupProof {
            binding: None,
            whole_tree: Check::Passed,
            storage: Check::Passed,
            channels: Check::Passed,
        })
    }
}
impl AttestationVerifier for Unavailable {
    fn authentic(&self, _: &Attestation) -> bool {
        false
    }
}

/// There is deliberately no runtime provider/attestation import or override.
pub(crate) fn unavailable_report() -> EnvironmentReport {
    supervise(&mut Unavailable, None, 0, &Unavailable, || ()).1
}

/// The runnable local surface is fake-only. No flag can substitute a provider.
pub(crate) fn refuse_unsupported_launch(
    task: &crate::tasks::Task,
    cfg: &crate::RunConfig,
) -> Option<crate::TaskOutcome> {
    // S3's scoped engine has a fixed --driver fake launch. Only the compiled
    // artifact is an offline backend; an arbitrary path plus fake_scope is not.
    let trusted_fake_scope = cfg.fake_scope.is_some()
        && cfg
            .gcu_bin
            .as_deref()
            .is_some_and(|path| crate::boundary::is_builtin_artifact(path, "gcu"));
    let external_backend = cfg.builtin_fake
        && cfg.gcu_bin.is_some()
        && !crate::boundary::builtin_gcu_trusted(cfg)
        && !trusted_fake_scope;
    if cfg.run_local && !crate::boundary::launches_real_client(cfg) && !external_backend {
        return None;
    }
    let mut row = crate::runner::blank(&task.id, &task.category, cfg, crate::RunStatus::NotRun,
        "environment_contract: provider_unavailable; --attest-disposable-desktop is not evidence; real evaluation remains stopped");
    row.env_destroyed = true; // Nothing was allocated, including no credential copy.
    row.assurance.environment = Some(unavailable_report());
    Some(row)
}

#[cfg(test)]
mod tests;
