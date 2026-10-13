//! Explicitly synthetic transport fixtures, not native/App permission evidence.
use super::*;
use grok_computer_use_core::broker::BrokerOptions;
use grok_computer_use_core::task_consent::HostRequestPhase;
mod cleanup;
#[cfg(windows)]
mod managed_wire;
mod recovery;

pub(crate) struct Fixture {
    endpoint: TransportFixture,
    key: HostRequestKey,
}

/// The original bound ACP/HTTP transport before any model request exists.
pub(crate) struct TransportFixture {
    pub(crate) client: Arc<AcpClient>,
    _peer: tokio::net::TcpStream,
    transport: Arc<RequestTransport>,
    _http: Option<crate::computer_use::ipc::IpcServer>,
}

impl std::ops::Deref for Fixture {
    type Target = TransportFixture;
    fn deref(&self) -> &Self::Target {
        &self.endpoint
    }
}
impl std::ops::DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.endpoint
    }
}

impl Fixture {
    #[cfg(windows)]
    pub(crate) fn pending_managed_approval(
        &self,
        pin: TaskApprovalPin,
    ) -> (
        grok_computer_use_core::broker::AccessApproval,
        crate::computer_use::task_access::ApprovalPublication,
    ) {
        let super::super::managed_approval::ManagedApprovalStart::Prepare(original, approval) =
            self.client
                .start_managed_approval("view-app", "synthetic-view-agent", &self.key)
                .unwrap()
        else {
            panic!("first managed approval")
        };
        assert!(Arc::ptr_eq(&original, &self.transport));
        let publication = self.client.approval_publication(
            "view-app",
            "synthetic-view-agent",
            self.transport.clone(),
            pin,
        );
        (approval, publication)
    }

    pub(crate) fn pending_approval(
        &self,
        pin: TaskApprovalPin,
    ) -> (
        Arc<grok_computer_use_core::fake::FakeAdapter>,
        grok_computer_use_core::broker::AccessApproval,
        crate::computer_use::task_access::ApprovalPublication,
    ) {
        use grok_computer_use_core::broker::AccessApprovalReady;
        let fake = Arc::new(grok_computer_use_core::fake::FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        self.transport
            .broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::Desktop, fake.clone())
            .unwrap();
        let selected = self
            .transport
            .broker
            .capture_task_scope_selection(
                &self.transport.token,
                &self.key.request_id,
                self.key.revision,
                crate::computer_use::SurfaceKind::Desktop,
                &fake.fixture_id(),
            )
            .unwrap();
        let AccessApprovalReady::Prepare(approval) = self
            .transport
            .broker
            .reserve_selected_access_approval(&self.transport.token, selected)
            .unwrap()
        else {
            panic!("first approval")
        };
        let publication = self.client.approval_publication(
            "view-app",
            "synthetic-view-agent",
            self.transport.clone(),
            pin,
        );
        (fake, approval, publication)
    }

    fn approved_execution(
        &self,
    ) -> (
        Arc<grok_computer_use_core::fake::FakeAdapter>,
        grok_computer_use_core::broker::HostExecutionAddress,
    ) {
        let fake = Arc::new(grok_computer_use_core::fake::FakeAdapter::new());
        fake.set_synthetic_scope_instance(Some(1));
        self.transport
            .broker
            .register_surface_adapter(crate::computer_use::SurfaceKind::Desktop, fake.clone())
            .unwrap();
        self.transport
            .broker
            .bind_synthetic_scope(&self.key.request_id, &fake.fixture_id())
            .unwrap();
        let outcome = self
            .transport
            .broker
            .commit_access(
                &self.transport.token,
                &self.key.request_id,
                self.key.revision,
            )
            .unwrap();
        let execution = self
            .transport
            .broker
            .host_task_view(&self.transport.token)
            .unwrap()
            .executions
            .remove(0);
        (
            fake,
            grok_computer_use_core::broker::HostExecutionAddress {
                key: self.key.clone(),
                run_id: outcome.run_id,
                generation: execution.generation,
            },
        )
    }

    pub(crate) async fn new() -> Self {
        Self::new_for_surface("desktop").await
    }

    pub(crate) async fn new_managed() -> Self {
        Self::new_for_surface("managed-browser").await
    }

    pub(crate) async fn new_existing() -> Self {
        Self::new_for_surface("existing-tabs").await
    }

    pub(crate) fn request_key(&self) -> HostRequestKey {
        self.key.clone()
    }

