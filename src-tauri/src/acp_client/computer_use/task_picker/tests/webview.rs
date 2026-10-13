//! Original ACP picker/publication with actual retained WebView receipts.
use super::*;
use crate::computer_use::webview::{LiveWebView, ScriptOperation, WebViewAdapter};
use grok_computer_use_core::{
    adapter::{ComputerUseAdapter, SurfaceKind},
    broker::BrokerOptions,
    fake::FakeAdapter,
    task_consent::HostRequestPhase,
};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

struct Page {
    alive: AtomicBool,
    generation: AtomicU64,
    label: String,
    native: String,
    title: String,
}

impl Page {
    fn new(name: &str) -> Arc<Self> {
        Arc::new(Self {
            alive: AtomicBool::new(true),
            generation: AtomicU64::new(1),
            label: format!("resource-browser-{name}"),
            native: format!("native-{name}"),
            title: format!("App page {name}"),
        })
    }
}

impl LiveWebView for Page {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        self.native.clone()
    }
    fn url(&self) -> Result<String, String> {
        Ok(format!("https://fixture.invalid/{}/", self.label))
    }
    fn title(&self) -> Result<String, String> {
        Ok(self.title.clone())
    }
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    fn with_document_authority(
        &self,
        generation: u64,
        commit: &mut dyn FnMut() -> Result<(), grok_computer_use_core::error::BrokerError>,
    ) -> Result<(), grok_computer_use_core::error::BrokerError> {
        if !self.is_alive() || self.navigation_generation() != generation {
            return Err(grok_computer_use_core::error::BrokerError::TargetUnauthorized);
        }
        commit()
    }
    fn navigate(&self, _url: &str) -> Result<u64, String> {
        Ok(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn host_script_owned(
        &self,
        _js: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        operation.finished();
        Err("ACP receipt test must not execute page scripts".into())
    }
}

struct WebViewFixture {
    client: Arc<AcpClient>,
    _peer: tokio::net::TcpStream,
    transport: Arc<RequestTransport>,
    adapter: Arc<WebViewAdapter>,
    pages: Vec<Arc<Page>>,
    key: HostRequestKey,
}

impl WebViewFixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::new(
            Arc::new(FakeAdapter::new()),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir().join(format!(
                    "cu-picker-acp-webview-{}.lease",
                    uuid::Uuid::new_v4()
                )),
                ..BrokerOptions::default()
            },
        ));
        let adapter = Arc::new(WebViewAdapter::new());
        let pages = vec![Page::new("first"), Page::new("second")];
        adapter.install_scope_fixture(
            pages
                .iter()
                .cloned()
                .map(|page| page as Arc<dyn LiveWebView>)
                .collect(),
        );
        broker
            .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
            .unwrap();
        let issued = broker.reserve_transport_connection(SESSION).unwrap();
        broker.bind_transport_agent(issued.token(), AGENT).unwrap();
        let transport = Arc::new(RequestTransport {
            broker,
            token: issued.token().into(),
            entry: Value::Null,
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: None,
            picker: ParkingMutex::new(None),
        });
        client.computer_use_connections.lock().insert(
            SESSION.into(),
            RequestConnection {
                transport: Some(transport.clone()),
                error: None,
            },
        );
        transport
            .broker
            .submit_access(
                &transport.token,
                json!({
                    "action":"request", "purpose":"original App WebView picker",
                    "scopeIntent":"two retained App pages", "surfaces":["app-webview"],
                    "capabilities":["observe","input"]
                }),
            )
            .unwrap();
        let key = transport
            .broker
            .host_task_view(&transport.token)
            .unwrap()
            .request
            .unwrap()
            .key;
        Self {
            client,
            _peer: peer,
            transport,
            adapter,
            pages,
            key,
        }
    }

    fn begin(&self) -> TaskPickerStarted {
        self.client
            .begin_computer_use_task_picker(SESSION, AGENT, &self.key, "app-webview", PROCESS)
            .unwrap()
    }

    async fn load(&self, picker: &str) -> TaskPickerView {
        self.client
            .load_picker_with(SESSION, AGENT, &self.key, picker, |_, call| async move {
                call.discover().map_err(|error| error.to_string())
            })
            .await
            .unwrap()
    }

    fn no_execution(&self) {
        assert!(!self.transport.broker.feature_enabled());
        assert_eq!(self.transport.broker.executable_grant_count(SESSION), 0);
        assert_eq!(self.transport.broker.continuation_count(SESSION), 0);
        assert!(self.adapter.list_targets().unwrap().is_empty());
    }

    async fn approve(
        &self,
        picker: &str,
        candidates: Vec<String>,
        pin: TaskApprovalPin,
    ) -> Result<CommitOutcome, String> {
        self.client
            .approve_picker_with(
                SESSION,
                AGENT,
                (&self.key, picker),
                candidates,
                pin,
                |_broker, mut approval, publication| async move {
                    approval
                        .publish_feature()
                        .map_err(|error| error.to_string())?;
                    approval
                        .prepare_execution()
                        .map_err(|error| error.to_string())?;
                    approval
                        .prepare_publication()
                        .map_err(|error| error.to_string())?;
                    publication(&mut approval)
                },
            )
            .await
    }

    async fn finish(self) {
        self.client.kill().await;
        self.transport.broker.fence_task_consent(SESSION);
        for ticket in self.transport.broker.take_cleanup_tickets() {
            self.transport.broker.finish_stop_cleanup(&ticket).unwrap();
        }
        assert!(self.adapter.list_targets().unwrap().is_empty());
    }
}

