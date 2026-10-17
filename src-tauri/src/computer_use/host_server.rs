//! Production Host callbacks. Approvals wait on the shared office.

use std::sync::Arc;

use gcu_host_protocol::approval::{host_callbacks, Office};
use gcu_host_protocol::server::HostCallbacks;

/// Callbacks for a supervised engine. The office is the one the UI command uses.
pub fn production_callbacks(office: Office) -> Arc<dyn HostCallbacks> {
    host_callbacks(office)
}
