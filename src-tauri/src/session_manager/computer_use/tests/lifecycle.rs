//! Handshake and ACP incarnation cleanup regression tests.

use super::*;

#[tokio::test]
async fn handshake_stop_kills_captured_acp_even_without_computer_use_run() {
    let session = format!("handshake-noop-{}", uuid::Uuid::new_v4());
    let stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-handshake", Arc::clone(&stub.client));
    let termination = detach_handshake_acp(&manager, &session);
    assert!(manager
        .inner
        .lock()
        .as_ref()
        .is_some_and(|live| live.acp.is_none()));

    manager
        .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
        .await
        .expect("no-op Computer Use cleanup must not block process termination");
    assert!(
        !stub.client.is_alive(),
        "captured handshake ACP must be dead"
    );
    stub.shutdown().await;
}

#[tokio::test]
async fn handshake_stop_kills_acp_when_computer_use_cleanup_fails() {
    let session = format!("handshake-cleanup-error-{}", uuid::Uuid::new_v4());
    let stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-handshake-error", Arc::clone(&stub.client));
    let termination = detach_handshake_acp(&manager, &session);
    let desired = sessions::McpDesiredState {
        generation: 1,
        run_id: None,
        attempt_generation: None,
    };
    let plan = FencedComputerUseStop {
        desired,
        cleanup: None,
        fence_error: Some("injected cleanup failure".into()),
    };

    let error = manager
        .finish_fenced_computer_use_stop_task(&session, plan, Some(termination))
        .await
        .expect_err("injected cleanup failure must remain observable");
    assert!(error.contains("injected cleanup failure"));
    assert!(
        !stub.client.is_alive(),
        "cleanup failure must not keep the cancelled handshake ACP alive"
    );
    stub.shutdown().await;
}

#[tokio::test]
async fn delayed_handshake_stop_cannot_kill_reconnected_acp() {
    let session = format!("handshake-reconnect-{}", uuid::Uuid::new_v4());
    let old_stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-old", Arc::clone(&old_stub.client));
    let termination = detach_handshake_acp(&manager, &session);

    let new_stub = TcpStub::connect().await;
    *manager.inner.lock() = Some(ready_live(
        &session,
        "agent-new",
        "process-new",
        Arc::clone(&new_stub.client),
    ));
    manager
        .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
        .await
        .expect("old handshake termination must complete");

    assert!(!old_stub.client.is_alive(), "old ACP must be terminated");
    assert!(
        new_stub.client.is_alive(),
        "replacement ACP must remain alive"
    );
    assert!(manager.inner.lock().as_ref().is_some_and(|live| {
        live.process_id == "process-new"
            && live
                .acp
                .as_ref()
                .is_some_and(|acp| Arc::ptr_eq(acp, &new_stub.client))
    }));
    new_stub.shutdown().await;
    old_stub.shutdown().await;
}

#[tokio::test]
async fn handshake_stop_detaches_shared_tenant_without_killing_co_tenant() {
    let session = format!("handshake-shared-{}", uuid::Uuid::new_v4());
    let co_tenant = format!("handshake-shared-peer-{}", uuid::Uuid::new_v4());
    let stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-shared", Arc::clone(&stub.client));
    manager.background.lock().insert(
        co_tenant.clone(),
        ready_live(
            &co_tenant,
            "agent-shared-peer",
            "process-shared",
            Arc::clone(&stub.client),
        ),
    );
    let termination = detach_handshake_acp(&manager, &session);
    manager
        .finish_fenced_computer_use_stop_task(&session, noop_stop_plan(), Some(termination))
        .await
        .expect("shared handshake detach must complete");

    assert!(
        stub.client.is_alive(),
        "a live co-tenant must keep the shared ACP alive"
    );
    assert!(manager.background.lock().contains_key(&co_tenant));
    stub.shutdown().await;
}

