//! Pending requests use the same authority as execution, without starting it.
use std::sync::Arc;

use super::{BrokerOptions, ComputerUseBroker};

pub(super) fn broker() -> Arc<ComputerUseBroker> {
    let mut slot = super::runtime_slot().lock();
    slot.get_or_insert_with(|| {
        let enabled = crate::store::load_settings().computer_use_enabled;
        super::set_feature_enabled(enabled);
        let root = crate::paths::app_data_root().join("computer-use");
        let _ = grok_computer_use_core::privacy::write_owner(&root, "grok-app");
        Arc::new(ComputerUseBroker::request_endpoint(BrokerOptions {
            feature_enabled: enabled,
            lease_path: root.join("desktop.lease"),
            action_timeout: std::time::Duration::from_secs(8),
            ..BrokerOptions::default()
        }))
    })
    .clone()
}

/// Only consent state and loopback transport. No native/browser/helper setup.
pub(crate) fn ensure_request_endpoint() -> Result<Arc<ComputerUseBroker>, String> {
    let broker = broker();
    super::ipc::ensure(Arc::clone(&broker))?;
    super::consent_watch::start(&broker);
    Ok(broker)
}
