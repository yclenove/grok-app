//! Bounded session-private cache; only Results can insert redacted deliveries.
use super::lease_view::{visible, Epoch};
use super::retire::{CallbackSlot, Hold, Sweep};
use gcu_core::action::PublishGate;
use gcu_core::scope::ScopeLease;
use gcu_host_protocol::messages::Image;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, TryLockError},
};

struct Slot {
    images: VecDeque<(String, Image, Option<Epoch>)>,
    lease: Option<ScopeLease>,
}

pub(crate) struct ImageCache {
    sweep_id: u64,
    callbacks: CallbackSlot,
    /// Tickets a publish left because this mutex was busy or already held here.
    sweep: Sweep,
    slot: Mutex<Slot>,
    /// Parks after the cache mutex is taken and before the entry is inserted.
    /// Production leaves it disarmed.
    gate: Arc<PublishGate>,
}

/// Cache lock and private insertion, prepared before entering the lease lock.
pub(crate) struct ImageAdmission<'a> {
    cache: &'a ImageCache,
    hold: Option<Hold>,
    slot: Option<MutexGuard<'a, Slot>>,
    pending: Option<(String, Image)>,
    epoch: Option<Epoch>,
    /// Pixels removed under the lock and dropped after it is released.
    retired: Vec<Image>,
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
    /// No timeout or cancellation. A closed admission registers nothing.
    pub(crate) fn commit(&mut self) {
        let Some(slot) = self.slot.as_mut() else {
            return;
        };
        let Some((id, image)) = self.pending.take() else {
            return;
        };
        let epoch = self.epoch.clone();
        let replaced = slot.images.iter().position(|(key, _, _)| *key == id);
        if let Some(old) = replaced.and_then(|index| slot.images.remove(index)) {
            self.bytes -= old.1.bytes.len();
            self.retired.push(old.1);
        }
        self.bytes += image.bytes.len();
        slot.images.push_back((id, image, epoch));
        while self.bytes > 32 * 1024 * 1024 || slot.images.len() > 64 {
            if let Some(old) = slot.images.pop_front() {
                self.bytes -= old.1.bytes.len();
                self.retired.push(old.1);
            } else {
                break;
            }
        }
    }
}

impl Drop for ImageAdmission<'_> {
    fn drop(&mut self) {
        self.slot.take();
        self.hold.take();
        self.cache.flush_retired();
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self {
            sweep_id: super::retire::next_sweep_id(),
            callbacks: CallbackSlot::new(),
            sweep: Sweep::new(),
            slot: Mutex::new(Slot {
                images: VecDeque::new(),
                lease: None,
            }),
            gate: PublishGate::new(),
        }
    }
}

impl ImageCache {
    /// Remember the lease that later `get` calls compare, and retire on invalidation.
    ///
    /// Registers a weak callback on `lease`. Revoke, rebind, and a lease read
    /// that publishes expiry run that callback on the calling thread. Dropping
    /// this cache drops the callback. A second bind arms the new lease and
    /// drops the previous hook. Expiry is not watched: if nobody reads the
    /// lease, pixels stay until that read. Pixel release stays outside the
    /// cache lock. No timeout or cancellation.
    pub(crate) fn bind_lease(self: &Arc<Self>, lease: ScopeLease) {
        {
            let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            slot.lease = Some(lease.clone());
        }
        let cache = Arc::downgrade(self);
        let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(cache) = cache.upgrade() {
                cache.release_stale();
            }
        });
        self.callbacks.arm(&lease, callback);
        self.release_stale();
    }

    /// Drop images whose epoch is no longer visible.
    ///
    /// Pixels are dropped after the cache lock. The ticket is stored before
    /// `try_lock`. Admission holds this mutex across the publication gate, so
    /// a revoke on another thread must not wait for it. A busy mutex, or
    /// `Hold` already set on this thread, returns and the holder catches up
    /// after unlock. Same-thread `try_lock` is unspecified. No timeout or
    /// cancellation. At most eight passes; a still-open ticket waits for the
    /// next unlock instead of spinning.
    pub(crate) fn release_stale(&self) {
        if Hold::contains(self.sweep_id) {
            self.sweep.request();
            return;
        }
        self.sweep.request();
        self.drive();
    }

    fn flush_retired(&self) {
        if Hold::contains(self.sweep_id) {
            self.sweep.request();
            return;
        }
        self.drive();
    }

    fn drive(&self) {
        for _ in 0..8 {
            let target = self.sweep.requested();
            if self.sweep.finished() >= target {
                return;
            }
            let Some(removed) = self.extract() else {
                return;
            };
            drop(removed);
        }
    }

    /// `None` means another thread holds the cache lock.
    fn extract(&self) -> Option<Vec<Image>> {
        let _hold = Hold::enter(self.sweep_id);
        let mut slot = match self.slot.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(err)) => err.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let seen = self.sweep.requested();
        let removed = take_invisible(&mut slot);
        self.sweep.mark(seen);
        Some(removed)
    }

    /// Acquire the cache lock and prepare an insertion without exposing pixels.
    ///
    /// Memory only. No await, timeout, or cancellation. Run before acquiring the
    /// lease lock so a concurrent reader never delays revocation. Dropping an
    /// uncommitted admission leaves the cache unchanged. Stale rows are moved
    /// out under this lock and dropped after it, before the byte budget is counted.
    pub(crate) fn admission(&self, id: String, image: Image) -> ImageAdmission<'_> {
        let hold = Hold::enter(self.sweep_id);
        let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
        let mut retired = take_invisible(&mut slot);
        self.gate.enter();
        let bytes = slot
            .images
            .iter()
            .map(|(_, image, _)| image.bytes.len())
            .sum();
        retired.reserve(slot.images.len() + 1);
        slot.images.reserve(1);
        ImageAdmission {
            cache: self,
            hold: Some(hold),
            slot: Some(slot),
            pending: Some((id, image)),
            epoch: None,
            retired,
            bytes,
        }
    }

    /// Clone one image that is still the bound lease's current generation.
    ///
    /// A stale row is moved out under the cache lock and dropped after it.
    /// Memory only aside from the lease read inside [`visible`]. No timeout or
    /// cancellation.
    pub fn get(&self, id: &str) -> Option<Image> {
        let (removed, found) = {
            let _hold = Hold::enter(self.sweep_id);
            let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            let removed = take_invisible(&mut slot);
            let found = slot
                .images
                .iter()
                .find(|(key, _, _)| key == id)
                .map(|(_, image, _)| image.clone());
            (removed, found)
        };
        drop(removed);
        self.flush_retired();
        found
    }

    /// Drop every image. The bound lease stays.
    ///
    /// Pixels are dropped after the cache lock. A ticket left while this mutex
    /// was held is finished after that. Memory only. No timeout or cancellation.
    pub fn clear(&self) {
        let removed = {
            let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            std::mem::take(&mut slot.images)
        };
        drop(removed);
        self.flush_retired();
    }

    /// True when `id` is still stored, without dropping stale rows.
    ///
    /// [`Self::get`] reclaims first, so it cannot prove the callback released
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

fn take_invisible(slot: &mut Slot) -> Vec<Image> {
    let lease = slot.lease.clone();
    let mut kept = VecDeque::new();
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
