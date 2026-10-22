//! Opt-in, test-only publication timings. No file paths, payloads or credentials.
//! Completed aggregates and active spans are bounded; these are wall times, not
//! OS lock-wait measurements. Never used to change publication decisions.
//! Parent and nested stage durations overlap and must not be added together.
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const MAX_ACTIVE: usize = 64;

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Stage {
    CancelCheck,
    DestinationDirectory,
    RenameRevision,
    VerifyRevision,
    PublishedReceipt,
    FingerprintBefore,
    ProofLookup,
    FullVerification,
    FingerprintAfter,
    ProofPersist,
    ProofSync,
    ProofPrune,
}

impl Stage {
    fn profile_name(self) -> &'static str {
        match self {
            Self::CancelCheck => "archive-cancel-check",
            Self::DestinationDirectory => "archive-destination-directory",
            Self::RenameRevision => "archive-rename-revision",
            Self::VerifyRevision => "archive-verify-revision",
            Self::PublishedReceipt => "archive-published-receipt",
            Self::FingerprintBefore => "archive-fingerprint-before",
            Self::ProofLookup => "archive-proof-lookup",
            Self::FullVerification => "archive-full-verification",
            Self::FingerprintAfter => "archive-fingerprint-after",
            Self::ProofPersist => "archive-proof-persist",
            Self::ProofSync => "archive-proof-sync",
            Self::ProofPrune => "archive-proof-prune",
        }
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Total {
    count: u64,
    total_us: u128,
    max_us: u128,
}

struct Active {
    stage: Stage,
    started: Instant,
    thread: std::thread::ThreadId,
}

#[derive(Default)]
struct Records {
    next: u64,
    completed: BTreeMap<Stage, Total>,
    active: BTreeMap<u64, Active>,
    dropped: u64,
}

#[derive(Default)]
struct Recorder(Mutex<Records>);

impl Recorder {
    fn snapshot(&self) -> serde_json::Value {
        let rows = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let active: Vec<_> = rows.active.values().map(|value| {
            serde_json::json!({"stage":value.stage, "elapsedUs":value.started.elapsed().as_micros(),
                "thread":format!("{:?}", value.thread)})
        }).collect();
        serde_json::json!({"completed":rows.completed, "active":active, "droppedSpans":rows.dropped})
    }
}

static CAPTURE: Mutex<Option<Arc<Recorder>>> = Mutex::new(None);

/// Only the isolated opt-in workload installs a capture. Ordinary tests do not.
pub(super) struct Capture(Arc<Recorder>);

impl Capture {
    pub fn start() -> Self {
        let mut slot = CAPTURE.lock().unwrap_or_else(|error| error.into_inner());
        assert!(slot.is_none(), "publication timing capture already active");
        let recorder = Arc::new(Recorder::default());
        *slot = Some(recorder.clone());
        Self(recorder)
    }

