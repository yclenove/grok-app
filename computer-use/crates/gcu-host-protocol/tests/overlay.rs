//! Independent Stop, cancellation, control replay and persisted restart tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::{
    auth::*,
    messages::*,
    overlay::{command_allowed, safety::*},
    server::*,
    Error,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
struct Store {
    stopped: AtomicBool,
    fail: AtomicBool,
}
impl LatchStore for Store {
    fn read(&self) -> std::io::Result<bool> {
        Ok(self.stopped.load(Ordering::SeqCst))
    }
    fn write(&self, stopped: bool) -> std::io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("fixture disk failure"));
        }
        self.stopped.store(stopped, Ordering::SeqCst);
        Ok(())
    }
}
async fn pair(channel: Channel) -> (Peer, Peer, Incoming) {
    let (a, b) = tokio::io::duplex(8192);
    let host = AuthConfig::current(Role::Host, channel).unwrap();
    let mut engine = host.clone();
    engine.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = host.instance.clone();
    let ep = expected.clone();
    let task =
        tokio::spawn(async move { Peer::connect(a, &host, &expected, &secret).await.unwrap() });
    let (engine, incoming, _) = Peer::connect(b, &engine, &ep, &other).await.unwrap();
    let (host, _, _) = task.await.unwrap();
    (host, engine, incoming)
}
async fn request(incoming: &mut Incoming, method: &str) -> Request {
    loop {
        let request = incoming.recv().await.unwrap();
        if request.method == method {
            return request;
        }
    }
}

#[tokio::test]
async fn stop_blocked_ordinary_rpc_latches_and_reaches_independent_control() {
    let (rpc, _engine_rpc, _ordinary) = pair(Channel::Rpc).await;
    let busy = tokio::spawn(async move {
        rpc.call::<_, Empty>("engine.system.doctor", &Empty {}, RPC_TIMEOUT)
            .await
    });
    let (control, engine, mut incoming) = pair(Channel::Stop).await;
    let store = Arc::new(Store::default());
    let safety = Safety::new(store.clone());
    safety.attach(control);
    safety.latch();
    assert!(safety.stopped());
    assert!(!busy.is_finished());
    let stop = request(&mut incoming, "engine.session.stop").await;
    engine.respond(&stop, Ok(serde_json::json!({})));
    safety.persist().await.unwrap();
    assert!(Safety::new(store).stopped());
    busy.abort();
}

#[tokio::test]
async fn stop_native_ui_thread_without_tokio_context_still_uses_control() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let safety = Safety::new(Arc::new(Store::default()));
    safety.attach(host);
    let ui = safety.clone();
    std::thread::spawn(move || ui.latch()).join().unwrap();
    assert!(safety.stopped());
    let stop = request(&mut incoming, "engine.session.stop").await;
    engine.respond(&stop, Ok(serde_json::json!({})));
    safety.persist().await.unwrap();
}

#[tokio::test]
async fn resume_success_requires_control_ack_and_persists_explicit_user_decision() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let store = Arc::new(Store::default());
    store.stopped.store(true, Ordering::SeqCst);
    let safety = Safety::new(store.clone());
    safety.attach(host);
    let job = tokio::spawn({
        let safety = safety.clone();
        async move { safety.resume_by_user().await }
    });
    let resume = request(&mut incoming, "engine.session.resume").await;
    let body: Resume = serde_json::from_value(resume.params.clone()).unwrap();
    assert_eq!(body.sequence, 1);
    assert_ne!(body.user_event, [0; 32]);
    assert!(safety.stopped());
    engine.respond(&resume, Ok(serde_json::json!({})));
    job.await.unwrap().unwrap();
    assert!(!safety.stopped());
    assert!(!Safety::new(store).stopped());
}

#[tokio::test]
async fn resume_racing_new_stop_cannot_clear_latch_even_after_success_reply() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let safety = Safety::new(Arc::new(Store::default()));
    safety.latch();
    safety.attach(host);
    let job = tokio::spawn({
        let safety = safety.clone();
        async move { safety.resume_by_user().await }
    });
    let resume = request(&mut incoming, "engine.session.resume").await;
    safety.latch();
    engine.respond(&resume, Ok(serde_json::json!({})));
    assert!(job.await.unwrap().is_err());
    assert!(safety.stopped());
}

#[tokio::test]
async fn resume_cancelled_after_delivery_closes_control_and_keeps_stop() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let safety = Safety::new(Arc::new(Store::default()));
    safety.latch();
    safety.attach(host);
    let job = tokio::spawn({
        let safety = safety.clone();
        async move { safety.resume_by_user().await }
    });
    request(&mut incoming, "engine.session.resume").await;
    job.abort();
    assert!(job.await.is_err());
    engine.disconnected().await;
    assert!(safety.stopped());
}