    async fn new_for_surface(surface: &str) -> Self {
        let endpoint = TransportFixture::new_for_surface_http(surface, false).await;
        endpoint.transport.broker.submit_access(&endpoint.transport.token, json!({"action":"request", "purpose":"synthetic readback", "surfaces":[surface], "scopeIntent":"owned fixture", "capabilities":["observe"]})).unwrap();
        let key = endpoint
            .client
            .computer_use_task_view("view-app", "synthetic-view-agent")
            .unwrap()
            .request
            .unwrap()
            .key;
        Self { endpoint, key }
    }

    pub(crate) async fn finish(self) {
        self.endpoint.finish().await;
    }
}

impl TransportFixture {
    /// Deliberately leaves the original bearer alive to isolate ACP Arc identity.
    pub(crate) fn replace_approval_transport_same_identity(&self) {
        let replacement = Arc::new(RequestTransport {
            broker: self.transport.broker.clone(),
            token: self.transport.token.clone(),
            entry: Value::Null,
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: None,
            picker: ParkingMutex::new(None),
        });
        self.client.computer_use_connections.lock().insert(
            "view-app".into(),
            RequestConnection {
                transport: Some(replacement),
                error: None,
            },
        );
    }

    /// Host-only fixture readback of the retained original, never follows a slot.
    #[cfg(windows)]
    pub(crate) fn original_host_task_view(&self) -> Result<HostTaskView, String> {
        self.original_host_task_reader()()
    }

