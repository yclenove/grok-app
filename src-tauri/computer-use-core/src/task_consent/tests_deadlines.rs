//! Synthetic authority tests: no native target or human/system approval.
use super::*;
use serde_json::json;

fn pending() -> (ConsentState, TransportContext, String) {
    let mut state = ConsentState::new();
    let token = state.issue("deadline-app", 1, &HashSet::new()).unwrap();
    state.bind_agent("deadline-app", "synthetic-agent").unwrap();
    let BearerClass::Live(ctx) = state.classify(token.token()) else {
        panic!("current owner");
    };
    let result = state
        .submit(
            &ctx,
            json!({
                "action": "request", "purpose": "deadline regression",
                "surfaces": ["desktop"], "scopeIntent": "A", "capabilities": ["observe"]
            }),
            &HashSet::new(),
            1,
        )
        .unwrap();
    let request = result["requestId"].as_str().unwrap().to_owned();
    state.bind_synthetic(&request, "fake:window:1").unwrap();
    (state, ctx, request)
}

fn prepare(state: &mut ConsentState, ctx: &TransportContext, request: &str) -> PrepareTicket {
    match state.start_preparation(ctx, request, 1, true, 1).unwrap() {
        CommitReady::Prepare(ticket) => ticket,
        CommitReady::Already(_) | CommitReady::WebViewSelection(_) => panic!("first preparation"),
    }
}

#[test]
fn expired_pending_cannot_prepare_without_sweep() {
    let (mut state, ctx, request) = pending();
    state.advance_clock_without_sweep(Duration::from_secs(9 * 60));
    let valid = prepare(&mut state, &ctx, &request);
    assert!(state.preparation_current(valid.id, true, 1));
    state.abort_preparation(valid.id, true, 1);
    state.advance_clock_without_sweep(Duration::from_secs(2 * 60));
    assert!(state.start_preparation(&ctx, &request, 1, true, 1).is_err());
}

#[test]
fn slow_preparation_cannot_finalize_after_deadline() {
    let (mut state, ctx, request) = pending();
    state.advance_clock_without_sweep(Duration::from_secs(9 * 60));
    let valid = prepare(&mut state, &ctx, &request);
    let (enabled, epoch) = state.publish_feature(valid.id, true, 1).unwrap();
    state.note_opened_run(valid.id);
    assert!(state.preparation_current(valid.id, enabled, epoch));
    state.advance_clock_without_sweep(Duration::from_secs(2 * 60));
    assert!(state.finalize(valid.id, enabled, epoch).is_err());
}

#[test]
fn idle_expired_continuation_cannot_consume_without_sweep() {
    let (mut state, ctx, request) = pending();
    let valid = prepare(&mut state, &ctx, &request);
    let (enabled, epoch) = state.publish_feature(valid.id, true, 1).unwrap();
    state.note_opened_run(valid.id);
    let committed = state.finalize(valid.id, enabled, epoch).unwrap();
    let claim = state.claim_continuation("deadline-app").unwrap();
    assert_eq!(claim.request_id, committed.request_id);
    state.advance_clock_without_sweep(Duration::from_secs(29 * 60));
    assert!(state.validate_continuation(&claim, enabled, epoch).is_ok());
    state.advance_clock_without_sweep(Duration::from_secs(2 * 60));
    assert!(state.validate_continuation(&claim, enabled, epoch).is_err());
    assert!(state.consume_continuation(&claim, enabled, epoch).is_err());
}

