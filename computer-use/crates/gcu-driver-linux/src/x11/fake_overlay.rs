//! Deterministic screenshot barrier used by FakeX and failure-path tests.
use super::{overlay::CaptureScope, XError};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
/// Fake overlay visibility and exact-target exclusion (no native windows).
#[derive(Clone, Default)]
pub struct FakeOverlayCapture(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    windows: HashSet<u32>,
    active: usize,
    transitions: Vec<bool>,
}
impl FakeOverlayCapture {
    /// Register an overlay window ID that may never be captured as a target.
    pub fn register(&self, window: u32) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .windows
            .insert(window);
    }
    /// Recorded hidden/visible transitions, including cleanup after errors.
    pub fn transitions(&self) -> Vec<bool> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .transitions
            .clone()
    }
    /// Whether at least one capture still requires the overlay hidden.
    pub fn is_hidden(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).active != 0
    }
    pub(super) fn begin(&self, target: u32) -> Result<Box<dyn CaptureScope>, XError> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.windows.contains(&target) {
            return Err(XError::Protocol("overlay is not a capture target".into()));
        }
        if state.active == 0 {
            state.transitions.push(true);
        }
        state.active += 1;
        let root_safe = state.windows.is_empty();
        Ok(Box::new(Lease(self.clone(), root_safe)))
    }
}
struct Lease(FakeOverlayCapture, bool);
impl CaptureScope for Lease {
    fn allows_root_capture(&self) -> bool {
        self.1
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut state = self.0 .0.lock().unwrap_or_else(|p| p.into_inner());
        state.active -= 1;
        if state.active == 0 {
            state.transitions.push(false);
        }
    }
}
