//! U01-R authority regressions. Synthetic Host decisions are test data.
//! These assertions describe the contract Codex rejected in U01. On the
//! pre-fix tree they fail; they are not a patch of the independent snapshot.
use super::*;
use crate::broker::gates::click_req;
use crate::fake::FakeAdapter;
use crate::protocol::OutcomeKind;
use crate::task_consent::BearerClass;
use serde_json::{json, Value};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn pair() -> (ComputerUseBroker, Arc<FakeAdapter>) {
    let adapter = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir().join(format!("cu-u01r-{}.lock", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    (broker, adapter)
}

fn args(purpose: &str, scope: &str, caps: &[&str]) -> Value {
    json!({
        "action": "request",
        "purpose": purpose,
        "surfaces": ["desktop"],
        "capabilities": caps,
        "scopeIntent": scope
    })
}

fn bind(broker: &ComputerUseBroker) -> crate::task_consent::IssuedTransport {
    let token = broker.issue_transport_credential("app-local").unwrap();
    broker
        .bind_agent_session("app-local", "synthetic-agent")
        .unwrap();
    token
}

#[test]
fn preparing_status_grant_and_ticket_stay_empty() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let seen = Arc::new(Mutex::new(None));
    let slot = seen.clone();
    let racing = broker.clone();
    let token_text = token.token().to_string();
    broker.arm_commit_interrupt_after_enable(Box::new(move || {
        let status = racing
            .submit_access(&token_text, json!({"action": "status"}))
            .unwrap();
        *slot.lock().unwrap() = Some((
            status["state"].as_str().unwrap_or("").to_string(),
            racing.executable_grant_count("app-local"),
            racing.continuation_count("app-local"),
            racing.claim_continuation("app-local").is_some(),
        ));
    }));
    let _ = broker.commit_access(token.token(), &request_id, 1);
    let (state, grants, continuations, ticket) = seen.lock().unwrap().clone().expect("barrier ran");
    assert_ne!(state, "approved", "preparation was published as approved");
    assert_eq!(grants, 0, "executable grant existed before bind");
    assert_eq!(continuations, 0, "continuation existed before bind");
    assert!(!ticket, "ticket could be taken before bind");
}

#[test]
fn second_approval_during_prepare_is_not_success() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let gate = Arc::new((Mutex::new(0u8), Condvar::new()));
    let hook_gate = gate.clone();
    broker.arm_commit_interrupt_after_enable(Box::new(move || {
        let (lock, cv) = &*hook_gate;
        let mut state = lock.lock().unwrap();
        *state = 1;
        cv.notify_all();
        while *state == 1 {
            state = cv.wait(state).unwrap();
        }
    }));
    let second_ok = Arc::new(Mutex::new(None));
    let result_slot = second_ok.clone();
    let waiting = broker.clone();
    let wait_gate = gate.clone();
    let token_text = token.token().to_string();
    let request_for_second = request_id.clone();
    let second = thread::spawn(move || {
        let (lock, cv) = &*wait_gate;
        let mut state = lock.lock().unwrap();
        while *state == 0 {
            state = cv.wait(state).unwrap();
        }
        let ok = waiting
            .commit_access(&token_text, &request_for_second, 1)
            .is_ok();
        *result_slot.lock().unwrap() = Some(ok);
        *state = 2;
        cv.notify_all();
    });
    let _ = broker.commit_access(token.token(), &request_id, 1);
    second.join().unwrap();
    let accepted = second_ok.lock().unwrap().expect("second approval ran");
    assert!(
        !accepted,
        "concurrent approval returned success before the first bind finished"
    );
}

