//! Native restart regression: completed mock turns must not leave active leases.

use crate::mock_acp::StreamChunk;
use crate::session_fsm::SessionState;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::{store, turn_interrupt, turn_lease};

use super::{routing_tests::bare_live_session, SessionManager};

fn setup(background: bool) -> (IsolatedAppHome, SessionManager, String) {
    let home = IsolatedAppHome::acquire("grok-mock-completion");
    let sid = "mock-completion".to_string();
    let mgr = SessionManager::new();
    let mut s = bare_live_session(&sid, "mock-process");
    s.meta =
        store::create_session_with_id(Some(sid.clone()), None, Some("Mock test".into()), false)
            .unwrap();
    store::append_message(
        &sid,
        store::ChatMessageStored {
            id: "user-1".into(),
            role: "user".into(),
            content: "Synthetic prompt".into(),
            thought: None,
            created_at: chrono::Utc::now(),
            is_error: false,
            attachments: None,
            marker: None,
        },
    )
    .unwrap();
    s.fsm.begin_stream().unwrap();
    s.active_turn_id = Some("turn-1".into());
    s.streaming_message_id = Some("message-1".into());
    s.stream_message_id_locked = true;
    s.prompt_in_flight = true;
    turn_lease::begin_active(&sid, None, Some("turn-1"));
    if background {
        *mgr.inner.lock() = Some(bare_live_session("other-chat", "other-process"));
        mgr.background.lock().insert(sid.clone(), s);
    } else {
        *mgr.inner.lock() = Some(s);
    }
    (home, mgr, sid)
}

fn chunk(sid: &str, text: &str, done: bool) -> StreamChunk {
    StreamChunk {
        session_id: sid.into(),
        message_id: "message-1".into(),
        text: text.into(),
        done,
    }
}

fn apply(mgr: &SessionManager, sid: &str, text: &str, done: bool) {
    let update = mgr
        .prepare_mock_stream_chunk(sid, "turn-1", &chunk(sid, text, done))
        .expect("current mock chunk accepted");
    if let Some(journal) = update.journal {
        SessionManager::commit_stream_journal_flush(journal);
    }
}

#[test]
fn mock_chunks_use_stream_coalescing_and_terminal_fence() {
    let (_home, mgr, sid) = setup(false);
    let first = mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &chunk(&sid, "First ", false))
        .expect("first mock chunk accepted");
    assert!(first.emits.is_empty());
    assert!(first.schedule.is_some());
    if let Some(journal) = first.journal {
        SessionManager::commit_stream_journal_flush(journal);
    }

    let last = mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &chunk(&sid, "answer", true))
        .expect("terminal mock chunk accepted");
    assert!(last.schedule.is_none());
    assert_eq!(last.emits.len(), 1);
    assert_eq!(last.emits[0].text, "First answer");
    assert!(last.emits[0].done);
    assert!(last.emits[0].host_finalized);
    if let Some(journal) = last.journal {
        SessionManager::commit_stream_journal_flush(journal);
    }
}

fn assert_clean_completion(background: bool) {
    let (_home, mgr, sid) = setup(background);
    apply(&mgr, &sid, "First ", false);
    assert_eq!(
        turn_lease::read_lease(&sid).unwrap().status,
        turn_lease::LeaseStatus::Active
    );
    apply(&mgr, &sid, "answer", true);
    mgr.with_session_mut(&sid, |s| {
        assert_eq!(s.fsm.state(), SessionState::Ready);
        assert!(!s.prompt_in_flight);
        assert!(s.active_turn_id.is_none());
        assert!(s.streaming_message_id.is_none());
        assert!(!s.stream_message_id_locked);
    })
    .unwrap();
    let messages = store::load_messages(&sid);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].content, "First answer");
    assert!(
        turn_lease::read_lease(&sid).is_none(),
        "successful mock turn left an active lease"
    );
    let before = std::fs::read(crate::paths::session_dir(&sid).join("messages.json")).unwrap();
    assert!(turn_interrupt::heal_interrupted_turn(&sid).is_none());
    assert_eq!(
        std::fs::read(crate::paths::session_dir(&sid).join("messages.json")).unwrap(),
        before
    );
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &chunk(&sid, "duplicate", true))
        .is_none());
    if background {
        assert_eq!(
            mgr.inner.lock().as_ref().unwrap().app_session_id,
            "other-chat"
        );
    }
}

