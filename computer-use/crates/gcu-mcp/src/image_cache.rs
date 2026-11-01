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
    /// Runs once, before [`Self::get`] takes the cache mutex. Test only.
    #[cfg(test)]
    before_get: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
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
            #[cfg(test)]
            before_get: Mutex::new(None),
        }
    }
}

impl Drop for ImageCache {
    fn drop(&mut self) {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .images
            .clear();
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
    /// Invisible rows are removed under the cache lock before the copy. Their
    /// pixels are dropped after that lock. Memory only aside from the lease
    /// read inside [`visible`]. No timeout or cancellation.
    pub fn get(&self, id: &str) -> Option<Image> {
        #[cfg(test)]
        if let Some(hook) = self
            .before_get
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take()
        {
            hook();
        }
        let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
        let removed = take_invisible(&mut slot);
        let found = slot
            .images
            .iter()
            .find(|(key, _, _)| key == id)
            .map(|(_, image, _)| image.clone());
        drop(slot);
        drop(removed);
        found
    }

    /// Drop every image. The bound lease stays. Memory only.
    pub fn clear(&self) {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .images
            .clear();
    }

    /// True when `id` is still stored. Does not remove stale rows.
    ///
    /// [`Self::get`] reclaims first, so it cannot prove that a row survived
    /// revoke. Memory only. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn resident(&self, id: &str) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .images
            .iter()
            .any(|(key, _, _)| key == id)
    }

    /// Run `hook` once, on the next [`Self::get`], before it locks the cache.
    ///
    /// Test only. The hook runs on the getter thread and must not take this
    /// cache lock. Memory only. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn arm_before_get_lock(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self
            .before_get
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(Box::new(hook));
    }

    /// Hold the cache lock until the guard drops. Rows stay as they are.
    ///
    /// Test only. Blocking: the cache mutex. No timeout or cancellation.
    #[cfg(test)]
    pub(crate) fn lock_for_test(&self) -> CacheHold<'_> {
        CacheHold {
            _guard: self.slot.lock().unwrap_or_else(|err| err.into_inner()),
        }
    }

    /// Pause used by [`Self::admission`]. Memory only. Production leaves it disarmed.
    #[cfg(test)]
    pub(crate) fn admission_gate(&self) -> Arc<PublishGate> {
        Arc::clone(&self.gate)
    }
}

/// Cache mutex held by a test. Drop releases it and does not remove rows.
#[cfg(test)]
pub(crate) struct CacheHold<'a> {
    _guard: MutexGuard<'a, Slot>,
}

fn reclaim(slot: &mut Slot) {
    drop(take_invisible(slot));
}

fn take_invisible(slot: &mut Slot) -> Vec<Image> {
    let lease = slot.lease.clone();
    let mut kept = VecDeque::with_capacity(slot.images.len());
    let mut dropped = Vec::new();
    for (id, image, epoch) in slot.images.drain(..) {
        if visible(lease.as_ref(), epoch.as_ref()) {
            kept.push_back((id, image, epoch));
        } else {
            dropped.push(image);
        }
    }
    slot.images = kept;
    dropped
}
