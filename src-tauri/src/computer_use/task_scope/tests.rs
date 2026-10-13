//! Synthetic selected witnesses, isolated settings, real Host/Broker workers.
//! No native OS grants, user App data, live CLI or human approval.
use super::*;
use crate::computer_use::{
    feature, feature_preferences::FeaturePreferences, task_access, ComputerUseAdapter, SurfaceKind,
};
use grok_computer_use_core::{broker::BrokerOptions, fake::FakeAdapter, owned::HostOwnedAdapter};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct Fixture {
    _feature_guard: tokio::sync::MutexGuard<'static, ()>,
    broker: Arc<ComputerUseBroker>,
    fake: Arc<FakeAdapter>,
    owned: Arc<HostOwnedAdapter>,
    token: String,
    request: String,
    preferences: FeaturePreferences,
    root: std::path::PathBuf,
    workers: Arc<Workers>,
}

impl Fixture {
    async fn new() -> Self {
        let feature_guard = feature::TEST_LOCK.lock().await;
        super::super::set_feature_enabled(false);
        let root = std::env::temp_dir().join(format!("cu-task-scope-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let preferences = FeaturePreferences::at(root.join("settings.json"));
        preferences.set_explicit(false).unwrap();
        let fake = Arc::new(FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        fake.pin_synthetic_member("fixture:synthetic-second");
        let owned = Arc::new(HostOwnedAdapter::new(fake.clone()));
        let broker = Arc::new(ComputerUseBroker::request_endpoint(BrokerOptions {
            feature_enabled: false,
            lease_path: root.join("desktop.lease"),
            ..BrokerOptions::default()
        }));
        let issued = broker
            .reserve_transport_connection("task-scope-app")
            .unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-agent")
            .unwrap();
        let pending = broker.submit_access(issued.token(), json!({
            "action":"request", "purpose":"synthetic two-member approval", "scopeIntent":"isolated fake windows",
            "surfaces":["desktop"], "capabilities":["observe"]
        })).unwrap();
        Self {
            _feature_guard: feature_guard,
            broker,
            fake,
            owned,
            token: issued.token().into(),
            request: pending["requestId"].as_str().unwrap().into(),
            preferences,
            root,
            workers: Arc::new(Workers::default()),
        }
    }

    fn begin(&self) -> HostScopeSelectionCall {
        self.broker
            .begin_task_scope_group(
                &self.token,
                &self.request,
                1,
                &[
                    (SurfaceKind::Desktop, &self.fake.fixture_id()),
                    (SurfaceKind::Desktop, "fixture:synthetic-second"),
                ],
            )
            .unwrap()
    }

    fn prepare(&self) -> impl FnOnce() -> Result<(), String> + Send + 'static {
        let broker = self.broker.clone();
        let owned = self.owned.clone();
        move || {
            broker
                .register_surface_adapter(SurfaceKind::Desktop, owned)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    fn no_authority(&self) {
        assert_eq!(self.preferences.snapshot()["computerUseEnabled"], false);
        assert!(!self.broker.feature_enabled());
        assert!(!feature::feature_enabled());
        assert_eq!(self.broker.executable_grant_count("task-scope-app"), 0);
        assert_eq!(self.broker.continuation_count("task-scope-app"), 0);
        assert_eq!(self.fake.list_calls(), 0);
        assert_eq!(self.fake.observe_calls(), 0);
        assert!(self.fake.executions().is_empty());
        assert!(!self.owned.worker_attached());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("task-scope-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        super::super::set_feature_enabled(false);
        for name in ["settings.json", "settings.json.lock", "desktop.lease"] {
            let path = self.root.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(&self.root).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_group_enters_the_existing_first_use_transaction_without_preapproval_effects() {
    let fixture = Fixture::new().await;
    let selection = fixture.begin();
    let call = SelectionCall::new(&selection);
    fixture.no_authority();
    let AccessApprovalReady::Prepare(approval) = reserve_with(
        fixture.broker.clone(),
        fixture.token.clone(),
        selection,
        fixture.workers.clone(),
        fixture.prepare(),
    )
    .await
    .unwrap() else {
        panic!("first approval prepares")
    };
    fixture.no_authority();
    assert!(fixture
        .broker
        .begin_model_call(&fixture.token, "computer_observe")
        .is_err());
    let approvals = Arc::new(super::super::owned_tasks::OwnedTasks::default());
    let outcome = task_access::approve_with(
        fixture.broker.clone(),
        approval,
        fixture.preferences.clone(),
        approvals.clone(),
        || Ok(()),
    )
    .await
    .unwrap();
    drop(call);
    assert_eq!(fixture.preferences.snapshot()["computerUseEnabled"], true);
    assert!(fixture.broker.feature_enabled());
    assert!(feature::feature_enabled());
    assert_eq!(fixture.broker.executable_grant_count("task-scope-app"), 1);
    assert_eq!(fixture.broker.continuation_count("task-scope-app"), 1);
    let inventory = fixture
        .broker
        .approved_task_targets(&fixture.token, &json!({"runId":outcome.run_id}))
        .unwrap();
    assert_eq!(inventory.len(), 2);
    assert_eq!(fixture.fake.list_calls(), 0);
    assert_eq!(fixture.fake.observe_calls(), 0);
    assert_eq!(
        outcome.decision_class,
        grok_computer_use_core::task_consent::SYNTHETIC_DECISION_CLASS
    );
    fixture.workers.join().await.unwrap();
    approvals.join().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_capture_waiter_fences_and_joins_its_original_blocking_query() {
    let fixture = Fixture::new().await;
    let selection = fixture.begin();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let entered = started.clone();
    let blocked = release.clone();
    fixture.fake.arm_liveness_hook(Box::new(move || {
        entered.store(true, Ordering::SeqCst);
        while !blocked.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }));
    let guard = SelectionCall::new(&selection);
    let reserve = reserve_with(
        fixture.broker.clone(),
        fixture.token.clone(),
        selection,
        fixture.workers.clone(),
        fixture.prepare(),
    );
    let waiter = tokio::spawn(async move {
        let _call = guard;
        reserve.await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
    fixture.no_authority();
    let original = fixture.workers.clone();
    let join = tokio::spawn(async move { original.join().await });
    tokio::task::yield_now().await;
    assert!(
        !join.is_finished(),
        "native callback must return before original join"
    );
    release.store(true, Ordering::SeqCst);
    join.await.unwrap().unwrap();
    fixture.no_authority();
    assert!(fixture
        .broker
        .reserve_access_approval(&fixture.token, &fixture.request, 1)
        .is_err());
}

#[tokio::test]
async fn dropped_selection_before_worker_start_never_prepares_a_runtime() {
    let fixture = Fixture::new().await;
    let selection = fixture.begin();
    drop(SelectionCall::new(&selection));
    assert!(reserve_with(
        fixture.broker.clone(),
        fixture.token.clone(),
        selection,
        fixture.workers.clone(),
        || panic!("cancelled selection started runtime")
    )
    .await
    .is_err());
    fixture.workers.join().await.unwrap();
    fixture.no_authority();
}

#[test]
fn host_selection_dto_rejects_aliases_duplicates_and_model_identity_fields() {
    let parse = |value| serde_json::from_value::<Vec<TaskScopeTarget>>(value);
    assert!(
        parse(json!([{"surface":"desktop","targetId":"fixture:one","grantId":"forged"}])).is_err()
    );
    for value in [
        json!([]),
        json!([{"surface":"webview","targetId":"wv:one"}]),
        json!([{"surface":"desktop","targetId":"fixture:one"},{"surface":"desktop","targetId":"fixture:one"}]),
        json!([{"surface":"desktop","targetId":"managed:one"}]),
        json!([{"surface":"desktop","targetId":" fixture:one"}]),
    ] {
        assert!(TaskScopeTarget::parse(&parse(value).unwrap()).is_err());
    }
}

#[tokio::test]
async fn stop_feature_off_and_transport_retirement_during_capture_never_publish_authority() {
    for mode in ["stop", "feature-off", "transport-retire"] {
        let fixture = Fixture::new().await;
        let selection = fixture.begin();
        let _call = SelectionCall::new(&selection);
        let broker = fixture.broker.clone();
        let token = fixture.token.clone();
        let fake = fixture.fake.clone();
        fixture.fake.arm_liveness_hook(Box::new(move || {
            match mode {
                "stop" => broker.fence_task_consent("task-scope-app"),
                "feature-off" => {
                    broker.set_feature_enabled(true);
                    broker.set_feature_enabled(false);
                }
                "transport-retire" => broker.retire_transport_connection(&token),
                _ => unreachable!(),
            }
            fake.arm_liveness_hook(Box::new(|| panic!("retired query read another member")));
        }));
        assert!(
            reserve_with(
                fixture.broker.clone(),
                fixture.token.clone(),
                selection,
                fixture.workers.clone(),
                fixture.prepare()
            )
            .await
            .is_err(),
            "{mode}"
        );
        fixture.workers.join().await.unwrap();
        fixture.no_authority();
    }
}
