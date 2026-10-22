use super::{
    capability::{ProbeEvidence, REQUIRED_IMPORT_TOOLS},
    continuation::*,
    errors::ImportError,
    hash::sha256_bytes,
    mcp_stdio::SessionMcpPolicy,
    test_home::IsolatedAppHome,
};

fn prepared(full: bool) -> String {
    let sid = uuid::Uuid::new_v4().to_string();
    write_sidecar(
        &sid,
        &ContinuationSidecar {
            import_id: format!("imp:{}", uuid::Uuid::new_v4()),
            revision_id: "synthetic-revision".into(),
            branch_leaf: "synthetic-leaf".into(),
            request_id: format!("request-{sid}"),
            handoff_hash: "synthetic-draft".into(),
            runtime_capability: if full { "local-full" } else { "fragment-only" }.into(),
            grant_token: "synthetic-grant".into(),
            ..Default::default()
        },
    )
    .unwrap();
    write_draft_text(&sid, "first line\nsecond line").unwrap();
    if full {
        record_advertised_tools(
            &sid,
            REQUIRED_IMPORT_TOOLS
                .iter()
                .map(|name| name.to_string())
                .collect(),
            ProbeEvidence::AgentAdvertised,
        )
        .unwrap();
    }
    sid
}

fn first_send(sid: &str) -> (String, String) {
    let ContinuationFirstSend::Handoff {
        prompt,
        handoff_text_hash,
    } = apply_continuation_first_send(sid, "continue", SessionMcpPolicy::default()).unwrap()
    else {
        panic!("expected handoff")
    };
    (prompt, handoff_text_hash)
}

fn claim(
    sid: &str,
    prompt: &str,
    digest: &str,
    policy: SessionMcpPolicy,
) -> Result<DispatchState, ImportError> {
    mark_dispatching_checked(
        sid,
        prompt,
        Some("synthetic-agent"),
        digest,
        policy,
        &uuid::Uuid::new_v4().to_string(),
    )
}

#[test]
fn edited_handoff_between_preflight_and_claim_is_not_sent() {
    let _home = IsolatedAppHome::acquire("claim-edited");
    let sid = prepared(false);
    let (prompt, digest) = first_send(&sid);
    write_draft_text(&sid, "replacement history").unwrap();
    assert!(claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
}

#[test]
fn shortened_handoff_is_not_accepted_as_a_matching_prefix() {
    let _home = IsolatedAppHome::acquire("claim-shortened");
    let sid = prepared(false);
    let (prompt, digest) = first_send(&sid);
    write_draft_text(&sid, "first line").unwrap();
    assert!(claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_err());
}

#[test]
fn revoked_tool_exposure_between_preflight_and_claim_blocks_full_mode() {
    let _home = IsolatedAppHome::acquire("claim-tools");
    let sid = prepared(true);
    let (prompt, digest) = first_send(&sid);
    record_advertised_tools(&sid, Vec::new(), ProbeEvidence::AgentAdvertised).unwrap();
    assert!(claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_err());
    assert_eq!(
        read_sidecar_strict(&sid).unwrap().unwrap().state,
        DispatchState::Prepared
    );
}

#[test]
fn unchanged_handoff_claims_once_and_blocks_later_edits() {
    let _home = IsolatedAppHome::acquire("claim-unchanged");
    let sid = prepared(false);
    let (prompt, digest) = first_send(&sid);
    claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).unwrap();
    let side = read_sidecar_strict(&sid).unwrap().unwrap();
    assert_eq!(side.state, DispatchState::Dispatching);
    assert_eq!(
        side.prompt_hash.as_deref(),
        Some(sha256_bytes(prompt.as_bytes()).as_str())
    );
    assert!(write_draft_text(&sid, "late edit").is_err());
    assert!(claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_err());
}

#[test]
fn rejected_stale_text_can_be_reviewed_and_sent_with_a_fresh_preflight() {
    let _home = IsolatedAppHome::acquire("claim-review-retry");
    let sid = prepared(false);
    let (old_prompt, old_digest) = first_send(&sid);
    write_draft_text(&sid, "new selected history").unwrap();
    let error = claim(&sid, &old_prompt, &old_digest, SessionMcpPolicy::default()).unwrap_err();
    assert_eq!(error.code, super::errors::ErrorCode::HandoffChanged);
    assert_eq!(error.retry, "retry");
    let (prompt, digest) = first_send(&sid);
    assert!(prompt.starts_with("new selected history\n"));
    claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).unwrap();
}

#[test]
fn handoff_removed_after_preflight_cannot_claim_dispatch() {
    let _home = IsolatedAppHome::acquire("claim-missing");
    let sid = prepared(false);
    let (prompt, digest) = first_send(&sid);
    std::fs::remove_file(sidecar_path(&sid).with_file_name("handoff_draft.txt")).unwrap();
    assert!(claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_err());
    assert!(read_sidecar_strict(&sid)
        .unwrap()
        .unwrap()
        .dispatch_id
        .is_none());
}

#[test]
fn concurrent_edit_and_claim_cannot_both_succeed() {
    let _home = IsolatedAppHome::acquire("claim-edit-race");
    for _ in 0..8 {
        let sid = prepared(false);
        let (prompt, digest) = first_send(&sid);
        let barrier = std::sync::Barrier::new(3);
        let (claimed, edited) = std::thread::scope(|scope| {
            let claimer = scope.spawn(|| {
                barrier.wait();
                claim(&sid, &prompt, &digest, SessionMcpPolicy::default()).is_ok()
            });
            let editor = scope.spawn(|| {
                barrier.wait();
                write_draft_text(&sid, "concurrent replacement").is_ok()
            });
            barrier.wait();
            (claimer.join().unwrap(), editor.join().unwrap())
        });
        assert_ne!(claimed, edited, "exactly one operation owns the handoff");
        let side = read_sidecar_strict(&sid).unwrap().unwrap();
        assert_eq!(
            side.state,
            if claimed {
                DispatchState::Dispatching
            } else {
                DispatchState::Prepared
            }
        );
    }
}