    #[cfg(windows)]
    pub(crate) fn original_host_task_reader(
        &self,
    ) -> impl Fn() -> Result<HostTaskView, String> + Send + Sync + 'static {
        let transport = self.transport.clone();
        move || {
            transport
                .broker
                .host_task_view(&transport.token)
                .map_err(|_| "original fixture transport unavailable".into())
        }
    }

    #[cfg(windows)]
    pub(crate) fn original_transport_is_current(&self) -> bool {
        self.client
            .computer_use_connections
            .lock()
            .get("view-app")
            .and_then(|connection| connection.transport.as_ref())
            .is_some_and(|current| Arc::ptr_eq(current, &self.transport))
    }

    pub(crate) fn broker(&self) -> Arc<crate::computer_use::ComputerUseBroker> {
        self.transport.broker.clone()
    }

    #[cfg(windows)]
    pub(crate) async fn new_existing_http() -> Self {
        Self::new_for_surface_http("existing-tabs", true).await
    }

    #[cfg(windows)]
    pub(crate) fn native_mcp_binding(&self) -> Value {
        json!({"endpoint":self._http.as_ref().expect("native fixture HTTP").url,
            "token":self.transport.token,"session":"view-app"})
    }

    async fn new_for_surface_http(surface: &str, real_http: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
            BrokerOptions {
                lease_path: std::env::temp_dir()
                    .join(format!("cu-task-view-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let issued = broker.reserve_transport_connection("view-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "synthetic-view-agent")
            .unwrap();
        let http = real_http.then(|| crate::computer_use::ipc::spawn(broker.clone()).unwrap());
        let transport = Arc::new(RequestTransport {
            broker,
            token: issued.token().into(),
            entry: if surface == "existing-tabs" {
                json!({"env":[{"name":"GROK_APP_CU_IPC","value":http.as_ref().map_or("http://127.0.0.1:9", |server| server.url.as_str())}]})
            } else {
                Value::Null
            },
            deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
            history_path: None,
            picker: ParkingMutex::new(None),
        });
        client.computer_use_connections.lock().insert(
            "view-app".into(),
            RequestConnection {
                transport: Some(transport.clone()),
                error: None,
            },
        );
        Self {
            client,
            _peer: peer,
            transport,
            _http: http,
        }
    }

    pub(crate) async fn finish(self) {
        self.client.kill().await;
        assert!(!self.client.is_alive());
        crate::computer_use::test_support::finish_fixture_cleanup(
            self.broker(),
            self.transport.broker.take_cleanup_tickets(),
            Duration::from_secs(20),
        )
        .await
        .expect("original fixture owners must be Stopped");
    }
}

#[tokio::test]
async fn original_bound_http_transport_does_not_precreate_a_request() {
    let fixture = TransportFixture::new_for_surface_http("existing-tabs", true).await;
    let view = fixture
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    assert!(view.request.is_none());
    assert!(view.grants.is_empty());
    assert!(view.executions.is_empty());
    assert!(!view.feature_enabled);
    assert_eq!(fixture.broker().continuation_count("view-app"), 0);
    assert_eq!(fixture.broker().capabilities().backend_id, "request-only");
    fixture.finish().await;
}

#[tokio::test]
async fn original_acp_readback_exposes_pending_without_starting_execution() {
    let fixture = Fixture::new().await;
    let view = fixture
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    assert_eq!(view.request.unwrap().key, fixture.key);
    assert!(view.executions.is_empty());
    assert!(view.grants.is_empty());
    assert!(!view.feature_enabled);
    assert_eq!(
        fixture.transport.broker.capabilities().backend_id,
        "request-only"
    );
    assert_eq!(fixture.transport.broker.continuation_count("view-app"), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn original_acp_deny_cancel_and_stop_work_without_a_run() {
    for (decision, expected) in [
        (HostTaskDecision::Deny, HostRequestPhase::Denied),
        (HostTaskDecision::Cancel, HostRequestPhase::Cancelled),
        (HostTaskDecision::Stop, HostRequestPhase::Cancelled),
    ] {
        let fixture = Fixture::new().await;
        let receipt = fixture
            .client
            .decide_computer_use_task("view-app", "synthetic-view-agent", &fixture.key, decision)
            .unwrap();
        assert_eq!(receipt.key, fixture.key);
        assert_eq!(receipt.decision, decision);
        let view = fixture
            .client
            .computer_use_task_view("view-app", "synthetic-view-agent")
            .unwrap();
        assert_eq!(view.request.unwrap().phase, expected);
        assert!(view.executions.is_empty());
        assert_eq!(
            fixture.transport.broker.executable_grant_count("view-app"),
            0
        );
        fixture.finish().await;
    }
}

#[tokio::test]
async fn acp_view_and_decision_reject_foreign_app_agent_and_request_before_fencing() {
    let fixture = Fixture::new().await;
    for (app, agent) in [
        ("foreign-app", "synthetic-view-agent"),
        ("view-app", "foreign-agent"),
    ] {
        assert!(fixture.client.computer_use_task_view(app, agent).is_err());
        assert!(fixture
            .client
            .decide_computer_use_task(app, agent, &fixture.key, HostTaskDecision::Stop)
            .is_err());
    }
    let foreign = HostRequestKey {
        request_id: "foreign".into(),
        ..fixture.key.clone()
    };
    assert!(fixture
        .client
        .decide_computer_use_task(
            "view-app",
            "synthetic-view-agent",
            &foreign,
            HostTaskDecision::Stop
        )
        .is_err());
    assert_eq!(
        fixture
            .transport
            .broker
            .host_task_view(&fixture.transport.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    fixture.finish().await;
}

#[tokio::test]
async fn old_card_cannot_stop_replacement_transport_even_with_same_agent_id() {
    let fixture = Fixture::new().await;
    let broker = fixture.transport.broker.clone();
    let fresh = broker.reserve_transport_connection("view-app").unwrap();
    broker
        .bind_transport_agent(fresh.token(), "synthetic-view-agent")
        .unwrap();
    let replacement = Arc::new(RequestTransport {
        broker: broker.clone(),
        token: fresh.token().into(),
        entry: Value::Null,
        deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
        history_path: None,
        picker: ParkingMutex::new(None),
    });
    fixture.client.computer_use_connections.lock().insert(
        "view-app".into(),
        RequestConnection {
            transport: Some(replacement.clone()),
            error: None,
        },
    );
    broker.submit_access(&replacement.token, json!({"action":"request", "purpose":"replacement fixture", "surfaces":["desktop"], "scopeIntent":"owned fixture", "capabilities":["observe"]})).unwrap();
    assert!(fixture
        .client
        .decide_computer_use_task(
            "view-app",
            "synthetic-view-agent",
            &fixture.key,
            HostTaskDecision::Stop
        )
        .is_err());
    assert!(broker.host_task_view(&fixture.transport.token).is_err());
    let view = fixture
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    assert_eq!(view.request.unwrap().phase, HostRequestPhase::Pending);
    assert_ne!(view.connection_id, fixture.key.connection_id);
    assert_eq!(
        broker
            .host_task_view(&replacement.token)
            .unwrap()
            .request
            .unwrap()
            .phase,
        HostRequestPhase::Pending
    );
    fixture.finish().await;
}

#[tokio::test]
async fn killed_or_missing_original_acp_does_not_boot_an_endpoint_for_ui_readback() {
    let fixture = Fixture::new().await;
    fixture
        .client
        .computer_use_connections
        .lock()
        .remove("view-app");
    assert!(fixture
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .is_err());
    assert!(fixture
        .client
        .decide_computer_use_task(
            "view-app",
            "synthetic-view-agent",
            &fixture.key,
            HostTaskDecision::Cancel
        )
        .is_err());
    fixture.client.kill().await;
    assert!(fixture
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .is_err());
    assert!(!fixture.client.is_alive());
}

#[tokio::test]
async fn task_watch_cannot_follow_same_agent_transport_replacement_on_a_different_broker() {
    let fixture = Fixture::new().await;
    let watch = fixture
        .client
        .computer_use_task_watch("view-app", "synthetic-view-agent")
        .unwrap();
    assert_eq!(
        watch.host_instance_id,
        fixture.transport.broker.host_instance_id()
    );
    assert!(fixture
        .client
        .read_computer_use_task_watch("view-app", "synthetic-view-agent", &watch)
        .is_ok());
    let replacement_broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
        BrokerOptions::default(),
    ));
    let fresh = replacement_broker
        .reserve_transport_connection("view-app")
        .unwrap();
    replacement_broker
        .bind_transport_agent(fresh.token(), "synthetic-view-agent")
        .unwrap();
    let replacement = Arc::new(RequestTransport {
        broker: replacement_broker,
        token: fresh.token().into(),
        entry: Value::Null,
        deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
        history_path: None,
        picker: ParkingMutex::new(None),
    });
    fixture.client.computer_use_connections.lock().insert(
        "view-app".into(),
        RequestConnection {
            transport: Some(replacement),
            error: None,
        },
    );
    // Old broker/token remain valid: rejection must be the ACP pointer fence,
    // not merely the old broker having happened to retire its connection.
    assert!(fixture
        .transport
        .broker
        .host_task_view(&fixture.transport.token)
        .is_ok());
    assert!(fixture
        .client
        .read_computer_use_task_watch("view-app", "synthetic-view-agent", &watch)
        .is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn watch_creation_and_read_reject_foreign_or_missing_owner_without_bootstrap() {
    let fixture = Fixture::new().await;
    assert!(fixture
        .client
        .computer_use_task_watch("foreign", "synthetic-view-agent")
        .is_err());
    assert!(fixture
        .client
        .computer_use_task_watch("view-app", "foreign")
        .is_err());
    let watch = fixture
        .client
        .computer_use_task_watch("view-app", "synthetic-view-agent")
        .unwrap();
    fixture
        .client
        .computer_use_connections
        .lock()
        .remove("view-app");
    assert!(fixture
        .client
        .computer_use_task_watch("view-app", "synthetic-view-agent")
        .is_err());
    assert!(fixture
        .client
        .read_computer_use_task_watch("view-app", "synthetic-view-agent", &watch)
        .is_err());
    assert_eq!(
        fixture.transport.broker.capabilities().backend_id,
        "request-only"
    );
    assert_eq!(fixture.transport.broker.continuation_count("view-app"), 0);
    fixture.finish().await;
}

#[tokio::test]
async fn native_control_uses_original_acp_task_without_legacy_session_mapping() {
    let f = Fixture::new().await;
    let (fake, address) = f.approved_execution();
    let listed_before_preview = fake.list_calls();
    let preview = f
        .client
        .computer_use_execution_call("view-app", "synthetic-view-agent", &address)
        .unwrap()
        .preview()
        .unwrap();
    assert_eq!(preview.run_id, address.run_id);
    assert_eq!(fake.observe_calls(), 1);
    assert_eq!(fake.list_calls(), listed_before_preview);
    f.client
        .computer_use_execution_call("view-app", "synthetic-view-agent", &address)
        .unwrap()
        .pause()
        .unwrap();
    let view = f
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    assert_eq!(
        view.executions[0].phase,
        grok_computer_use_core::broker::HostExecutionPhase::Paused
    );
    assert_ne!(view.executions[0].generation, address.generation);
    assert!(!view.grants[0].executable);
    f.finish().await;
}

#[tokio::test]
async fn a_queued_control_cannot_follow_same_agent_transport_replacement() {
    let f = Fixture::new().await;
    let (fake, address) = f.approved_execution();
    let pause = f
        .client
        .computer_use_execution_call("view-app", "synthetic-view-agent", &address)
        .unwrap();
    let preview = f
        .client
        .computer_use_execution_call("view-app", "synthetic-view-agent", &address)
        .unwrap();
    let replacement_broker = Arc::new(crate::computer_use::ComputerUseBroker::request_endpoint(
        BrokerOptions::default(),
    ));
    let issued = replacement_broker
        .reserve_transport_connection("view-app")
        .unwrap();
    replacement_broker
        .bind_transport_agent(issued.token(), "synthetic-view-agent")
        .unwrap();
    f.client.computer_use_connections.lock().insert(
        "view-app".into(),
        RequestConnection {
            transport: Some(Arc::new(RequestTransport {
                broker: replacement_broker,
                token: issued.token().into(),
                entry: Value::Null,
                deliveries: ParkingMutex::new(delivery::DeliveryLedger::default()),
                history_path: None,
                picker: ParkingMutex::new(None),
            })),
            error: None,
        },
    );
    // The old Core authority remains live in this deliberately synthetic race.
    // Rejection must come from the original ACP pointer, before native work.
    assert!(f
        .transport
        .broker
        .host_task_view(&f.transport.token)
        .is_ok());
    assert!(pause.pause().is_err());
    assert!(preview.preview().is_err());
    assert!(!f.transport.broker.is_paused(&address.run_id).unwrap());
    assert_eq!(fake.observe_calls(), 0);
    f.transport.retire();
    for ticket in f.transport.broker.take_cleanup_tickets() {
        f.transport.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    f.finish().await;
}

#[tokio::test]
async fn preview_result_after_acp_replacement_is_not_published() {
    let f = Fixture::new().await;
    let (fake, address) = f.approved_execution();
    let call = f
        .client
        .computer_use_execution_call("view-app", "synthetic-view-agent", &address)
        .unwrap();
    let client = f.client.clone();
    fake.arm_liveness_hook(Box::new(move || {
        client.computer_use_connections.lock().remove("view-app");
    }));
    assert!(call.preview().is_err());
    assert_eq!(fake.observe_calls(), 1);
    f.transport.retire();
    for ticket in f.transport.broker.take_cleanup_tickets() {
        f.transport.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    f.finish().await;
}

#[tokio::test]
async fn picker_descriptors_are_metadata_only_and_do_not_advertise_unimplemented_surfaces() {
    let f = Fixture::new().await;
    let view = f
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .unwrap();
    let expected = if cfg!(all(target_os = "windows", feature = "computer-use-probe")) {
        vec!["desktop", "managed-browser", "app-webview"]
    } else if cfg!(target_os = "windows") {
        vec!["desktop", "managed-browser"]
    } else if cfg!(any(target_os = "macos", target_os = "linux")) {
        vec!["managed-browser"]
    } else {
        vec![]
    };
    assert_eq!(view.picker_surfaces, expected);
    assert!(view.executions.is_empty());
    assert!(view.grants.is_empty());
    assert_eq!(
        view.request.as_ref().unwrap().phase,
        HostRequestPhase::Pending
    );
    assert_eq!(view.request.as_ref().unwrap().key, f.key);
    assert_eq!(f.transport.broker.continuation_count("view-app"), 0);
    assert!(!view.feature_enabled);
    assert_eq!(f.transport.broker.capabilities().backend_id, "request-only");
    f.finish().await;
}

#[tokio::test]
async fn managed_descriptor_and_watch_do_not_prepare_resources_or_borrow_a_picker() {
    let f = Fixture::new_managed().await;
    let watch = f
        .client
        .computer_use_task_watch("view-app", "synthetic-view-agent")
        .unwrap();
    let view = f
        .client
        .read_computer_use_task_watch("view-app", "synthetic-view-agent", &watch)
        .unwrap();
    assert_eq!(view.request.as_ref().unwrap().key, f.key);
    assert_eq!(
        view.request.as_ref().unwrap().phase,
        HostRequestPhase::Pending
    );
    assert!(view.grants.is_empty());
    assert!(view.executions.is_empty());
    assert!(!view.feature_enabled);
    assert!(!view.picker_surfaces.contains(&"existing-tabs".into()));
    assert_eq!(
        view.picker_surfaces.contains(&"app-webview".into()),
        cfg!(all(target_os = "windows", feature = "computer-use-probe"))
    );
    assert_eq!(f.transport.broker.capabilities().backend_id, "request-only");
    assert_eq!(f.transport.broker.continuation_count("view-app"), 0);
    assert!(f.transport.picker.lock().is_none());
    assert!(f
        .transport
        .broker
        .host_task_view(&f.transport.token)
        .unwrap()
        .picker_surfaces
        .is_empty());
    f.finish().await;
}
