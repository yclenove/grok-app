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
    /// Set when the retire owner could not start. Commits then register nothing.
    publish_blocked: bool,
}

pub(crate) struct ImageCache {
    slot: Mutex<Slot>,
    /// Parks after the cache mutex is taken and before the entry is inserted.
    /// Production leaves it disarmed.
    gate: Arc<PublishGate>,
    /// Invalidation owner. Shutdown joins a synchronous thread before the slot drops.
    observer: Mutex<crate::retire::Observer>,
    /// Test parks copied into the owner at [`ImageCache::bind_lease`].
    #[cfg(test)]
    hooks: Mutex<crate::retire::DriveHooks>,
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
    ///
    /// When the retire owner failed to start, the pending image is left
    /// unregistered and drops with this admission. No timeout or cancellation.
    pub(crate) fn commit(&mut self) {
        if self.slot.publish_blocked {
            return;
        }
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
                publish_blocked: false,
            }),
            gate: PublishGate::new(),
            observer: Mutex::new(crate::retire::Observer::default()),
            #[cfg(test)]
            hooks: Mutex::new(crate::retire::DriveHooks::default()),
        }
    }
}

impl ImageCache {
    /// Remember the lease that later `get` calls compare, and retire on invalidation.
    ///
    /// Call before taking this cache's lock for a commit. One owner runs the
    /// sweep: a task on the current runtime, or a dedicated thread when there
    /// is none. A second bind replaces the lease and does not start another
    /// owner. If that owner cannot start, later commits register nothing. This
    /// call waits only until a synchronous thread has built its runtime.
    pub(crate) fn bind_lease(self: &Arc<Self>, lease: ScopeLease) {
        {
            let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            slot.lease = Some(lease.clone());
        }
        let mut observer = self.observer.lock().unwrap_or_else(|err| err.into_inner());
        if observer.watching() {
            return;
        }
        let cache = Arc::downgrade(self);
        #[cfg(test)]
        let hooks = self
            .hooks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        #[cfg(not(test))]
        let hooks = crate::retire::DriveHooks::default();
        let next = crate::retire::start(
            lease,
            Arc::new(move || {
                if let Some(cache) = cache.upgrade() {
                    cache.release_stale();
                }
            }),
            hooks,
        );
        // Observer, then the slot. `start` returns before the owner takes the slot.
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .publish_blocked = !next.watching();
        *observer = next;
    }

    /// Install parks and the exit count for the next [`Self::bind_lease`].
    ///
    /// Memory only. No timeout or cancellation. A bind that already started
    /// keeps the hooks it copied.
    #[cfg(test)]
    pub(crate) fn set_retire_hooks(&self, hooks: crate::retire::DriveHooks) {
        *self.hooks.lock().unwrap_or_else(|err| err.into_inner()) = hooks;
    }

    /// Drop images whose epoch is no longer visible. Memory only.
    ///
    /// The cache lock is released before this returns. No timeout or cancellation.
    pub(crate) fn release_stale(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
        reclaim(&mut slot);
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

    /// True when `id` is still stored, without dropping stale rows.
    ///
    /// [`Self::get`] reclaims first, so it cannot prove the watcher released
    /// the pixels. Memory only. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn resident(&self, id: &str) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .images
            .iter()
            .any(|(key, _, _)| key == id)
    }

    /// Pause used by [`Self::admission`]. Memory only. Production leaves it disarmed.
    #[cfg(test)]
    pub(crate) fn admission_gate(&self) -> Arc<PublishGate> {
        Arc::clone(&self.gate)
    }
}

impl Drop for ImageCache {
    fn drop(&mut self) {
        let _ = self
            .observer
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .shutdown();
    }
}

fn reclaim(slot: &mut Slot) {
    let lease = slot.lease.clone();
    slot.images
        .retain(|(_, _, epoch)| visible(lease.as_ref(), epoch.as_ref()));
}