    pub fn snapshot(&self) -> serde_json::Value {
        self.0.snapshot()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut slot = CAPTURE.lock().unwrap_or_else(|error| error.into_inner());
        if slot
            .as_ref()
            .is_some_and(|recorder| Arc::ptr_eq(recorder, &self.0))
        {
            *slot = None;
        }
    }
}

pub(super) struct Span {
    recording: Option<(Arc<Recorder>, u64)>,
    // Also follow the current invocation when the legacy global capture is off.
    _profile: crate::session_migration::codec_profile::Span,
}

impl Span {
    fn new(recorder: Arc<Recorder>, stage: Stage) -> Self {
        let profile = crate::session_migration::codec_profile::Span::start(stage.profile_name(), 0);
        let id = {
            let mut rows = recorder.0.lock().unwrap_or_else(|error| error.into_inner());
            if rows.active.len() >= MAX_ACTIVE {
                rows.dropped += 1;
                return Self {
                    recording: None,
                    _profile: profile,
                };
            }
            rows.next += 1;
            let id = rows.next;
            rows.active.insert(
                id,
                Active {
                    stage,
                    started: Instant::now(),
                    thread: std::thread::current().id(),
                },
            );
            id
        };
        Self {
            recording: Some((recorder, id)),
            _profile: profile,
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some((recorder, id)) = &self.recording else {
            return;
        };
        let mut rows = recorder.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(active) = rows.active.remove(id) {
            let elapsed = active.started.elapsed().as_micros();
            let total = rows.completed.entry(active.stage).or_default();
            total.count += 1;
            total.total_us += elapsed;
            total.max_us = total.max_us.max(elapsed);
        }
    }
}

pub(super) fn span(stage: Stage) -> Span {
    let recorder = CAPTURE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    match recorder {
        Some(recorder) => Span::new(recorder, stage),
        None => Span {
            recording: None,
            _profile: crate::session_migration::codec_profile::Span::start(stage.profile_name(), 0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_active_then_completed_without_payloads() {
        let recorder = Arc::new(Recorder::default());
        let active = Span::new(recorder.clone(), Stage::RenameRevision);
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot["active"][0]["stage"], "renameRevision");
        assert!(snapshot["active"][0]["elapsedUs"].is_u64());
        assert!(snapshot["completed"].as_object().unwrap().is_empty());
        drop(active);
        let snapshot = recorder.snapshot();
        assert!(snapshot["active"].as_array().unwrap().is_empty());
        assert_eq!(snapshot["completed"]["renameRevision"]["count"], 1);
        assert_eq!(snapshot["droppedSpans"], 0);
    }

    #[test]
    fn nested_stages_keep_independent_ownership() {
        let recorder = Arc::new(Recorder::default());
        let outer = Span::new(recorder.clone(), Stage::VerifyRevision);
        let inner = Span::new(recorder.clone(), Stage::FingerprintBefore);
        assert_eq!(recorder.snapshot()["active"].as_array().unwrap().len(), 2);
        drop(outer);
        assert_eq!(
            recorder.snapshot()["active"][0]["stage"],
            "fingerprintBefore"
        );
        drop(inner);
        assert_eq!(
            recorder.snapshot()["completed"]["verifyRevision"]["count"],
            1
        );
    }

    #[test]
    fn unwinding_clears_active_span_and_records_elapsed_time() {
        let recorder = Arc::new(Recorder::default());
        let result = std::panic::catch_unwind({
            let recorder = recorder.clone();
            move || {
                let _span = Span::new(recorder, Stage::ProofPersist);
                panic!("synthetic unwind");
            }
        });
        assert!(result.is_err());
        let snapshot = recorder.snapshot();
        assert!(snapshot["active"].as_array().unwrap().is_empty());
        assert_eq!(snapshot["completed"]["proofPersist"]["count"], 1);
    }

    #[test]
    fn bounds_active_memory_and_reports_dropped_observations() {
        let recorder = Arc::new(Recorder::default());
        let spans: Vec<_> = (0..MAX_ACTIVE + 3)
            .map(|_| Span::new(recorder.clone(), Stage::ProofLookup))
            .collect();
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), MAX_ACTIVE);
        assert_eq!(snapshot["droppedSpans"], 3);
        drop(spans);
        assert_eq!(
            recorder.snapshot()["completed"]["proofLookup"]["count"],
            MAX_ACTIVE
        );
    }

    #[test]
    fn worker_threads_share_bounded_aggregates_without_losing_counts() {
        let recorder = Arc::new(Recorder::default());
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let recorder = recorder.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        let _span = Span::new(recorder.clone(), Stage::PublishedReceipt);
                    }
                });
            }
        });
        let snapshot = recorder.snapshot();
        assert!(snapshot["active"].as_array().unwrap().is_empty());
        assert_eq!(snapshot["completed"]["publishedReceipt"]["count"], 400);
        assert_eq!(snapshot["droppedSpans"], 0);
    }
}
