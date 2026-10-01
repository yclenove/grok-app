//! Native WebView identities. A label is a routing address, never an owner.
//!
//! Navigation fences document-scoped Computer Use immediately. These identities
//! do not authorize a new document or prove an outstanding script has completed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentIdentity {
    pub instance: u64,
    pub generation: u64,
}

#[derive(Debug)]
struct ViewState {
    document: DocumentIdentity,
    live: bool,
    #[cfg(windows)]
    world: Arc<super::isolated_windows::NativeWorld>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeView(Arc<Mutex<ViewState>>);

impl NativeView {
    #[cfg(windows)]
    pub(crate) fn process_failed(&self, failure: super::isolated_windows::process::Failure) {
        match failure {
            super::isolated_windows::process::Failure::MainRendererExited => {
                self.navigation_started()
            }
            super::isolated_windows::process::Failure::BrowserExited
            | super::isolated_windows::process::Failure::NativeViewClosed => self.retire(),
        }
    }

    #[cfg(windows)]
    pub fn script_world(&self) -> Option<Arc<super::isolated_windows::NativeWorld>> {
        self.0.lock().ok().map(|state| state.world.clone())
    }

    pub fn document(&self) -> Option<DocumentIdentity> {
        let state = self.0.lock().ok()?;
        state.live.then_some(state.document)
    }

    pub fn matches(&self, expected: DocumentIdentity) -> bool {
        self.document() == Some(expected)
    }

    /// Called before a native navigation is allowed/dispatched. A duplicate
    /// callback may retire more than one epoch; it can never restore an old one.
    pub fn navigation_started(&self) {
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        if !state.live {
            return;
        }
        match state.document.generation.checked_add(1) {
            Some(next) => state.document.generation = next,
            None => state.live = false,
        }
    }

    fn retire(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.live = false;
        }
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    views: Mutex<HashMap<String, NativeView>>,
    sequence: AtomicU64,
}

impl Registry {
    pub fn reserve(&self) -> Result<NativeView, String> {
        let instance = self
            .sequence
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_err(|_| "WebView native identity exhausted")?
            + 1;
        Ok(NativeView(Arc::new(Mutex::new(ViewState {
            document: DocumentIdentity {
                instance,
                generation: 1,
            },
            live: true,
            #[cfg(windows)]
            world: Arc::default(),
        }))))
    }

