//! The test executable needs a subscriber too; worker stderr is inherited.
#![cfg_attr(not(windows), allow(dead_code))]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Once, OnceLock},
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

type Stages = Arc<Mutex<BTreeMap<String, Option<u64>>>>;
fn stages() -> &'static Stages {
    static STAGES: OnceLock<Stages> = OnceLock::new();
    STAGES.get_or_init(Stages::default)
}
pub fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = tracing::subscriber::set_global_default(Recorder(stages().clone()));
    });
}
pub fn snapshot() -> String {
    format!("{:?}", stages().lock().unwrap())
}
struct Recorder(Stages);
#[derive(Default)]
struct Fields {
    stage: Option<String>,
    phase: Option<String>,
    elapsed_ms: Option<u64>,
}
impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "stage" => self.stage = Some(value.into()),
            "phase" => self.phase = Some(value.into()),
            _ => (),
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "elapsed_ms" {
            self.elapsed_ms = Some(value);
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
impl Subscriber for Recorder {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "gcu_windows_timing"
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if let (Some(stage), Some(phase)) = (fields.stage, fields.phase) {
            super::harness::progress(format!(
                "NATIVE TIMING: stage={stage}, phase={phase}, elapsed_ms={:?}",
                fields.elapsed_ms
            ));
            self.0.lock().unwrap().insert(stage, fields.elapsed_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorder_retains_pending_and_completed_stage_without_payload_fields() {
        let stages = Stages::default();
        tracing::subscriber::with_default(Recorder(stages.clone()), || {
            tracing::info!(target: "gcu_windows_timing", stage = "worker_spawn", phase = "start");
            assert_eq!(stages.lock().unwrap().get("worker_spawn"), Some(&None));
            tracing::info!(target: "gcu_windows_timing", stage = "worker_spawn", phase = "end", elapsed_ms = 123u64, payload = "must not record");
        });
        let stages = stages.lock().unwrap();
        assert_eq!(stages.get("worker_spawn"), Some(&Some(123)));
        assert_eq!(stages.len(), 1);
    }
}