fn candidate_ids(view: &TaskPickerView) -> Vec<String> {
    view.candidates
        .iter()
        .map(|row| row.candidate_id.clone())
        .collect()
}

#[tokio::test]
async fn exact_app_webview_picker_uses_original_publication_pin_and_idempotent_selection() {
    let fixture = WebViewFixture::new().await;
    let picker = fixture.begin();
    assert_eq!(picker.surface, "app-webview");
    fixture.no_execution();
    let view = fixture.load(&picker.picker_id).await;
    assert_eq!(view.candidates.len(), 2);
    assert!(view
        .candidates
        .iter()
        .all(|row| row.surface == "app-webview"));
    let encoded = serde_json::to_string(&view).unwrap();
    assert!(!encoded.contains("native-first"));
    assert!(!encoded.contains("wv:selection:"));
    fixture.no_execution();

    let chosen = vec![candidate_ids(&view)[0].clone()];
    let hits = Arc::new(AtomicUsize::new(0));
    let expected_client = fixture.client.clone();
    let pin_hits = hits.clone();
    let pin: TaskApprovalPin = Arc::new(move |owner, approval| {
        assert!(owner.matches_endpoint(&expected_client, SESSION, AGENT));
        pin_hits.fetch_add(1, Ordering::SeqCst);
        owner.commit(approval)
    });
    let outcome = fixture
        .approve(&picker.picker_id, chosen.clone(), pin)
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.transport.broker.executable_grant_count(SESSION), 1);
    assert_eq!(fixture.transport.broker.continuation_count(SESSION), 1);
    let state = fixture
        .transport
        .broker
        .host_task_view(&fixture.transport.token)
        .unwrap();
    assert_eq!(state.request.unwrap().phase, HostRequestPhase::Approved);
    assert_eq!(state.grants.len(), 1);
    assert_eq!(state.grants[0].run_id, Some(outcome.run_id.clone()));
    assert_eq!(state.grants[0].members.len(), 1);
    assert_eq!(state.grants[0].members[0].surface, "app-webview");
    assert!(state.grants[0].members[0].target_id.starts_with("wv|"));
    assert_eq!(
        fixture
            .adapter
            .list_targets_for_run(&outcome.run_id)
            .unwrap()
            .len(),
        1
    );

    let retry_hits = Arc::new(AtomicUsize::new(0));
    let retry_counter = retry_hits.clone();
    let retry = fixture
        .approve(
            &picker.picker_id,
            chosen,
            Arc::new(move |_, _| {
                retry_counter.fetch_add(1, Ordering::SeqCst);
                Err("idempotent retry reached publication".into())
            }),
        )
        .await
        .unwrap();
    assert_eq!(retry.run_id, outcome.run_id);
    assert_eq!(retry.continuation_id, outcome.continuation_id);
    assert_eq!(retry_hits.load(Ordering::SeqCst), 0);
    assert!(fixture
        .approve(
            &picker.picker_id,
            candidate_ids(&view),
            Arc::new(|_, _| Err("changed selection reached publication".into())),
        )
        .await
        .is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn same_agent_transport_replacement_at_publication_rejects_and_reclaims_binding() {
    let fixture = WebViewFixture::new().await;
    let picker = fixture.begin();
    let view = fixture.load(&picker.picker_id).await;
    fixture.no_execution();
    let client = fixture.client.clone();
    let replacement = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
        BrokerOptions::default(),
    ));
    let issued = replacement.reserve_transport_connection(SESSION).unwrap();
    replacement
        .bind_transport_agent(issued.token(), AGENT)
        .unwrap();
    let fresh = Arc::new(RequestTransport {
        broker: replacement,
        token: issued.token().into(),
        entry: Value::Null,
        deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
        history_path: None,
        picker: ParkingMutex::new(None),
    });
    let pin: TaskApprovalPin = Arc::new(move |owner, approval| {
        client.computer_use_connections.lock().insert(
            SESSION.into(),
            RequestConnection {
                transport: Some(fresh.clone()),
                error: None,
            },
        );
        owner.commit(approval)
    });
    let result = fixture
        .approve(
            &picker.picker_id,
            vec![candidate_ids(&view)[0].clone()],
            pin,
        )
        .await;
    assert!(result.is_err());
    assert_eq!(fixture.transport.broker.executable_grant_count(SESSION), 0);
    assert_eq!(fixture.transport.broker.continuation_count(SESSION), 0);
    assert!(fixture.adapter.list_targets().unwrap().is_empty());
    assert_eq!(fixture.pages[0].navigation_generation(), 1);
    fixture.finish().await;
}

