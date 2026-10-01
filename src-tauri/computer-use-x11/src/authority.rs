//! Small, independently locked model authority. No X11/AT-SPI calls under this
//! lock: Stop and a replacement capture can retire even a blocked native read.
use crate::DispatchRequest;
use grok_computer_use_core::protocol::ActionTarget;
use std::collections::VecDeque;

#[derive(Default)]
pub(crate) struct Authority {
    entries: VecDeque<Entry>,
}

struct Entry {
    run: String,
    target: String,
    snapshot: String,
    geometry: Option<u64>,
    screenshot: bool,
    generation: u64,
}

impl Authority {
    pub fn retire_all(&mut self) {
        self.entries.clear();
    }

    pub fn begin(&mut self, run: &str, target: &str, snapshot: &str) {
        self.retire(run, target);
        if self.entries.len() >= 8 {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            run: run.into(),
            target: target.into(),
            snapshot: snapshot.into(),
            geometry: None,
            screenshot: false,
            generation: 0,
        });
    }

    pub fn publish(
        &mut self,
        run: &str,
        target: &str,
        snapshot: &str,
        geometry: u64,
        screenshot: bool,
    ) -> Result<(), String> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.run == run && e.target == target && e.snapshot == snapshot)
            .ok_or("X11 capture was retired before publication")?;
        if entry.geometry.is_some() {
            return Err("X11 capture already published".into());
        }
        entry.geometry = Some(geometry);
        entry.screenshot = screenshot;
        Ok(())
    }

    pub fn check(&self, req: &DispatchRequest) -> Result<(), String> {
        let entry = self
            .entries
            .iter()
            .find(|e| {
                e.run == req.run_id
                    && e.target == req.target_id
                    && e.snapshot == req.snapshot_id
                    && e.geometry == Some(req.geometry_revision)
            })
            .ok_or("X11 model authority is absent or stale for this run")?;
        if req.generation < entry.generation {
            return Err("X11 action belongs to a retired generation".into());
        }
        if matches!(req.target, ActionTarget::Coord { .. }) && !entry.screenshot {
            return Err("X11 coordinate input requires this run's model screenshot".into());
        }
        Ok(())
    }

    pub fn admit(&mut self, req: &DispatchRequest) -> Result<(), String> {
        self.check(req)?;
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.run == req.run_id && e.target == req.target_id)
            .expect("checked entry");
        entry.generation = req.generation;
        Ok(())
    }

    pub fn retire(&mut self, run: &str, target: &str) {
        self.entries.retain(|e| e.run != run || e.target != target);
    }

    pub fn retire_before(&mut self, run: &str, generation: u64) {
        // Unknown/pending capture is retired too. A proven newer action must
        // not lose its snapshot because an older Stop callback arrives late.
        self.entries
            .retain(|e| e.run != run || e.generation >= generation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grok_computer_use_core::{adapter::ActionScope, protocol::ActionKind};
    fn request(run: &str, snapshot: &str) -> DispatchRequest {
        DispatchRequest {
            managed_request: None,
            cancellation: Default::default(),
            run_id: run.into(),
            action_id: "test".into(),
            generation: 1,
            target_id: "window".into(),
            target_generation: 1,
            snapshot_id: snapshot.into(),
            geometry_revision: 7,
            action: ActionKind::Click,
            target: ActionTarget::Coord { x: 1., y: 1. },
            parameters: serde_json::json!({}),
            scope: ActionScope::Directed,
        }
    }
    fn publish(a: &mut Authority, run: &str, snapshot: &str, pixels: bool) {
        a.begin(run, "window", snapshot);
        a.publish(run, "window", snapshot, 7, pixels).unwrap();
    }
    #[test]
    fn native_recovery_retires_all_precompletion_authority_and_pending_publication() {
        let mut a = Authority::default();
        publish(&mut a, "one", "old", true);
        publish(&mut a, "two", "other", true);
        a.begin("pending", "window", "pending-capture");
        a.retire_all();
        assert!(a.check(&request("one", "old")).is_err());
        assert!(a.check(&request("two", "other")).is_err());
        assert!(a
            .publish("pending", "window", "pending-capture", 7, true)
            .is_err());
        publish(&mut a, "one", "fresh", true);
        assert!(a.check(&request("one", "fresh")).is_ok());
    }
    #[test]
    fn foreign_run_cannot_reuse_even_exact_snapshot() {
        let mut a = Authority::default();
        publish(&mut a, "one", "snap", true);
        assert!(a.check(&request("two", "snap")).is_err());
        assert!(a.check(&request("one", "snap")).is_ok());
    }
    #[test]
    fn pending_failed_or_late_capture_never_revives_old_authority() {
        let mut a = Authority::default();
        publish(&mut a, "one", "old", true);
        a.begin("one", "window", "pending");
        assert!(a.check(&request("one", "old")).is_err());
        assert!(a.check(&request("one", "pending")).is_err());
        a.begin("one", "window", "new");
        assert!(a.publish("one", "window", "pending", 7, true).is_err());
        a.retire("one", "window");
        assert!(a.publish("one", "window", "new", 7, true).is_err());
    }
    #[test]
    fn no_screenshot_means_no_coordinate_authority() {
        let mut a = Authority::default();
        publish(&mut a, "one", "snap", false);
        let mut req = request("one", "snap");
        assert!(a.check(&req).is_err());
        req.target = ActionTarget::Element {
            element_ref: "node".into(),
        };
        assert!(a.check(&req).is_ok());
    }
    #[test]
    fn abort_retires_pending_and_old_but_not_proven_newer_generation() {
        let mut a = Authority::default();
        publish(&mut a, "one", "snap", true);
        let mut req = request("one", "snap");
        req.generation = 3;
        a.admit(&req).unwrap();
        a.retire_before("one", 2);
        assert!(a.check(&req).is_ok());
        req.generation = 1;
        assert!(a.check(&req).is_err());
        a.retire_before("one", 4);
        assert!(a.check(&req).is_err());
        a.begin("one", "window", "pending");
        a.retire_before("one", 4);
        assert!(a.publish("one", "window", "pending", 7, true).is_err());
    }
    #[test]
    fn replacement_is_run_scoped_and_storage_is_bounded() {
        let mut a = Authority::default();
        publish(&mut a, "one", "snap", true);
        publish(&mut a, "two", "other", true);
        a.retire("two", "window");
        assert!(a.check(&request("one", "snap")).is_ok());
        for i in 0..8 {
            publish(&mut a, &i.to_string(), "x", true);
        }
        assert_eq!(a.entries.len(), 8);
        assert!(a.check(&request("one", "snap")).is_err());
    }
}