#[test]
fn failed_bind_advances_epoch_past_one() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(pending["requestId"].as_str().unwrap(), "missing-window")
        .unwrap();
    assert!(broker
        .commit_access(token.token(), pending["requestId"].as_str().unwrap(), 1)
        .is_err());
    assert!(!broker.feature_enabled());
    assert!(
        broker.feature_epoch() > 1,
        "rollback restored epoch {}",
        broker.feature_epoch()
    );
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(broker.continuation_count("app-local"), 0);
    assert_eq!(adapter.list_calls(), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn stop_after_last_check_and_after_bind_publish_nothing() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let racing = broker.clone();
    broker.arm_commit_interrupt_before_open(Box::new(move || {
        racing.consent_stop("app-local");
    }));
    let outcome = broker.commit_access(token.token(), &request_id, 1);
    let runs = broker.test_run_count();
    let grants = broker.executable_grant_count("app-local");
    assert!(
        outcome.is_err() && runs == 0 && grants == 0,
        "stop between check and open_run left err={} runs={runs} grants={grants}",
        outcome.is_err()
    );

    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    let racing = broker.clone();
    broker.arm_commit_interrupt_after_bind(Box::new(move || {
        racing.consent_stop("app-local");
    }));
    let outcome = broker.commit_access(token.token(), &request_id, 1);
    assert!(
        outcome.is_err(),
        "stop after bind still returned an approval"
    );
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(broker.continuation_count("app-local"), 0);
    assert_eq!(adapter.observe_calls(), 0);
}

#[test]
fn unbound_agent_cannot_request_or_commit() {
    let (broker, _adapter) = pair();
    let token = broker.issue_transport_credential("app-local").unwrap();
    let submitted = broker.submit_access(
        token.token(),
        args("read the fixture window", "GrokCuFixture", &["observe"]),
    );
    assert!(
        submitted.is_err(),
        "None agent identity was accepted as a model request"
    );
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert_eq!(broker.continuation_count("app-local"), 0);
    assert!(broker.scope_memories("app-local").is_empty());
}

#[test]
fn captured_owner_context_dies_after_reissue() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let BearerClass::Live(old) = broker.classify_bearer(token.token()) else {
        panic!("expected a live captured context");
    };
    broker.retire_owner("app-local");
    let renewed = broker.issue_transport_credential("app-local").unwrap();
    broker
        .bind_agent_session("app-local", "synthetic-agent")
        .unwrap();
    let pending = broker
        .submit_access(
            renewed.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let request_id = pending["requestId"].as_str().unwrap().to_string();
    let revision = pending["revision"].as_u64().unwrap();
    broker
        .bind_synthetic_scope(&request_id, &adapter.fixture_id())
        .unwrap();
    broker
        .commit_access(renewed.token(), &request_id, revision)
        .unwrap();
    let mut accepted = Vec::new();
    if broker
        .submit_captured(&old, json!({"action": "status"}))
        .is_ok()
    {
        accepted.push("status");
    }
    let fresh = broker
        .submit_access(
            renewed.token(),
            args("type in the fixture window", "FreshIntent", &["input"]),
        )
        .unwrap();
    let fresh_id = fresh["requestId"].as_str().unwrap();
    let fresh_revision = fresh["revision"].as_u64().unwrap();
    broker
        .bind_synthetic_scope(fresh_id, &adapter.fixture_id())
        .unwrap();
    let BearerClass::Live(current) = broker.classify_bearer(renewed.token()) else {
        panic!("renewed transport lost its live context");
    };
    if broker
        .start_model_preparation(&old, fresh_id, fresh_revision)
        .is_ok()
    {
        accepted.push("prepare");
    }
    assert!(
        matches!(
            broker
                .start_model_preparation(&current, fresh_id, fresh_revision)
                .unwrap(),
            crate::task_consent::CommitReady::Prepare(_)
        ),
        "current owner could not start a real preparation"
    );
    if broker
        .execution_with_context(&old, "computer_observe")
        .is_ok()
    {
        accepted.push("execution");
    }
    if broker
        .submit_captured(
            &old,
            args("read the fixture window", "OtherIntent", &["observe"]),
        )
        .is_ok()
    {
        accepted.push("request");
    }
    if broker
        .submit_captured(&old, json!({"action": "cancel"}))
        .is_ok()
    {
        accepted.push("cancel");
    }
    assert!(
        accepted.is_empty(),
        "retired captured owner context was accepted after reissue: {accepted:?}"
    );
}

