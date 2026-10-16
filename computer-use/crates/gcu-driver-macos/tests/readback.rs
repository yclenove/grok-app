//! SetValue readback uses an injected clock. No wall-clock settle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use gcu_core::driver::{Cancel, Delivery, DriverAction, DriverError, DRIVER_DEADLINE};
use gcu_core::model::NodeValue;
use gcu_driver_macos::ax::{
    perform_clocked, AxSource, AxStateBits, FakeAct, FakeAxNode, FakeAxSource, READBACK_BUDGET,
    READBACK_GAP,
};
use gcu_driver_macos::input::clock::FakeClock;

fn slider(value: f64) -> FakeAxNode {
    FakeAxNode {
        id: 1,
        role: "AXSlider".into(),
        value: Some(NodeValue::Number(value)),
        states: AxStateBits {
            settable: true,
            enabled: Some(true),
            ..AxStateBits::default()
        },
        ..FakeAxNode::default()
    }
}

fn source(node: FakeAxNode, act: FakeAct) -> FakeAxSource {
    let source = FakeAxSource {
        act: Arc::new(std::sync::Mutex::new(act)),
        ..FakeAxSource::default()
    };
    source.bind_root(node);
    source
}

async fn wait_for_readback(clock: &FakeClock) {
    let start = Instant::now();
    while clock.waiters() == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "readback did not wait on the injected clock"
        );
        tokio::task::yield_now().await;
    }
}

async fn run_clocked(
    source: FakeAxSource,
    value: &str,
    advance: Option<Duration>,
    cancel: Cancel,
) -> (gcu_core::driver::Result<Delivery>, Vec<String>) {
    let handle = source.handle(&slider_of(&source));
    let clock = Arc::new(FakeClock::new());
    let clock_task = Arc::clone(&clock);
    let cancel_task = cancel.clone();
    let value = value.to_string();
    let task_source = source.clone();
    let task = tokio::spawn(async move {
        perform_clocked(
            &task_source,
            &DriverAction::SetValue { el: handle, value },
            &cancel_task,
            DRIVER_DEADLINE,
            clock_task.as_ref(),
        )
        .await
    });
    wait_for_readback(&clock).await;
    if let Some(step) = advance {
        clock.advance(step);
    } else {
        cancel.cancel();
    }
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("readback hung")
        .expect("readback task panicked");
    (result, source.calls())
}

fn slider_of(source: &FakeAxSource) -> FakeAxNode {
    source
        .root
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .expect("slider root")
}

#[tokio::test]
async fn delayed_one_frame_value_readback_succeeds() {
    let mut act = FakeAct::default();
    act.value_lag_reads = 1;
    let source = source(slider(50.0), act);
    let (result, calls) = run_clocked(source, "80", Some(READBACK_GAP), Cancel::default()).await;
    assert_eq!(result.unwrap(), Delivery::Accessibility);
    assert!(calls.iter().any(|entry| entry == "set:AXValue"));
    assert!(calls.iter().all(|entry| entry != "reject:readback"));
}

#[tokio::test]
async fn unchanged_value_readback_is_not_settable_and_maybe_executed() {
    let mut act = FakeAct::default();
    act.ignore_value_write = true;
    let source = source(slider(50.0), act);
    let (result, calls) = run_clocked(source, "80", Some(READBACK_BUDGET), Cancel::default()).await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::NotSettable {
            maybe_executed: true
        }
    );
    assert!(calls.iter().any(|entry| entry == "set:AXValue"));
    assert!(calls.iter().any(|entry| entry == "reject:readback"));
    assert!(calls.iter().all(|entry| !entry.contains("80")));
}

#[tokio::test]
async fn cancel_during_readback_wait_is_executed() {
    let mut act = FakeAct::default();
    act.ignore_value_write = true;
    let source = source(slider(50.0), act);
    let (result, _) = run_clocked(source, "80", None, Cancel::default()).await;
    assert_eq!(
        result.unwrap_err(),
        DriverError::Cancelled {
            maybe_executed: true
        }
    );
}
