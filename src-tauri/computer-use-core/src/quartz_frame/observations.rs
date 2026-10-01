//! Atomic model observation authority shared by image and semantic actions.
use super::{CapturedFrame, WindowInstance};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// One publication owns both screenshot authority and its semantic payload.
/// Native acquisition/validation never runs with the registry lock held.
pub struct ObservationRegistry<T> {
    entries: Mutex<VecDeque<ObservationEntry<T>>>,
}

impl<T> Default for ObservationRegistry<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
        }
    }
}

struct ObservationEntry<T> {
    key: (String, String),
    ticket: uuid::Uuid,
    observation: Option<Arc<ModelObservation<T>>>,
}

pub struct ModelObservation<T> {
    frame: CapturedFrame,
    screenshot: bool,
    payload: Mutex<T>,
}

impl<T> ModelObservation<T> {
    pub fn frame(&self) -> &CapturedFrame {
        &self.frame
    }
    pub fn has_screenshot(&self) -> bool {
        self.screenshot
    }
    pub fn payload(&self) -> &Mutex<T> {
        &self.payload
    }
}

pub struct CaptureTicket {
    key: (String, String),
    ticket: uuid::Uuid,
}

impl<T> ObservationRegistry<T> {
    /// A model capture retires the whole observation before native work starts. A
    /// preview never calls begin/publish and cannot grant or replace authority.
    pub fn begin(&self, run: &str, target: &str) -> CaptureTicket {
        let ticket = CaptureTicket {
            key: (run.into(), target.into()),
            ticket: uuid::Uuid::new_v4(),
        };
        let retired = {
            let mut entries = self.entries.lock();
            let old = entries
                .iter()
                .position(|e| e.key == ticket.key)
                .and_then(|index| entries.remove(index));
            let evicted = if entries.len() >= 8 {
                entries.pop_front()
            } else {
                None
            };
            entries.push_back(ObservationEntry {
                key: ticket.key.clone(),
                ticket: ticket.ticket,
                observation: None,
            });
            (old, evicted)
        };
        // A native payload destructor must not run under the registry mutex.
        drop(retired);
        ticket
    }

    pub fn publish(
        &self,
        ticket: CaptureTicket,
        frame: CapturedFrame,
        screenshot: bool,
        payload: T,
    ) -> Result<(), String> {
        // A process/window key is metadata, not Host-issued instance authority.
        let window = WindowInstance::parse(&ticket.key.1)?.window;
        if window != frame.target {
            return Err("macOS capture target mismatch".into());
        }
        let candidate = Arc::new(ModelObservation {
            frame,
            screenshot,
            payload: Mutex::new(payload),
        });
        let mut entries = self.entries.lock();
        let entry = entries
            .iter_mut()
            .find(|e| e.key == ticket.key)
            .filter(|e| e.ticket == ticket.ticket)
            .ok_or("macOS capture was retired before completion")?;
        entry.observation = Some(candidate);
        Ok(())
    }

    pub fn get(&self, run: &str, target: &str) -> Result<Arc<ModelObservation<T>>, String> {
        self.entries
            .lock()
            .iter()
            .find(|e| e.key.0 == run && e.key.1 == target)
            .and_then(|e| e.observation.clone())
            .ok_or_else(|| "a fresh model observation is required for macOS input".into())
    }

