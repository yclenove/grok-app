use super::*;

#[test]
fn codex_retired_slots_are_reusable_across_many_distinct_chats() {
    let (broker, adapter) = review_pair();
    let mut identities = HashSet::new();
    let mut first_token = String::new();
    let mut last_token = String::new();
    for index in 0..96 {
        let session = format!("review-sequential-{index}");
        let issued = broker.issue_transport_credential(&session);
        assert!(
            issued.is_ok(),
            "sequential retired chat exhausted owner slots at {index}: {}",
            broker.consent_debug()
        );
        let issued = issued.unwrap();
        if index == 0 {
            first_token = issued.token().to_string();
        }
        last_token = issued.token().to_string();
        broker
            .bind_agent_session(&session, "synthetic-review-agent")
            .unwrap();
        let old = match broker.classify_bearer(issued.token()) {
            BearerClass::Live(ctx) => ctx,
            _ => panic!("positive current context missing"),
        };
        assert!(
            identities.insert((old.owner_nonce, old.incarnation)),
            "nonce/incarnation reused at {index}"
        );
        let pending = broker
            .submit_captured(&old, review_args("A", &["observe"]))
            .unwrap();
        let request_id = pending["requestId"].as_str().unwrap().to_string();
        let revision = pending["revision"].as_u64().unwrap();
        broker.retire_owner(&session);
        assert!(
            broker
                .submit_captured(&old, review_args("A", &["observe"]))
                .is_err(),
            "retired token could request at {index}"
        );
        assert!(broker
            .submit_captured(&old, json!({"action": "status"}))
            .is_err());
        assert!(broker
            .submit_captured(&old, json!({"action": "cancel"}))
            .is_err());
        assert!(broker
            .start_model_preparation(&old, &request_id, revision)
            .is_err());
        assert!(broker
            .execution_with_context(&old, "computer_observe")
            .is_err());
        assert!(
            broker.authority_census().owners <= 2,
            "retired chats were retained without a resource at {index}"
        );
    }
    let census = broker.authority_census();
    assert!(census.retired_outbox <= 64);
    assert!(census.retired_tokens <= 64);
    assert!(census.retired_outbox <= census.retired_tokens);
    let drained = broker.take_retired_tokens();
    assert!(drained.len() <= 64);
    assert!(drained.iter().any(|token| token == &last_token));
    assert!(!drained.iter().any(|token| token == &first_token));
    assert!(broker.take_retired_tokens().is_empty());
    let current_session = "review-sequential-live";
    let current = broker.issue_transport_credential(current_session).unwrap();
    broker
        .bind_agent_session(current_session, "synthetic-review-agent")
        .unwrap();
    let BearerClass::Live(current_ctx) = broker.classify_bearer(current.token()) else {
        panic!("current context missing");
    };
    assert!(
        identities.insert((current_ctx.owner_nonce, current_ctx.incarnation)),
        "reclaimed slot reused a retired nonce"
    );
    let pending = broker
        .submit_access(current.token(), review_args("Live", &["observe"]))
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    let ready = broker
        .start_model_preparation(
            &current_ctx,
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let crate::task_consent::CommitReady::Prepare(prep) = ready else {
        panic!("current owner could not start a preparation");
    };
    assert!(broker.abort_model_preparation(prep.id));
    let outcome = review_commit(
        &broker,
        current.token(),
        "LiveObserve",
        &["observe"],
        &adapter.fixture_id(),
    );
    let seen = model_observe(
        &broker,
        current.token(),
        &outcome.run_id,
        &adapter.fixture_id(),
    );
    assert_eq!(
        seen["isError"], false,
        "current owner observe failed: {seen}"
    );
    assert_eq!(adapter.last_observe_target_id(), adapter.fixture_id());
    assert_eq!(adapter.observe_calls(), 1);
    assert!(adapter.executions().is_empty());
}

#[test]
fn collection_caps_keep_grants_and_cleanup_obligations() {
    let (broker, adapter) = review_pair();
    let mut runs = Vec::new();
    for owner in 0..4 {
        let session = format!("cap-owner-{owner}");
        let token = broker.issue_transport_credential(&session).unwrap();
        broker
            .bind_agent_session(&session, "synthetic-review-agent")
            .unwrap();
        for index in 0..8 {
            let outcome = review_commit(
                &broker,
                token.token(),
                &format!("scope-{index}"),
                &["observe"],
                &adapter.fixture_id(),
            );
            runs.push(outcome.run_id);
        }
        assert!(broker
            .submit_access(token.token(), review_args("overflow", &["observe"]))
            .is_err());
        assert!(broker.claim_continuation(&session).is_some());
    }
    let census = broker.authority_census();
    assert_eq!(census.grants, 32);
    assert!(census.scopes <= 16);
    assert_eq!(census.decisions, 32);
    assert_eq!(census.requests, 32);
    for index in 4..32 {
        broker
            .issue_transport_credential(&format!("cap-owner-{index}"))
            .unwrap();
    }
    assert!(broker.issue_transport_credential("cap-overflow").is_err());
    broker.sweep_consent_clock_for_test(Duration::from_secs(1801));
    assert_eq!(broker.authority_census().expired_runs, 32);
    broker.advance_consent_clock(Duration::ZERO);
    assert_eq!(broker.authority_census().expired_runs, 0);
    for run in &runs {
        assert_ne!(broker.stop_state(run).unwrap(), StopState::Running);
    }
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    for run in &runs {
        assert_eq!(broker.stop_state(run).unwrap(), StopState::Stopped);
    }
    assert!(broker.feature_enabled());

    // Fill the delivery queue, then stop one additional real approved run.
    // Overflow stays owned by its RunState until the consumer has capacity.
    let (broker, adapter) = review_pair();
    let mut original = Vec::new();
    for index in 0..32 {
        let run = format!("legacy-cap-{index}");
        broker.open_run("legacy-cap", &run).unwrap();
        assert!(broker.fence_stop(&run).unwrap().is_some());
        original.push(run);
    }
    let token = broker.issue_transport_credential("cleanup-owner").unwrap();
    broker
        .bind_agent_session("cleanup-owner", "synthetic-review-agent")
        .unwrap();
    broker.begin_new_task("cleanup-owner").unwrap();
    assert_eq!(broker.authority_census().cleanup_tickets, 32);
    let outcome = review_commit(
        &broker,
        token.token(),
        "Keep",
        &["observe"],
        &adapter.fixture_id(),
    );
    broker.observe(&outcome.run_id).unwrap();
    assert!(adapter.observe_calls() > 0);
    broker.complete_task("cleanup-owner").unwrap();
    assert_eq!(broker.authority_census().cleanup_tickets, 32);
    assert!(broker.stop_cleanup_pending(&outcome.run_id).unwrap());
    for ticket in broker.take_cleanup_tickets() {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    let overflow = broker.take_cleanup_tickets();
    assert!(overflow
        .iter()
        .any(|ticket| ticket.run_id() == outcome.run_id));
    for ticket in overflow {
        broker.finish_stop_cleanup(&ticket).unwrap();
    }
    original.push(outcome.run_id);
    for run in original {
        assert_eq!(broker.stop_state(&run).unwrap(), StopState::Stopped);
    }
}

#[test]
fn preparation_cap_keeps_unfinished_work_until_abort() {
    let (broker, adapter) = review_pair();
    let token = review_token(&broker);
    let BearerClass::Live(ctx) = broker.classify_bearer(token.token()) else {
        panic!("context");
    };
    let mut preps = Vec::new();
    for index in 0..4 {
        let pending = broker
            .submit_access(
                token.token(),
                review_args(&format!("prep-{index}"), &["observe"]),
            )
            .unwrap();
        broker
            .bind_synthetic_scope(
                pending["requestId"].as_str().unwrap(),
                &adapter.fixture_id(),
            )
            .unwrap();
        let ready = broker
            .start_model_preparation(
                &ctx,
                pending["requestId"].as_str().unwrap(),
                pending["revision"].as_u64().unwrap(),
            )
            .unwrap();
        let crate::task_consent::CommitReady::Prepare(prep) = ready else {
            panic!("prepare");
        };
        preps.push(prep.id);
    }
    let extra = broker
        .submit_access(token.token(), review_args("prep-extra", &["observe"]))
        .unwrap();
    broker
        .bind_synthetic_scope(extra["requestId"].as_str().unwrap(), &adapter.fixture_id())
        .unwrap();
    assert!(broker
        .start_model_preparation(
            &ctx,
            extra["requestId"].as_str().unwrap(),
            extra["revision"].as_u64().unwrap(),
        )
        .is_err());
    assert_eq!(broker.authority_census().preparations, 4);
    broker.retire_owner("review-app");
    assert!(broker.issue_transport_credential("review-app").is_err());
    for id in preps {
        assert!(broker.abort_model_preparation(id));
    }
    assert!(broker.issue_transport_credential("review-app").is_ok());
}
