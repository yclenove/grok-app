//! Core publication routing only; the Host adapter/native guard is synthetic.
use super::*;
use crate::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, NativeScopeIdentity,
    TargetInfo,
};
use crate::fake::FakeAdapter;
use crate::protocol::Observation;
use crate::task_consent::PrepareTicket;
use std::sync::atomic::AtomicUsize;

struct GuardedAdapter {
    fallback: FakeAdapter,
    current: Mutex<Vec<NativeScopeIdentity>>,
    guards: AtomicUsize,
    omit_callback: AtomicBool,
}
impl ComputerUseAdapter for GuardedAdapter {
    fn backend_id(&self) -> &'static str {
        "webview-publication-fixture"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::for_surface(SurfaceKind::WebView, self.backend_id())
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.fallback.list_targets()
    }
    fn target_alive(&self, target: &str) -> bool {
        self.fallback.target_alive(target)
    }
    fn observe(&self, target: &str) -> Result<Observation, String> {
        self.fallback.observe(target)
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.fallback.act(request)
    }
    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String> {
        self.fallback.abort(run_id, generation)
    }
    fn is_idle(&self, run_id: &str) -> bool {
        self.fallback.is_idle(run_id)
    }
    fn start_periodic_preview(&self, target_id: &str) {
        self.fallback.start_periodic_preview(target_id);
    }
    fn stop_periodic_preview(&self) {
        self.fallback.stop_periodic_preview();
    }
    fn periodic_preview_active(&self) -> bool {
        self.fallback.periodic_preview_active()
    }
    fn with_scope_authority(
        &self,
        identities: &[NativeScopeIdentity],
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        self.guards.fetch_add(1, Ordering::SeqCst);
        let current = self.current.lock();
        if current.as_slice() != identities {
            return Err(BrokerError::DeadTarget);
        }
        if self.omit_callback.load(Ordering::SeqCst) {
            return Ok(());
        }
        let result = commit();
        drop(current);
        result
    }
}

fn identity(native: &str) -> NativeScopeIdentity {
    NativeScopeIdentity::WebViewPage {
        target_id: format!("wv|resource-browser-{native}|native-{native}@binding|7|app|run"),
        native_tab_id: format!("native-{native}"),
        document_generation: 7,
        session_id: "app".into(),
        run_id: "run".into(),
    }
}
fn broker() -> ComputerUseBroker {
    ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        BrokerOptions {
            lease_path: std::env::temp_dir()
                .join(format!("cu-wv-authority-{}.lease", Uuid::new_v4())),
            ..Default::default()
        },
    )
}
fn fixture(
    identities: Vec<NativeScopeIdentity>,
) -> (ComputerUseBroker, Arc<GuardedAdapter>, PrepareTicket) {
    let broker = broker();
    let adapter = Arc::new(GuardedAdapter {
        fallback: FakeAdapter::new(),
        current: Mutex::new(identities.clone()),
        guards: AtomicUsize::new(0),
        omit_callback: AtomicBool::new(false),
    });
    broker
        .register_surface_adapter(SurfaceKind::WebView, adapter.clone())
        .unwrap();
    let ticket = ticket(&broker, identities);
    (broker, adapter, ticket)
}
fn ticket(broker: &ComputerUseBroker, identities: Vec<NativeScopeIdentity>) -> PrepareTicket {
    let executor = broker
        .require_surface_executor(SurfaceKind::WebView)
        .unwrap();
    let members: Vec<_> = identities
        .into_iter()
        .map(|identity| ScopeMember {
            target_id: identity.target_id().unwrap(),
            surface: SurfaceKind::WebView,
            display_name: None,
            native: Some(NativeScopeProof {
                executor_generation: executor.generation,
                identity,
            }),
        })
        .collect();
    PrepareTicket {
        id: 1,
        session_id: "app".into(),
        run_id: "run".into(),
        target_id: members[0].target_id.clone(),
        surface: SurfaceKind::WebView,
        native: members[0].native.clone(),
        members,
        request_id: "original-request".into(),
        revision: 1,
        task_id: "original-task".into(),
    }
}

