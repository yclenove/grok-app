//! Bounded session-private cache; only Results can insert redacted deliveries.
use gcu_core::action::PublishGate;
use gcu_host_protocol::messages::Image;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};
pub(crate) struct ImageCache {
    images: Mutex<VecDeque<(String, Image)>>,
    /// Parks after the cache mutex is taken and before the entry is inserted.
    /// Production leaves it disarmed.
    gate: Arc<PublishGate>,
}
/// Cache lock and private insertion, prepared before entering the lease lock.
pub(crate) struct ImageAdmission<'a> {
    images: MutexGuard<'a, VecDeque<(String, Image)>>,
    pending: Option<(String, Image)>,
    retired: Vec<(String, Image)>,
    bytes: usize,
}
impl ImageAdmission<'_> {
    /// Infallible memory registration. Retired pixels drop after the lease lock.
    pub(crate) fn commit(&mut self) {
        let Some((id, image)) = self.pending.take() else {
            return;
        };
        let replaced = self.images.iter().position(|(key, _)| *key == id);
        if let Some(old) = replaced.and_then(|index| self.images.remove(index)) {
            self.bytes -= old.1.bytes.len();
            self.retired.push(old);
        }
        self.bytes += image.bytes.len();
        self.images.push_back((id, image));
        while self.bytes > 32 * 1024 * 1024 || self.images.len() > 64 {
            if let Some(old) = self.images.pop_front() {
                self.bytes -= old.1.bytes.len();
                self.retired.push(old);
            } else {
                break;
            }
        }
    }
}
impl Default for ImageCache {
    fn default() -> Self {
        Self {
            images: Mutex::new(VecDeque::new()),
            gate: PublishGate::new(),
        }
    }
}
impl ImageCache {
    /// Acquire the cache lock and prepare an insertion without exposing pixels.
    ///
    /// Memory only. No await, timeout, or cancellation. Run before acquiring the
    /// lease lock so a concurrent reader never delays revocation. Dropping an
    /// uncommitted admission leaves the cache unchanged.
    pub(crate) fn admission(&self, id: String, image: Image) -> ImageAdmission<'_> {
        let mut images = self.images.lock().unwrap_or_else(|e| e.into_inner());
        self.gate.enter();
        let bytes = images.iter().map(|(_, image)| image.bytes.len()).sum();
        let retired = Vec::with_capacity(images.len() + 1);
        images.reserve(1);
        ImageAdmission {
            images,
            pending: Some((id, image)),
            retired,
            bytes,
        }
    }
    pub fn get(&self, id: &str) -> Option<Image> {
        self.images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, i)| i.clone())
    }
    pub fn clear(&self) {
        self.images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    /// Pause used by [`Self::admission`]. Memory only. Production leaves it disarmed.
    #[cfg(test)]
    pub(crate) fn admission_gate(&self) -> Arc<PublishGate> {
        Arc::clone(&self.gate)
    }
}
