//! Bounded session-private cache; only Results can insert redacted deliveries.
use super::lease_view::{visible, Epoch};
use gcu_core::action::PublishGate;
use gcu_core::scope::ScopeLease;
use gcu_host_protocol::messages::Image;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

struct Slot {
    images: VecDeque<(String, Image, Option<Epoch>)>,
    lease: Option<ScopeLease>,
}

pub(crate) struct ImageCache {
    slot: Mutex<Slot>,
    /// Parks after the cache mutex is taken and before the entry is inserted.
    /// Production leaves it disarmed.
    gate: Arc<PublishGate>,
}

/// Cache lock and private insertion, prepared before entering the lease lock.
pub(crate) struct ImageAdmission<'a> {
    slot: MutexGuard<'a, Slot>,
    pending: Option<(String, Image)>,
    epoch: Option<Epoch>,
    retired: Vec<(String, Image)>,
    bytes: usize,
}

impl ImageAdmission<'_> {
    /// Generation copied from the authority pairs inside the commit closure.
    ///
    /// Memory only. No await, timeout, or cancellation. `None` is unscoped.
    pub(crate) fn stamp(&mut self, epoch: Option<Epoch>) {
        self.epoch = epoch;
    }

    /// Infallible memory registration. Retired pixels drop after the lease lock.
    pub(crate) fn commit(&mut self) {
        let Some((id, image)) = self.pending.take() else {
            return;
        };
        let epoch = self.epoch.clone();
        let replaced = self.slot.images.iter().position(|(key, _, _)| *key == id);
        if let Some(old) = replaced.and_then(|index| self.slot.images.remove(index)) {
            self.bytes -= old.1.bytes.len();
            self.retired.push((old.0, old.1));
        }
        self.bytes += image.bytes.len();
        self.slot.images.push_back((id, image, epoch));
        while self.bytes > 32 * 1024 * 1024 || self.slot.images.len() > 64 {
            if let Some(old) = self.slot.images.pop_front() {
                self.bytes -= old.1.bytes.len();
                self.retired.push((old.0, old.1));
            } else {
                break;
            }
        }
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self {
            slot: Mutex::new(Slot {
                images: VecDeque::new(),
                lease: None,
            }),
            gate: PublishGate::new(),
        }
    }
}

impl ImageCache {
    /// Remember the lease that later `get` calls compare. Memory only.
    ///
    /// Call before taking this cache's lock for a commit. Replaces any previous
    /// lease. No timeout or cancellation.
    pub(crate) fn bind_lease(&self, lease: ScopeLease) {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .lease = Some(lease);
    }

    /// Acquire the cache lock and prepare an insertion without exposing pixels.
    ///
    /// Memory only. No await, timeout, or cancellation. Run before acquiring the
    /// lease lock so a concurrent reader never delays revocation. Dropping an
    /// uncommitted admission leaves the cache unchanged. Stale rows are dropped
    /// under this same lock before the byte budget is counted.
    pub(crate) fn admission(&self, id: String, image: Image) -> ImageAdmission<'_> {
        let gate = Arc::clone(&self.gate);
        let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
        reclaim(&mut slot);
        gate.enter();
        let bytes = slot
            .images
            .iter()
            .map(|(_, image, _)| image.bytes.len())
            .sum();
        let retired = Vec::with_capacity(slot.images.len() + 1);
        slot.images.reserve(1);
        ImageAdmission {
            slot,
            pending: Some((id, image)),
            epoch: None,
            retired,
            bytes,
        }
    }

    /// Clone one image that is still the bound lease's current generation.
    ///
    /// A stale row is dropped under the cache lock before the copy. Memory only
    /// aside from the lease read inside [`visible`]. No timeout or cancellation.
    pub fn get(&self, id: &str) -> Option<Image> {
        let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
        reclaim(&mut slot);
        slot.images
            .iter()
            .find(|(key, _, _)| key == id)
            .map(|(_, image, _)| image.clone())
    }

    /// Drop every image. The bound lease stays. Memory only.
    pub fn clear(&self) {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .images
            .clear();
    }

    /// Pause used by [`Self::admission`]. Memory only. Production leaves it disarmed.
    #[cfg(test)]
    pub(crate) fn admission_gate(&self) -> Arc<PublishGate> {
        Arc::clone(&self.gate)
    }
}

fn reclaim(slot: &mut Slot) {
    let lease = slot.lease.clone();
    slot.images
        .retain(|(_, _, epoch)| visible(lease.as_ref(), epoch.as_ref()));
}
