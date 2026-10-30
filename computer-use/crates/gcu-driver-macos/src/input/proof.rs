//! Arm the delivery gate for one action and drop it afterwards.
//!
//! A live backend posts nothing when the gate is missing. Unscoped work
//! installs a gate that allows the post. Scoped work installs the binding
//! captured with the job and refuses when that binding is missing. Cleanup
//! ups use `post_release` and skip the gate.

use std::sync::Arc;

use gcu_core::driver::Result;
use gcu_core::scope::WindowBinding;

use super::events::{EventApi, EventKind, PostFault, PostGate, PostPath, SyntheticEvent};
use crate::scope_os::{not_proved, Evidence, JobScope, PostView};

/// Clears the gate when dropped, including on error and cancel.
pub(crate) struct Armed<'a> {
    api: &'a dyn EventApi,
}

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.api.arm_post_gate(None);
    }
}

/// Install the proof for `window_id` from `scope`.
///
/// Scoped jobs use the binding passed in. This function does not read the pin
/// and does not look up a replacement window for them. A scoped job with no
/// binding clears the gate and refuses. Unscoped enforcing evidence still
/// looks up `window_id`. Unscoped live evidence installs a gate that allows
/// the post. `None` is never an allow. Memory only aside from that lookup. No extra
/// timeout or cancellation.
pub(crate) fn arm<'a>(
    api: &'a dyn EventApi,
    evidence: Arc<Evidence>,
    window_id: u32,
    scope: &JobScope,
) -> Result<Armed<'a>> {
    if scope.is_scoped() {
        let Some(admitted) = scope.expected().cloned() else {
            api.arm_post_gate(None);
            return Err(not_proved());
        };
        if admitted.native_id() != window_id.to_string() {
            api.arm_post_gate(None);
            return Err(not_proved());
        }
        api.arm_post_gate(Some(Arc::new(EvidenceGate {
            evidence,
            admitted,
            scoped: true,
        })));
        return Ok(Armed { api });
    }
    if evidence.enforcing() {
        let Some(admitted) = evidence.window_binding_cg(window_id) else {
            api.arm_post_gate(None);
            return Err(not_proved());
        };
        api.arm_post_gate(Some(Arc::new(EvidenceGate {
            evidence,
            admitted,
            scoped: false,
        })));
        return Ok(Armed { api });
    }
    api.arm_post_gate(Some(Arc::new(UnscopedGate)));
    Ok(Armed { api })
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