#[test]
fn final_permit_and_binding_reject_expiry_without_a_poll() {
    let (mut state, ctx, request) = pending();
    let valid = prepare(&mut state, &ctx, &request);
    let (enabled, epoch) = state.publish_feature(valid.id, true, 1).unwrap();
    state.note_opened_run(valid.id);
    let committed = state.finalize(valid.id, enabled, epoch).unwrap();
    let permit = state
        .begin_model_call(
            &ctx,
            "computer_observe",
            enabled,
            epoch,
            &HashSet::new(),
            ExecutionTargetClaim::default(),
        )
        .unwrap();
    assert!(state.recheck_permit(&permit, enabled, epoch).is_ok());
    assert!(state.binding_capability(&committed.binding, "observe"));
    state.advance_clock_without_sweep(Duration::from_secs(31 * 60));
    assert!(state.recheck_permit(&permit, enabled, epoch).is_err());
    assert!(state
        .recheck_binding(&committed.binding, enabled, epoch)
        .is_err());
    assert!(!state.binding_capability(&committed.binding, "observe"));
    assert!(state
        .start_preparation(&ctx, &request, 1, enabled, epoch)
        .is_err());
}

#[test]
fn full_expiry_queue_keeps_cleanup_ownership_without_execution_authority() {
    let mut state = ConsentState::new();
    // Authority-only resources; this test claims no physical cleanup evidence.
    for owner in 0..4 {
        let session = format!("expiry-owner-{owner}");
        let token = state.issue(&session, 1, &HashSet::new()).unwrap();
        state.bind_agent(&session, "synthetic-agent").unwrap();
        let BearerClass::Live(ctx) = state.classify(token.token()) else {
            panic!("owner")
        };
        for index in 0..8 {
            let result = state.submit(&ctx, json!({
                "action": "request", "purpose": "expiry queue regression", "surfaces": ["desktop"],
                "scopeIntent": format!("scope-{index}"), "capabilities": ["observe"]
            }), &HashSet::new(), 1).unwrap();
            let request = result["requestId"].as_str().unwrap();
            state.bind_synthetic(request, "fake:window:1").unwrap();
            let prep = prepare(&mut state, &ctx, request);
            state.publish_feature(prep.id, true, 1).unwrap();
            state.note_opened_run(prep.id);
            state.finalize(prep.id, true, 1).unwrap();
        }
    }
    state.advance_clock(Duration::from_secs(31 * 60), &HashSet::new());
    assert_eq!(state.expired_runs.len(), 32);
    let token = state.issue("overflow-app", 1, &HashSet::new()).unwrap();
    state.bind_agent("overflow-app", "synthetic-agent").unwrap();
    let BearerClass::Live(ctx) = state.classify(token.token()) else {
        panic!("owner")
    };
    let pending = state.submit(&ctx, json!({
        "action": "request", "purpose": "expiry overflow regression", "surfaces": ["desktop"],
        "scopeIntent": "overflow", "capabilities": ["observe"]
    }), &HashSet::new(), 1).unwrap();
    let request = pending["requestId"].as_str().unwrap();
    state.bind_synthetic(request, "fake:window:1").unwrap();
    let prep = prepare(&mut state, &ctx, request);
    state.publish_feature(prep.id, true, 1).unwrap();
    state.note_opened_run(prep.id);
    let committed = state.finalize(prep.id, true, 1).unwrap();
    let claim = state.claim_continuation("overflow-app").unwrap();
    assert!(state
        .begin_model_call(
            &ctx,
            "computer_observe",
            true,
            1,
            &HashSet::new(),
            ExecutionTargetClaim::default()
        )
        .is_ok());
    state.advance_clock_without_sweep(Duration::from_secs(31 * 60));
    assert!(state
        .begin_model_call(
            &ctx,
            "computer_observe",
            true,
            1,
            &HashSet::new(),
            ExecutionTargetClaim::default()
        )
        .is_err());
    assert!(state.validate_continuation(&claim, true, 1).is_err());
    assert_eq!(state.grant_count("overflow-app"), 0);
    assert_eq!(state.expired_runs.len(), 32);
    assert_eq!(
        state.grants.len(),
        1,
        "the unqueued resource must still have an owner"
    );
    let handed_off = state.take_expired_runs();
    assert_eq!(handed_off.len(), 32);
    state.sweep(&HashSet::new());
    assert_eq!(state.take_expired_runs(), vec![committed.run_id]);
    assert!(state.grants.is_empty());
}