#[tokio::test]
async fn process_exit_fences_live_background_and_parked_tenants_once() {
    let suffix = uuid::Uuid::new_v4().to_string();
    let live_id = format!("exit-live-{suffix}");
    let background_id = format!("exit-background-{suffix}");
    let parked_id = format!("exit-parked-{suffix}");
    let adapter = Arc::new(McpTestAdapter::default());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-exit-{suffix}.lease")),
            ..BrokerOptions::default()
        },
    ));
    let live_ticket = activate_test_run(&broker, &live_id, "exit-live-attempt");
    let background_ticket = activate_test_run(&broker, &background_id, "exit-background-attempt");
    let parked_ticket = activate_test_run(&broker, &parked_id, "exit-parked-attempt");
    let stub = TcpStub::connect().await;
    let manager = manager_with_live(&live_id, "agent-exit-live", Arc::clone(&stub.client));
    manager.background.lock().insert(
        background_id.clone(),
        ready_live(
            &background_id,
            "agent-exit-background",
            "process-shared",
            Arc::clone(&stub.client),
        ),
    );
    let parked_live = ready_live(
        &parked_id,
        "agent-exit-parked",
        "process-shared",
        Arc::clone(&stub.client),
    );
    manager.parked.lock().insert(
        parked_id.clone(),
        ParkedAgent {
            process_id: parked_live.process_id,
            app_session_id: parked_live.app_session_id,
            meta: parked_live.meta,
            acp: parked_live.acp.expect("parked ACP"),
            last_activity: parked_live.last_activity,
            model_id: parked_live.model_id,
            effort: parked_live.effort,
            product_mode: parked_live.product_mode,
            project_path: parked_live.project_path,
            policy: parked_live.policy,
            sandbox_profile: None,
            needs_history_bootstrap: parked_live.needs_history_bootstrap,
            backend: parked_live.backend,
        },
    );

    let plans = manager
        .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
    assert_eq!(plans.len(), 3, "every process tenant must be fenced");
    for (session, plan) in &plans {
        assert!(plan.desired.run_id.is_none(), "{session} remains absent");
        assert_eq!(
            plan.desired.generation,
            manager
                .mcp_catalog_status(session)
                .expect("transport-gone status")
                .desired_generation
        );
    }
    assert!(broker.authorized_target(&live_ticket.run_id).is_err());
    assert!(broker.authorized_target(&background_ticket.run_id).is_err());
    assert!(broker.authorized_target(&parked_ticket.run_id).is_err());

    let duplicate = manager
        .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
    assert!(
        duplicate.is_empty(),
        "duplicate exit must not create a second fence"
    );
    for (session, plan) in plans {
        manager
            .finish_process_exit_cleanup(&session, plan)
            .await
            .expect("surface cleanup after process exit");
        sessions::forget_session(&session);
    }
    stub.shutdown().await;
}

#[tokio::test]
async fn stale_process_exit_does_not_fence_replacement_session() {
    let session = format!("exit-replacement-{}", uuid::Uuid::new_v4());
    let adapter = Arc::new(McpTestAdapter::default());
    let broker = Arc::new(ComputerUseBroker::new(
        adapter,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-replacement-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    let old_ticket = activate_test_run(&broker, &session, "exit-old-attempt");
    let old_stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-exit-old", Arc::clone(&old_stub.client));
    let old_fenced = sessions::fence_revoke_checked_for_test(Arc::clone(&broker), &session);
    if let Some(cleanup) = old_fenced.cleanup {
        sessions::finish_fenced_cleanup_blocking(cleanup)
            .expect("old run cleanup before reconnect");
    }
    let new_stub = TcpStub::connect().await;
    *manager.inner.lock() = Some(ready_live(
        &session,
        "agent-exit-new",
        "process-exit-new",
        Arc::clone(&new_stub.client),
    ));
    let new_ticket = activate_test_run(&broker, &session, "exit-new-attempt");

    let plans = manager
        .fence_computer_use_for_process_exit_with("process-shared", Some(Arc::clone(&broker)));
    assert!(
        plans.is_empty(),
        "old exit must not match replacement process"
    );
    assert!(broker.authorized_target(&new_ticket.run_id).is_ok());
    assert!(broker.authorized_target(&old_ticket.run_id).is_err());

    sessions::fail_checked(&broker, &new_ticket).expect("cleanup replacement run");
    sessions::forget_session(&session);
    new_stub.shutdown().await;
    old_stub.shutdown().await;
}

#[test]
fn stale_authorization_failure_is_host_noop_for_newer_present_catalog() {
    let session = format!("stale-auth-failure-{}", uuid::Uuid::new_v4());
    let (broker, first) = desired_present(&session, "attempt-a");
    let broker = Arc::new(broker);
    sessions::complete(broker.as_ref(), &first).expect("complete first authorization");
    sessions::fail_checked(broker.as_ref(), &first).expect("retire first authorization");

    let second = sessions::begin(
        broker.as_ref(),
        &session,
        Some(&first.run_id),
        "attempt-b",
        1,
    )
    .expect("begin newer authorization");
    broker
        .authorize_target(&second.run_id, MCP_TEST_TARGET)
        .expect("authorize newer target");
    sessions::activate(broker.as_ref(), &second).expect("request newer MCP attach");
    sessions::complete(broker.as_ref(), &second).expect("complete newer authorization");

    let manager = SessionManager::new();
    let desired = sessions::mcp_desired(&session);
    let applied = McpCatalogStatus {
        desired_generation: desired.generation,
        applied_generation: Some(desired.generation),
        desired_present: true,
        pending: false,
        resource_cleanup_pending: false,
        last_error: None,
    };
    manager.set_mcp_status(&session, applied.clone());

    let plan = manager.fence_failed_computer_use_authorization(Arc::clone(&broker), &first);
    assert!(
        plan.is_noop(),
        "a stale failure must not schedule Host cleanup"
    );
    assert_eq!(manager.mcp_catalog_status(&session), Some(applied));
    assert!(sessions::is_current(&second));

    sessions::fail_checked(broker.as_ref(), &second).expect("clean up newer authorization");
    sessions::forget_session(&session);
}
