use super::gates;
use super::*;
use crate::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, SurfaceKind, TargetInfo,
};
use crate::fake::FakeAdapter;
use crate::protocol::{
    ActionKind, ActionRequest, ActionTarget, Observation, ObservationImage, ObservationNode,
    OutcomeKind, PROTOCOL_VERSION,
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

struct CountingSurfaceAdapter {
    surface: SurfaceKind,
    target_id: String,
    alive: AtomicBool,
    observes: AtomicU32,
    actions: AtomicU32,
    aborts: AtomicU32,
    abort_failures: AtomicU32,
    previews_started: AtomicU32,
    previews_stopped: AtomicU32,
    releases: AtomicU32,
    run_lists: parking_lot::Mutex<Vec<String>>,
    run_claims: parking_lot::Mutex<Vec<String>>,
    run_observes: parking_lot::Mutex<Vec<String>>,
    run_releases: parking_lot::Mutex<Vec<String>>,
}

impl CountingSurfaceAdapter {
    fn new(surface: SurfaceKind, target_id: &str) -> Self {
        Self {
            surface,
            target_id: target_id.into(),
            alive: AtomicBool::new(true),
            observes: AtomicU32::new(0),
            actions: AtomicU32::new(0),
            aborts: AtomicU32::new(0),
            abort_failures: AtomicU32::new(0),
            previews_started: AtomicU32::new(0),
            previews_stopped: AtomicU32::new(0),
            releases: AtomicU32::new(0),
            run_lists: parking_lot::Mutex::new(Vec::new()),
            run_claims: parking_lot::Mutex::new(Vec::new()),
            run_observes: parking_lot::Mutex::new(Vec::new()),
            run_releases: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn fail_next_aborts(&self, count: u32) {
        self.abort_failures.store(count, Ordering::SeqCst);
    }
}

impl ComputerUseAdapter for CountingSurfaceAdapter {
    fn backend_id(&self) -> &'static str {
        match self.surface {
            SurfaceKind::Desktop => "counting-desktop",
            SurfaceKind::ManagedBrowser => "counting-managed",
            SurfaceKind::ExistingTab => "counting-existing",
            SurfaceKind::WebView => "counting-webview",
        }
    }

    fn capabilities(&self) -> Capabilities {
        let mut capabilities = Capabilities::for_surface(self.surface, self.backend_id());
        capabilities.semantic_click = true;
        capabilities.click = crate::adapter::ActionSupport::SEMANTIC;
        capabilities
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(vec![TargetInfo {
            target_id: self.target_id.clone(),
            title: format!("{} fixture", self.surface.as_wire()),
            app_name: self.surface.as_wire().into(),
            kind: self.surface.as_wire().into(),
            pid: None,
            backend: self.backend_id().into(),
            execution_mode: "test".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: 1,
            display_id: "test".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: "test surface".into(),
        }])
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        self.run_lists.lock().push(run_id.to_string());
        self.list_targets()
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.alive.load(Ordering::SeqCst) && target_id == self.target_id
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        if !self.target_alive(target_id) {
            return Err("dead test target".into());
        }
        let sequence = self.observes.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Observation {
            text: String::new(),
            version: PROTOCOL_VERSION,
            run_id: String::new(),
            target_id: target_id.into(),
            target_generation: 1,
            snapshot_id: format!("surface-snapshot-{sequence}"),
            captured_at: "2026-09-13T00:00:00Z".into(),
            geometry_revision: 1,
            coordinate_space: "image-pixels".into(),
            image: ObservationImage {
                width: 1,
                height: 1,
                content_id: "surface-test".into(),
                png_base64: None,
            },
            nodes: vec![ObservationNode {
                node_ref: "surface-node".into(),
                role: "button".into(),
                name: "Run".into(),
                actions: vec!["click".into()],
                truncated: false,
                x: None,
                y: None,
                width: None,
                height: None,
            }],
            truncated: false,
            crop_x: 0,
            crop_y: 0,
            crop_width: 1,
            crop_height: 1,
            scale: 1.0,
            dpi: 96.0,
            origin_x: 0,
            origin_y: 0,
            topology_revision: 1,
        })
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.run_observes.lock().push(run_id.to_string());
        self.observe(target_id)
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        req.admit(self)?;
        self.actions.fetch_add(1, Ordering::SeqCst);
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: true,
            verifiable: true,
            detail: format!("{} action", self.surface.as_wire()),
        })
    }

    fn abort(&self, _run_id: &str, _generation: u64) -> Result<(), String> {
        self.aborts.fetch_add(1, Ordering::SeqCst);
        if self
            .abort_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err("injected surface cleanup failure".into());
        }
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        true
    }

    fn start_periodic_preview(&self, target_id: &str) {
        if target_id == self.target_id {
            self.previews_started.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn stop_periodic_preview(&self) {
        self.previews_stopped.fetch_add(1, Ordering::SeqCst);
    }

    fn periodic_preview_active(&self) -> bool {
        self.previews_started.load(Ordering::SeqCst) > self.previews_stopped.load(Ordering::SeqCst)
    }

    fn current_geometry_revision_for(&self, _target_id: &str) -> u64 {
        1
    }

    fn claim_target_for_run(&self, run_id: &str, target_id: &str) -> Result<(), String> {
        self.run_claims.lock().push(run_id.to_string());
        if self.target_alive(target_id) {
            Ok(())
        } else {
            Err("dead test target".into())
        }
    }

    fn release_target(&self, target_id: &str) {
        if target_id == self.target_id {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn release_target_for_run(&self, run_id: &str, target_id: &str) {
        self.run_releases.lock().push(run_id.to_string());
        self.release_target(target_id);
    }
}

fn click_request(
    run_id: &str,
    target_id: &str,
    generation: u64,
    obs: &Observation,
) -> ActionRequest {
    ActionRequest {
        version: PROTOCOL_VERSION,
        run_id: run_id.into(),
        action_id: format!("action-{run_id}"),
        target_id: target_id.into(),
        target_generation: generation,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: obs.nodes[0].node_ref.clone(),
        },
        parameters: serde_json::json!({}),
    }
}

#[test]
fn managed_browser_listing_does_not_fall_back_to_desktop() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    let desktop = broker
        .list_targets_for_surface(SurfaceKind::Desktop)
        .unwrap();
    assert!(
        desktop.iter().any(|t| t.title == "GrokCuFixture"),
        "{desktop:?}"
    );
    let managed = broker
        .list_targets_for_surface(SurfaceKind::ManagedBrowser)
        .unwrap();
    assert!(
        managed
            .iter()
            .all(|t| t.kind != "window" && t.title != "GrokCuFixture"),
        "managed-browser must not list desktop windows: {managed:?}"
    );
    let existing = broker
        .list_targets_for_surface(SurfaceKind::ExistingTab)
        .unwrap();
    assert!(
        existing
            .iter()
            .all(|t| t.kind != "window" && t.title != "GrokCuFixture"),
        "{existing:?}"
    );
    assert!(matches!(
        broker
            .list_targets_for_surface(SurfaceKind::WebView)
            .unwrap_err(),
        BrokerError::SurfaceUnavailable {
            surface: SurfaceKind::WebView
        }
    ));
}

#[test]
fn host_picker_preserves_run_for_desktop_and_retained_webview_targets() {
    let desktop = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::Desktop,
        "wayland:owned",
    ));
    let webview = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::WebView,
        "wv|owned",
    ));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker
        .register_surface_adapter(SurfaceKind::WebView, webview.clone())
        .unwrap();
    broker.open_run("picker-chat", "picker-run").unwrap();
    let windows = broker
        .list_targets_for_surface_in_run(SurfaceKind::Desktop, "picker-run")
        .unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].target_id, "wayland:owned");
    assert_eq!(*desktop.run_lists.lock(), ["picker-run"]);
    assert!(webview.run_lists.lock().is_empty());
    let pages = broker
        .list_targets_for_surface_in_run(SurfaceKind::WebView, "picker-run")
        .unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].target_id, "wv|owned");
    assert_eq!(*webview.run_lists.lock(), ["picker-run"]);
    assert_eq!(*desktop.run_lists.lock(), ["picker-run"]);
    // Listing metadata is not implicit target authorization or input.
    assert!(broker.authorized_target("picker-run").is_err());
    assert!(desktop.run_claims.lock().is_empty());
    assert!(webview.run_claims.lock().is_empty());
}

