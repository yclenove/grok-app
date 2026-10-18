//! In-memory broker gate. It records consent and refuses L2.
//!
//! `serve` does not call this type. A future production channel must keep
//! refusing while [`super::proof::mapping_verdict`] stays unproven. Pending DOM
//! mutations finish as unknown and are not replayed on L0.
//!
//! Blocking: none. Timeout: none. Cancellation: none.

use super::proof::{
    check_launch, mapping_verdict, select_engine, select_session, EngineEndpoint, ExpectedBuild,
    L2Decision, L2Refusal, LaunchSnapshot, MapClaim, OsWindow, ProofBrowser, ProofPlatform,
    SessionCandidate, SharedDocument,
};
use super::wire;
use super::{ExtensionIdentity, RoutedResult, RouterGate, Scope};
use serde_json::Value;

/// Inputs for one native-host connection. None of them grant L2.
pub struct LaunchInput<'a> {
    /// Embedded extension identity.
    pub identity: &'a ExtensionIdentity,
    /// Origin the browser put on argv.
    pub origin: &'a str,
    /// Platform the evidence is about.
    pub platform: ProofPlatform,
    /// Browser the parent image must be.
    pub browser: ProofBrowser,
    /// OS process and argv snapshot.
    pub launch: &'a LaunchSnapshot,
    /// gcu build this engine will accept.
    pub expected: &'a ExpectedBuild,
    /// Listening engines. Zero or several refuse the connection.
    pub engines: &'a [EngineEndpoint],
}

/// How a pending DOM request was retired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelKind {
    /// Pipe closed or the host process dropped the connection.
    Disconnect,
    /// The user revoked that share.
    Revoke,
    /// The core session ended. Shares die with it.
    SessionEnd,
}

/// A DOM mutation whose delivery is unknown. Do not send it again on L0 or L2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncertainMutation {
    /// Server-owned session.
    pub session_id: String,
    /// Server-owned request id.
    pub request_id: String,
}

/// Refusal plus any mutations that must stay unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofOutcome {
    /// Always [`L2Decision::Unavailable`].
    pub decision: L2Decision,
    /// Unique authorized session that was still refused, when one existed.
    pub refused_session: Option<String>,
    /// Mutations retired by this call. Already-retired ids are not repeated.
    pub uncertain: Vec<UncertainMutation>,
}

#[derive(Debug)]
struct Retired {
    request_id: String,
    session_id: String,
    mutation: bool,
    settled: bool,
}

/// Per-connection proof state. Dropping it drops consent; it does not clear Stop.
#[derive(Debug)]
pub struct BrokerGate {
    epoch: u64,
    live: bool,
    platform: ProofPlatform,
    browser: ProofBrowser,
    engine_id: String,
    foreground_hwnd: Option<u64>,
    share: Option<SharedDocument>,
    revoked_share: Option<String>,
    retired: Vec<Retired>,
}

impl BrokerGate {
    /// Accept a browser-launched host process, or refuse it.
    ///
    /// Success means the pipe may carry consent announcements. It does not mean
    /// L2 is available. Blocking: none. Timeout: none. Cancellation: none.
    pub fn open(input: LaunchInput<'_>) -> Result<Self, L2Refusal> {
        let checked = check_launch(
            input.identity,
            input.origin,
            input.platform,
            input.browser,
            input.launch,
            input.expected,
        )?;
        let engine = select_engine(input.engines)?;
        Ok(Self {
            epoch: 1,
            live: true,
            platform: input.platform,
            browser: input.browser,
            engine_id: engine.engine_id.clone(),
            foreground_hwnd: checked.foreground_hwnd,
            share: None,
            revoked_share: None,
            retired: Vec::new(),
        })
    }

    /// Connection generation. A disconnect advances it. Zero is never current.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Windows foreground HWND parsed from argv, if the launch had one.
    #[must_use]
    pub const fn foreground_hwnd(&self) -> Option<u64> {
        self.foreground_hwnd
    }

