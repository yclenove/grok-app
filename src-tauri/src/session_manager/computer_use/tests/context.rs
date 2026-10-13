//! Context retuning must fence CU before sending the live ACP request.

use super::*;

struct TestHome {
    previous: Option<std::ffi::OsString>,
    path: std::path::PathBuf,
}

impl TestHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cu-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("isolated App home");
        let previous = std::env::var_os("GROK_APP_HOME");
        std::env::set_var("GROK_APP_HOME", &path);
        crate::paths::ensure_app_dirs().expect("initialize App home");
        Self { previous, path }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("GROK_APP_HOME", value),
            None => std::env::remove_var("GROK_APP_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn seed_context_catalog() {
    let settings = crate::store::AppSettings {
        session_data_mode: "independent".into(),
        ..Default::default()
    };
    crate::store::save_settings(&settings).expect("isolated settings");
    std::fs::write(
        crate::paths::agent_home_dir().join("models_cache.json"),
        json!({"models": {"grok-context-test": {"info": {
            "id": "grok-context-test", "name": "Context fixture",
            "context_window": 256000, "context_windows": [256000, 500000]
        }}}})
        .to_string(),
    )
    .expect("isolated model catalog");
}

struct RunGuard {
    adapter: Arc<McpTestAdapter>,
    session: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.adapter.set_abort_blocked(false);
        sessions::forget_session(&self.session);
    }
}

fn context_run(
    session: &str,
) -> (
    Arc<ComputerUseBroker>,
    sessions::AuthorizationTicket,
    RunGuard,
) {
    let adapter = Arc::new(McpTestAdapter::default());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: crate::paths::app_data_root().join(format!("{session}.lease")),
            ..BrokerOptions::default()
        },
    ));
    let ticket = activate_test_run(&broker, session, "context-attempt");
    (
        broker,
        ticket,
        RunGuard {
            adapter,
            session: session.into(),
        },
    )
}

fn cleanup_run(broker: &Arc<ComputerUseBroker>, session: &str) {
    let fenced = sessions::fence_revoke_checked_with_broker(Arc::clone(broker), session);
    assert!(fenced.error.is_none());
    sessions::finish_fenced_cleanup_blocking(fenced.cleanup.expect("cleanup ticket"))
        .expect("surface cleanup");
}