#[test]
fn authorized_metadata_uses_the_exact_bound_executor_for_all_surfaces() {
    for (surface, target) in [
        (SurfaceKind::Desktop, "wayland:owned"),
        (SurfaceKind::ManagedBrowser, "managed:owned"),
        (SurfaceKind::ExistingTab, "tab:7"),
        (SurfaceKind::WebView, "wv:owned"),
    ] {
        let desktop = Arc::new(CountingSurfaceAdapter::new(
            SurfaceKind::Desktop,
            "wayland:desktop",
        ));
        let selected = Arc::new(CountingSurfaceAdapter::new(surface, target));
        let broker = ComputerUseBroker::new(
            if surface == SurfaceKind::Desktop {
                selected.clone()
            } else {
                desktop.clone()
            },
            gates::enabled_opts(100),
        );
        if surface != SurfaceKind::Desktop {
            broker
                .register_surface_adapter(surface, selected.clone())
                .unwrap();
        }
        broker.open_run("chat", "readback-run").unwrap();
        if surface == SurfaceKind::Desktop {
            broker
                .authorize_desktop_window("readback-run", target)
                .unwrap();
        } else {
            // Exercise the committed binding boundary; browser picker/worker
            // negotiation has its own real-adapter tests, not this double.
            broker
                .authorize_non_desktop("readback-run", target, surface)
                .unwrap();
        }
        selected.run_lists.lock().clear();
        let row = broker
            .authorized_target_info("readback-run", surface)
            .unwrap();
        assert_eq!(row.target_id, target);
        assert_eq!(row.app_name, surface.as_wire());
        assert_eq!(row.backend, selected.backend_id());
        assert_eq!(*selected.run_lists.lock(), ["readback-run"]);
        assert!(desktop.run_lists.lock().is_empty());
        assert_eq!(selected.actions.load(Ordering::SeqCst), 0);
        assert_eq!(selected.observes.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn authorized_metadata_rejects_dead_target_and_stopped_run() {
    let adapter = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::WebView,
        "wv:owned",
    ));
    let broker = ComputerUseBroker::new(Arc::new(FakeAdapter::new()), gates::enabled_opts(100));
    broker
        .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
        .unwrap();
    broker.open_run("chat", "owned-run").unwrap();
    broker
        .authorize_on_surface("chat", "owned-run", SurfaceKind::WebView, "wv:owned")
        .unwrap();
    adapter.alive.store(false, Ordering::SeqCst);
    assert!(matches!(
        broker.authorized_target_info("owned-run", SurfaceKind::WebView),
        Err(BrokerError::DeadTarget)
    ));
    broker.fence_stop("owned-run").unwrap();
    adapter.run_lists.lock().clear();
    assert!(matches!(
        broker.authorized_target_info("owned-run", SurfaceKind::WebView),
        Err(BrokerError::StopRequested)
    ));
    assert!(adapter.run_lists.lock().is_empty());
}

#[test]
fn host_picker_unknown_run_and_disabled_feature_fail_before_native_enumeration() {
    let desktop = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::Desktop,
        "wayland:owned",
    ));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    assert!(matches!(
        broker.list_targets_for_surface_in_run(SurfaceKind::Desktop, "missing"),
        Err(BrokerError::RunNotFound)
    ));
    assert!(desktop.run_lists.lock().is_empty());
    broker.open_run("picker-chat", "picker-run").unwrap();
    broker.set_feature_enabled(false);
    assert!(matches!(
        broker.list_targets_for_surface_in_run(SurfaceKind::Desktop, "picker-run"),
        Err(BrokerError::FeatureDisabled)
    ));
    assert!(desktop.run_lists.lock().is_empty());
}

