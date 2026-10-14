//! Bounded session-private cache; only Results can insert redacted deliveries.
use gcu_host_protocol::messages::Image;
use std::{collections::VecDeque, sync::Mutex};
#[derive(Default)]
pub(crate) struct ImageCache(Mutex<VecDeque<(String, Image)>>);
impl ImageCache {
    pub fn insert(&self, id: String, image: Image) {
        let mut images = self.0.lock().unwrap_or_else(|e| e.into_inner());
        images.retain(|(key, _)| *key != id);
        images.push_back((id, image));
        while images.iter().map(|(_, i)| i.bytes.len()).sum::<usize>() > 32 * 1024 * 1024
            || images.len() > 64
        {
            images.pop_front();
        }
    }
    pub fn get(&self, id: &str) -> Option<Image> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, i)| i.clone())
    }
    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