    /// A loaded Arc preserves memory, not authorization. Revalidate immediately
    /// before new side effects; an owned button release has separate rules.
    pub fn require_current(
        &self,
        run: &str,
        target: &str,
        observation: &Arc<ModelObservation<T>>,
    ) -> Result<(), String> {
        if self.entries.lock().iter().any(|e| {
            e.key.0 == run
                && e.key.1 == target
                && e.observation
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, observation))
        }) {
            Ok(())
        } else {
            Err("macOS model observation was retired during validation".into())
        }
    }

    pub fn retire_run(&self, run: &str) {
        self.retire_where(|key| key.0 == run);
    }

    pub fn retire_target(&self, run: &str, target: &str) {
        self.retire_where(|key| key.0 == run && key.1 == target);
    }

    pub fn retire_window(&self, target: &str) {
        self.retire_where(|key| key.1 == target);
    }

    fn retire_where(&self, matches: impl Fn(&(String, String)) -> bool) {
        let retired = {
            let mut entries = self.entries.lock();
            let mut retired = Vec::new();
            let mut index = 0;
            while index < entries.len() {
                if matches(&entries[index].key) {
                    retired.push(entries.remove(index));
                } else {
                    index += 1;
                }
            }
            retired
        };
        drop(retired);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quartz_frame::{WindowBounds, WindowKey};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    fn frame(name: &str) -> CapturedFrame {
        CapturedFrame::new(
            WindowKey::new(7, 11, 1_700_000_000, 123_456).unwrap(),
            WindowBounds::new(-1000.0, 80.0, 600.0, 400.0).unwrap(),
            9,
            name.into(),
            1200,
            800,
        )
        .unwrap()
    }
    fn id() -> String {
        WindowInstance::fresh(frame("s").target).target_id()
    }

    #[test]
    fn image_mode_metadata_and_semantic_payload_publish_as_one_observation() {
        let registry = ObservationRegistry::default();
        let id = id();
        for screenshot in [true, false] {
            let ticket = registry.begin("run", &id);
            assert!(registry.get("run", &id).is_err());
            registry
                .publish(ticket, frame("paired"), screenshot, "paired")
                .unwrap();
            let image_reader = registry.get("run", &id).unwrap();
            let semantic_reader = registry.get("run", &id).unwrap();
            assert!(Arc::ptr_eq(&image_reader, &semantic_reader));
            assert_eq!(
                image_reader.frame().snapshot_id,
                *semantic_reader.payload().lock()
            );
            assert_eq!(image_reader.has_screenshot(), screenshot);
            registry
                .require_current("run", &id, &semantic_reader)
                .unwrap();
        }
    }

    #[test]
    fn beginning_a_replacement_revokes_loaded_pixels_and_nodes_before_native_work() {
        let registry = ObservationRegistry::default();
        let id = id();
        let first = registry.begin("run", &id);
        registry.publish(first, frame("old"), true, 1).unwrap();
        let loaded = registry.get("run", &id).unwrap();
        let next = registry.begin("run", &id);
        assert!(registry.get("run", &id).is_err());
        assert!(registry.require_current("run", &id, &loaded).is_err());
        // Retained old memory is available for cleanup but never restores authority.
        assert_eq!(loaded.frame().snapshot_id, "old");
        assert_eq!(*loaded.payload().lock(), 1);
        registry.publish(next, frame("new"), false, 2).unwrap();
        let current = registry.get("run", &id).unwrap();
        assert!(!current.has_screenshot());
        assert_eq!(current.frame().snapshot_id, "new");
        assert_eq!(*current.payload().lock(), 2);
        assert!(registry.require_current("run", &id, &loaded).is_err());
    }

    #[test]
    fn delayed_producer_cannot_mix_old_payload_with_new_pixels_or_image_mode() {
        let registry = Arc::new(ObservationRegistry::default());
        let id = id();
        let worker_registry = registry.clone();
        let worker_id = id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let old = worker_registry.begin("run", &worker_id);
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            worker_registry.publish(old, frame("old-image"), true, "old-nodes")
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let current = registry.begin("run", &id);
        registry
            .publish(current, frame("new-image"), false, "new-nodes")
            .unwrap();
        finish_tx.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        let observed = registry.get("run", &id).unwrap();
        assert_eq!(observed.frame().snapshot_id, "new-image");
        assert_eq!(*observed.payload().lock(), "new-nodes");
        assert!(!observed.has_screenshot());
    }

    #[test]
    fn stop_during_native_read_rejects_the_entire_late_publication() {
        let registry = Arc::new(ObservationRegistry::default());
        let id = id();
        let worker_registry = registry.clone();
        let worker_id = id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let ticket = worker_registry.begin("run", &worker_id);
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            worker_registry.publish(ticket, frame("late"), true, "late-nodes")
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        registry.retire_run("run");
        finish_tx.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(registry.get("run", &id).is_err());
    }

    #[test]
    fn run_target_registry_and_window_retirement_boundaries_are_exact() {
        let registry = ObservationRegistry::default();
        let other_registry = ObservationRegistry::default();
        let id = id();
        let second = WindowInstance::fresh(frame("s").target).target_id();
        for (run, target) in [("one", &id), ("two", &id), ("one", &second)] {
            let ticket = registry.begin(run, target);
            registry.publish(ticket, frame("same"), true, ()).unwrap();
        }
        let one = registry.get("one", &id).unwrap();
        let ticket = other_registry.begin("one", &id);
        other_registry
            .publish(ticket, frame("same"), true, ())
            .unwrap();
        assert!(other_registry.require_current("one", &id, &one).is_err());
        assert!(registry.require_current("two", &id, &one).is_err());
        assert!(registry.require_current("one", &second, &one).is_err());
        registry.retire_target("one", &id);
        assert!(registry.get("one", &id).is_err());
        assert!(registry.get("two", &id).is_ok());
        registry.retire_window(&id);
        assert!(registry.get("two", &id).is_err());
        assert!(registry.get("one", &second).is_ok());
    }

    #[test]
    fn revoked_loaded_payload_does_not_hold_up_stop_or_restore_authority() {
        let registry = Arc::new(ObservationRegistry::default());
        let id = id();
        let ticket = registry.begin("run", &id);
        registry.publish(ticket, frame("model"), true, ()).unwrap();
        let loaded = registry.get("run", &id).unwrap();
        let payload_guard = loaded.payload().lock();
        let stopping = registry.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            stopping.retire_run("run");
            done_tx.send(()).unwrap();
        });
        let stopped = done_rx.recv_timeout(Duration::from_secs(3));
        drop(payload_guard); // Also unblocks a faulty implementation before asserting.
        worker.join().unwrap();
        assert!(stopped.is_ok());
        assert!(registry.require_current("run", &id, &loaded).is_err());
    }

    struct DropProbe {
        on_drop: Option<Box<dyn FnOnce() + Send>>,
    }
    impl Drop for DropProbe {
        fn drop(&mut self) {
            if let Some(callback) = self.on_drop.take() {
                callback();
            }
        }
    }

    #[test]
    fn retired_and_rejected_payloads_drop_outside_registry_mutex() {
        for case in 0..6 {
            let registry = Arc::new(ObservationRegistry::default());
            let free = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicUsize::new(0));
            let weak = Arc::downgrade(&registry);
            let free_result = free.clone();
            let drop_count = dropped.clone();
            let payload = DropProbe {
                on_drop: Some(Box::new(move || {
                    free_result.store(
                        weak.upgrade().unwrap().entries.try_lock().is_some(),
                        Ordering::SeqCst,
                    );
                    drop_count.fetch_add(1, Ordering::SeqCst);
                })),
            };
            let id = id();
            let ticket = registry.begin("run", &id);
            if case == 5 {
                registry.retire_run("run");
                assert!(registry
                    .publish(ticket, frame("late"), true, payload)
                    .is_err());
            } else {
                registry
                    .publish(ticket, frame("old"), true, payload)
                    .unwrap();
                match case {
                    0 => {
                        registry.begin("run", &id);
                    }
                    1 => {
                        for n in 0..8 {
                            registry.begin(&format!("new-{n}"), &id);
                        }
                    }
                    2 => registry.retire_run("run"),
                    3 => registry.retire_target("run", &id),
                    _ => registry.retire_window(&id),
                }
            }
            assert_eq!(dropped.load(Ordering::SeqCst), 1, "case {case}");
            assert!(free.load(Ordering::SeqCst), "case {case}");
        }
    }

    #[test]
    fn active_reader_keeps_native_payload_memory_alive_only_until_cleanup_finishes() {
        let registry = ObservationRegistry::default();
        let id = id();
        let drops = Arc::new(AtomicUsize::new(0));
        let count = drops.clone();
        let payload = DropProbe {
            on_drop: Some(Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })),
        };
        let ticket = registry.begin("run", &id);
        registry
            .publish(ticket, frame("active"), true, payload)
            .unwrap();
        let loaded = registry.get("run", &id).unwrap();
        registry.retire_run("run");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(registry.require_current("run", &id, &loaded).is_err());
        drop(loaded);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pending_capacity_eviction_cannot_be_undone_by_a_late_producer() {
        let registry = ObservationRegistry::default();
        let id = id();
        let stale = registry.begin("run", &id);
        for n in 0..8 {
            let ticket = registry.begin(&format!("new-{n}"), &id);
            registry
                .publish(ticket, frame(&format!("snapshot-{n}")), n % 2 == 0, n)
                .unwrap();
        }
        assert!(registry.publish(stale, frame("stale"), true, 99).is_err());
        assert!(registry.get("run", &id).is_err());
        assert_eq!(registry.entries.lock().len(), 8);
        for n in 0..8 {
            let ready = registry.get(&format!("new-{n}"), &id).unwrap();
            assert_eq!(ready.frame().snapshot_id, format!("snapshot-{n}"));
            assert_eq!(*ready.payload().lock(), n);
            assert_eq!(ready.has_screenshot(), n % 2 == 0);
        }
    }
}
