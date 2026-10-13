//! Presentation cannot authorize or change an exact prepared scope.
use super::*;
use crate::adapter::{NativeScopeIdentity, SurfaceKind};
use serde_json::json;

#[test]
fn presentation_changes_do_not_change_prepared_authority_but_identity_changes_do() {
    let mut state = ConsentState::new();
    let token = state
        .issue("synthetic-presentation-app", 1, &HashSet::new())
        .unwrap();
    state
        .bind_agent("synthetic-presentation-app", "synthetic-agent")
        .unwrap();
    let BearerClass::Live(ctx) = state.classify(token.token()) else {
        panic!("current owner")
    };
    let result = state.submit(&ctx, json!({"action":"request", "purpose":"synthetic presentation", "surfaces":["desktop"], "scopeIntent":"owned fixture", "capabilities":["observe"]}), &HashSet::new(), 1).unwrap();
    let request = result["requestId"].as_str().unwrap();
    state.bind_synthetic(request, "fixture:original").unwrap();
    let CommitReady::Prepare(mut ticket) =
        state.start_preparation(&ctx, request, 1, true, 1).unwrap()
    else {
        panic!("first preparation")
    };
    ticket.members[0].display_name = Some("A different displayed title".into());
    assert!(state.preparation_matches(&ticket, true, 1));
    ticket.members[0].target_id = "fixture:foreign".into();
    assert!(!state.preparation_matches(&ticket, true, 1));
    ticket.members[0].target_id = "fixture:original".into();
    ticket.members[0].surface = SurfaceKind::ManagedBrowser;
    assert!(!state.preparation_matches(&ticket, true, 1));
    ticket.members[0].surface = SurfaceKind::Desktop;
    ticket.members[0].native = Some(NativeScopeProof {
        executor_generation: 1,
        identity: NativeScopeIdentity::Synthetic {
            target_id: "fixture:original".into(),
            instance: 1,
        },
    });
    assert!(!state.preparation_matches(&ticket, true, 1));
    ticket.members[0].native = None;
    assert!(state.preparation_matches(&ticket, true, 1));
    state.abort_preparation(ticket.id, true, 1);
}
