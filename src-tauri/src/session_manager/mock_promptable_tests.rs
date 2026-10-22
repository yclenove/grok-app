#[test]
fn mock_promptable_accepts_only_connected_states() {
    let mut s = sample_live_for_empty_run("", "", 0, "agent");
    assert!(SessionManager::has_connected_backend(&s));
    s.fsm.await_permission().unwrap();
    assert!(SessionManager::has_connected_backend(&s));
    s.fsm.end_stream().unwrap();
    assert!(SessionManager::has_connected_backend(&s));
    s.fsm.soft_disconnect();
    assert!(!SessionManager::has_connected_backend(&s));
    s.fsm.start_connect().unwrap();
    assert!(!SessionManager::has_connected_backend(&s));
    s.fsm = SessionFsm::new();
    assert!(!SessionManager::has_connected_backend(&s));
}

#[test]
fn mock_promptable_never_grants_a_real_backend_without_a_child() {
    let mut s = sample_live_for_empty_run("", "", 0, "agent");
    for backend in ["acp", "grok", "custom", "", "MOCK_ACP"] {
        s.backend = backend.into();
        assert!(!SessionManager::has_connected_backend(&s), "{backend}");
    }
}

#[test]
fn mock_promptable_checks_exact_live_target() {
    let mgr = SessionManager::new();
    *mgr.inner.lock() = Some(sample_live_for_empty_run("", "", 0, "agent"));
    assert!(mgr.has_promptable_session("session-1"));
    assert!(!mgr.has_promptable_session("other-session"));
    assert!(!mgr.has_promptable_session(""));
}

#[test]
fn mock_promptable_checks_background_without_stealing_focus() {
    let mgr = SessionManager::new();
    let live = sample_live_for_empty_run("", "", 0, "agent");
    let mut background = sample_live_for_empty_run("", "", 0, "agent");
    background.app_session_id = "background-mock".into();
    *mgr.inner.lock() = Some(live);
    mgr.background
        .lock()
        .insert("background-mock".into(), background);
    assert!(mgr.has_promptable_session("background-mock"));
    assert_eq!(mgr.inner.lock().as_ref().unwrap().app_session_id, "session-1");
    assert!(!mgr.has_promptable_session("unknown"));
}

#[test]
fn mock_promptable_demotes_busy_stream_without_dropping_it() {
    let mgr = SessionManager::new();
    let mut live = sample_live_for_empty_run("partial", "", 0, "agent");
    live.prompt_in_flight = true;
    *mgr.inner.lock() = Some(live);
    mgr.try_park_live().unwrap();
    assert!(mgr.inner.lock().is_none());
    assert!(mgr.has_promptable_session("session-1"));
    let background = mgr.background.lock();
    let saved = background.get("session-1").unwrap();
    assert_eq!(saved.stream_buf, "partial");
    assert!(saved.prompt_in_flight);
}

#[test]
fn mock_promptable_rejects_disconnected_background() {
    let mgr = SessionManager::new();
    let mut background = sample_live_for_empty_run("", "", 0, "agent");
    background.fsm.soft_disconnect();
    mgr.background.lock().insert("session-1".into(), background);
    assert!(!mgr.has_promptable_session("session-1"));
}
