//! Catalog reconciliation, retry, and redaction regression tests.

use super::*;

#[tokio::test]
async fn stop_during_mcp_update_converges_to_absent() {
    let session = format!("stop-during-update-{}", uuid::Uuid::new_v4());
    let (broker, ticket) = desired_present(&session, "attempt-a");
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-a", Arc::clone(&stub.client));
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
    let reconcile = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let first = stub.next().await;
    assert_catalog(&first, "base-1", 1);
    sessions::fail_checked(&broker, &ticket).expect("revoke while update is blocked");
    first.reply.send(StubReply::Ok).expect("reply first attach");
    let second = stub.next().await;
    assert_catalog(&second, "base-1", 0);
    second.reply.send(StubReply::Ok).expect("reply detach");
    reconcile.await.expect("join reconcile").expect("converge");
    let status = manager
        .mcp_catalog_status(&session)
        .expect("catalog status");
    assert!(!status.pending);
    assert!(!status.desired_present);
    assert_eq!(status.applied_generation, Some(status.desired_generation));
    stub.shutdown().await;
}

#[tokio::test]
async fn applied_but_timed_out_update_remains_pending_then_retries_idempotently() {
    let session = format!("timeout-applied-{}", uuid::Uuid::new_v4());
    let (_broker, _ticket) = desired_present(&session, "attempt-timeout");
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-timeout", Arc::clone(&stub.client));
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_millis(80));
    let first_run = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let first = stub.next().await;
    assert_catalog(&first, "base-1", 1);
    let result = first_run.await.expect("join timeout reconcile");
    assert!(result.is_err(), "ambiguous timeout must not claim applied");
    let status = manager
        .mcp_catalog_status(&session)
        .expect("timeout status");
    assert!(status.pending);
    assert!(status
        .last_error
        .as_deref()
        .is_some_and(|e| e.contains("timed out")));

    // The server may have applied the full replacement before losing its
    // reply. A late success is ignored; an explicit retry sends the same
    // desired replacement and is therefore safe and idempotent.
    first.reply.send(StubReply::Ok).expect("late ACP response");
    tokio::task::yield_now().await;
    let retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let second = stub.next().await;
    assert_catalog(&second, "base-1", 1);
    second.reply.send(StubReply::Ok).expect("retry response");
    retry.await.expect("join retry").expect("retry converges");
    assert!(!manager.mcp_catalog_status(&session).unwrap().pending);
    stub.shutdown().await;
}

#[tokio::test]
async fn failed_detach_is_cleanup_pending_until_retry_succeeds() {
    let session = format!("detach-retry-{}", uuid::Uuid::new_v4());
    let (broker, ticket) = desired_present(&session, "attempt-detach");
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-detach", Arc::clone(&stub.client));
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));

    let attach = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let attached = stub.next().await;
    assert_catalog(&attached, "base-1", 1);
    attached.reply.send(StubReply::Ok).unwrap();
    attach.await.unwrap().unwrap();

    sessions::fail_checked(&broker, &ticket).expect("revoke desired state");
    let detach = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let failed = stub.next().await;
    assert_catalog(&failed, "base-1", 0);
    failed
        .reply
        .send(StubReply::Error("injected detach failure"))
        .unwrap();
    assert!(detach.await.unwrap().is_err());
    let failed_status = manager.mcp_catalog_status(&session).unwrap();
    assert!(failed_status.pending && !failed_status.desired_present);

    let retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let recovered = stub.next().await;
    assert_catalog(&recovered, "base-1", 0);
    recovered.reply.send(StubReply::Ok).unwrap();
    retry.await.unwrap().unwrap();
    let recovered_status = manager.mcp_catalog_status(&session).unwrap();
    assert!(!recovered_status.pending && !recovered_status.desired_present);
    stub.shutdown().await;
}