#[test]
fn completed_live_mock_turn_does_not_heal_as_host_exit() {
    assert_clean_completion(false);
}

#[test]
fn completed_background_mock_turn_does_not_heal_as_host_exit() {
    assert_clean_completion(true);
}

#[test]
fn unfinished_mock_turn_remains_recoverable_after_host_exit() {
    let (_home, mgr, sid) = setup(false);
    apply(&mgr, &sid, "Partial answer", false);
    drop(mgr);
    assert!(turn_interrupt::heal_interrupted_turn(&sid).is_some());
    assert!(turn_interrupt::heal_interrupted_turn(&sid).is_none());
    let messages = store::load_messages(&sid);
    assert_eq!(messages.last().unwrap().content, "turn_cancelled|host_exit");
    assert_eq!(
        turn_lease::read_lease(&sid).unwrap().status,
        turn_lease::LeaseStatus::Interrupted
    );
}

#[test]
fn stopped_mock_callback_cannot_complete_or_clear_a_replacement_turn() {
    for background in [false, true] {
        let (_home, mgr, sid) = setup(background);
        let boundary = mgr
            .with_session_mut(&sid, |s| {
                let boundary = SessionManager::prepare_journal_turn_cancelled(s, "user_stop");
                s.fsm.end_stream().unwrap();
                s.active_turn_id = None;
                s.streaming_message_id = None;
                s.prompt_in_flight = false;
                boundary
            })
            .unwrap();
        if let Some(boundary) = boundary {
            SessionManager::commit_turn_boundary_persist(None, boundary);
        }
        assert!(mgr
            .prepare_mock_stream_chunk(&sid, "turn-1", &chunk(&sid, "", true))
            .is_none());
        mgr.with_session_mut(&sid, |s| {
            s.fsm.begin_stream().unwrap();
            s.active_turn_id = Some("turn-2".into());
            s.streaming_message_id = Some("message-2".into());
            s.prompt_in_flight = true;
            s.stream_buf = "new answer".into();
            turn_lease::begin_active(&sid, None, Some("turn-2"));
        });
        let lease = turn_lease::read_lease(&sid).unwrap();
        for done in [false, true] {
            assert!(mgr
                .prepare_mock_stream_chunk(&sid, "turn-1", &chunk(&sid, "late", done))
                .is_none());
        }
        assert_eq!(turn_lease::read_lease(&sid).unwrap(), lease);
        mgr.with_session_mut(&sid, |s| {
            assert_eq!(s.stream_buf, "new answer");
            assert!(s.prompt_in_flight);
            assert_eq!(s.fsm.state(), SessionState::Streaming);
            assert_eq!(s.active_turn_id.as_deref(), Some("turn-2"));
        });
        assert_eq!(
            store::load_messages(&sid).last().unwrap().content,
            "turn_cancelled|user_stop"
        );
    }
}

#[test]
fn mock_chunk_requires_matching_session_message_backend_and_state() {
    let (_home, mgr, sid) = setup(false);
    let lease = turn_lease::read_lease(&sid).unwrap();
    let mut foreign = chunk("other-chat", "wrong session", true);
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &foreign)
        .is_none());
    foreign.session_id = sid.clone();
    foreign.message_id = "wrong-message".into();
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &foreign)
        .is_none());
    let valid = chunk(&sid, "wrong backend or state", true);
    mgr.with_session_mut(&sid, |s| s.backend = "acp".into());
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &valid)
        .is_none());
    mgr.with_session_mut(&sid, |s| {
        s.backend = "mock_acp".into();
        s.prompt_in_flight = false;
    });
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &valid)
        .is_none());
    mgr.with_session_mut(&sid, |s| {
        s.prompt_in_flight = true;
        s.fsm.end_stream().unwrap();
    });
    assert!(mgr
        .prepare_mock_stream_chunk(&sid, "turn-1", &valid)
        .is_none());
    assert_eq!(turn_lease::read_lease(&sid).unwrap(), lease);
    assert_eq!(store::load_messages(&sid).len(), 1);
}
