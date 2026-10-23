//! Bounded session-private cache; only Results can insert redacted deliveries.
use gcu_core::action::PublishGate;
use gcu_host_protocol::messages::Image;
use std::{collections::VecDeque, sync::Arc, sync::Mutex};
pub(crate) struct ImageCache {
    images: Mutex<VecDeque<(String, Image)>>,
    /// Parks after the cache mutex is taken and before the entry is inserted.
    /// Production leaves it disarmed.
    gate: Arc<PublishGate>,
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
    /// Insert only when `admit` is true while this cache mutex is held.
    ///
    /// `admit` runs after the mutex is acquired, so a revoke that arrived while
    /// the caller waited is visible. Memory only. No await, timeout, or
    /// cancellation. The scope lock is not held by this function; `admit` may
    /// take it briefly and must drop it before returning.
    pub fn insert_if(&self, id: String, image: Image, admit: impl FnOnce() -> bool) -> bool {
        let mut images = self.images.lock().unwrap_or_else(|e| e.into_inner());
        self.gate.enter();
        if !admit() {
            return false;
        }
        images.retain(|(key, _)| *key != id);
        images.push_back((id, image));
        while images.iter().map(|(_, i)| i.bytes.len()).sum::<usize>() > 32 * 1024 * 1024
            || images.len() > 64
        {
            images.pop_front();
        }
        true
    }
    /// Forget one id. Memory only. No timeout or cancellation.
    pub fn remove(&self, id: &str) {
        self.images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(key, _)| key != id);
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
    /// Pause used by [`Self::insert_if`]. Memory only. Production leaves it disarmed.
    #[cfg(test)]
    pub(crate) fn admission_gate(&self) -> Arc<PublishGate> {
        Arc::clone(&self.gate)
    }
}