#[tokio::test]
async fn resource_cleanup_retry_is_generation_bound_and_releases_once() {
    let session = format!("resource-cleanup-{}", uuid::Uuid::new_v4());
    let adapter = Arc::new(McpTestAdapter::default());
    adapter.fail_next_aborts(1);
    let broker = Arc::new(ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "cu-resource-cleanup-{}.lease",
                uuid::Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    ));
    let ticket = sessions::begin(
        broker.as_ref(),
        &session,
        None,
        "resource-cleanup-attempt",
        1,
    )
    .expect("begin cleanup fixture");
    broker
        .authorize_target(&ticket.run_id, MCP_TEST_TARGET)
        .expect("authorize cleanup fixture");
    sessions::activate(broker.as_ref(), &ticket).expect("publish desired-present");
    sessions::complete(broker.as_ref(), &ticket).expect("complete authorization");

    let fenced = sessions::fence_revoke_checked_for_test(Arc::clone(&broker), &session);
    assert_eq!(fenced.error, None);
    let cleanup = fenced.cleanup.expect("generation-bound cleanup hand-off");
    let desired = sessions::mcp_desired(&session);
    assert!(desired.run_id.is_none());
    let manager = Arc::new(SessionManager::new());
    manager.record_fenced_cleanup(&session, &desired, true);
    let plan = FencedComputerUseStop {
        desired: desired.clone(),
        cleanup: Some(cleanup),
        fence_error: None,
    };

    let error = manager
        .finish_fenced_computer_use_stop(&session, plan)
        .await
        .expect_err("first resource cleanup must fail");
    assert!(error.contains("surface cancellation"), "{error}");
    let failed = manager.mcp_catalog_status(&session).unwrap();
    assert!(!failed.pending);
    assert!(failed.resource_cleanup_pending);
    assert_eq!(failed.applied_generation, Some(desired.generation));
    assert_eq!(
        failed.last_error.as_deref(),
        Some("Computer Use surface cleanup failed")
    );
    assert_eq!(adapter.aborts.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.releases.load(Ordering::SeqCst), 0);

    let retry_a = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        tokio::spawn(async move { manager.retry_computer_use_cleanup(&session).await })
    };
    let retry_b = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        tokio::spawn(async move { manager.retry_computer_use_cleanup(&session).await })
    };
    retry_a
        .await
        .expect("join first retry")
        .expect("first same-generation resource retry converges");
    retry_b
        .await
        .expect("join duplicate retry")
        .expect("duplicate same-generation retry is idempotent");
    let settled = manager.mcp_catalog_status(&session).unwrap();
    assert!(!settled.pending);
    assert!(!settled.resource_cleanup_pending);
    assert_eq!(settled.applied_generation, Some(desired.generation));
    assert_eq!(sessions::mcp_desired(&session), desired);
    assert_eq!(adapter.aborts.load(Ordering::SeqCst), 2);
    assert_eq!(adapter.releases.load(Ordering::SeqCst), 1);

    manager
        .retry_computer_use_cleanup(&session)
        .await
        .expect("settled retry is idempotent");
    manager.record_fenced_cleanup(&session, &desired, false);
    assert!(
        !manager
            .mcp_catalog_status(&session)
            .unwrap()
            .resource_cleanup_pending
    );
    assert_eq!(adapter.aborts.load(Ordering::SeqCst), 2);
    assert_eq!(adapter.releases.load(Ordering::SeqCst), 1);
    sessions::forget_session(&session);
}

#[tokio::test]
async fn cleanup_retry_converges_pending_absent_and_repeated_calls_are_idempotent() {
    let session = format!("cleanup-command-{}", uuid::Uuid::new_v4());
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-cleanup", Arc::clone(&stub.client));
    let desired = sessions::mcp_desired(&session);
    manager.set_mcp_status(
        &session,
        McpCatalogStatus {
            desired_generation: desired.generation,
            applied_generation: None,
            desired_present: false,
            pending: true,
            resource_cleanup_pending: false,
            last_error: Some("Computer Use MCP catalog update failed".into()),
        },
    );
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
    let retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            manager
                .retry_computer_use_cleanup_with(&session, deps)
                .await
        })
    };
    let request = stub.next().await;
    assert_catalog(&request, "base-1", 0);
    request.reply.send(StubReply::Ok).unwrap();
    retry.await.unwrap().unwrap();

    let settled = manager.mcp_catalog_status(&session).unwrap();
    assert!(!settled.pending && !settled.desired_present);
    assert_eq!(settled.applied_generation, Some(desired.generation));
    manager
        .retry_computer_use_cleanup_with(&session, deps)
        .await
        .expect("a repeated cleanup retry is an idempotent acknowledgement");
    stub.assert_no_request().await;
    stub.shutdown().await;
}

