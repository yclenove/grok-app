//! Owned synthetic Host decisions test the real persistence/Broker transaction.
//! These are not production Bound, native App or human gesture acceptance.
use super::*;
use crate::computer_use::feature_lifecycle::persist_feature_transition_with_broker;
use crate::session_manager::SessionManager;
use grok_computer_use_core::{
    broker::{AccessApprovalReady, BrokerOptions},
    fake::FakeAdapter,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

struct Fixture {
    _feature_guard: tokio::sync::MutexGuard<'static, ()>,
    broker: Arc<ComputerUseBroker>,
    adapter: Arc<FakeAdapter>,
    approval: Option<AccessApproval>,
    preferences: FeaturePreferences,
    root: PathBuf,
    token: String,
    workers: Arc<workers::Workers>,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_feature(false).await
    }

    async fn with_feature(enabled: bool) -> Self {
        let feature_guard = super::super::feature::TEST_LOCK.lock().await;
        let root = std::env::temp_dir().join(format!("cu-task-access-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preferences = FeaturePreferences::at(root.join("settings.json"));
        preferences.set_explicit(false).unwrap();
        if enabled {
            assert!(preferences.enable(false).unwrap().is_some());
        }
        let adapter = Arc::new(FakeAdapter::new());
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: enabled,
                lease_path: root.join("desktop.lease"),
                ..BrokerOptions::default()
            },
        ));
        super::super::set_feature_enabled(enabled);
        let transport = broker
            .issue_transport_credential("task-access-app")
            .unwrap();
        broker
            .bind_agent_session("task-access-app", "synthetic-agent")
            .unwrap();
        let request = broker.submit_access(transport.token(), json!({
            "action":"request", "purpose":"synthetic first-use transaction", "surfaces":["desktop"],
            "scopeIntent":"owned fake fixture", "capabilities":["observe"]
        })).unwrap();
        let request_id = request["requestId"].as_str().unwrap();
        broker
            .bind_synthetic_scope(request_id, &adapter.fixture_id())
            .unwrap();
        let AccessApprovalReady::Prepare(approval) = broker
            .reserve_access_approval(transport.token(), request_id, 1)
            .unwrap()
        else {
            panic!("first decision must prepare")
        };
        Self {
            _feature_guard: feature_guard,
            broker,
            adapter,
            approval: Some(approval),
            preferences,
            root,
            token: transport.token().into(),
            workers: Arc::new(workers::Workers::default()),
        }
    }

    fn assert_no_authority_or_effect(&self) {
        assert_eq!(self.broker.executable_grant_count("task-access-app"), 0);
        assert_eq!(self.broker.continuation_count("task-access-app"), 0);
        assert_eq!(self.adapter.observe_calls(), 0);
        assert!(self.adapter.executions().is_empty());
        assert!(self
            .broker
            .begin_model_call(&self.token, "computer_observe")
            .is_err());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.approval.take();
        self.broker.fence_task_consent("task-access-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        self.broker.set_feature_enabled(false);
        super::super::set_feature_enabled(false);
        // Only exact files in this owned UUID fixture; never the App data root.
        for name in ["settings.json", "settings.json.lock", "desktop.lease"] {
            let path = self.root.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(&self.root).unwrap();
    }
}

async fn entered(flag: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("original preparation entered");
}

fn barrier() -> (
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    impl FnOnce() -> Result<(), String> + Send + 'static,
) {
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let entered = started.clone();
    let gate = release.clone();
    (started, release, move || {
        entered.store(true, Ordering::SeqCst);
        while !gate.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_use_commits_all_three_states_and_exactly_one_continuation() {
    let mut fixture = Fixture::new().await;
    let outcome = approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        || Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], true);
    assert!(fixture.broker.feature_enabled());
    assert!(super::super::feature::feature_enabled());
    assert_eq!(fixture.broker.executable_grant_count("task-access-app"), 1);
    assert_eq!(fixture.broker.continuation_count("task-access-app"), 1);
    assert_eq!(
        fixture
            .broker
            .execution_run(&fixture.token, "computer_observe")
            .unwrap(),
        outcome.run_id
    );
    assert_eq!(fixture.adapter.observe_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn already_enabled_owned_preference_commits_without_rewriting_or_rolling_back() {
    let mut fixture = Fixture::with_feature(true).await;
    let before = fixture.preferences.snapshot();
    let owner = before[super::super::feature_preferences::OWNER_KEY]
        .as_str()
        .unwrap()
        .to_owned();
    let outcome = approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        || Ok(()),
    )
    .await
    .unwrap();
    let after = fixture.preferences.snapshot();
    assert_eq!(after["computerUseEnabled"], true);
    assert_eq!(after[super::super::feature_preferences::OWNER_KEY], owner);
    assert!(fixture.broker.feature_enabled());
    assert!(super::super::feature::feature_enabled());
    assert_eq!(fixture.broker.executable_grant_count("task-access-app"), 1);
    assert_eq!(fixture.broker.continuation_count("task-access-app"), 1);
    assert_eq!(
        fixture
            .broker
            .execution_run(&fixture.token, "computer_observe")
            .unwrap(),
        outcome.run_id
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_host_pin_rejection_rolls_back_preference_without_any_grant_or_continuation() {
    let mut fixture = Fixture::new().await;
    let broker = fixture.broker.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let publications = calls.clone();
    let result = approve_with_publication(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        Arc::new(move |approval| {
            publications.fetch_add(1, Ordering::SeqCst);
            assert!(approval.is_current());
            assert_eq!(broker.executable_grant_count("task-access-app"), 0);
            assert_eq!(broker.continuation_count("task-access-app"), 0);
            Err("synthetic original App owner replaced".into())
        }),
        || Ok(()),
    )
    .await;
    fixture.workers.join().await.unwrap();
    assert_eq!(
        result.err().as_deref(),
        Some("synthetic original App owner replaced")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], false);
    assert!(!fixture.broker.feature_enabled());
    assert!(!super::super::feature::feature_enabled());
    fixture.assert_no_authority_or_effect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_persistence_never_starts_runtime_or_changes_the_original_file() {
    let mut fixture = Fixture::new().await;
    crate::store_lock::write_bytes_atomic(&fixture.root.join("settings.json"), b"{invalid")
        .unwrap();
    let calls = Arc::new(AtomicBool::new(false));
    let count = calls.clone();
    assert!(approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        move || {
            count.store(true, Ordering::SeqCst);
            Ok(())
        }
    )
    .await
    .is_err());
    assert!(!calls.load(Ordering::SeqCst));
    assert!(!fixture.broker.feature_enabled());
    fixture.assert_no_authority_or_effect();
    assert_eq!(
        std::fs::read(fixture.root.join("settings.json")).unwrap(),
        b"{invalid"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_runtime_failure_rolls_back_without_grant_or_continuation() {
    let mut fixture = Fixture::new().await;
    let result = approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        || Err("synthetic selected executor unavailable".into()),
    )
    .await;
    assert_eq!(
        result.err().as_deref(),
        Some("synthetic selected executor unavailable")
    );
    fixture.workers.join().await.unwrap();
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], false);
    assert!(!fixture.broker.feature_enabled());
    assert!(!super::super::feature::feature_enabled());
    assert_eq!(fixture.adapter.list_calls(), 0);
    fixture.assert_no_authority_or_effect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_during_prepare_rolls_back_only_cu_and_preserves_concurrent_locale() {
    let mut fixture = Fixture::new().await;
    let (started, release, prepare) = barrier();
    let work = tokio::spawn(approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        prepare,
    ));
    entered(&started).await;
    fixture.assert_no_authority_or_effect();
    let mut settings: crate::store::AppSettings =
        serde_json::from_value(fixture.preferences.snapshot()).unwrap();
    settings.locale = "ja".into();
    crate::store::save_settings_at(&fixture.root.join("settings.json"), &settings).unwrap();
    fixture.broker.fence_task_consent("task-access-app");
    release.store(true, Ordering::SeqCst);
    assert!(work.await.unwrap().is_err());
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], false);
    assert_eq!(fixture.preferences.snapshot()["locale"], "ja");
    assert!(!fixture.broker.feature_enabled());
    assert!(!super::super::feature::feature_enabled());
    fixture.assert_no_authority_or_effect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn caller_cancellation_fences_and_original_worker_finishes_rollback() {
    let mut fixture = Fixture::new().await;
    let (started, release, prepare) = barrier();
    let work = tokio::spawn(approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        prepare,
    ));
    entered(&started).await;
    work.abort();
    assert!(work.await.err().expect("caller cancelled").is_cancelled());
    fixture.assert_no_authority_or_effect();
    release.store(true, Ordering::SeqCst);
    fixture.workers.join().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.preferences.snapshot()["computerUseEnabled"] != false
            || fixture.broker.feature_enabled()
            || super::super::feature::feature_enabled()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("original worker rolled back after caller cancellation");
    fixture.assert_no_authority_or_effect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_off_and_on_supersede_late_approval_without_undoing_user_choice() {
    let mut fixture = Fixture::new().await;
    let (started, release, prepare) = barrier();
    let work = tokio::spawn(approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        prepare,
    ));
    entered(&started).await;
    let manager = Arc::new(SessionManager::new());
    let store = fixture.preferences.clone();
    persist_feature_transition_with_broker(
        manager.clone(),
        fixture.broker.clone(),
        false,
        move || store.set_explicit(false),
    )
    .await
    .unwrap();
    let store = fixture.preferences.clone();
    persist_feature_transition_with_broker(manager, fixture.broker.clone(), true, move || {
        store.set_explicit(true)
    })
    .await
    .unwrap();
    release.store(true, Ordering::SeqCst);
    assert!(work.await.unwrap().is_err());
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], true);
    assert!(fixture.broker.feature_enabled());
    assert!(super::super::feature::feature_enabled());
    fixture.assert_no_authority_or_effect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_use_failure_keeps_another_sessions_committed_preference_and_grant() {
    let mut fixture = Fixture::new().await;
    let (started, release, prepare) = barrier();
    let first = tokio::spawn(approve_with(
        fixture.broker.clone(),
        fixture.approval.take().unwrap(),
        fixture.preferences.clone(),
        fixture.workers.clone(),
        prepare,
    ));
    entered(&started).await;
    let second_token = fixture
        .broker
        .issue_transport_credential("second-task-app")
        .unwrap();
    fixture
        .broker
        .bind_agent_session("second-task-app", "synthetic-second")
        .unwrap();
    let request = fixture.broker.submit_access(second_token.token(), json!({
        "action":"request", "purpose":"another synthetic first-use transaction", "surfaces":["desktop"],
        "scopeIntent":"owned fake fixture", "capabilities":["observe"]
    })).unwrap();
    let request_id = request["requestId"].as_str().unwrap();
    fixture
        .broker
        .bind_synthetic_scope(request_id, &fixture.adapter.fixture_id())
        .unwrap();
    let AccessApprovalReady::Prepare(second) = fixture
        .broker
        .reserve_access_approval(second_token.token(), request_id, 1)
        .unwrap()
    else {
        panic!("second session prepares")
    };
    approve_with(
        fixture.broker.clone(),
        second,
        fixture.preferences.clone(),
        fixture.workers.clone(),
        || Ok(()),
    )
    .await
    .unwrap();
    fixture.broker.fence_task_consent("task-access-app");
    release.store(true, Ordering::SeqCst);
    assert!(first.await.unwrap().is_err());
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], true);
    assert!(fixture.broker.feature_enabled());
    assert!(super::super::feature::feature_enabled());
    assert!(fixture
        .broker
        .begin_model_call(second_token.token(), "computer_observe")
        .is_ok());
    assert_eq!(fixture.broker.executable_grant_count("second-task-app"), 1);
    assert_eq!(fixture.broker.continuation_count("second-task-app"), 1);
    fixture.broker.fence_task_consent("second-task-app");
}
