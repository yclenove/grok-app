//! UI preview demand, independent of model observations.

use parking_lot::Mutex;

pub struct PreviewController {
    visible: Mutex<bool>,
}

impl PreviewController {
    pub fn new() -> Self {
        Self {
            visible: Mutex::new(false),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        *self.visible.lock() = visible;
    }

    pub fn is_visible(&self) -> bool {
        *self.visible.lock()
    }
}

impl Default for PreviewController {
    fn default() -> Self {
        Self::new()
    }
}