    /// Record browser consent. A second document makes the session ambiguous.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn share(&mut self, document: SharedDocument) -> Result<(), L2Refusal> {
        if !self.live {
            return Err(L2Refusal::Disconnected);
        }
        if document.frame_id != 0 || !wire::identifier(&document.document_id) {
            return Err(L2Refusal::WrongDocument);
        }
        if !wire::identifier(&document.share_id) {
            return Err(L2Refusal::Unshared);
        }
        if document.tab_id > i32::MAX as u32 || document.browser_window_id > i32::MAX as u32 {
            return Err(L2Refusal::WrongTab);
        }
        if self.share.is_some() {
            return Err(L2Refusal::AmbiguousSession);
        }
        self.revoked_share = None;
        self.share = Some(document);
        Ok(())
    }

    /// Decide L2 for this claim. A unique authorized session is still refused
    /// when the window mapping is unproven.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn evaluate(&self, claim: &MapClaim, sessions: &[SessionCandidate]) -> ProofOutcome {
        let _ = claim.similarity_discarded();
        if !self.live {
            return outcome(L2Refusal::Disconnected, None, Vec::new());
        }
        if claim.epoch != self.epoch {
            return outcome(L2Refusal::StaleEpoch, None, Vec::new());
        }
        let Some(share) = self.share.clone() else {
            let reason = if self.revoked_share.as_deref() == Some(claim.share_id.as_str()) {
                L2Refusal::Revoked
            } else {
                L2Refusal::Unshared
            };
            return outcome(reason, None, Vec::new());
        };
        if claim.share_id != share.share_id {
            return outcome(L2Refusal::Unshared, None, Vec::new());
        }
        if claim.tab_id != share.tab_id {
            return outcome(L2Refusal::WrongTab, None, Vec::new());
        }
        if claim.document_id != share.document_id {
            return outcome(L2Refusal::WrongDocument, None, Vec::new());
        }
        if !window_consistent(
            self.platform,
            self.foreground_hwnd,
            claim,
            share.browser_window_id,
        ) {
            return outcome(L2Refusal::WrongWindow, None, Vec::new());
        }
        let session = match select_session(&self.engine_id, self.epoch, sessions) {
            Ok(session) => session,
            Err(reason) => return outcome(reason, None, Vec::new()),
        };
        if session.window_id != claim.core_window_id || claim.core_window_id.is_empty() {
            return outcome(L2Refusal::WrongWindow, None, Vec::new());
        }
        // `admits_l2` is false for every reviewed platform. Do not replace this
        // with an allow: a proven mapping needs its own evidence and review.
        let _ = mapping_verdict(self.platform, self.browser).admits_l2();
        outcome(
            L2Refusal::WindowUnproven,
            Some(session.session_id.clone()),
            Vec::new(),
        )
    }

    /// Retire pending wire results. Mutations are unknown and are not replayed.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn absorb_cancel(&mut self, kind: CancelKind, cancelled: &[RoutedResult]) -> ProofOutcome {
        if !self.live {
            return outcome(L2Refusal::Disconnected, None, Vec::new());
        }
        let uncertain = self.record_cancelled(cancelled);
        match kind {
            CancelKind::Disconnect => {
                self.live = false;
                self.share = None;
                self.epoch = self.epoch.wrapping_add(1).max(1);
                outcome(L2Refusal::Disconnected, None, uncertain)
            }
            CancelKind::Revoke | CancelKind::SessionEnd => {
                if let Some(share) = self.share.take() {
                    self.revoked_share = Some(share.share_id);
                }
                let reason = if self.revoked_share.is_some() || !uncertain.is_empty() {
                    L2Refusal::Revoked
                } else {
                    L2Refusal::Unshared
                };
                outcome(reason, None, uncertain)
            }
        }
    }

    /// Close the pipe and advance the epoch.
    pub fn disconnect(&mut self) -> ProofOutcome {
        self.absorb_cancel(CancelKind::Disconnect, &[])
    }

    /// Withdraw one share. A mutation cancelled with it stays unknown.
    pub fn revoke(&mut self, share_id: &str, cancelled: &[RoutedResult]) -> ProofOutcome {
        if !self.live {
            return outcome(L2Refusal::Disconnected, None, Vec::new());
        }
        let matches = self
            .share
            .as_ref()
            .is_some_and(|share| share.share_id == share_id);
        if !matches && self.revoked_share.as_deref() != Some(share_id) {
            return outcome(L2Refusal::Unshared, None, Vec::new());
        }
        self.absorb_cancel(CancelKind::Revoke, cancelled)
    }

    /// Reject a response that is late, stale, or no longer owned.
    ///
    /// A matching unsettled mutation is reported unknown once. Blocking: none.
    /// Timeout: none. Cancellation: none.
    pub fn late_response(&mut self, epoch: u64, request_id: &str) -> ProofOutcome {
        if !self.live {
            return outcome(L2Refusal::Disconnected, None, Vec::new());
        }
        if epoch != self.epoch || epoch == 0 {
            return outcome(L2Refusal::StaleEpoch, None, Vec::new());
        }
        let uncertain = self
            .retired
            .iter_mut()
            .find(|item| item.request_id == request_id)
            .filter(|item| item.mutation && !item.settled)
            .map(|item| {
                item.settled = true;
                vec![UncertainMutation {
                    session_id: item.session_id.clone(),
                    request_id: request_id.to_owned(),
                }]
            })
            .unwrap_or_default();
        outcome(L2Refusal::LateResponse, None, uncertain)
    }

    fn record_cancelled(&mut self, cancelled: &[RoutedResult]) -> Vec<UncertainMutation> {
        let mut fresh = Vec::new();
        for item in cancelled {
            if self
                .retired
                .iter()
                .any(|retired| retired.request_id == item.request_id)
            {
                continue;
            }
            let mutation = item
                .result
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "action");
            if mutation {
                fresh.push(UncertainMutation {
                    session_id: item.session_id.clone(),
                    request_id: item.request_id.clone(),
                });
            }
            self.retired.push(Retired {
                request_id: item.request_id.clone(),
                session_id: item.session_id.clone(),
                mutation,
                settled: mutation,
            });
        }
        fresh
    }
}

