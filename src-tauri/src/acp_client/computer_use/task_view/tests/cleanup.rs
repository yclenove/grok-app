//! Teardown must prove the original fenced owner stopped, not just accept a receipt.
use super::*;
use crate::computer_use::test_support::finish_fixture_cleanup;
use grok_computer_use_core::broker::StopState;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fixture_finish_waits_for_original_cleanup_already_in_flight() {
    let f = Fixture::new().await;
    let (fake, address) = f.approved_execution();
    fake.set_abort_blocked(true);
    f.transport.retire();
    let broker = f.broker();
    let ticket = broker.take_cleanup_tickets().remove(0);
    let original_broker = broker.clone();
    let original =
        tokio::task::spawn_blocking(move || original_broker.finish_stop_cleanup(&ticket));
    let entered = fake.clone();
    let started =
        tokio::task::spawn_blocking(move || entered.wait_for_abort_call(Duration::from_secs(2)))
            .await
            .unwrap();
    // Release even when an assertion fails; never leave this synthetic owner
    // blocked while unwinding a failed test.
    if !started {
        fake.set_abort_blocked(false);
        original.await.unwrap().unwrap();
        panic!("original cleanup did not enter");
    }
    assert_eq!(
        broker.stop_state(&address.run_id).unwrap(),
        StopState::StopRequested
    );
    let finish = tokio::spawn(f.finish());
    tokio::time::sleep(Duration::from_millis(50)).await;
    let premature = finish.is_finished();
    fake.set_abort_blocked(false);
    assert_eq!(original.await.unwrap().unwrap(), StopState::Stopped);
    finish.await.unwrap();
    assert_eq!(
        broker.stop_state(&address.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(
        !premature,
        "fixture accepted StopRequested before original physical cleanup"
    );
    assert_eq!(
        fake.abort_called(),
        1,
        "the original physical abort was redispatched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cleanup_observation_deadline_does_not_turn_a_fenced_owner_into_success() {
    let f = Fixture::new().await;
    let (_, address) = f.approved_execution();
    f.transport.retire();
    let broker = f.broker();
    let tickets = broker.take_cleanup_tickets();
    assert!(!tickets.is_empty());
    let error = finish_fixture_cleanup(broker.clone(), tickets.clone(), Duration::ZERO)
        .await
        .unwrap_err();
    assert!(error.starts_with("original fixture cleanup unconfirmed"));
    assert_eq!(
        broker.stop_state(&address.run_id).unwrap(),
        StopState::StopRequested
    );
    let evidence = finish_fixture_cleanup(broker.clone(), tickets, Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(evidence.stopped, 1);
    assert_eq!(evidence.pending_replies, 0);
    assert_eq!(
        broker.stop_state(&address.run_id).unwrap(),
        StopState::Stopped
    );
    f.finish().await;
}
