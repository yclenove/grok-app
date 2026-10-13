//! Router safety, delivery, and lifecycle integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod router_support;
use gcu_driver_fake::faults::Fault;
use gcu_protocol::{result::Outcome, ErrorCode};
use router_support::*;

#[tokio::test]
async fn click_deleted_element_returns_fresh_state() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject(Fault::ElementDeleted {
            window: "form-window".into(),
            identifier: "submit".into(),
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.as_ref().unwrap().code, ErrorCode::StaleElement);
    assert!(out.error.unwrap().state.is_some());
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn pointer_out_of_bounds_returns_zero_delivery() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(point(-1., 20.)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::OutOfBounds);
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn uncertain_timeout_returns_unknown_without_retry() {
    let h = Harness::new(true, true);
    h.observe().await;
    h.driver
        .inject_action(Fault::Timeout {
            maybe_executed: true,
        })
        .await
        .unwrap();
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.outcome, Outcome::Unknown);
    assert!(out.state.is_some());
    assert_eq!(h.driver.perform_calls(), 1);
    assert!(h.session.action_ledger().has_unknown());
}

#[tokio::test]
async fn latched_stop_refuses_and_releases_input() {
    let h = Harness::new(true, true);
    h.activity.request_stop();
    let out = h.execute(click(3)).await;
    assert_eq!(out.error.unwrap().code, ErrorCode::StoppedByUser);
    assert!(h.driver.release_calls() > 0);
    assert_eq!(h.driver.perform_calls(), 0);
}

#[tokio::test]
async fn successful_action_returns_state_and_ledger_entry() {
    let h = Harness::new(true, true);
    h.observe().await;
    let out = h.execute(click(3)).await;
    assert_eq!(out.action.outcome, Outcome::Done);
    assert!(out.state.is_some());
    assert_eq!(h.session.action_ledger().entries().len(), 1);
}

#[path = "router_cases/approvals.rs"]
mod approvals;
#[path = "router_cases/delivery.rs"]
mod delivery;
#[path = "router_cases/gates.rs"]
mod gates;

#[path = "router_cases/routes.rs"]
mod routes;