#[tokio::test]
async fn aliases_and_navigation_before_publication_fail_closed_without_binding_leaks() {
    let fixture = WebViewFixture::new().await;
    for alias in ["webview", "app_webview", "App-WebView"] {
        assert!(fixture
            .client
            .begin_computer_use_task_picker(SESSION, AGENT, &fixture.key, alias, PROCESS,)
            .is_err());
    }
    let picker = fixture.begin();
    let view = fixture.load(&picker.picker_id).await;
    fixture.pages[0]
        .navigate("https://fixture.invalid/replaced")
        .unwrap();
    assert!(fixture
        .approve(
            &picker.picker_id,
            vec![candidate_ids(&view)[0].clone()],
            Arc::new(|owner, approval| owner.commit(approval)),
        )
        .await
        .is_err());
    assert!(fixture.adapter.list_targets().unwrap().is_empty());
    assert_eq!(fixture.transport.broker.executable_grant_count(SESSION), 0);
    assert_eq!(fixture.transport.broker.continuation_count(SESSION), 0);
    fixture.finish().await;
}

#[cfg(windows)]
#[test]
fn original_owned_approval_worker_persists_then_publishes_the_webview_grant_once() {
    let root = std::env::temp_dir().join(format!(
        "cu-picker-acp-webview-approval-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let approvals = Arc::new(crate::computer_use::test_support::FixtureApprovals::new(
        &root,
    ));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = WebViewFixture::new().await;
        let picker = fixture.begin();
        let view = fixture.load(&picker.picker_id).await;
        fixture.no_execution();
        let hits = Arc::new(AtomicUsize::new(0));
        let pin_hits = hits.clone();
        let original_client = fixture.client.clone();
        let pin: TaskApprovalPin = Arc::new(move |owner, approval| {
            assert!(owner.matches_endpoint(&original_client, SESSION, AGENT));
            pin_hits.fetch_add(1, Ordering::SeqCst);
            owner.commit(approval)
        });
        let outcome = fixture
            .client
            .approve_webview_fixture(
                (SESSION, AGENT),
                (
                    &fixture.key,
                    &picker.picker_id,
                    vec![candidate_ids(&view)[0].clone()],
                ),
                pin,
                approvals.clone(),
            )
            .await
            .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(approvals.started(), 1);
        assert_eq!(approvals.owners(), 0);
        assert_eq!(approvals.settings()["computerUseEnabled"], true);
        assert_eq!(fixture.transport.broker.executable_grant_count(SESSION), 1);
        assert_eq!(fixture.transport.broker.continuation_count(SESSION), 1);
        assert_eq!(
            fixture
                .adapter
                .list_targets_for_run(&outcome.run_id)
                .unwrap()
                .len(),
            1
        );
        approvals.join().await.unwrap();
        let broker = fixture.transport.broker.clone();
        fixture.finish().await;
        approvals.reset_feature(&broker);
    });
    drop(runtime);
    drop(approvals);
    for name in ["settings.json", "settings.json.lock"] {
        let file = root.join(name);
        if file.exists() {
            assert!(!std::fs::symlink_metadata(&file)
                .unwrap()
                .file_type()
                .is_symlink());
            std::fs::remove_file(file).unwrap();
        }
    }
    std::fs::remove_dir(&root).unwrap();
}