#[test]
fn final_publication_uses_the_retained_adapter_group_guard_exactly_once() {
    let (broker, adapter, ticket) = fixture(vec![identity("a"), identity("b")]);
    let mut commits = 0;
    let outcome = broker
        .with_prepared_scope_authority(&ticket, || {
            commits += 1;
            Ok(43)
        })
        .unwrap();
    assert_eq!(outcome, 43);
    assert_eq!(commits, 1);
    assert_eq!(adapter.guards.load(Ordering::SeqCst), 1);
}

#[test]
fn document_retirement_after_publication_preflight_rejects_the_final_commit() {
    let (broker, adapter, ticket) = fixture(vec![identity("a")]);
    {
        let mut current = adapter.current.lock();
        let NativeScopeIdentity::WebViewPage {
            document_generation,
            ..
        } = &mut current[0]
        else {
            unreachable!()
        };
        *document_generation += 1;
    }
    let mut commits = 0;
    assert_eq!(
        broker.with_prepared_scope_authority(&ticket, || {
            commits += 1;
            Ok(())
        }),
        Err(BrokerError::DeadTarget)
    );
    assert_eq!(commits, 0);
    assert_eq!(adapter.guards.load(Ordering::SeqCst), 1);
}

#[test]
fn a_legacy_or_foreign_run_cannot_be_published_as_the_new_task_binding() {
    for field in ["run", "session", "executor", "surface"] {
        let (broker, adapter, mut ticket) = fixture(vec![identity("a")]);
        match field {
            "run" => ticket.run_id = "new-task-run".into(),
            "session" => ticket.session_id = "different-app".into(),
            "executor" => {
                ticket.members[0]
                    .native
                    .as_mut()
                    .unwrap()
                    .executor_generation += 1
            }
            "surface" => {
                ticket.members[0].surface = SurfaceKind::Desktop;
                ticket.members[0]
                    .native
                    .as_mut()
                    .unwrap()
                    .executor_generation = broker
                    .require_surface_executor(SurfaceKind::Desktop)
                    .unwrap()
                    .generation;
            }
            _ => unreachable!(),
        }
        let mut commits = 0;
        assert!(broker
            .with_prepared_scope_authority(&ticket, || {
                commits += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(commits, 0, "{field}");
        assert_eq!(adapter.guards.load(Ordering::SeqCst), 0, "{field}");
    }
}

#[test]
fn unsupported_native_authority_never_falls_back_to_a_liveness_read() {
    let broker = broker();
    broker
        .register_surface_adapter(SurfaceKind::WebView, Arc::new(FakeAdapter::new()))
        .unwrap();
    let ticket = ticket(&broker, vec![identity("a")]);
    let mut commits = 0;
    assert_eq!(
        broker.with_prepared_scope_authority(&ticket, || {
            commits += 1;
            Ok(())
        }),
        Err(BrokerError::TargetUnauthorized)
    );
    assert_eq!(commits, 0);
}

#[test]
fn an_adapter_returning_without_commit_cannot_manufacture_publication_success() {
    let (broker, adapter, ticket) = fixture(vec![identity("a")]);
    adapter.omit_callback.store(true, Ordering::SeqCst);
    assert_eq!(
        broker.with_prepared_scope_authority(&ticket, || Ok(43)),
        Err(BrokerError::IdentityMismatch("scope commit missing"))
    );
}

#[test]
fn a_core_commit_failure_is_returned_under_the_original_guard() {
    let (broker, adapter, ticket) = fixture(vec![identity("a")]);
    assert_eq!(
        broker.with_prepared_scope_authority::<()>(&ticket, || Err(BrokerError::StopRequested)),
        Err(BrokerError::StopRequested)
    );
    assert_eq!(adapter.guards.load(Ordering::SeqCst), 1);
}