#[test]
fn capability_change_revises_one_pending_request() {
    let (broker, _adapter) = pair();
    let token = bind(&broker);
    let first = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    let second = broker
        .submit_access(
            token.token(),
            args(
                "read the fixture window",
                "GrokCuFixture",
                &["observe", "input"],
            ),
        )
        .unwrap();
    assert_eq!(broker.live_request_count("app-local"), 1);
    assert_eq!(first["requestId"], second["requestId"]);
    assert!(second["revision"].as_u64().unwrap() > first["revision"].as_u64().unwrap());
    assert_eq!(broker.executable_grant_count("app-local"), 0);
}

#[test]
fn pending_intent_is_not_scope_memory_and_incremental_keeps_a() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "FixtureA", &["observe"]),
        )
        .unwrap();
    assert!(
        broker.scope_memories("app-local").is_empty(),
        "untrusted pending intent was recorded as approved scope memory"
    );
    let request_id = pending["requestId"].as_str().unwrap();
    broker
        .bind_synthetic_scope(request_id, &adapter.fixture_id())
        .unwrap();
    broker
        .commit_access(
            token.token(),
            request_id,
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    assert_eq!(broker.scope_memories("app-local").len(), 1);
    let before = adapter.observe_calls();
    let asked = broker.submit_access(
        token.token(),
        args("read the fixture window", "FixtureB", &["input"]),
    );
    assert!(asked.is_ok(), "requesting B should stay pending");
    assert_eq!(asked.unwrap()["state"], "pending");
    assert!(broker
        .execution_run(token.token(), "computer_observe")
        .is_ok());
    let seen = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(seen["isError"], false, "{seen}");
    assert!(adapter.observe_calls() > before);
    let input = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_act",
        json!({}),
        None,
    );
    assert_eq!(input["isError"], true, "{input}");
    assert!(adapter.executions().is_empty());
    let asked = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "FixtureB", &["input"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(asked["requestId"].as_str().unwrap(), "missing-window")
        .unwrap();
    assert!(broker
        .commit_access(
            token.token(),
            asked["requestId"].as_str().unwrap(),
            asked["revision"].as_u64().unwrap(),
        )
        .is_err());
    assert_eq!(broker.executable_grant_count("app-local"), 1);
    let again = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(again["isError"], false, "{again}");
}

#[test]
fn agent_replacement_before_dispatch_reaches_no_adapter() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let before_observe = adapter.observe_calls();
    let before_exec = adapter.executions().len();
    let racing = broker.clone();
    broker.arm_execution_barrier(Box::new(move || {
        racing
            .bind_agent_session("app-local", "synthetic-agent-2")
            .unwrap();
    }));
    let seen = crate::tools::dispatch_with_transport(
        &broker,
        token.token(),
        "computer_observe",
        json!({"screenshot": false}),
        None,
    );
    assert_eq!(seen["isError"], true, "{seen}");
    assert_eq!(adapter.observe_calls(), before_observe);
    assert_eq!(adapter.executions().len(), before_exec);
}

