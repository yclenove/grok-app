//! Production Host callbacks. Approvals wait on the shared office.

use std::sync::Arc;

use gcu_host_protocol::approval::{host_callbacks, Office};
use gcu_host_protocol::events::{observe, Gallery};
use gcu_host_protocol::server::HostCallbacks;

/// Callbacks for a supervised engine. The office is the one the UI command uses.
///
/// `gallery` keeps redacted `host.event.emit` thumbnails. The wrapper still
/// answers the engine with an empty object.
pub fn production_callbacks(office: Office, gallery: Gallery) -> Arc<dyn HostCallbacks> {
    observe(host_callbacks(office), gallery)
}
