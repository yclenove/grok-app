use super::*;
use crate::{
    extensions::ExtensionsPrefs,
    providers::{MainHistoryRequest, MainProfileRequest, PreparedMainProfile},
    session_execution::ExecutionReceipt,
};
use std::sync::atomic::AtomicBool;

fn migrated(fixture: &mut Fixture) -> (Arc<AcpClient>, tokio::io::DuplexStream, PathBuf) {
    fixture.config("selected", "grok-4.6", false);
    fixture.meta.agent_session_id = Some("old-agent".into());
    fixture.meta.agent_execution = Some(ExecutionReceipt {
        schema_version: 1,
        agent_session_id: "old-agent".into(),
        source: ExecutionSource::LegacyShared,
        provider_id: Some("old-provider".into()),
        model_id: Some("old-model".into()),
        process_id: Some("old-process".into()),
    });
    store::update_session_meta(&fixture.meta).unwrap();
    let cwd = crate::paths::general_workspace_dir();
    let source = ExecutionSource::LegacyShared
        .home()
        .unwrap()
        .unwrap()
        .join("sessions/synthetic-physical-history/old-agent");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.parent().unwrap().join(".cwd"), cwd.to_str().unwrap()).unwrap();
    fs::write(source.join("plan.md"), "中文迁移计划").unwrap();
    fs::write(
        source.join("chat_history.jsonl"),
        "{\"content\":\"中文\"}\n",
    )
    .unwrap();
    let request = MainProfileRequest {
        owner_session_id: fixture.meta.id.clone(),
        provider_id: "selected".into(),
        model_id: "grok-4.6".into(),
        history: Some(MainHistoryRequest {
            session: fixture.meta.clone(),
            cwd: cwd.to_str().unwrap().into(),
            legacy_source: ExecutionSource::LegacyShared,
        }),
    };
    let profile = PreparedMainProfile::prepare(
        &request,
        &fixture.settings,
        "ask",
        false,
        &ExtensionsPrefs::default(),
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    let (client, peer, _) = test_transport::client_from_main_profile(profile, cwd);
    (client, peer, source)
}

#[test]
fn actual_migration_load_commits_agent_and_new_source_without_replacing_desired_preferences() {
    let mut fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer, source) = migrated(&mut fixture);
        let original = fs::read(source.join("chat_history.jsonl")).unwrap();
        let manager = fixture.manager(&client, "migration-process");
        execution_publication::validate_resume(&fixture.meta, &client).unwrap();
        let result = rpc(&mut peer, test_transport::open_with_empty_mcp(&client, Some("old-agent"), false),
            json!({"sessionId":"old-agent","models":{"currentModelId":"selected"}})).await.unwrap();
        assert_eq!(result, ("old-agent".into(), true));
        let mut desired = fixture.stored(); desired.title = "用户刚改的标题".into();
        desired.provider_id = Some("desired-future-route".into());
        store::update_session_meta(&desired).unwrap();
        let published = manager.publish_opened_execution(&fixture.meta, &client, "migration-process", "old-agent", false).unwrap();
        assert_eq!(published.title, "用户刚改的标题");
        assert_eq!(published.provider_id.as_deref(), Some("desired-future-route"));
        let receipt = published.agent_execution.as_ref().unwrap();
        assert!(matches!(&receipt.source, ExecutionSource::AppProfile { owner_session_id, .. } if owner_session_id == &fixture.meta.id));
        assert_eq!(receipt.provider_id.as_deref(), Some("selected"));
        assert_eq!(receipt.process_id.as_deref(), Some("migration-process"));
        assert_eq!(fixture.stored().agent_execution, published.agent_execution);
        manager.with_session_mut(&fixture.meta.id, |live| {
            assert_eq!(live.meta.agent_execution, published.agent_execution);
            assert_eq!(live.fsm.state(), SessionState::Ready);
        }).unwrap();
        assert!(execution_publication::validate_resume(&fixture.meta, &client).is_err());
        execution_publication::validate_resume(&published, &client).unwrap();
        assert_eq!(fs::read(source.join("chat_history.jsonl")).unwrap(), original);
        client.kill().await;
    });
}

