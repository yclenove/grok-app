//! Wayland and portal scope. A source that is not a peer window is closed.
//!
//! Portal monitor and desktop streams, and a window label without a native
//! peer credential, cannot prove which client owns the pixels. Those paths
//! return [`DriverError::ScopeDenied`] and do not fall back to the desktop.
//! This driver does not receive a peer credential on the portal capture path,
//! so that path stays closed. A caller that already holds a peer proof can
//! pass it here; a mismatched stream or generation is still a refusal.

use crate::identity::scope::{
    wayland_closed, ProcessView, ScopeFault, ScopeGate, WaylandOrigin, WaylandView,
};
use crate::wayland::PortalSourceKind;
use gcu_core::driver::DriverError;

/// Peer credential for one compositor window. A portal dialog selection is not this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerProof {
    /// Peer pid.
    pub pid: u32,
    /// `/proc` start time of that pid.
    pub start_time: u64,
    /// Boot id paired with the start time.
    pub boot_id: String,
    /// Compositor window generation.
    pub generation: u64,
    /// Stream source id. It must equal the window id.
    pub stream_source: String,
}

/// Build the observation a scope gate can prove.
///
/// `peer == None` is always a closed origin, including when the portal says
/// the source type is a window.
pub(crate) fn view_for_scope(
    window_id: &str,
    source: PortalSourceKind,
    peer: Option<&PeerProof>,
) -> WaylandView {
    let Some(peer) = peer else {
        if source == PortalSourceKind::Monitor {
            return WaylandView {
                origin: WaylandOrigin::Desktop,
                window_id: window_id.to_string(),
                generation: 0,
                peer_pid: None,
                owner: None,
                stream_source: String::new(),
            };
        }
        return wayland_closed(window_id);
    };
    if source != PortalSourceKind::Window || peer.stream_source != window_id {
        return WaylandView {
            origin: WaylandOrigin::UnprovedWindow,
            window_id: window_id.to_string(),
            generation: peer.generation,
            peer_pid: Some(peer.pid),
            owner: ProcessView::new(peer.pid, peer.start_time, peer.boot_id.clone()),
            stream_source: peer.stream_source.clone(),
        };
    }
    WaylandView {
        origin: WaylandOrigin::PeerWindow,
        window_id: window_id.to_string(),
        generation: peer.generation,
        peer_pid: Some(peer.pid),
        owner: ProcessView::new(peer.pid, peer.start_time, peer.boot_id.clone()),
        stream_source: peer.stream_source.clone(),
    }
}

/// Admit `window_id` for a scoped Wayland read or delivery.
pub(crate) fn admit(
    gate: &ScopeGate,
    window_id: &str,
    source: PortalSourceKind,
    peer: Option<&PeerProof>,
) -> Result<(), DriverError> {
    gate.prove_wayland(&view_for_scope(window_id, source, peer))
        .map(|_| ())
        .map_err(ScopeFault::deny)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::scope::{ScopeGate, ScopeInstance, ScopeWindow};

    fn gate() -> ScopeGate {
        let owner = ScopeInstance::try_new(42, 1000, "boot-a").unwrap();
        let window = ScopeWindow::try_new("wayland:1", 5, owner, "app.fixture").unwrap();
        ScopeGate::try_new(vec![window]).unwrap()
    }

    fn peer() -> PeerProof {
        PeerProof {
            pid: 42,
            start_time: 1000,
            boot_id: "boot-a".into(),
            generation: 5,
            stream_source: "wayland:1".into(),
        }
    }

    #[test]
    fn portal_without_a_peer_is_closed_for_monitor_and_window() {
        let gate = gate();
        assert_eq!(
            admit(&gate, "wayland:1", PortalSourceKind::Monitor, None),
            Err(DriverError::ScopeDenied)
        );
        assert_eq!(
            admit(&gate, "wayland:1", PortalSourceKind::Window, None),
            Err(DriverError::ScopeDenied)
        );
    }

    #[test]
    fn peer_window_matches_only_that_stream_and_generation() {
        let gate = gate();
        assert!(admit(&gate, "wayland:1", PortalSourceKind::Window, Some(&peer())).is_ok());
        let mut other_stream = peer();
        other_stream.stream_source = "wayland:2".into();
        assert_eq!(
            admit(
                &gate,
                "wayland:1",
                PortalSourceKind::Window,
                Some(&other_stream)
            ),
            Err(DriverError::ScopeDenied)
        );
        let mut reused = peer();
        reused.generation = 8;
        assert_eq!(
            admit(&gate, "wayland:1", PortalSourceKind::Window, Some(&reused)),
            Err(DriverError::ScopeDenied)
        );
        let mut owner = peer();
        owner.pid = 99;
        owner.stream_source = "wayland:1".into();
        assert_eq!(
            admit(&gate, "wayland:1", PortalSourceKind::Window, Some(&owner)),
            Err(DriverError::ScopeDenied)
        );
        assert_eq!(
            admit(&gate, "wayland:1", PortalSourceKind::Monitor, Some(&peer())),
            Err(DriverError::ScopeDenied)
        );
    }
}