#[tokio::test]
async fn cleanup_retry_failure_stays_pending_redacted_and_retryable() {
    let session = format!("cleanup-failure-{}", uuid::Uuid::new_v4());
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-cleanup-failure", Arc::clone(&stub.client));
    let desired = sessions::mcp_desired(&session);
    manager.set_mcp_status(
        &session,
        McpCatalogStatus {
            desired_generation: desired.generation,
            applied_generation: None,
            desired_present: false,
            pending: true,
            resource_cleanup_pending: false,
            last_error: Some("Computer Use MCP catalog update failed".into()),
        },
    );
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));

    let failed_retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            manager
                .retry_computer_use_cleanup_with(&session, deps)
                .await
        })
    };
    let failed = stub.next().await;
    assert_catalog(&failed, "base-1", 0);
    failed
        .reply
        .send(StubReply::Error(
            "injected raw ACP failure Authorization: Bearer TOPSECRET",
        ))
        .unwrap();
    let error = failed_retry.await.unwrap().unwrap_err();
    assert_eq!(error, "Computer Use MCP catalog update failed");
    let failed_status = manager.mcp_catalog_status(&session).unwrap();
    assert!(failed_status.pending);
    assert!(!failed_status.desired_present);
    assert_eq!(
        failed_status.last_error.as_deref(),
        Some("Computer Use MCP catalog update failed")
    );
    assert!(!format!("{failed_status:?}").contains("TOPSECRET"));

    let recovered_retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            manager
                .retry_computer_use_cleanup_with(&session, deps)
                .await
        })
    };
    let recovered = stub.next().await;
    assert_catalog(&recovered, "base-1", 0);
    recovered.reply.send(StubReply::Ok).unwrap();
    recovered_retry.await.unwrap().unwrap();
    let settled = manager.mcp_catalog_status(&session).unwrap();
    assert!(!settled.pending);
    assert!(!settled.desired_present);
    assert_eq!(settled.applied_generation, Some(desired.generation));
    stub.shutdown().await;
}

#[tokio::test]
async fn cleanup_retry_rejects_desired_present_without_sending_absent() {
    let session = format!("cleanup-present-{}", uuid::Uuid::new_v4());
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-cleanup-present", Arc::clone(&stub.client));
    let (broker, ticket) = desired_present(&session, "attempt-present");
    let desired = sessions::mcp_desired(&session);
    manager.set_mcp_status(
        &session,
        McpCatalogStatus {
            desired_generation: desired.generation,
            applied_generation: None,
            desired_present: true,
            pending: true,
            resource_cleanup_pending: false,
            last_error: None,
        },
    );

    let error = manager
        .retry_computer_use_cleanup_with(
            &session,
            deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1)),
        )
        .await
        .expect_err("cleanup retry must not follow desired-present state");
    assert!(error.contains("only available after Stop"), "{error}");
    stub.assert_no_request().await;
    sessions::fail_checked(&broker, &ticket).expect("clean up desired-present grant");
    stub.shutdown().await;
}

#[tokio::test]
async fn cleanup_retry_never_follows_a_newer_desired_present_state() {
    let session = format!("cleanup-superseded-{}", uuid::Uuid::new_v4());
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-superseded", Arc::clone(&stub.client));
    let desired = sessions::mcp_desired(&session);
    manager.set_mcp_status(
        &session,
        McpCatalogStatus {
            desired_generation: desired.generation,
            applied_generation: None,
            desired_present: false,
            pending: true,
            resource_cleanup_pending: false,
            last_error: Some("Computer Use MCP catalog update failed".into()),
        },
    );
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
    let retry = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move {
            manager
                .retry_computer_use_cleanup_with(&session, deps)
                .await
        })
    };
    let absent = stub.next().await;
    assert_catalog(&absent, "base-1", 0);
    let (broker, ticket) = desired_present(&session, "attempt-after-retry");
    absent.reply.send(StubReply::Ok).unwrap();

    let error = retry.await.unwrap().unwrap_err();
    assert!(error.contains("superseded"), "{error}");
    stub.assert_no_request().await;
    sessions::fail_checked(&broker, &ticket).expect("clean up newer grant");
    stub.shutdown().await;
}

#[tokio::test]
async fn late_old_detach_cannot_overwrite_new_attach() {
    let session = format!("detach-then-attach-{}", uuid::Uuid::new_v4());
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-race", Arc::clone(&stub.client));
    let deps = deps(Arc::new(AtomicU64::new(1)), Duration::from_secs(1));
    let old_detach = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let first = stub.next().await;
    assert_catalog(&first, "base-1", 0);
    let (_broker, ticket) = desired_present(&session, "attempt-new");
    first.reply.send(StubReply::Ok).unwrap();
    let second = stub.next().await;
    assert_catalog(&second, "base-1", 1);
    assert!(catalogs(&second).iter().any(|row| {
        row.get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|env| env.get("value") == Some(&json!(ticket.run_id)))
    }));
    second.reply.send(StubReply::Ok).unwrap();
    old_detach.await.unwrap().unwrap();
    let status = manager.mcp_catalog_status(&session).unwrap();
    assert!(!status.pending && status.desired_present);
    stub.shutdown().await;
}