#[test]
fn host_picker_keeps_pre_authorization_browser_choices_without_desktop_fallback() {
    let desktop = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::Desktop,
        "wayland:owned",
    ));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker.open_run("picker-chat", "picker-run").unwrap();
    // Seed actual pre-authorization choices: comparing two empty lists would
    // not detect accidentally replacing this picker with run-owned tabs.
    let root = std::env::temp_dir().join(format!("cu-picker-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("alice")).unwrap();
    broker.tabs().set_profile_root(root.clone());
    let pairing = broker.tabs().handshake_pairing().unwrap();
    let origin = format!("chrome-extension://{}", crate::pairing::EXTENSION_ID);
    broker
        .tabs()
        .offer_shared_tab(crate::browser::SharedTabOffer {
            pairing_token: &pairing,
            origin: &origin,
            extension_id: Some(crate::pairing::EXTENSION_ID),
            tab_id: "picker-user-tab",
            title: "User-shared candidate",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 7,
            connection_generation: 1,
            focused: true,
        })
        .unwrap();
    assert!(broker.tabs().list_for_run("picker-run").is_empty());
    for surface in [SurfaceKind::ManagedBrowser, SurfaceKind::ExistingTab] {
        let before = broker.list_targets_for_surface(surface).unwrap();
        assert_eq!(before.len(), 1, "nonempty {surface:?} picker fixture");
        let scoped = broker
            .list_targets_for_surface_in_run(surface, "picker-run")
            .unwrap();
        assert_eq!(
            before.iter().map(|t| &t.target_id).collect::<Vec<_>>(),
            scoped.iter().map(|t| &t.target_id).collect::<Vec<_>>()
        );
        assert!(scoped.iter().all(|t| t.target_id != "wayland:owned"));
    }
    assert!(desktop.run_lists.lock().is_empty());
    assert!(matches!(
        broker.list_targets_for_surface_in_run(SurfaceKind::WebView, "picker-run"),
        Err(BrokerError::SurfaceUnavailable {
            surface: SurfaceKind::WebView
        })
    ));
    assert!(desktop.run_lists.lock().is_empty());
    assert!(broker.authorized_target("picker-run").is_err());
    assert!(broker.tabs().list_for_run("picker-run").is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_wire_surface_is_unsupported() {
    let err = SurfaceKind::from_wire("desktop-fallback").unwrap_err();
    assert!(err.starts_with("unsupported_surface:"), "{err}");
    assert_eq!(
        SurfaceKind::from_wire("browser").unwrap().as_wire(),
        "managed-browser"
    );
    assert!(SurfaceKind::from_wire("computer-use")
        .unwrap_err()
        .contains("unsupported_surface"));
}

#[test]
fn profile_id_never_reaches_desktop_adapter() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("sess", "run-a").unwrap();
    let err = broker
        .authorize_target("run-a", "managed-profile:alice")
        .expect_err("profile id");
    assert!(
        matches!(err, BrokerError::DeadTarget)
            || matches!(
                err,
                BrokerError::SurfaceUnavailable {
                    surface: SurfaceKind::ManagedBrowser
                }
            )
            || err.to_string().contains("desktop window")
            || err.to_string().contains("open_managed_profile"),
        "{err}"
    );
    assert_eq!(fake.executions().len(), 0, "desktop adapter must not run");
}

#[test]
fn webview_and_existing_tabs_do_not_fallback_to_desktop() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("sess", "run-a").unwrap();
    let wv = broker
        .authorize_on_surface("sess", "run-a", SurfaceKind::WebView, "not-a-webview")
        .expect_err("webview");
    assert!(matches!(wv, BrokerError::DeadTarget), "{wv}");
    let tab = broker
        .authorize_on_surface(
            "sess",
            "run-a",
            SurfaceKind::ExistingTab,
            "existing-tab:missing",
        )
        .expect_err("existing");
    assert!(
        matches!(
            tab,
            BrokerError::SurfaceUnavailable {
                surface: SurfaceKind::ExistingTab
            }
        ),
        "{tab}"
    );
    assert_eq!(fake.executions().len(), 0);
}

#[test]
fn unknown_wire_surface_router_is_unsupported() {
    let err = crate::broker::SurfaceRouter::parse_wire("desktop-fallback").unwrap_err();
    assert!(err.to_string().contains("unsupported_surface"), "{err}");
}

#[test]
fn cross_surface_generation_reuse_is_rejected() {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("sess", "run-a").unwrap();
    let desktop = fake.fixture_id().to_string();
    broker
        .authorize_on_surface("sess", "run-a", SurfaceKind::Desktop, &desktop)
        .expect("desktop");
    let err = broker
        .authorize_on_surface("sess", "run-a", SurfaceKind::WebView, "wv|x|y|1|s|r")
        .expect_err("cross surface");
    assert!(err.to_string().contains("across surfaces"), "{err}");
}

#[test]
fn unregistered_non_desktop_surface_is_typed_unavailable_without_desktop_calls() {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker.open_run("sess", "run-a").unwrap();

    let err = broker
        .authorize_on_surface(
            "sess",
            "run-a",
            SurfaceKind::WebView,
            "wv|fixture|tab|1|sess|run-a",
        )
        .expect_err("missing WebView executor must fail closed");

    assert!(
        matches!(
            err,
            BrokerError::SurfaceUnavailable {
                surface: SurfaceKind::WebView
            }
        ),
        "{err}"
    );
    assert_eq!(desktop.observe_calls(), 0);
    assert!(desktop.executions().is_empty());
}

#[test]
fn generic_webview_route_uses_registered_executor_for_full_lifecycle() {
    let desktop = Arc::new(FakeAdapter::new());
    let webview_target = "wv|fixture|tab|1|sess|run-wv";
    let webview = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::WebView,
        webview_target,
    ));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker
        .register_surface_adapter(SurfaceKind::WebView, webview.clone())
        .unwrap();
    broker.open_run("sess", "run-wv").unwrap();
    let generation = broker
        .authorize_on_surface("sess", "run-wv", SurfaceKind::WebView, webview_target)
        .unwrap();

    assert_eq!(
        broker.capabilities_for_run("run-wv").unwrap().backend_id,
        "counting-webview"
    );
    assert!(broker
        .list_targets("run-wv")
        .unwrap()
        .iter()
        .any(|target| target.target_id == webview_target));
    let observation = broker.observe("run-wv").unwrap();
    assert_eq!(
        broker
            .act(click_request(
                "run-wv",
                webview_target,
                generation,
                &observation,
            ))
            .kind,
        OutcomeKind::Verified
    );
    broker.set_preview_visible("run-wv", true).unwrap();
    assert_eq!(broker.request_stop("run-wv").unwrap(), StopState::Stopped);

    assert_eq!(webview.observes.load(Ordering::SeqCst), 1);
    assert_eq!(webview.actions.load(Ordering::SeqCst), 1);
    assert_eq!(webview.previews_started.load(Ordering::SeqCst), 1);
    assert!(webview.previews_stopped.load(Ordering::SeqCst) >= 1);
    assert_eq!(webview.aborts.load(Ordering::SeqCst), 1);
    assert_eq!(webview.releases.load(Ordering::SeqCst), 1);
    assert_eq!(desktop.observe_calls(), 0);
    assert!(desktop.executions().is_empty());
    assert_eq!(desktop.abort_called(), 0);
    assert_eq!(&*webview.run_claims.lock(), &["run-wv"]);
    assert!(webview.run_lists.lock().iter().all(|run| run == "run-wv"));
    assert_eq!(&*webview.run_observes.lock(), &["run-wv"]);
    assert_eq!(&*webview.run_releases.lock(), &["run-wv"]);
}

