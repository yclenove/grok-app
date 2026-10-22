//! Opt-in invocation-local test timing, never a product flag or SQL witness.
//! Nested inclusive durations must not be summed as exclusive CPU attribution.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

#[derive(Default)]
struct Totals {
    calls: u64,
    nanos: u64,
    bytes: u64,
}

type Shared = Arc<Mutex<BTreeMap<(&'static str, &'static str), Totals>>>;

#[derive(Clone)]
struct Active {
    shared: Shared,
    lane: &'static str,
}

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

#[derive(Default)]
pub(super) struct Session {
    shared: Shared,
}

pub(super) struct Scope {
    previous: Option<Active>,
    _thread: PhantomData<Rc<()>>,
}

impl Session {
    pub(super) fn enter(&self, lane: &'static str) -> Scope {
        let previous = ACTIVE.with(|active| {
            active.replace(Some(Active {
                shared: self.shared.clone(),
                lane,
            }))
        });
        Scope {
            previous,
            _thread: PhantomData,
        }
    }

    pub(super) fn report(&self, label: &str) {
        println!(
            "WINDOW4_CODEC_PROFILE {}",
            serde_json::json!({"label": label, "durationsAreInclusive": true, "rows": self.rows()})
        );
    }

    /// Calls recorded so far for one lane/stage (complexity assertions).
    pub(super) fn calls(&self, lane: &'static str, stage: &'static str) -> u64 {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(lane, stage))
            .map_or(0, |totals| totals.calls)
    }

    pub(super) fn bytes(&self, lane: &'static str, stage: &'static str) -> u64 {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(lane, stage))
            .map_or(0, |totals| totals.bytes)
    }

    fn rows(&self) -> Vec<Row> {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(|(&(lane, stage), totals)| Row {
                lane,
                stage,
                calls: totals.calls,
                total_nanos: totals.nanos,
                total_bytes: totals.bytes,
            })
            .collect()
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.previous.take());
        });
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    lane: &'static str,
    stage: &'static str,
    calls: u64,
    total_nanos: u64,
    total_bytes: u64,
}

pub(super) struct Span(Option<(Active, Instant, &'static str, usize)>);

impl Span {
    pub(super) fn start(stage: &'static str, bytes: usize) -> Self {
        Self(ACTIVE.with(|active| {
            active
                .borrow()
                .clone()
                .map(|active| (active, Instant::now(), stage, bytes))
        }))
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((active, started, stage, bytes)) = self.0.take() {
            let nanos = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let mut shared = active
                .shared
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let totals = shared.entry((active.lane, stage)).or_default();
            totals.calls = totals.calls.saturating_add(1);
            totals.nanos = totals.nanos.saturating_add(nanos);
            totals.bytes = totals.bytes.saturating_add(bytes as u64);
        }
    }
}

#[test]
fn inactive_spans_do_not_enable_measurement() {
    let session = Session::default();
    drop(Span::start("synthetic", 7));
    assert!(session.rows().is_empty());
}

#[test]
fn publication_timing_follows_the_current_invocation_and_stops_at_scope_exit() {
    use super::publish_timing::{span, Stage};
    let session = Session::default();
    let scope = session.enter("worker");
    drop(span(Stage::RenameRevision));
    drop(scope);
    drop(span(Stage::RenameRevision));
    let rows = session.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lane, "worker");
    assert_eq!(rows[0].stage, "archive-rename-revision");
    assert_eq!(rows[0].calls, 1);
}

#[test]
fn nested_sessions_and_worker_lanes_are_isolated_and_restore_scope() {
    let outer = Session::default();
    let inner = Session::default();
    let scope = outer.enter("caller");
    drop(Span::start("synthetic", 2));
    {
        let _inner = inner.enter("nested");
        drop(Span::start("synthetic", 3));
    }
    std::thread::scope(|threads| {
        threads.spawn(|| {
            drop(Span::start("not-inherited", 99));
            let _worker = outer.enter("worker");
            drop(Span::start("synthetic", 5));
        });
    });
    drop(Span::start("synthetic", 7));
    drop(scope);
    drop(Span::start("after-scope", 99));
    let rows = outer.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].lane, rows[0].calls, rows[0].total_bytes),
        ("caller", 2, 9)
    );
    assert_eq!(
        (rows[1].lane, rows[1].calls, rows[1].total_bytes),
        ("worker", 1, 5)
    );
    let rows = inner.rows();
    assert_eq!(
        (rows[0].lane, rows[0].calls, rows[0].total_bytes),
        ("nested", 1, 3)
    );
}

#[test]
fn scope_and_failed_operation_span_unwind_without_leaking_activation() {
    let outer = Session::default();
    let inner = Session::default();
    let _outer = outer.enter("outer");
    let failure = std::panic::catch_unwind(|| {
        let _inner = inner.enter("inner");
        let _span = Span::start("failure", 11);
        panic!("synthetic profiler scope unwind");
    });
    assert!(failure.is_err());
    drop(Span::start("restored", 13));
    assert_eq!(inner.rows()[0].calls, 1);
    assert_eq!(inner.rows()[0].total_bytes, 11);
    assert_eq!(outer.rows()[0].stage, "restored");
    assert_eq!(outer.rows()[0].total_bytes, 13);
}