#[tokio::test]
async fn base_extension_revision_race_preserves_latest_base_and_cu_entry() {
    let session = format!("base-race-{}", uuid::Uuid::new_v4());
    let (_broker, _ticket) = desired_present(&session, "attempt-base");
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live(&session, "agent-base", Arc::clone(&stub.client));
    let base_version = Arc::new(AtomicU64::new(1));
    let deps = deps(Arc::clone(&base_version), Duration::from_secs(1));
    let reconcile = {
        let manager = Arc::clone(&manager);
        let session = session.clone();
        let deps = deps.clone();
        tokio::spawn(async move { manager.reconcile_session_mcp_with(&session, deps).await })
    };
    let stale = stub.next().await;
    assert_catalog(&stale, "base-1", 1);
    base_version.store(2, Ordering::SeqCst);
    manager.note_mcp_base_changed();
    stale.reply.send(StubReply::Ok).unwrap();
    let latest = stub.next().await;
    assert_catalog(&latest, "base-2", 1);
    assert!(!catalogs(&latest)
        .iter()
        .any(|row| row.get("name") == Some(&json!("base-1"))));
    latest.reply.send(StubReply::Ok).unwrap();
    reconcile.await.unwrap().unwrap();
    stub.shutdown().await;
}

#[tokio::test]
async fn live_background_and_parked_sessions_all_receive_catalog_updates() {
    let mut stub = TcpStub::connect().await;
    let manager = manager_with_live("live-mcp", "agent-live", Arc::clone(&stub.client));
    manager.background.lock().insert(
        "background-mcp".into(),
        ready_live(
            "background-mcp",
            "agent-background",
            "process-shared",
            Arc::clone(&stub.client),
        ),
    );
    let parked_live = ready_live(
        "parked-mcp",
        "agent-parked",
        "process-shared",
        Arc::clone(&stub.client),
    );
    manager.parked.lock().insert(
        "parked-mcp".into(),
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
            needs_history_bootstrap: false,
            backend: parked_live.backend,
        },
    );
    assert_eq!(
        manager.mcp_session_ids(),
        vec!["background-mcp", "live-mcp", "parked-mcp"]
    );
    let deps = deps(Arc::new(AtomicU64::new(3)), Duration::from_secs(1));
    let updates = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move {
            let mut tasks = Vec::new();
            for session in ["live-mcp", "background-mcp", "parked-mcp"] {
                let manager = Arc::clone(&manager);
                let deps = deps.clone();
                tasks.push(tokio::spawn(async move {
                    manager.reconcile_session_mcp_with(session, deps).await
                }));
            }
            for task in tasks {
                task.await.expect("join session update")?;
            }
            Ok::<(), String>(())
        })
    };
    let mut agents = std::collections::BTreeSet::new();
    for _ in 0..3 {
        let request = stub.next().await;
        assert_catalog(&request, "base-3", 0);
        agents.insert(
            request
                .params
                .get("sessionId")
                .and_then(Value::as_str)
                .expect("agent session id")
                .to_string(),
        );
        request.reply.send(StubReply::Ok).unwrap();
    }
    updates.await.unwrap().unwrap();
    assert_eq!(
        agents,
        ["agent-background", "agent-live", "agent-parked"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    stub.shutdown().await;
}

#[test]
fn reconnect_retries_a_pending_desired_absent_catalog() {
    let manager = SessionManager::new();
    manager.set_mcp_status(
        "cleanup-pending",
        McpCatalogStatus {
            desired_generation: 0,
            applied_generation: None,
            desired_present: false,
            pending: true,
            resource_cleanup_pending: false,
            last_error: Some("injected detach failure".into()),
        },
    );
    assert!(manager.computer_use_reconcile_needed_after_connect("cleanup-pending"));

    manager.set_mcp_status(
        "cleanup-pending",
        McpCatalogStatus {
            desired_generation: 0,
            applied_generation: Some(0),
            desired_present: false,
            pending: false,
            resource_cleanup_pending: false,
            last_error: None,
        },
    );
    assert!(!manager.computer_use_reconcile_needed_after_connect("cleanup-pending"));
}

#[test]
fn mcp_errors_exposed_to_status_never_retain_transport_payloads() {
    let secret = "cu-secret-sentinel-0123456789";
    let raw = format!(
        "ACP rejected catalog env GROK_COMPUTER_USE_TOKEN={secret}; Authorization: Bearer {secret}"
    );
    let public = safe_error(raw);
    assert_eq!(public, "Computer Use MCP catalog update failed");
    assert!(!public.contains(secret));
    assert_eq!(
        safe_error("request timed out after 15 seconds"),
        "Computer Use MCP catalog update timed out"
    );
}