#[test]
fn stop_ack_does_not_wait_for_slow_abort() {
    let (broker, adapter) = pair();
    let broker = Arc::new(broker);
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args(
                "read the fixture window",
                "GrokCuFixture",
                &["observe", "input"],
            ),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    let outcome = broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let obs = broker.observe(&outcome.run_id).unwrap();
    let (target_id, generation) = broker.authorized_target(&outcome.run_id).unwrap();
    broker.open_run("other", "run-2").unwrap();
    let next_gen = broker
        .authorize_target("run-2", &adapter.fixture_id())
        .unwrap();
    let next_obs = broker.observe("run-2").unwrap();
    adapter.set_hang(true);
    adapter.set_abort_blocked(true);
    let acting = broker.clone();
    let run_id = outcome.run_id.clone();
    let snap = obs.snapshot_id.clone();
    let geo = obs.geometry_revision;
    let act_thread = thread::spawn(move || {
        acting.act(click_req(
            &run_id,
            &target_id,
            generation,
            &snap,
            geo,
            "slow-action",
        ))
    });
    let started = Instant::now();
    while adapter.executions().is_empty() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !adapter.executions().is_empty(),
        "slow action never reached the owned adapter"
    );
    let stopping = broker.clone();
    let stop_thread = thread::spawn(move || stopping.consent_stop("app-local"));
    let ack_started = Instant::now();
    while !stop_thread.is_finished() && ack_started.elapsed() < Duration::from_millis(400) {
        thread::sleep(Duration::from_millis(20));
    }
    let quick = stop_thread.is_finished();
    if !quick {
        adapter.set_abort_blocked(false);
    }
    let tickets = stop_thread.join().unwrap();
    if quick {
        let rejected = broker.act(click_req(
            "run-2",
            &adapter.fixture_id(),
            next_gen,
            &next_obs.snapshot_id,
            next_obs.geometry_revision,
            "while-fenced",
        ));
        assert_eq!(rejected.kind, OutcomeKind::Rejected);
        assert!(!rejected.executed);
        assert_eq!(adapter.executions(), ["slow-action"]);
        adapter.set_abort_blocked(false);
    }
    for ticket in &tickets {
        let _ = broker.finish_stop_cleanup(ticket);
    }
    let _ = act_thread.join().unwrap();
    let _ = broker.wait_stopped(&outcome.run_id, Duration::from_secs(2));
    adapter.set_hang(false);
    let released = broker.act(click_req(
        "run-2",
        &adapter.fixture_id(),
        next_gen,
        &next_obs.snapshot_id,
        next_obs.geometry_revision,
        "after-release",
    ));
    assert_eq!(released.kind, OutcomeKind::Verified);
    assert!(released.executed);
    assert!(quick, "Stop ack waited for the slow abort");
}

#[test]
fn terminal_reservations_and_live_continuations_stay_bounded() {
    let (broker, _adapter) = pair();
    let token = bind(&broker);
    for index in 0..40 {
        let pending = broker
            .submit_access(
                token.token(),
                args(&format!("purpose {index}"), "GrokCuFixture", &["observe"]),
            )
            .unwrap();
        let request_id = pending["requestId"].as_str().unwrap().to_string();
        let revision = pending["revision"].as_u64().unwrap();
        let _ = broker.commit_access(token.token(), &request_id, revision);
        broker
            .deny_access(token.token(), &request_id, revision)
            .unwrap();
    }
    assert!(
        broker.reservation_count() <= 32,
        "reservations grew to {}",
        broker.reservation_count()
    );
}

#[test]
fn unconsumed_continuation_survives_decision_flood() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    assert_eq!(broker.continuation_count("app-local"), 1);
    let other = broker.issue_transport_credential("other-app").unwrap();
    broker
        .bind_agent_session("other-app", "synthetic-agent")
        .unwrap();
    for index in 0..40 {
        let pending = broker
            .submit_access(
                other.token(),
                args(&format!("noise {index}"), "Nowhere", &["observe"]),
            )
            .unwrap();
        broker
            .deny_access(
                other.token(),
                pending["requestId"].as_str().unwrap(),
                pending["revision"].as_u64().unwrap(),
            )
            .unwrap();
    }
    assert!(
        broker.claim_continuation("app-local").is_some(),
        "a live continuation was discarded by another owner's flood"
    );
}

