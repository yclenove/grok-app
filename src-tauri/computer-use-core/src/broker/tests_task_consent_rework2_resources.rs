use super::*;

#[test]
fn stop_gaps_finish_the_original_reservation_for_both_feature_paths() {
    for feature_on in [true, false] {
        for gap in ["open", "bind", "finalize"] {
            let (broker, _adapter, tickets) = stop_during_commit(feature_on, gap);
            for ticket in &tickets {
                let mut wrong = ticket.clone();
                wrong.incarnation = wrong.incarnation.wrapping_add(1);
                assert!(
                    broker.finish_stop_cleanup(&wrong).is_err(),
                    "{gap} accepted a mismatched incarnation"
                );
                assert!(broker.stop_cleanup_pending(ticket.run_id()).unwrap());
                let state = broker.finish_stop_cleanup(ticket).unwrap();
                assert_ne!(state, StopState::Running, "{gap}");
                assert!(!broker.stop_cleanup_pending(ticket.run_id()).unwrap());
            }
        }
    }
}

#[test]
fn failed_prepare_can_be_retried_only_as_a_new_reservation() {
    let (broker, adapter, tickets) = stop_during_commit(true, "finalize");
    for ticket in &tickets {
        broker.finish_stop_cleanup(ticket).unwrap();
    }
    let token = review_token(&broker);
    let old = broker
        .submit_access(token.token(), json!({"action": "status"}))
        .unwrap();
    assert_ne!(old["state"], "pending");
    let outcome = review_commit(
        &broker,
        token.token(),
        "retry",
        &["observe"],
        &adapter.fixture_id(),
    );
    assert_ne!(outcome.request_id, old["requestId"].as_str().unwrap());
    let seen = model_observe(
        &broker,
        token.token(),
        &outcome.run_id,
        &adapter.fixture_id(),
    );
    assert_eq!(
        seen["isError"], false,
        "new reservation could not observe: {seen}"
    );
    assert_eq!(adapter.last_observe_target_id(), adapter.fixture_id());
    let claim = broker.claim_continuation("review-app").unwrap();
    assert!(!claim.id.is_empty());
    assert_eq!(claim.dispatch_id, claim.id);
    assert_eq!(claim.request_id, outcome.request_id);
    assert_eq!(claim.owner_nonce, outcome.binding.owner_nonce);
    assert_eq!(claim.feature_epoch, outcome.binding.feature_epoch);
    broker.consume_continuation(&claim).unwrap();
    assert!(broker.consume_continuation(&claim).is_err());
    assert_eq!(broker.continuation_count("review-app"), 1);
}

#[test]
fn cleanup_ticket_blocks_the_next_lease_until_the_original_joins() {
    let (broker, adapter) = review_pair();
    let broker = Arc::new(broker);
    let token = review_token(&broker);
    let outcome = review_commit(
        &broker,
        token.token(),
        "A",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let click = legal_click(&broker, &outcome.run_id, "slow-owned");
    adapter.set_hang(true);
    adapter.set_abort_blocked(true);
    let acting = Arc::clone(&broker);
    let token_string = token.token().to_string();
    let act_thread = thread::spawn(move || model_act(&acting, &token_string, click));
    let started = Instant::now();
    while adapter.executions().is_empty() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(adapter.executions(), vec!["slow-owned".to_string()]);
    for _ in 0..6 {
        let status = crate::tools::dispatch_with_transport(
            &broker,
            token.token(),
            "computer_status",
            json!({}),
            None,
        );
        assert_eq!(status["isError"], false, "{status}");
    }
    let stopping = Arc::clone(&broker);
    let stop_thread = thread::spawn(move || stopping.consent_stop("review-app"));
    let ack = Instant::now();
    while !stop_thread.is_finished() && ack.elapsed() < Duration::from_millis(400) {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        stop_thread.is_finished(),
        "Stop ack waited for the slow abort"
    );
    let tickets = stop_thread.join().unwrap();
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].incarnation(), outcome.binding.incarnation);
    let mut wrong = tickets[0].clone();
    wrong.incarnation = wrong.incarnation.wrapping_add(1);
    assert!(broker.finish_stop_cleanup(&wrong).is_err());
    assert_ne!(
        broker.stop_state(&outcome.run_id).unwrap(),
        StopState::Running
    );
    let other = broker.issue_transport_credential("review-next").unwrap();
    broker
        .bind_agent_session("review-next", "synthetic-review-agent")
        .unwrap();
    let next = review_commit(
        &broker,
        other.token(),
        "next",
        &["observe", "input"],
        &adapter.fixture_id(),
    );
    let _ = legal_click(&broker, &next.run_id, "while-held");
    let blocked = model_act(
        &broker,
        other.token(),
        legal_click(&broker, &next.run_id, "while-held-2"),
    );
    assert_eq!(
        blocked["isError"], true,
        "next owner acted before join: {blocked}"
    );
    assert_eq!(adapter.executions(), vec!["slow-owned".to_string()]);
    adapter.set_abort_blocked(false);
    broker.finish_stop_cleanup(&tickets[0]).unwrap();
    let _ = act_thread.join().unwrap();
    let _ = broker.wait_stopped(&outcome.run_id, Duration::from_secs(2));
    adapter.set_hang(false);
    let released = model_act(
        &broker,
        other.token(),
        legal_click(&broker, &next.run_id, "after-join"),
    );
    assert_eq!(
        released["isError"], false,
        "new owner could not act: {released}"
    );
    assert_eq!(adapter.last_dispatch_target_id(), adapter.fixture_id());
    assert!(adapter.executions().iter().any(|id| id == "after-join"));
    assert!(adapter.abort_called() > 0);
}

#[test]
fn no_authority_notice_is_typed_once_and_does_not_execute() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let pending = broker
        .submit_access(token.token(), review_args("Notice", &["observe"]))
        .unwrap();
    let BearerClass::Live(ctx) = broker.classify_bearer(token.token()) else {
        panic!("context");
    };
    broker
        .deny_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let notes = broker.take_no_authority("review-app", ctx.owner_nonce);
    assert_eq!(notes.len(), 1);
    let note = &notes[0];
    assert_eq!(note.id, note.dispatch_id);
    assert!(!note.dispatch_id.is_empty());
    assert_eq!(note.app_session_id, "review-app");
    assert_eq!(note.owner_nonce, ctx.owner_nonce);
    assert_eq!(note.incarnation, ctx.incarnation);
    assert_eq!(note.connection_generation, ctx.connection_generation);
    assert_eq!(note.agent_session_id, "synthetic-review-agent");
    assert_eq!(note.request_id, pending["requestId"].as_str().unwrap());
    assert!(!note.task_id.is_empty());
    assert_eq!(note.revision, pending["revision"].as_u64().unwrap());
    assert_eq!(note.feature_epoch, broker.feature_epoch());
    assert_eq!(note.reason, "denied");
    assert!(broker
        .take_no_authority("review-app", ctx.owner_nonce)
        .is_empty());
    assert_eq!(adapter.observe_calls(), 0);
    assert!(broker
        .execution_run(token.token(), "computer_observe")
        .is_err());
    assert!(adapter.executions().is_empty());
}
