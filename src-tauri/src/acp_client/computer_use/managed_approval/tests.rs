//! Synthetic local ACP ownership only; no settings, browser or OS permission.
use super::super::task_view::tests::Fixture;
use super::*;
use grok_computer_use_core::broker::HostTaskDecision;
use grok_computer_use_core::task_consent::HostRequestPhase;

fn phase(f: &Fixture) -> HostRequestPhase {
    f.client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap()
        .request
        .unwrap()
        .phase
}

fn start(f: &Fixture) -> ManagedApprovalStart {
    f.client
        .start_managed_approval("view-app", "synthetic-view-agent", &f.request_key())
        .unwrap()
}

fn zero_execution(f: &Fixture) {
    assert_eq!(f.broker().executable_grant_count("view-app"), 0);
    assert_eq!(f.broker().continuation_count("view-app"), 0);
    let view = f
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    assert!(view.executions.is_empty());
    assert!(view.grants.is_empty());
    assert!(!view.feature_enabled);
    assert_eq!(f.broker().capabilities().backend_id, "request-only");
}

#[tokio::test]
async fn managed_host_entry_rejects_foreign_session_agent_and_every_key_field_without_resources() {
    let f = Fixture::new_managed().await;
    for field in [
        "session",
        "agent",
        "connection",
        "task",
        "request",
        "revision",
    ] {
        let mut key = f.request_key();
        match field {
            "connection" => key.connection_id = "foreign".into(),
            "task" => key.task_id = "foreign".into(),
            "request" => key.request_id = "foreign".into(),
            "revision" => key.revision += 1,
            _ => {}
        }
        let session = if field == "session" {
            "foreign"
        } else {
            "view-app"
        };
        let agent = if field == "agent" {
            "foreign"
        } else {
            "synthetic-view-agent"
        };
        assert!(
            f.client
                .start_managed_approval(session, agent, &key)
                .is_err(),
            "{field}"
        );
        assert_eq!(phase(&f), HostRequestPhase::Pending);
        zero_execution(&f);
    }
    drop(start(&f));
    f.finish().await;
}

#[tokio::test]
async fn managed_reservation_is_non_executable_duplicate_gesture_is_rejected_and_drop_allows_retry()
{
    let f = Fixture::new_managed().await;
    let reservation = start(&f);
    assert_eq!(phase(&f), HostRequestPhase::Preparing);
    zero_execution(&f);
    assert!(f
        .client
        .start_managed_approval("view-app", "synthetic-view-agent", &f.request_key())
        .is_err());
    drop(reservation);
    assert_eq!(phase(&f), HostRequestPhase::Pending);
    drop(start(&f));
    zero_execution(&f);
    f.finish().await;
}

#[tokio::test]
async fn managed_creation_cannot_replace_a_desktop_picker_decision() {
    let f = Fixture::new().await;
    assert!(f
        .client
        .start_managed_approval("view-app", "synthetic-view-agent", &f.request_key())
        .is_err());
    assert_eq!(phase(&f), HostRequestPhase::Pending);
    zero_execution(&f);
    f.finish().await;
}

#[tokio::test]
async fn replacement_of_original_transport_prevents_managed_preparation_before_the_worker_starts() {
    let f = Fixture::new_managed().await;
    let reservation = start(&f);
    f.replace_approval_transport_same_identity();
    assert!(f
        .client
        .finish_managed_approval(
            "view-app",
            "synthetic-view-agent",
            reservation,
            |_, _| async { panic!("replacement entered native preparation") }
        )
        .await
        .is_err());
    assert_eq!(phase(&f), HostRequestPhase::Pending);
    zero_execution(&f);
    f.finish().await;
}

#[tokio::test]
async fn cancelling_managed_approval_waiter_fences_its_reservation_without_starting_a_runtime() {
    let f = Fixture::new_managed().await;
    let reservation = start(&f);
    let client = f.client.clone();
    let (entered, started) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async move {
        client
            .finish_managed_approval(
                "view-app",
                "synthetic-view-agent",
                reservation,
                |_, approval| async move {
                    entered.send(()).unwrap();
                    std::future::pending::<()>().await;
                    drop(approval);
                    Err("synthetic waiter cancelled".into())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
    assert_eq!(phase(&f), HostRequestPhase::Pending);
    drop(start(&f));
    zero_execution(&f);
    f.finish().await;
}

#[tokio::test]
async fn stop_of_exact_managed_request_retires_the_pending_owner_and_late_result_cannot_publish() {
    let f = Fixture::new_managed().await;
    let reservation = start(&f);
    let client = f.client.clone();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async move {
        client
            .finish_managed_approval(
                "view-app",
                "synthetic-view-agent",
                reservation,
                |_, approval| async move {
                    entered.send(()).unwrap();
                    blocked.await.unwrap();
                    assert!(!approval.is_current());
                    Err("original approval fenced".into())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .unwrap()
        .unwrap();
    f.client
        .decide_computer_use_task(
            "view-app",
            "synthetic-view-agent",
            &f.request_key(),
            HostTaskDecision::Stop,
        )
        .unwrap();
    release.send(()).unwrap();
    assert!(waiter.await.unwrap().is_err());
    assert_ne!(phase(&f), HostRequestPhase::Preparing);
    assert!(f
        .client
        .start_managed_approval("view-app", "synthetic-view-agent", &f.request_key())
        .is_err());
    zero_execution(&f);
    f.finish().await;
}