#[test]
fn retire_does_not_free_a_global_owner_slot() {
    // Rejected contract: a retired chat occupies a slot for the life of the
    // process. The bound is live and retained resources, still MAX_OWNERS.
    let (broker, adapter) = pair();
    for index in 0..32 {
        broker
            .issue_transport_credential(&format!("session-{index}"))
            .unwrap();
    }
    assert!(broker
        .issue_transport_credential("session-overflow")
        .is_err());
    broker.retire_owner("session-0");
    assert!(
        broker.issue_transport_credential("session-new").is_ok(),
        "a fully terminal retire did not free one owner slot"
    );
    for index in 1..32 {
        broker.retire_owner(&format!("session-{index}"));
    }
    broker.retire_owner("session-new");
    for index in 0..31 {
        broker
            .issue_transport_credential(&format!("held-{index}"))
            .unwrap();
    }
    let held = broker.issue_transport_credential("held-prep").unwrap();
    broker
        .bind_agent_session("held-prep", "synthetic-agent")
        .unwrap();
    let pending = broker
        .submit_access(
            held.token(),
            args("read the fixture window", "Held", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    let BearerClass::Live(current) = broker.classify_bearer(held.token()) else {
        panic!("held transport was not live");
    };
    let ready = broker
        .start_model_preparation(
            &current,
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let crate::task_consent::CommitReady::Prepare(prep) = ready else {
        panic!("preparation did not start");
    };
    broker.retire_owner("held-prep");
    assert!(
        broker
            .issue_transport_credential("session-blocked")
            .is_err(),
        "an unfinished preparation was dropped and its owner slot reused"
    );
    broker.abort_model_preparation(prep.id);
    assert!(
        broker
            .issue_transport_credential("session-after-abort")
            .is_ok(),
        "aborting the original preparation did not free the slot"
    );
}

#[test]
fn host_new_task_retires_only_the_old_grant() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    assert_eq!(broker.scope_memories("app-local").len(), 1);
    broker.begin_new_task("app-local").unwrap();
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    assert!(broker
        .execution_run(token.token(), "computer_observe")
        .is_err());
    assert_eq!(broker.scope_memories("app-local").len(), 1);
    let again = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    assert_eq!(again["state"], "pending");
    assert_eq!(broker.executable_grant_count("app-local"), 0);
}

#[test]
fn approved_memory_does_not_execute_for_the_next_agent() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    broker
        .bind_agent_session("app-local", "synthetic-agent-2")
        .unwrap();
    assert_eq!(broker.scope_memories("app-local").len(), 1);
    assert_eq!(broker.executable_grant_count("app-local"), 0);
    let renewed = broker.issue_transport_credential("app-local").unwrap();
    assert!(broker
        .execution_run(renewed.token(), "computer_observe")
        .is_err());
    assert!(adapter.executions().is_empty());
}

#[test]
fn typed_ticket_cannot_be_consumed_after_stop_or_epoch_change() {
    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let claim = broker.claim_continuation("app-local").unwrap();
    broker.validate_continuation(&claim).unwrap();
    broker.consent_stop("app-local");
    assert!(broker.claim_continuation("app-local").is_none());
    assert!(broker.consume_continuation(&claim).is_err());

    let (broker, adapter) = pair();
    let token = bind(&broker);
    let pending = broker
        .submit_access(
            token.token(),
            args("read the fixture window", "GrokCuFixture", &["observe"]),
        )
        .unwrap();
    broker
        .bind_synthetic_scope(
            pending["requestId"].as_str().unwrap(),
            &adapter.fixture_id(),
        )
        .unwrap();
    broker
        .commit_access(
            token.token(),
            pending["requestId"].as_str().unwrap(),
            pending["revision"].as_u64().unwrap(),
        )
        .unwrap();
    let claim = broker.claim_continuation("app-local").unwrap();
    broker.set_feature_enabled(false);
    broker.set_feature_enabled(true);
    assert!(broker.validate_continuation(&claim).is_err());
    assert!(broker.consume_continuation(&claim).is_err());
    assert!(adapter.executions().is_empty());
}
