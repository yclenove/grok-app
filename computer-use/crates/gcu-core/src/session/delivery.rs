//! Transport encoding can resize a safe image after the observation is produced.
use super::{lock, Session};
use crate::action::SessionActions;
use crate::snapshot::{DeliverySize, SnapshotId, WindowKey};
use std::sync::MutexGuard;

/// Locked, private screenshot coordinate update. Construct before acquiring a
/// scope lease, then commit it alongside cache/file registration.
///
/// Memory only. Holds the snapshot mutex until dropped; must not cross an await.
/// Dropping it without `commit` leaves the stored dimensions unchanged.
pub struct ScreenshotDelivery<'a> {
    actions: MutexGuard<'a, SessionActions>,
    size: Option<DeliverySize>,
}

impl ScreenshotDelivery<'_> {
    /// Record the prepared dimensions. Memory only, no lock acquisition, IO,
    /// timeout, or cancellation. Keep the guard until all output is registered.
    pub fn commit(&mut self) {
        if let Some(size) = self.size.take() {
            self.actions.snapshots.apply_delivery_size(size);
        }
    }
}

impl Session {
    /// Prepare an encoded screenshot's coordinate mapping without publishing it.
    ///
    /// Acquires the snapshot mutex and waits at the bounded test barrier before
    /// any lease lock is acquired. No IO, timeout, or async cancellation. Never
    /// pass model-supplied IDs/sizes. Closed sessions, malformed IDs and empty
    /// sizes return `None`. Only validated retained observations are updated.
    pub fn prepare_screenshot_delivery(
        &self,
        app_id: &str,
        window_id: &str,
        snapshot_id: &str,
        width: u32,
        height: u32,
    ) -> Option<ScreenshotDelivery<'_>> {
        let id = snapshot_id.strip_prefix("s_")?.parse::<u64>().ok()?;
        if width == 0 || height == 0 || self.is_closed() {
            return None;
        }
        let window = WindowKey {
            app_id: app_id.into(),
            window_id: window_id.into(),
        };
        let actions = lock(&self.actions);
        self.delivery_gate.enter();
        if self.is_closed() {
            return None;
        }
        let size = actions
            .snapshots
            .prepare_delivery_size(&window, SnapshotId(id), width, height);
        Some(ScreenshotDelivery { actions, size })
    }

    /// Register encoded screenshot dimensions without a fixture scope.
    ///
    /// Memory only; briefly locks snapshot memory. No IO, timeout, or
    /// cancellation. Scoped publishers must prepare first and call `commit`
    /// inside `ScopeLease::commit_if_current` with their other registrations.
    pub fn screenshot_delivered(
        &self,
        app_id: &str,
        window_id: &str,
        snapshot_id: &str,
        width: u32,
        height: u32,
    ) {
        if let Some(mut delivery) =
            self.prepare_screenshot_delivery(app_id, window_id, snapshot_id, width, height)
        {
            delivery.commit();
        }
    }
}
