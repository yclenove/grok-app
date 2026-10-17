//! Offline logical-time skeleton. This is not a native performance measurement.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use async_trait::async_trait;
use gcu_core::{
    driver::{Cancel, Driver, DriverAction, TreeOpts},
    session::{InputRelease, Session},
    settle::{SettleEngine, SettleRequest},
};
use gcu_driver_fake::{faults::Fault, FakeDriver};
use gcu_host_api::testing::FakeUserActivity;
use gcu_protocol::result::{Effect, SettleReason};
use serde_json::json;
use std::{sync::Arc, time::Duration};

struct Release(Arc<FakeDriver>);
#[async_trait]
impl InputRelease for Release {
    async fn release_all_input(&self) {
        self.0.release_all_input().await;
    }
}
async fn sample(events: bool, delay: Option<Duration>) -> u64 {
    let driver = Arc::new(FakeDriver::from_scene("form").unwrap().with_events(events));
    let win = driver.fixture_window("fake.form").await.unwrap();
    let opts = TreeOpts::default();
    let before = driver
        .snapshot_tree(&win, &opts, &Cancel::default())
        .await
        .unwrap();
    let session = Session::open(
        "bench-fake",
        Arc::new(FakeUserActivity::new()),
        Arc::new(Release(driver.clone())),
    );
    session.begin_turn().unwrap();
    let stream = driver.subscribe(&win);
    if let Some(delay) = delay {
        driver
            .inject_action(Fault::SlowSettle(delay))
            .await
            .unwrap();
    }
    let button = driver.element(&win, "submit").await.unwrap();
    driver
        .perform(&DriverAction::Press { el: button }, &Cancel::default())
        .await
        .unwrap();
    let engine = SettleEngine::default();
    let report = engine
        .wait(SettleRequest {
            driver: driver.as_ref(),
            session: &session,
            window: &win,
            tree_opts: &opts,
            events: stream,
            before: &before,
            budget: Duration::from_millis(800),
        })
        .await
        .unwrap();
    let after = driver
        .snapshot_tree(&win, &opts, &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        report.effect(&before, &after, None).unwrap(),
        Effect::Changed
    );
    assert_eq!(driver.oracle().await.unwrap().presses["submit"], 1);
    assert_eq!(driver.perform_calls(), 1);
    assert_ne!(report.settle.reason, SettleReason::Budget);
    report.settle.ms
}
fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut scenarios = Vec::new();
        for (name, events, delay) in [
            ("event_quiet", true, None),
            ("polling", false, None),
            ("delayed_event", true, Some(Duration::from_millis(500))),
            ("silent_delayed_poll", false, Some(Duration::from_millis(500))),
        ] {
            let mut samples = Vec::new();
            for _ in 0..50 { samples.push(sample(events, delay).await); }
            samples.sort_unstable();
            scenarios.push(json!({ "scenario": name, "samples": samples.len(),
                "logical_settle_ms": { "p50": samples[24], "p95": samples[47] } }));
        }
        println!("{}", json!({ "kind": "fake_logical_time", "native_status": "not_run", "scenarios": scenarios }));
    });
}