#[test]
fn managed_action_does_not_contend_for_desktop_lease() {
    let desktop_a = Arc::new(FakeAdapter::new());
    let mut opts_a = gates::enabled_opts(100);
    let lease_path = opts_a.lease_path.clone();
    let desktop_b = Arc::new(FakeAdapter::new());
    let mut opts_b = gates::enabled_opts(100);
    opts_b.lease_path = lease_path;
    opts_a.instance_id = "desktop-owner".into();
    opts_b.instance_id = "managed-owner".into();

    let desktop_broker = ComputerUseBroker::new(desktop_a.clone(), opts_a);
    desktop_broker.open_run("sess-a", "run-desktop").unwrap();
    let desktop_target = desktop_a.fixture_id();
    let desktop_generation = desktop_broker
        .authorize_target("run-desktop", &desktop_target)
        .unwrap();
    let desktop_observation = desktop_broker.observe("run-desktop").unwrap();
    assert_eq!(
        desktop_broker
            .act(click_request(
                "run-desktop",
                &desktop_target,
                desktop_generation,
                &desktop_observation,
            ))
            .kind,
        OutcomeKind::Verified
    );

    let managed_target = "managed:profile:page";
    let managed = Arc::new(CountingSurfaceAdapter::new(
        SurfaceKind::ManagedBrowser,
        managed_target,
    ));
    let managed_broker = ComputerUseBroker::new(desktop_b.clone(), opts_b);
    managed_broker
        .tabs()
        .set_worker(Arc::new(crate::browser::RecordingBrowserWorker::default()));
    managed_broker
        .register_surface_adapter(SurfaceKind::ManagedBrowser, managed.clone())
        .unwrap();
    managed_broker.open_run("sess-b", "run-managed").unwrap();
    let managed_generation = managed_broker
        .authorize_on_surface(
            "sess-b",
            "run-managed",
            SurfaceKind::ManagedBrowser,
            managed_target,
        )
        .unwrap();
    let managed_observation = managed_broker.observe("run-managed").unwrap();
    let managed_outcome = managed_broker.act(click_request(
        "run-managed",
        managed_target,
        managed_generation,
        &managed_observation,
    ));

    assert_eq!(
        managed_outcome.kind,
        OutcomeKind::Verified,
        "{managed_outcome:?}"
    );
    assert_eq!(managed.actions.load(Ordering::SeqCst), 1);
    assert!(desktop_b.executions().is_empty());
    desktop_broker.request_stop("run-desktop").unwrap();
}

