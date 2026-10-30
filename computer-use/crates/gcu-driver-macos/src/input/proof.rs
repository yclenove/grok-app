//! Build the delivery gate for one action.
//!
//! The gate is a value owned by that action. Dropping it does not clear
//! another action's gate. A live backend posts nothing when the caller does
//! not pass a gate. Unscoped work returns a gate that allows the post. Scoped
//! work returns the binding captured with the job and refuses when that
//! binding is missing. Cleanup ups use `post_release` and skip the gate.

use std::sync::Arc;

use gcu_core::driver::Result;
use gcu_core::scope::WindowBinding;

use super::events::{EventApi, EventKind, PostFault, PostGate, PostPath, SyntheticEvent};
use crate::scope_os::{not_proved, Evidence, JobScope, PostView};

/// Proof for one action. Drop does not touch any other action.
pub(crate) struct Armed {
    gate: Arc<dyn PostGate>,
}

impl Armed {
    /// Gate to pass into each post of this action.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn gate(&self) -> Arc<dyn PostGate> {
        Arc::clone(&self.gate)
    }
}

/// Proof for `window_id` from `scope`.
///
/// Scoped jobs use the binding passed in. This function does not read the pin
/// and does not look up a replacement window for them. A scoped job with no
/// binding refuses and returns no gate. Unscoped enforcing evidence still
/// looks up `window_id`. Unscoped live evidence returns a gate that allows
/// the post. Nothing here writes a shared slot. Memory only aside from that
/// lookup. No extra timeout or cancellation.
pub(crate) fn arm(
    _api: &dyn EventApi,
    evidence: Arc<Evidence>,
    window_id: u32,
    scope: &JobScope,
) -> Result<Armed> {
    if scope.is_scoped() {
        let Some(admitted) = scope.expected().cloned() else {
            return Err(not_proved());
        };
        if admitted.native_id() != window_id.to_string() {
            return Err(not_proved());
        }
        return Ok(Armed {
            gate: Arc::new(EvidenceGate {
                evidence,
                admitted,
                scoped: true,
            }),
        });
    }
    if evidence.enforcing() {
        let Some(admitted) = evidence.window_binding_cg(window_id) else {
            return Err(not_proved());
        };
        return Ok(Armed {
            gate: Arc::new(EvidenceGate {
                evidence,
                admitted,
                scoped: false,
            }),
        });
    }
    Ok(Armed {
        gate: Arc::new(UnscopedGate),
    })
}

/// Allows one unscoped post. A missing gate does not use this type.
struct UnscopedGate;

impl PostGate for UnscopedGate {
    fn before_post(&self, _event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
        Ok(())
    }
}

struct EvidenceGate {
    evidence: Arc<Evidence>,
    admitted: WindowBinding,
    scoped: bool,
}

impl PostGate for EvidenceGate {
    fn blocks_global_hid(&self) -> bool {
        self.scoped
    }

    fn before_post(&self, event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
        self.evidence.run_barrier();
        if self.scoped && event.path == PostPath::HidTap {
            return Err(PostFault::NotSent);
        }
        let post = PostView {
            foreground: event.path == PostPath::HidTap,
            coordinate: matches!(
                event.kind,
                EventKind::MouseDown
                    | EventKind::MouseUp
                    | EventKind::MouseDrag
                    | EventKind::MouseMove
                    | EventKind::Scroll
            ),
            pid: event.pid,
            window_id: event.window_id,
            screen: event.screen,
            window_local: event.window_local,
        };
        self.evidence
            .prove_delivery(&self.admitted, &post)
            .map_err(|_| PostFault::NotSent)
    }
}