#[test]
fn ready_producer_witness_alone_is_insufficient_for_execution_publication() {
    let mut fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, _peer, _) = migrated(&mut fixture);
        let manager = fixture.manager(&client, "migration-process");
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        assert!(manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "migration-process",
                "old-agent",
                false
            )
            .is_err());
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        assert_eq!(
            fixture.stored().agent_execution,
            fixture.meta.agent_execution
        );
        client.kill().await;
    });
}

#[test]
fn successful_migrated_open_cannot_overwrite_a_newer_execution_receipt() {
    let mut fixture = Fixture::new(false);
    runtime().block_on(async {
        let (client, mut peer, _) = migrated(&mut fixture);
        let manager = fixture.manager(&client, "migration-process");
        rpc(
            &mut peer,
            test_transport::open_with_empty_mcp(&client, Some("old-agent"), false),
            json!({"sessionId":"old-agent"}),
        )
        .await
        .unwrap();
        let mut newer = fixture.stored();
        newer.agent_execution.as_mut().unwrap().process_id = Some("newer-process".into());
        store::update_session_meta(&newer).unwrap();
        let before = fs::read(crate::paths::sessions_index_file()).unwrap();
        assert!(manager
            .publish_opened_execution(
                &fixture.meta,
                &client,
                "migration-process",
                "old-agent",
                false
            )
            .is_err());
        assert_eq!(
            fs::read(crate::paths::sessions_index_file()).unwrap(),
            before
        );
        assert_eq!(fixture.stored().agent_execution, newer.agent_execution);
        client.kill().await;
    });
}

#[test]
fn abandoned_migration_publication_future_cannot_commit_after_store_contention_ends() {
    let mut fixture = Fixture::new(false);
    let rt = runtime();
    let (manager, before, source, source_before, timed_out, revoked, runtime_available) = rt
        .block_on(async {
            let (client, mut peer, source) = migrated(&mut fixture);
            rpc(
                &mut peer,
                test_transport::open_with_empty_mcp(&client, Some("old-agent"), false),
                json!({"sessionId":"old-agent","models":{"currentModelId":"selected"}}),
            )
            .await
            .unwrap();
            let manager = Arc::new(fixture.manager(&client, "migration-process"));
            let before = fs::read(crate::paths::sessions_index_file()).unwrap();
            let source_before = fs::read(source.join("chat_history.jsonl")).unwrap();
            let lease = manager
                .with_session_mut(&fixture.meta.id, |live| {
                    live.fsm.connection_lease().unwrap()
                })
                .unwrap();
            let held =
                crate::store_lock::lock_exclusive(&fixture.root.join("slow-migration-writer.json"))
                    .unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(80),
                manager.complete_opened_execution(
                    &fixture.meta,
                    &client,
                    "migration-process",
                    "old-agent",
                    false,
                ),
            )
            .await;
            let revoked = lease.check().is_err();
            let runtime_available = manager.inner.try_lock().is_some();
            drop(held);
            client.kill().await;
            (
                manager,
                before,
                source,
                source_before,
                result.is_err(),
                revoked,
                runtime_available,
            )
        });
    // Join every blocking publication worker before checking the final index;
    // an observation delay is not evidence that a late worker has completed.
    drop(rt);
    assert!(timed_out && revoked && runtime_available);
    assert_eq!(
        fs::read(crate::paths::sessions_index_file()).unwrap(),
        before
    );
    assert_eq!(
        fs::read(source.join("chat_history.jsonl")).unwrap(),
        source_before
    );
    assert_eq!(
        manager
            .with_session_mut(&fixture.meta.id, |live| live.fsm.state())
            .unwrap(),
        SessionState::Connecting
    );
}