#[test]
fn failed_surface_stop_cleanup_stays_pending_and_retries_before_release() {
    let desktop = Arc::new(FakeAdapter::new());
    let target_id = "wv|fixture|tab|1|sess|run-wv";
    let webview = Arc::new(CountingSurfaceAdapter::new(SurfaceKind::WebView, target_id));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker
        .register_surface_adapter(SurfaceKind::WebView, webview.clone())
        .unwrap();
    broker.open_run("sess", "run-wv").unwrap();
    broker
        .authorize_on_surface("sess", "run-wv", SurfaceKind::WebView, target_id)
        .unwrap();
    webview.fail_next_aborts(1);

    let error = broker
        .request_stop("run-wv")
        .expect_err("failed cleanup must not publish stopped");
    assert!(
        error.to_string().contains("surface cancellation"),
        "{error}"
    );
    assert_eq!(
        broker.stop_state("run-wv").unwrap(),
        StopState::StopRequested
    );
    assert_eq!(webview.aborts.load(Ordering::SeqCst), 1);
    assert_eq!(webview.releases.load(Ordering::SeqCst), 0);
    assert_eq!(desktop.abort_called(), 0);

    assert_eq!(broker.request_stop("run-wv").unwrap(), StopState::Stopped);
    assert_eq!(webview.aborts.load(Ordering::SeqCst), 2);
    assert_eq!(webview.releases.load(Ordering::SeqCst), 1);
    assert_eq!(desktop.abort_called(), 0);
}

