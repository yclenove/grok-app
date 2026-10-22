#[test]
fn stale_prompt_waiter_cannot_clear_new_live_or_background_turn() {
    for background in [false, true] {
        let mgr = SessionManager::new();
        let mut session = bare_live_session("ownership-synthetic", "process-synthetic");
        session.active_turn_id = Some("new-turn".into());
        session.prompt_in_flight = true;
        session.streaming_message_id = Some("new-message".into());
        session.fsm.begin_stream().unwrap();
        if background {
            mgr.background
                .lock()
                .insert(session.app_session_id.clone(), session);
        } else {
            *mgr.inner.lock() = Some(session);
        }
        let stale = mgr.with_turn_mut("ownership-synthetic", "old-turn", |s| {
            s.prompt_in_flight = false;
            s.fsm.end_stream().unwrap();
        });
        assert!(stale.is_none());
        mgr.with_session_mut("ownership-synthetic", |s| {
            assert!(s.prompt_in_flight);
            assert_eq!(s.fsm.state(), SessionState::Streaming);
            assert_eq!(s.streaming_message_id.as_deref(), Some("new-message"));
        })
        .unwrap();
        assert!(mgr
            .with_turn_mut("ownership-synthetic", "new-turn", |s| {
                s.prompt_in_flight = false;
                s.fsm.end_stream().unwrap();
            })
            .is_some());
    }
}

#[test]
fn finished_or_absent_turn_does_not_accept_a_late_prompt_waiter() {
    let mgr = SessionManager::new();
    *mgr.inner.lock() = Some(bare_live_session("finished-synthetic", "process-synthetic"));
    assert!(mgr
        .with_turn_mut("finished-synthetic", "old-turn", |_| panic!(
            "finished turn"
        ))
        .is_none());
    assert!(mgr
        .with_turn_mut("absent-synthetic", "old-turn", |_| panic!("missing turn"))
        .is_none());
}
