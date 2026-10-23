//! Transport encoding can resize a safe image after the observation is produced.
use super::{lock, Session};
use crate::snapshot::{SnapshotId, WindowKey};
impl Session {
    /// Register the actual encoded screenshot dimensions for coordinate mapping.
    /// Only updates this session's retained, server-issued observation. No IO,
    /// timeout, or cancellation; briefly locks snapshot memory. Never call with
    /// model-supplied IDs/sizes. Closed sessions are not repopulated.
    pub fn screenshot_delivered(
        &self,
        app_id: &str,
        window_id: &str,
        snapshot_id: &str,
        width: u32,
        height: u32,
    ) {
        let Some(id) = snapshot_id
            .strip_prefix("s_")
            .and_then(|id| id.parse::<u64>().ok())
        else {
            return;
        };
        if width == 0 || height == 0 || self.is_closed() {
            return;
        }
        let mut actions = lock(&self.actions);
        // Test barrier while this mutex is held, before the size is committed.
        // The scope lock is not taken here.
        self.delivery_gate.enter();
        actions.snapshots.set_delivery_size(
            &WindowKey {
                app_id: app_id.into(),
                window_id: window_id.into(),
            },
            SnapshotId(id),
            width,
            height,
        );
    }
}