fn assert_context_boundary(rpc_fails: bool) {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _grants_guard = crate::computer_use::test_support::process_grants_guard();
    let _home = TestHome::new();
    seed_context_catalog();
    tauri::async_runtime::block_on(async {
        let session = format!("context-{}", uuid::Uuid::new_v4());
        let peer = format!("context-peer-{}", uuid::Uuid::new_v4());
        let (broker, ticket, guard) = context_run(&session);
        let (peer_broker, peer_ticket, _peer_guard) = context_run(&peer);
        guard.adapter.set_abort_blocked(true);
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "context-agent", Arc::clone(&stub.client));
        manager.with_session_mut(&session, |s| {
            s.model_id = Some("grok-context-test".into());
            s.meta.model_id = s.model_id.clone();
        });
        *manager.pending_context_window.lock() = Some(256000);
        let task_manager = Arc::clone(&manager);
        let task_session = session.clone();
        let task_broker = Arc::clone(&broker);
        let mut task = tokio::spawn(async move {
            task_manager
                .set_context_window_fenced(500000, Some(&task_session), |m, sid| {
                    m.fence_computer_use_stop_with_broker(task_broker, sid)
                })
                .await
        });
        let mut saw_context = false;
        let mut context_result = None;
        // Retuning and catalog removal are independent ACP requests. Either
        // wire order is allowed, but authority must already be fenced for both.
        for _ in 0..2 {
            // The production cleanup builder also discovers configured MCPs
            // via the CLI. Its cold start is outside the retune's latency
            // contract; do not reuse the pure TCP stub's 2-second deadline.
            let request = tokio::time::timeout(Duration::from_secs(15), stub.requests.recv())
                .await
                .expect("context or production catalog request")
                .expect("ACP request channel closed");
            assert!(sessions::mcp_desired(&session).run_id.is_none());
            assert_eq!(
                sessions::mcp_desired(&peer).run_id.as_deref(),
                Some(peer_ticket.run_id.as_str())
            );
            assert_eq!(
                broker.stop_state(&ticket.run_id).unwrap(),
                StopState::StopRequested
            );
            if request.method == "session/set_model" {
                assert_eq!(
                    request.params,
                    json!({
                        "sessionId": "context-agent", "modelId": "grok-context-test",
                        "_meta": {"contextWindow": 500000}
                    })
                );
                saw_context = true;
                request
                    .reply
                    .send(if rpc_fails {
                        StubReply::Error("context rejected")
                    } else {
                        StubReply::Ok
                    })
                    .unwrap();
                context_result = Some(
                    tokio::time::timeout(Duration::from_secs(2), &mut task)
                        .await
                        .expect("retune reply must not wait for catalog or blocked surface cleanup")
                        .expect("context task joined"),
                );
            } else {
                assert!(catalogs(&request).iter().all(|row| {
                    row.get("name").and_then(Value::as_str)
                        != Some(crate::computer_use::MCP_SERVER_NAME)
                }));
                request.reply.send(StubReply::Ok).unwrap();
            }
        }
        assert!(saw_context);
        let result = context_result.expect("context response received");
        assert_eq!(result.is_err(), rpc_fails);
        assert_eq!(
            *manager.pending_context_window.lock(),
            rpc_fails.then_some(256000)
        );
        guard.adapter.set_abort_blocked(false);
        tokio::time::timeout(Duration::from_secs(3), async {
            while broker.stop_state(&ticket.run_id).unwrap() != StopState::Stopped {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("surface cleanup finishes independently");
        assert_eq!(guard.adapter.aborts.load(Ordering::SeqCst), 1);
        assert_eq!(guard.adapter.releases.load(Ordering::SeqCst), 1);
        assert!(sessions::mcp_desired(&session).run_id.is_none());
        // Join the peer's explicit cleanup before dropping its isolated home.
        cleanup_run(&peer_broker, &peer);
        stub.shutdown().await;
    });
}

#[test]
fn context_retune_revokes_before_acp_without_waiting_for_surface_cleanup() {
    assert_context_boundary(false);
}

#[test]
fn failed_context_retune_does_not_restore_computer_authority() {
    assert_context_boundary(true);
}

#[test]
fn draft_and_invalid_context_choices_preserve_live_authority() {
    let _lock = crate::paths::APP_HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _grants_guard = crate::computer_use::test_support::process_grants_guard();
    let _home = TestHome::new();
    seed_context_catalog();
    tauri::async_runtime::block_on(async {
        let session = format!("context-validate-{}", uuid::Uuid::new_v4());
        let (broker, ticket, _guard) = context_run(&session);
        let mut stub = TcpStub::connect().await;
        let manager = manager_with_live(&session, "context-agent", Arc::clone(&stub.client));
        manager.with_session_mut(&session, |s| s.model_id = Some("grok-context-test".into()));
        let desired = sessions::mcp_desired(&session);
        for (tokens, sid, succeeds) in [
            (0, Some(session.as_str()), false),
            (1, Some(session.as_str()), false),
            (500000, Some("other-chat"), false),
            (500000, None, true),
            (500000, Some("  "), true),
        ] {
            let result = manager
                .set_context_window_fenced(tokens, sid, |_, _| {
                    panic!("validation and drafts must not fence a live chat")
                })
                .await;
            assert_eq!(result.is_ok(), succeeds);
            assert_eq!(sessions::mcp_desired(&session), desired);
            assert_eq!(
                broker.stop_state(&ticket.run_id).unwrap(),
                StopState::Running
            );
        }
        assert_eq!(*manager.pending_context_window.lock(), Some(500000));
        stub.assert_no_request().await;
        cleanup_run(&broker, &session);
        stub.shutdown().await;
    });
}