#[test]
fn failed_surface_pause_cleanup_stays_pending_until_idempotent_retry() {
    let desktop = Arc::new(FakeAdapter::new());
    let target_id = "wv|fixture|tab|1|sess|run-wv";
    let webview = Arc::new(CountingSurfaceAdapter::new(SurfaceKind::WebView, target_id));
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    broker
        .register_surface_adapter(SurfaceKind::WebView, webview.clone())
        .unwrap();
    broker.open_run("sess", "run-wv").unwrap();
    broker
        .authorize_on_surface("sess", "run-wv", SurfaceKind::WebView, target_id)
        .unwrap();
    webview.fail_next_aborts(1);

    let error = broker
        .pause("run-wv")
        .expect_err("failed pause cleanup must remain visible");
    assert!(error.to_string().contains("surface pause"), "{error}");
    assert!(broker.is_paused("run-wv").unwrap());
    let resume_error = broker
        .resume("run-wv")
        .expect_err("resume must wait for pending pause cleanup");
    assert!(resume_error.to_string().contains("cleanup still pending"));
    assert_eq!(webview.aborts.load(Ordering::SeqCst), 1);
    assert_eq!(webview.releases.load(Ordering::SeqCst), 0);
    assert_eq!(desktop.abort_called(), 0);

    broker.pause("run-wv").unwrap();
    assert_eq!(webview.aborts.load(Ordering::SeqCst), 2);
    broker.resume("run-wv").unwrap();
    assert!(!broker.is_paused("run-wv").unwrap());
    assert_eq!(desktop.abort_called(), 0);
}
