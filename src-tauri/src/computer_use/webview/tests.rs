use super::super::adapter::{
    DownloadError, DownloadRequest, DownloadResult, NavigationDisposition, NavigationKind,
};
use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::Barrier;

struct TestView {
    label: String,
    native_tab_id: String,
    url: Mutex<String>,
    title: Mutex<String>,
    alive: AtomicBool,
    generation: AtomicU64,
    scripts: AtomicU64,
    upload_field: AtomicBool,
    reply: Mutex<Option<serde_json::Value>>,
    pending: Mutex<Option<ScriptOperation>>,
    timeout_at: AtomicU64,
    wait_match_at: AtomicU64,
    history_available: AtomicBool,
    navigation_unknown: AtomicBool,
    download_calls: AtomicU64,
    prepared_download_url: Mutex<Option<String>>,
    authority: Mutex<()>,
    block_authority: AtomicBool,
    block_at: u64,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl TestView {
    fn new(block_at: u64) -> Arc<Self> {
        Self::named("test", block_at)
    }

    fn named(name: &str, block_at: u64) -> Arc<Self> {
        Arc::new(Self {
            label: format!("resource-browser-{name}"),
            native_tab_id: format!("native-{name}"),
            url: Mutex::new(format!("https://fixture.invalid/{name}")),
            title: Mutex::new(format!("owned fixture {name}")),
            alive: AtomicBool::new(true),
            generation: AtomicU64::new(1),
            scripts: AtomicU64::new(0),
            upload_field: AtomicBool::new(false),
            reply: Mutex::new(None),
            pending: Mutex::new(None),
            timeout_at: AtomicU64::new(0),
            wait_match_at: AtomicU64::new(0),
            history_available: AtomicBool::new(false),
            navigation_unknown: AtomicBool::new(false),
            download_calls: AtomicU64::new(0),
            prepared_download_url: Mutex::new(None),
            authority: Mutex::new(()),
            block_authority: AtomicBool::new(false),
            block_at,
            entered: Arc::new(Barrier::new(2)),
            release: Arc::new(Barrier::new(2)),
        })
    }
}

impl LiveWebView for TestView {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        self.native_tab_id.clone()
    }
    fn url(&self) -> Result<String, String> {
        Ok(self.url.lock().clone())
    }
    fn title(&self) -> Result<String, String> {
        Ok(self.title.lock().clone())
    }
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    fn navigate(&self, url: &str) -> Result<u64, String> {
        let _authority = self.authority.lock();
        *self.url.lock() = url.to_string();
        Ok(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn with_document_authority(
        &self,
        generation: u64,
        commit: &mut dyn FnMut() -> Result<(), super::super::BrokerError>,
    ) -> Result<(), super::super::BrokerError> {
        if self.block_authority.load(Ordering::SeqCst) {
            self.entered.wait();
            self.release.wait();
        }
        let _authority = self.authority.lock();
        if !self.alive.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != generation
        {
            return Err(super::super::BrokerError::DeadTarget);
        }
        commit()
    }
    fn host_script(&self, js: &str) -> Result<String, String> {
        let call = self.scripts.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.block_at {
            self.entered.wait();
            self.release.wait();
        }
        Ok(self
            .reply
            .lock()
            .clone()
            .unwrap_or_else(|| {
                let payload = js.rsplit_once("\n(").unwrap().1.strip_suffix(')').unwrap();
                let command: serde_json::Value = serde_json::from_str(payload).unwrap();
                if command["kind"] == "observe" {
                    let snapshot = command["snapshot"].as_str().unwrap();
                    let actions = if self.upload_field.load(Ordering::SeqCst) {
                        vec!["upload", "wait"]
                    } else {
                        vec!["click", "type_text", "wait", "download"]
                    };
                    serde_json::json!({"ok": true, "snapshot": snapshot, "width": 800, "height": 600,
                        "text": "Fixture", "truncated": false,
                        "nodes": [{"elementRef": format!("{snapshot}:1"), "role": "button", "name": "Owned",
                            "actions": actions, "x": 10, "y": 10, "width": 100, "height": 30}]})
                } else if command["kind"] == "wait_check" {
                    let match_at = self.wait_match_at.load(Ordering::SeqCst);
                    serde_json::json!({"ok": true, "matched": match_at > 0 && call >= match_at})
                } else if (command["kind"] == "act" && command["action"] == "type_text")
                    || command["kind"] == "upload_dispatch" {
                    serde_json::json!({"ok": true, "verified": true})
                } else if command["kind"] == "download_prepare" {
                    serde_json::json!({"ok": true, "url": "https://fixture.invalid/download.bin"})
                } else {
                    serde_json::json!({"ok": true})
                }
            })
            .to_string())
    }
    fn host_script_owned(
        &self,
        js: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        if let Err(error) = operation.check() {
            operation.finished();
            return Err(error);
        }
        let result = self.host_script(js);
        if self.scripts.load(Ordering::SeqCst) == self.timeout_at.load(Ordering::SeqCst) {
            *self.pending.lock() = Some(operation);
            return Err("WebView typed script outcome unknown".into());
        }
        operation.finished();
        result
    }
    fn navigate_for_run(
        &self,
        request: &NavigationRequest,
        expected_generation: u64,
    ) -> Result<LiveNavigationDocument, NavigationError> {
        request
            .cancellation
            .check()
            .map_err(NavigationError::not_dispatched)?;
        let _authority = self.authority.lock();
        if !self.alive.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != expected_generation
        {
            return Err(NavigationError::not_dispatched(
                "fixture document changed before navigation",
            ));
        }
        request
            .cancellation
            .check()
            .map_err(NavigationError::not_dispatched)?;
        match request.kind {
            NavigationKind::Url => {
                let url = request.url.as_deref().ok_or_else(|| {
                    NavigationError::not_dispatched("url navigation requires url")
                })?;
                super::super::browser::is_allowed_navigate_url(url)
                    .map_err(|error| NavigationError::not_dispatched(error.to_string()))?;
                *self.url.lock() = url.to_string();
            }
            NavigationKind::Reload => {
                if request.url.is_some() {
                    return Err(NavigationError::not_dispatched(
                        "url is only valid for kind=url",
                    ));
                }
            }
            NavigationKind::Back | NavigationKind::Forward => {
                if request.url.is_some() {
                    return Err(NavigationError::not_dispatched(
                        "url is only valid for kind=url",
                    ));
                }
                if !self.history_available.load(Ordering::SeqCst) {
                    return Err(NavigationError::not_dispatched(
                        "WebView has no history entry",
                    ));
                }
                let suffix = match request.kind {
                    NavigationKind::Back => "back",
                    NavigationKind::Forward => "forward",
                    _ => unreachable!(),
                };
                *self.url.lock() = format!("https://fixture.invalid/{suffix}");
            }
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if self.navigation_unknown.swap(false, Ordering::SeqCst) {
            return Err(NavigationError::dispatched_unknown(
                "fixture navigation outcome unknown",
            ));
        }
        Ok(LiveNavigationDocument {
            generation,
            url: self.url.lock().clone(),
            title: self.title.lock().clone(),
        })
    }
    fn download_for_run(
        &self,
        request: &DownloadRequest,
        expected_generation: u64,
        prepared_url: &str,
        dispatch_script: &str,
        operation: ScriptOperation,
    ) -> Result<DownloadResult, DownloadError> {
        if let Err(error) = operation.check() {
            operation.finished();
            return Err(DownloadError::not_dispatched(error));
        }
        let _authority = self.authority.lock();
        if !self.alive.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != expected_generation
        {
            operation.finished();
            return Err(DownloadError::not_dispatched(
                "fixture document changed before download",
            ));
        }
        *self.prepared_download_url.lock() = Some(prepared_url.to_string());

        let child = operation.child();
        if let Err(error) = child.check() {
            operation.finished();
            return Err(DownloadError::not_dispatched(error));
        }
        let reply = self.host_script(dispatch_script);
        child.finished();
        let dispatched = match reply
            .and_then(|raw| parse_script_json(&raw))
            .and_then(|value| dom::acknowledge(&value))
        {
            Ok(()) => true,
            Err(_) => {
                operation.finished();
                return Err(DownloadError::dispatched_unknown(
                    "fixture download dispatch outcome unknown",
                ));
            }
        };
        debug_assert!(dispatched);
        self.download_calls.fetch_add(1, Ordering::SeqCst);
        let write = std::fs::write(&request.destination, b"fixture-download");
        operation.finished();
        write.map_err(|_| DownloadError::dispatched_rejected("fixture download write failed"))?;
        Ok(DownloadResult {
            path: request.destination.clone(),
        })
    }
}

#[test]
fn native_timeout_remains_occupied_after_unbind_and_same_view_reauthorization() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    view.timeout_at.store(2, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    assert!(adapter
        .act(&click_request(target.clone()))
        .unwrap_err()
        .contains("unknown"));
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    adapter.release_target(&target.target_id);
    assert!(!adapter.is_idle("run"));
    let new = adapter
        .bind(view.clone(), "next", "next-run", &serde_json::json!({}))
        .unwrap();
    assert!(adapter
        .claim_target_for_run("next-run", &new.target_id)
        .unwrap_err()
        .contains("still pending"));
    assert!(adapter
        .observe(&new.target_id)
        .unwrap_err()
        .contains("still pending"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    let callback = view.pending.lock().take().unwrap();
    assert!(callback.check().is_err());
    callback.finished();
    assert!(adapter.is_idle("run"));
    adapter
        .claim_target_for_run("next-run", &new.target_id)
        .unwrap();
    adapter.observe(&new.target_id).unwrap();
    assert!(adapter.target_alive(&new.target_id));
}

#[test]
fn abort_during_preflight_prevents_the_side_effect_even_without_caller_polling() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    let target = bind(&adapter, view.clone());
    let acting = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.act(&click_request(target)))
    };
    view.entered.wait();
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    view.release.wait();
    assert!(acting.join().unwrap().is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
    assert!(adapter.is_idle("run"));
}

#[test]
fn wrong_run_observation_and_cancelled_capture_never_queue_native_work() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    assert!(adapter.observe_for_run("wrong", &target.target_id).is_err());
    let options = super::super::adapter::CaptureOptions::model(false);
    options.cancellation.cancel();
    assert!(adapter
        .capture_for_run("run", &target.target_id, options)
        .is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    assert!(adapter.is_idle("run"));
}

#[test]
fn unrelated_run_cannot_list_claim_or_release_the_bound_target() {
    let adapter = WebViewAdapter::new();
    let target = bind(&adapter, TestView::new(0));
    assert!(adapter.list_targets_for_run("wrong").unwrap().is_empty());
    assert_eq!(adapter.list_targets_for_run("run").unwrap().len(), 1);
    assert!(adapter
        .claim_target_for_run("wrong", &target.target_id)
        .is_err());
    adapter.release_target_for_run("wrong", &target.target_id);
    assert!(adapter.target_alive(&target.target_id));
    adapter
        .claim_target_for_run("run", &target.target_id)
        .unwrap();
    adapter.release_target_for_run("run", &target.target_id);
    assert!(!adapter.target_alive(&target.target_id));
}

#[test]
fn rebind_revokes_queued_old_script_without_falsely_settling_it() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    view.timeout_at.store(1, Ordering::SeqCst);
    let old = bind(&adapter, view.clone());
    assert!(adapter.observe(&old.target_id).is_err());
    let old_callback = view.pending.lock().take().unwrap();
    assert!(old_callback.check().is_ok());
    // Transfer requires release; native completion stays pending.
    assert!(adapter.unbind_for_run("session", "run"));
    let new = adapter
        .bind(view, "next", "new-run", &serde_json::json!({}))
        .unwrap();
    assert!(old_callback.check().is_err());
    assert!(!adapter.is_idle("run"));
    adapter.abort("run", 2).unwrap();
    assert!(adapter.target_alive(&new.target_id));
    old_callback.finished();
    assert!(adapter.is_idle("run"));
}

#[test]
fn broker_stays_stop_requested_until_native_completion_even_after_release() {
    use super::super::test_support::CountingAdapter;
    use super::super::{BrokerOptions, ComputerUseBroker, StopState, SurfaceKind};
    let adapter = Arc::new(WebViewAdapter::new());
    let desktop = Arc::new(CountingAdapter::default());
    let broker = ComputerUseBroker::new(
        desktop.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-wv-{}.lease", uuid::Uuid::new_v4())),
            ..Default::default()
        },
    );
    broker
        .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
        .unwrap();
    let view = TestView::new(0);
    view.timeout_at.store(1, Ordering::SeqCst);
    let target = bind(&adapter, view.clone());
    broker.open_run("session", "run").unwrap();
    broker
        .authorize_on_surface("session", "run", SurfaceKind::WebView, &target.target_id)
        .unwrap();
    assert!(broker.observe("run").is_err());
    assert_eq!(
        broker.request_stop("run").unwrap(),
        StopState::StopRequested
    );
    assert!(adapter.list_targets().unwrap().is_empty());
    assert_eq!(broker.stop_state("run").unwrap(), StopState::StopRequested);
    let callback = view.pending.lock().take().unwrap();
    callback.finished();
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
    assert_eq!(desktop.aborts(), 0);
    assert_eq!(desktop.releases(), 0);
}

fn bind(adapter: &WebViewAdapter, view: Arc<TestView>) -> TargetInfo {
    let target = adapter
        .bind(view, "session", "run", &serde_json::json!({}))
        .unwrap();
    // Race fixtures seed a mock observation without advancing native call
    // counters. DOM behavior is exercised separately against the actual kernel.
    adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot = Some(dom::Published {
        id: "mock-snapshot".into(),
        nodes: [(
            "mock-snapshot:1".into(),
            vec!["click".into(), "type_text".into(), "wait".into()],
        )]
        .into_iter()
        .collect(),
    });
    target
}

fn download_request(target: &TargetInfo, destination: std::path::PathBuf) -> DownloadRequest {
    DownloadRequest {
        run_id: "run".into(),
        target_id: target.target_id.clone(),
        target_generation: target.lifecycle_stamp,
        action_id: "download-once".into(),
        snapshot_id: "mock-snapshot".into(),
        element_ref: "mock-snapshot:1".into(),
        destination,
        cancellation: Default::default(),
    }
}

fn download_destination(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let directory = root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

#[test]
fn download_is_staged_once_and_published_only_under_original_document_authority() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .as_mut()
        .unwrap()
        .nodes
        .get_mut("mock-snapshot:1")
        .unwrap()
        .push("download".into());
    let root = std::env::temp_dir().join(format!("cu-webview-download-{}", uuid::Uuid::new_v4()));
    let destination = download_destination(&root, "report.bin");
    let mut request = download_request(&target, destination.clone());
    // Broker run generations and native WebView document generations are
    // independent clocks. The adapter authorizes the exact encoded target,
    // retained snapshot and native document rather than comparing the clocks.
    request.target_generation += 41;

    let staged = adapter.stage_download(&request).unwrap();
    assert_eq!(staged.result().path, destination);
    assert_eq!(std::fs::read(&destination).unwrap(), b"fixture-download");
    assert_eq!(view.download_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        view.prepared_download_url.lock().as_deref(),
        Some("https://fixture.invalid/download.bin")
    );
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    assert!(adapter.is_idle("run"));
    assert!(adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .is_none());

    let mut commits = 0;
    staged
        .commit(&mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(commits, 1);
    assert!(destination.is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dropped_or_retired_download_cleans_only_its_action_private_staging() {
    for retire_with_abort in [false, true] {
        let adapter = WebViewAdapter::new();
        let view = TestView::new(0);
        let target = bind(&adapter, view.clone());
        adapter
            .bound_target(&target.target_id)
            .unwrap()
            .observation
            .lock()
            .snapshot
            .as_mut()
            .unwrap()
            .nodes
            .get_mut("mock-snapshot:1")
            .unwrap()
            .push("download".into());
        let root =
            std::env::temp_dir().join(format!("cu-webview-download-{}", uuid::Uuid::new_v4()));
        let destination = download_destination(&root, "report.bin");
        let sibling = download_destination(&root, "keep.bin");
        std::fs::write(&sibling, b"keep").unwrap();
        let staged = adapter
            .stage_download(&download_request(&target, destination.clone()))
            .unwrap();

        if retire_with_abort {
            adapter.abort("run", 1).unwrap();
        } else {
            view.navigate("https://fixture.invalid/replaced").unwrap();
        }
        let error = staged.commit(&mut || Ok(())).unwrap_err();
        assert!(matches!(
            error,
            super::super::BrokerError::StopRequested | super::super::BrokerError::DeadTarget
        ));
        assert!(!destination.exists());
        assert!(!destination.parent().unwrap().exists());
        assert_eq!(std::fs::read(&sibling).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn invalid_download_run_snapshot_or_ref_never_prepares_or_dispatches() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .as_mut()
        .unwrap()
        .nodes
        .get_mut("mock-snapshot:1")
        .unwrap()
        .push("download".into());
    let root = std::env::temp_dir().join(format!("cu-webview-download-{}", uuid::Uuid::new_v4()));

    let mut requests = Vec::new();
    let mut wrong_run = download_request(&target, download_destination(&root, "run.bin"));
    wrong_run.run_id = "other-run".into();
    requests.push(wrong_run);
    let mut wrong_snapshot = download_request(&target, download_destination(&root, "snapshot.bin"));
    wrong_snapshot.snapshot_id = "other-snapshot".into();
    requests.push(wrong_snapshot);
    let mut wrong_ref = download_request(&target, download_destination(&root, "ref.bin"));
    wrong_ref.element_ref = "mock-snapshot:missing".into();
    requests.push(wrong_ref);

    for request in requests {
        let error = adapter.stage_download(&request).err().unwrap();
        assert_eq!(
            error.disposition,
            super::super::adapter::DownloadDisposition::NotDispatched
        );
        assert!(!request.destination.exists());
    }
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    assert_eq!(view.download_calls.load(Ordering::SeqCst), 0);
    assert!(adapter
        .bound_target(&target.target_id)
        .unwrap()
        .observation
        .lock()
        .snapshot
        .is_some());
    let _ = std::fs::remove_dir_all(root);
}

fn click_request(target: TargetInfo) -> DispatchRequest {
    use super::super::adapter::ActionScope;
    use super::super::protocol::{ActionKind, ActionTarget};
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "old".into(),
        generation: 1,
        target_id: target.target_id,
        target_generation: target.lifecycle_stamp,
        snapshot_id: "mock-snapshot".into(),
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: "mock-snapshot:1".into(),
        },
        parameters: serde_json::json!({}),
        scope: ActionScope::Directed,
    }
}

fn wait_request(target: TargetInfo, name_equals: &str, timeout_ms: u64) -> DispatchRequest {
    use super::super::adapter::ActionScope;
    use super::super::protocol::{ActionKind, ActionTarget};
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "wait".into(),
        generation: 1,
        target_id: target.target_id,
        target_generation: target.lifecycle_stamp,
        snapshot_id: "mock-snapshot".into(),
        geometry_revision: target.lifecycle_stamp,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: "mock-snapshot:1".into(),
        },
        parameters: serde_json::json!({
            "nameEquals": name_equals,
            "timeoutMs": timeout_ms,
        }),
        scope: ActionScope::Directed,
    }
}

fn type_text_request(target: TargetInfo, text: &str) -> DispatchRequest {
    let mut request = click_request(target);
    request.action_id = "type-text".into();
    request.action = ActionKind::TypeText;
    request.parameters = serde_json::json!({"text": text});
    request
}

fn navigation_request(
    target: &TargetInfo,
    action_id: &str,
    kind: NavigationKind,
    url: Option<&str>,
) -> NavigationRequest {
    NavigationRequest {
        run_id: "run".into(),
        target_id: target.target_id.clone(),
        target_generation: 1,
        action_id: action_id.into(),
        kind,
        url: url.map(str::to_string),
        cancellation: Default::default(),
    }
}

struct NavigationBrokerWorld {
    broker: Arc<super::super::ComputerUseBroker>,
    adapter: Arc<WebViewAdapter>,
    views: Vec<(String, Arc<TestView>)>,
    owners: Mutex<Vec<String>>,
}

struct NavigationGrant {
    token: String,
    run_id: String,
}

impl NavigationBrokerWorld {
    fn new(names: &[&str]) -> Self {
        let adapter = Arc::new(WebViewAdapter::new());
        let views = names
            .iter()
            .map(|name| ((*name).to_string(), TestView::named(name, 0)))
            .collect::<Vec<_>>();
        adapter.install_scope_fixture(
            views
                .iter()
                .map(|(_, view)| view.clone() as Arc<dyn LiveWebView>)
                .collect(),
        );
        let broker = Arc::new(super::super::ComputerUseBroker::request_endpoint(
            super::super::BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-wv-navigation-{}.lease", uuid::Uuid::new_v4())),
                ..Default::default()
            },
        ));
        broker
            .register_surface_adapter(super::super::SurfaceKind::WebView, adapter.clone())
            .unwrap();
        Self {
            broker,
            adapter,
            views,
            owners: Mutex::new(Vec::new()),
        }
    }

    fn view(&self, name: &str) -> Arc<TestView> {
        self.views
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, view)| view.clone())
            .unwrap()
    }

    fn grant(&self, owner: &str, selected: &[&str]) -> NavigationGrant {
        let connection = self.broker.reserve_transport_connection(owner).unwrap();
        let token = connection.token().to_string();
        self.broker
            .bind_transport_agent(&token, &format!("agent-{owner}"))
            .unwrap();
        self.broker
            .submit_access(
                &token,
                serde_json::json!({
                    "action": "request",
                    "purpose": "App WebView navigation replay regression",
                    "scopeIntent": "selected fixture pages only",
                    "surfaces": ["app-webview"],
                    "capabilities": ["observe", "input"]
                }),
            )
            .unwrap();
        let key = self
            .broker
            .host_task_view(&token)
            .unwrap()
            .request
            .unwrap()
            .key;
        let picker = self
            .broker
            .begin_task_scope_picker(&token, &key, super::super::SurfaceKind::WebView)
            .unwrap()
            .discover()
            .unwrap();
        let wanted = selected
            .iter()
            .map(|name| format!("owned fixture {name}"))
            .collect::<std::collections::HashSet<_>>();
        let candidate_ids = picker
            .views()
            .unwrap()
            .into_iter()
            .filter(|candidate| wanted.contains(&candidate.title))
            .map(|candidate| candidate.candidate_id)
            .collect::<Vec<_>>();
        assert_eq!(candidate_ids.len(), selected.len());
        let selection = picker.select(&candidate_ids).unwrap();
        let grok_computer_use_core::broker::AccessApprovalReady::Prepare(mut approval) = self
            .broker
            .reserve_selected_access_approval(&token, selection)
            .unwrap()
        else {
            panic!("fresh WebView grant must require preparation")
        };
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.prepare_publication().unwrap();
        let outcome = approval.commit_for_transport(&token).unwrap();
        self.owners.lock().push(owner.to_string());
        NavigationGrant {
            token,
            run_id: outcome.run_id,
        }
    }
}

impl Drop for NavigationBrokerWorld {
    fn drop(&mut self) {
        for owner in self.owners.get_mut().drain(..) {
            self.broker.fence_task_consent(&owner);
        }
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

fn navigation_tool_args(
    run_id: &str,
    target_id: Option<&str>,
    target_generation: u64,
    action_id: &str,
    kind: &str,
    url: Option<&str>,
) -> serde_json::Value {
    let mut args = serde_json::json!({
        "runId": run_id,
        "targetGeneration": target_generation,
        "actionId": action_id,
        "kind": kind
    });
    if let Some(target_id) = target_id {
        args["targetId"] = serde_json::json!(target_id);
    }
    if let Some(url) = url {
        args["url"] = serde_json::json!(url);
    }
    args
}

fn tool_payload(result: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

mod navigation;

mod actions;
mod upload;
pub(crate) mod upload_permission;