#[tokio::test]
async fn resume_disk_failure_relatches_and_disconnected_resume_never_clears() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let store = Arc::new(Store::default());
    store.fail.store(true, Ordering::SeqCst);
    let safety = Safety::new(store);
    safety.latch();
    safety.attach(host);
    let job = tokio::spawn({
        let safety = safety.clone();
        async move { safety.resume_by_user().await }
    });
    let resume = request(&mut incoming, "engine.session.resume").await;
    engine.respond(&resume, Ok(serde_json::json!({})));
    assert!(matches!(job.await.unwrap(), Err(Error::Io(_))));
    assert!(safety.stopped());
    engine.close();
    assert!(safety.resume_by_user().await.is_err());
    assert!(safety.stopped());
}

#[test]
fn webview_identity_overlay_and_browser_cannot_read_media_approve_or_emit_events() {
    for method in [
        "cu_approval_decide",
        "cu_confirm_decide",
        "media_server_endpoint",
        "session_send",
        "plugin:event|emit",
        "cu_resume",
    ] {
        assert!(!command_allowed("cu-overlay", method));
        assert!(!command_allowed("cu-browser", method));
    }
    assert!(command_allowed("cu-overlay-controls", "cu_stop"));
    assert!(command_allowed("cu-overlay-controls", "cu_resume"));
    assert!(!command_allowed(
        "cu-overlay-controls",
        "cu_approval_decide"
    ));
}

#[test]
fn persisted_file_restart_and_corruption_never_clear_stop() {
    let path = std::env::temp_dir().join(format!("cu-overlay-latch-{:x}", rand::random::<u64>()));
    let store = Arc::new(FileLatch(path.clone()));
    assert!(!Safety::new(store.clone()).stopped());
    store.write(true).unwrap();
    assert!(Safety::new(store.clone()).stopped());
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(Safety::new(store).stopped());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn stalled_control_injected_deadline_closes_within_stop_budget() {
    let (host, engine, mut incoming) = pair(Channel::Stop).await;
    let safety = Safety::new(Arc::new(Store::default()));
    safety.attach(host);
    tokio::time::pause();
    safety.latch();
    assert!(safety.stopped());
    let stop = request(&mut incoming, "engine.session.stop").await;
    assert!(stop.id.is_some());
    tokio::time::advance(std::time::Duration::from_millis(501)).await;
    engine.disconnected().await;
    assert!(!safety.connected());
    assert!(safety.stopped());
}

#[tokio::test]
async fn overlay_show_then_hide_is_consumed_in_wire_order_before_next_request() {
    use gcu_host_protocol::overlay::{Desk, Phase};
    struct Ordered(std::sync::Mutex<Desk>);
    impl HostCallbacks for Ordered {
        fn notification(&self, request: &Request) -> Option<Result<serde_json::Value, RpcError>> {
            let mut desk = self.0.lock().unwrap();
            match request.method.as_str() {
                "host.overlay.show" => desk
                    .show(serde_json::from_value(request.params.clone()).unwrap())
                    .unwrap(),
                "host.overlay.hide" => desk.hide(
                    &serde_json::from_value::<SessionId>(request.params.clone())
                        .unwrap()
                        .session_id,
                ),
                _ => return None,
            }
            Some(Ok(serde_json::json!({})))
        }
        fn handle(&self, _: Request) -> CallbackFuture<'_> {
            Box::pin(async move {
                Ok(serde_json::json!(
                    self.0.lock().unwrap().snapshot().phase == Phase::Hidden
                ))
            })
        }
        fn disconnected(&self) {}
    }
    let (a, b) = tokio::io::duplex(8192);
    let host = AuthConfig::current(Role::Host, Channel::Rpc).unwrap();
    let mut engine = host.clone();
    engine.role = Role::Engine;
    let secret = Secret::random();
    let other = secret.clone();
    let expected = host.instance.clone();
    let ep = expected.clone();
    let job = tokio::spawn(async move {
        HostServer::accept(a, &host, &expected, &secret)
            .await
            .unwrap()
    });
    let (engine, _, _) = Peer::connect(b, &engine, &ep, &other).await.unwrap();
    let server = job.await.unwrap();
    let runner =
        tokio::spawn(server.run(Arc::new(Ordered(std::sync::Mutex::new(Desk::default())))));
    for _ in 0..10 {
        engine
            .notify(
                "host.overlay.show",
                &OverlayShow {
                    session_id: "s".into(),
                    window_id: "w".into(),
                    title: "fixture".into(),
                    scale: 1.,
                    x: 0.,
                    y: 0.,
                    frame: gcu_protocol::Rect {
                        x: 0.,
                        y: 0.,
                        w: 800.,
                        h: 600.,
                    },
                    kind: CursorKind::Pointer,
                },
            )
            .unwrap();
        engine
            .notify(
                "host.overlay.hide",
                &SessionId {
                    session_id: "s".into(),
                },
            )
            .unwrap();
        let hidden: bool = engine
            .call("host.policy.load", &Empty {}, RPC_TIMEOUT)
            .await
            .unwrap();
        assert!(hidden);
    }
    engine.close();
    runner.await.unwrap();
}