    /// Publish only after the native child was successfully created. A failed
    /// concurrent create must not retire another instance with the same label.
    pub fn publish(&self, label: &str, view: &NativeView) -> Result<(), String> {
        let mut views = self
            .views
            .lock()
            .map_err(|_| "WebView registry unavailable")?;
        let document = view.document().ok_or("WebView native identity retired")?;
        if views.get(label).is_some_and(|current| {
            current
                .document()
                .is_some_and(|current| current.instance > document.instance)
        }) {
            view.retire();
            return Err("WebView native instance already replaced".into());
        }
        if let Some(previous) = views.insert(label.into(), view.clone()) {
            if !Arc::ptr_eq(&previous.0, &view.0) {
                previous.retire();
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn created(&self, label: &str) -> Result<NativeView, String> {
        let view = self.reserve()?;
        self.publish(label, &view)?;
        Ok(view)
    }

    pub fn current(&self, label: &str) -> Option<NativeView> {
        let view = self.views.lock().ok()?.get(label)?.clone();
        view.document()?;
        Some(view)
    }

    /// Exact-instance cleanup: a late old close/create failure must never
    /// remove a new native WebView reusing the same Tauri label.
    pub fn closed(&self, label: &str, expected: &NativeView) {
        expected.retire();
        #[cfg(windows)]
        if let Some(world) = expected.script_world() {
            super::isolated_windows::process::closing(&world);
        }
        let Ok(mut views) = self.views.lock() else {
            return;
        };
        if views
            .get(label)
            .is_some_and(|current| Arc::ptr_eq(&current.0, &expected.0))
        {
            views.remove(label);
        }
    }
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::default);

pub(crate) fn creating() -> Result<NativeView, String> {
    REGISTRY.reserve()
}

pub(crate) fn created(label: &str, view: &NativeView) -> Result<(), String> {
    REGISTRY.publish(label, view)
}

pub(crate) fn current(label: &str) -> Option<NativeView> {
    REGISTRY.current(label)
}

pub(crate) fn closed(label: &str, expected: &NativeView) {
    REGISTRY.closed(label, expected);
}

pub(crate) fn navigation_started(label: &str) {
    if let Some(view) = current(label) {
        view.navigation_started();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn navigation_synchronously_retires_the_captured_document() {
        let registry = Registry::default();
        let view = registry.created("resource-browser-a").unwrap();
        let before = view.document().unwrap();
        view.navigation_started();
        assert!(!view.matches(before));
        let after = view.document().unwrap();
        assert_eq!(after.instance, before.instance);
        assert!(after.generation > before.generation);
    }

    #[test]
    fn same_label_replacement_has_a_distinct_native_identity() {
        let registry = Registry::default();
        let old = registry.created("resource-browser-a").unwrap();
        let old_document = old.document().unwrap();
        let replacement = registry.created("resource-browser-a").unwrap();
        assert!(!old.matches(old_document));
        assert_ne!(
            replacement.document().unwrap().instance,
            old_document.instance
        );
    }

    #[test]
    fn old_navigation_callback_cannot_fence_the_replacement() {
        let registry = Registry::default();
        let old = registry.created("resource-browser-a").unwrap();
        let replacement = registry.created("resource-browser-a").unwrap();
        let expected = replacement.document().unwrap();
        old.navigation_started();
        assert!(replacement.matches(expected));
    }

    #[test]
    fn old_close_callback_cannot_remove_the_replacement() {
        let registry = Registry::default();
        let old = registry.created("resource-browser-a").unwrap();
        let replacement = registry.created("resource-browser-a").unwrap();
        let expected = replacement.document().unwrap();
        registry.closed("resource-browser-a", &old);
        assert!(registry
            .current("resource-browser-a")
            .unwrap()
            .matches(expected));
    }

    #[test]
    fn failed_concurrent_create_leaves_the_published_instance_untouched() {
        let registry = Registry::default();
        let published = registry.created("resource-browser-a").unwrap();
        let expected = published.document().unwrap();
        let failed = registry.reserve().unwrap();
        registry.closed("resource-browser-a", &failed);
        assert!(registry
            .current("resource-browser-a")
            .unwrap()
            .matches(expected));
    }

    #[test]
    fn a_late_old_creation_cannot_replace_a_newer_native_instance() {
        let registry = Registry::default();
        let old = registry.reserve().unwrap();
        let replacement = registry.created("resource-browser-a").unwrap();
        let expected = replacement.document().unwrap();
        assert!(registry.publish("resource-browser-a", &old).is_err());
        assert!(old.document().is_none());
        assert!(registry
            .current("resource-browser-a")
            .unwrap()
            .matches(expected));
    }

    #[test]
    fn failed_create_or_close_fences_without_reviving_the_view() {
        let registry = Registry::default();
        let view = registry.created("resource-browser-a").unwrap();
        let before = view.document().unwrap();
        registry.closed("resource-browser-a", &view);
        view.navigation_started();
        assert!(!view.matches(before));
        assert!(registry.current("resource-browser-a").is_none());
    }

    #[test]
    fn racing_old_callbacks_leave_new_instance_untouched() {
        let registry = Arc::new(Registry::default());
        let old = registry.created("resource-browser-a").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let cleanup = {
            let registry = registry.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                old.navigation_started();
                registry.closed("resource-browser-a", &old);
            })
        };
        let replacement = registry.created("resource-browser-a").unwrap();
        let expected = replacement.document().unwrap();
        barrier.wait();
        cleanup.join().unwrap();
        assert!(registry
            .current("resource-browser-a")
            .unwrap()
            .matches(expected));
    }

    #[test]
    fn a_different_label_keeps_its_document() {
        let registry = Registry::default();
        let a = registry.created("resource-browser-a").unwrap();
        let b = registry.created("resource-browser-b").unwrap();
        let expected = b.document().unwrap();
        a.navigation_started();
        registry.closed("resource-browser-a", &a);
        assert!(b.matches(expected));
    }

    #[test]
    fn document_generation_overflow_fails_closed() {
        let view = NativeView(Arc::new(Mutex::new(ViewState {
            document: DocumentIdentity {
                instance: 1,
                generation: u64::MAX,
            },
            live: true,
            #[cfg(windows)]
            world: Arc::default(),
        })));
        view.navigation_started();
        assert!(view.document().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn renderer_failure_retires_only_its_captured_native_document() {
        use super::super::isolated_windows::process::Failure;
        let registry = Registry::default();
        let old = registry.created("resource-browser-a").unwrap();
        let original = old.document().unwrap();
        let other = registry.created("resource-browser-b").unwrap();
        let other_document = other.document().unwrap();
        old.process_failed(Failure::MainRendererExited);
        assert!(!old.matches(original));
        assert!(other.matches(other_document));
        let replacement = registry.created("resource-browser-a").unwrap();
        let replacement_document = replacement.document().unwrap();
        old.process_failed(Failure::MainRendererExited);
        old.process_failed(Failure::BrowserExited);
        assert!(replacement.matches(replacement_document));
        assert!(registry
            .current("resource-browser-a")
            .unwrap()
            .matches(replacement_document));
    }

    #[cfg(windows)]
    #[test]
    fn browser_failure_cannot_be_revived_by_a_navigation_callback() {
        use super::super::isolated_windows::process::Failure;
        let registry = Registry::default();
        let view = registry.created("resource-browser-a").unwrap();
        view.process_failed(Failure::BrowserExited);
        view.navigation_started();
        assert!(view.document().is_none());
        assert!(registry.current("resource-browser-a").is_none());
    }
}