impl RouterGate for BrokerGate {
    fn authorize(&self, session: &str, scope: &Scope, operation: &Value) -> bool {
        // The mapping table is the admission switch. Unproven platforms cannot
        // turn a share, a session id, or a cached flag into a DOM dispatch.
        mapping_verdict(self.platform, self.browser).admits_l2()
            && self.live
            && wire::identifier(session)
            && scope.valid()
            && wire::valid_operation(operation)
            && self.share.as_ref().is_some_and(|share| {
                share.share_id == scope.share_id
                    && share.tab_id == scope.tab_id
                    && share.document_id == scope.document_id
            })
    }
}

fn outcome(
    reason: L2Refusal,
    refused_session: Option<String>,
    uncertain: Vec<UncertainMutation>,
) -> ProofOutcome {
    ProofOutcome {
        decision: L2Decision::Unavailable { reason },
        refused_session,
        uncertain,
    }
}

fn window_consistent(
    platform: ProofPlatform,
    foreground_hwnd: Option<u64>,
    claim: &MapClaim,
    browser_window_id: u32,
) -> bool {
    if claim.browser_window_id != browser_window_id {
        return false;
    }
    match &claim.os_window {
        None => true,
        Some(OsWindow::Win32 { hwnd }) => {
            platform == ProofPlatform::Windows
                && *hwnd > 0
                && foreground_hwnd.is_none_or(|live| live == *hwnd)
        }
        Some(OsWindow::X11 { xid }) => platform == ProofPlatform::LinuxX11 && *xid > 0,
        Some(OsWindow::Wayland { handle }) => {
            platform == ProofPlatform::LinuxWayland && wire::identifier(handle)
        }
        Some(OsWindow::Macos { window_id }) => platform == ProofPlatform::Macos && *window_id > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::proof::{
        EngineEndpoint, ExpectedBuild, LaunchSnapshot, MapClaim, ProofBrowser, ProofPlatform,
        SessionCandidate, SharedDocument,
    };
    use super::{BrokerGate, L2Refusal, LaunchInput};
    use crate::native_host::ExtensionIdentity;
    use async_trait::async_trait;
    use gcu_core::browser::{DomChannel, DomEffect, DomFault, DomOperation, Hub};
    use gcu_core::driver::{AppRef, WindowRef};
    use gcu_core::model::RawTree;
    use gcu_protocol::AppInfo;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn refused_mapping_leaves_the_accessibility_tree_unread() {
        let identity = ExtensionIdentity::embedded().expect("manifest key");
        let origin = identity.origin();
        let launch = LaunchSnapshot {
            host_pid: 40,
            host_instance: 8,
            host_exe: "/usr/bin/gcu".into(),
            host_build_id: super::super::proof::build_id(),
            parent_pid: 4,
            parent_instance: 3,
            parent_exe_name: "google-chrome-stable".into(),
            argv: vec!["native-host".into(), origin.clone()],
        };
        let expected = ExpectedBuild {
            exe: "/usr/bin/gcu".into(),
            build_id: super::super::proof::build_id(),
        };
        let engines = [EngineEndpoint {
            engine_id: "engine-a".into(),
            instance: 5,
        }];
        let mut gate = BrokerGate::open(LaunchInput {
            identity: &identity,
            origin: &origin,
            platform: ProofPlatform::LinuxX11,
            browser: ProofBrowser::Chrome,
            launch: &launch,
            expected: &expected,
            engines: &engines,
        })
        .expect("launch shape");
        gate.share(SharedDocument {
            share_id: "share-1".into(),
            tab_id: 7,
            frame_id: 0,
            document_id: "document-1".into(),
            browser_window_id: 3,
        })
        .unwrap();
        let outcome = gate.evaluate(
            &MapClaim {
                share_id: "share-1".into(),
                tab_id: 7,
                document_id: "document-1".into(),
                browser_window_id: 3,
                epoch: 1,
                core_window_id: "window-1".into(),
                os_window: None,
                title_similarity: 100,
                geometry_similarity: 100,
            },
            &[SessionCandidate {
                session_id: "session-a".into(),
                engine_id: "engine-a".into(),
                authorized: true,
                epoch: 1,
                window_id: "window-1".into(),
            }],
        );
        assert!(matches!(
            outcome.decision,
            super::L2Decision::Unavailable {
                reason: L2Refusal::WindowUnproven
            }
        ));
        let channel = Arc::new(Counting::default());
        let hub = Hub::new(channel.clone());
        let window = WindowRef {
            app: AppRef {
                info: AppInfo {
                    id: "chrome".into(),
                    name: "Chrome".into(),
                    pid: None,
                    identity: None,
                },
            },
            id: "window-1".into(),
        };
        let aligned = hub.align("session-a", &window, RawTree::default()).await;
        assert!(aligned.root.is_none());
        assert_eq!(channel.observes.load(Ordering::SeqCst), 0);
        assert_eq!(channel.performs.load(Ordering::SeqCst), 0);
    }

    #[derive(Default)]
    struct Counting {
        observes: AtomicUsize,
        performs: AtomicUsize,
    }
    #[async_trait]
    impl DomChannel for Counting {
        fn extension_id(&self) -> &str {
            "jgpkhdjlddaoopklipcnpcjcpiancmff"
        }
        async fn observe(&self, _: &str) -> Result<Value, DomFault> {
            self.observes.fetch_add(1, Ordering::SeqCst);
            Err(DomFault::Unavailable)
        }
        async fn perform(&self, _: &DomOperation) -> Result<DomEffect, DomFault> {
            self.performs.fetch_add(1, Ordering::SeqCst);
            Err(DomFault::Unavailable)
        }
    }
}
