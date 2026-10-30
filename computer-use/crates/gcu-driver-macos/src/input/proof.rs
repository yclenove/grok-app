//! Arm the delivery gate for one action and drop it afterwards.
//!
//! A live backend posts nothing unless this gate is installed. The gate
//! re-reads the window list at the OS call. Cleanup ups use `post_release`
//! and skip the gate.

use std::sync::Arc;

use gcu_core::driver::Result;
use gcu_core::scope::WindowBinding;

use super::events::{EventApi, EventKind, PostFault, PostGate, PostPath, SyntheticEvent};
use crate::scope_os::{not_proved, Evidence, PostView};

/// Clears the gate when dropped, including on error and cancel.
pub(crate) struct Armed<'a> {
    api: &'a dyn EventApi,
}

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.api.arm_post_gate(None);
    }
}

/// Install the proof for `window_id`. Closed evidence clears the gate and allows the post.
pub(crate) fn arm(
    api: &dyn EventApi,
    evidence: Arc<Evidence>,
    window_id: u32,
) -> Result<Armed<'_>> {
    if !evidence.enforcing() {
        api.arm_post_gate(None);
        return Ok(Armed { api });
    }
    let Some(admitted) = evidence.window_binding_cg(window_id) else {
        api.arm_post_gate(None);
        return Err(not_proved());
    };
    api.arm_post_gate(Some(Arc::new(EvidenceGate { evidence, admitted })));
    Ok(Armed { api })
}

struct EvidenceGate {
    evidence: Arc<Evidence>,
    admitted: WindowBinding,
}

impl PostGate for EvidenceGate {
    fn before_post(&self, event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
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
