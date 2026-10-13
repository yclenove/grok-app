//! Stop latch: only `clear_stop_by_user` clears it.
#![cfg(feature = "testing")]

use gcu_host_api::testing::FakeUserActivity;
use gcu_host_api::UserActivity;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn ms_since_physical_input_uses_injected_timestamps() {
    let activity = FakeUserActivity::new();
    assert!(activity.ms_since_physical_input().is_none());
    let start = Instant::now();
    activity.set_now(start + Duration::from_millis(250));
    activity.set_last_physical_input(start);
    assert_eq!(activity.ms_since_physical_input(), Some(250));
    activity.set_now(start);
    activity.set_last_physical_input(start + Duration::from_secs(5));
    assert_eq!(activity.ms_since_physical_input(), Some(0));
    activity.clear_physical_input();
    assert!(activity.ms_since_physical_input().is_none());
}

#[test]
fn stop_latch_after_request_stop_stays_set_until_clear_stop_by_user() {
    let activity = FakeUserActivity::new();
    assert!(!activity.stop_requested());
    assert!(!activity.latched_stop());

    activity.request_stop();
    assert!(activity.stop_requested());
    assert!(activity.latched_stop());
    activity.request_stop();
    assert!(activity.stop_requested());
    assert!(activity.latched_stop());

    let start = Instant::now();
    activity.set_now(start + Duration::from_millis(40));
    activity.set_last_physical_input(start);
    assert_eq!(activity.ms_since_physical_input(), Some(40));
    assert!(activity.latched_stop());
    activity.clear_physical_input();
    assert!(activity.stop_requested());
    assert!(activity.latched_stop());

    activity.clear_stop_by_user();
    assert!(!activity.stop_requested());
    assert!(!activity.latched_stop());
    activity.clear_stop_by_user();
    assert!(!activity.stop_requested());
    assert!(!activity.latched_stop());

    activity.request_stop();
    assert!(activity.stop_requested());
    assert!(activity.latched_stop());
}

#[test]
fn stop_request_from_another_thread_is_visible() {
    let activity = Arc::new(FakeUserActivity::new());
    let worker = {
        let activity = Arc::clone(&activity);
        thread::spawn(move || activity.request_stop())
    };
    match worker.join() {
        Ok(()) => {}
        Err(_) => panic!("worker panicked"),
    }
    assert!(activity.stop_requested());
    assert!(activity.latched_stop());
    activity.clear_stop_by_user();
    assert!(!activity.stop_requested());
    assert!(!activity.latched_stop());
}
