//! Production Host callbacks. Approvals wait on the shared office.

use std::sync::Arc;

use gcu_host_protocol::approval::{host_callbacks, Office};
use gcu_host_protocol::events::observe_panel;
use gcu_host_protocol::events::Gallery;
use gcu_host_protocol::panel::Panel;
use gcu_host_protocol::{
    auth::ProcessInstance,
    messages::{Request, RpcError},
    server::{CallbackFuture, HostCallbacks, Peer},
};

/// Callbacks for a supervised engine. The office is the one the UI command uses.
///
/// `gallery` keeps redacted `host.event.emit` thumbnails. `panel` is the side
/// timeline. Every other callback, including the overlay Stop state, is forwarded.
pub fn production_callbacks(
    office: Office,
    gallery: Gallery,
    panel: Panel,
    overlay: super::overlay::Overlay,
) -> Arc<dyn HostCallbacks> {
    observe_panel(
        Arc::new(Callbacks {
            approvals: host_callbacks(office),
            overlay,
        }),
        gallery,
        panel,
    )
}
struct Callbacks {
    approvals: Arc<dyn HostCallbacks>,
    overlay: super::overlay::Overlay,
}
impl HostCallbacks for Callbacks {
    fn notification(&self, request: &Request) -> Option<Result<serde_json::Value, RpcError>> {
        request.method.starts_with("host.overlay.").then(|| {
            self.overlay
                .notification(request)
                .map(|()| serde_json::json!({}))
                .map_err(|_| RpcError::new(-32602, "invalid overlay"))
        })
    }
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        if request.method.starts_with("host.overlay.") {
            Box::pin(async move {
                self.overlay
                    .notification(&request)
                    .map_err(|_| RpcError::new(-32602, "invalid overlay"))?;
                Ok(serde_json::json!({}))
            })
        } else {
            self.approvals.handle(request)
        }
    }
    fn attached(&self, engine: ProcessInstance, peer: Peer) {
        self.approvals.attached(engine, peer);
    }
    fn stop_state(&self) -> Option<bool> {
        Some(self.overlay.safety.stopped())
    }
    fn control_attached(&self, peer: Peer) {
        self.overlay.safety.attach(peer);
    }
    fn disconnected(&self) {
        self.approvals.disconnected();
        self.overlay.disconnected();
    }
}
