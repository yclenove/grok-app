//! Actual GTK xdg-foreign export -> private Portal -> PipeWire/EI -> Broker.
//! Isolated labwc is not native GNOME or installed App acceptance.
use super::*;
use crate::registry::SelectionOwner;
use crate::tests::{
    adapter::FixturePolicy,
    authorization::broker,
    registry::{closed, ready},
};
use grok_computer_use_core::{
    adapter::{ComputerUseAdapter, SurfaceKind},
    native_parent::NativeParent,
    protocol::{ActionRequest, PROTOCOL_VERSION},
    session_grants::{AuthorizationTicket, SessionGrants},
};
use grok_computer_use_gtk_parent::ParentLease;
use gtk::prelude::*;
use std::sync::mpsc;
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<ParentLease, String>>;
enum UiCommand {
    Export {
        ticket: AuthorizationTicket,
        reply: Reply,
        held: Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>,
    },
    Hold(oneshot::Sender<()>, mpsc::Receiver<()>),
    HideAndHold(oneshot::Sender<()>, mpsc::Receiver<()>),
    Stop,
}
struct Ui {
    queue: mpsc::Sender<UiCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Ui {
    async fn start() -> Self {
        assert_eq!(
            std::env::var("GROK_CU_PARENT_FIXTURE").unwrap(),
            "owned-labwc"
        );
        assert!(std::env::var("XDG_RUNTIME_DIR")
            .unwrap()
            .starts_with("/var/tmp/grok-cu-parent-runtime."));
        let (queue, commands) = mpsc::channel();
        let (boot, booted) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("owned-gtk-parent-main".into())
            .spawn(move || {
                gtk::init().expect("owned GTK initialization");
                let context = gtk::glib::MainContext::default();
                let window = gtk::Window::new(gtk::WindowType::Toplevel);
                window.set_title("Owned Portal parent integration");
                window.set_default_size(240, 120);
                boot.send(()).unwrap();
                loop {
                    while context.pending() {
                        context.iteration(false);
                    }
                    match commands.recv_timeout(Duration::from_millis(2)) {
                        Ok(UiCommand::Export {
                            ticket,
                            reply,
                            held,
                        }) => {
                            if reply.is_closed() {
                                continue;
                            }
                            let result = ticket.check_preparation().and_then(|()| {
                                window.show_all();
                                let deadline = Instant::now() + Duration::from_secs(3);
                                while !window.is_mapped() {
                                    assert!(Instant::now() < deadline, "owned window did not map");
                                    context.iteration(false);
                                }
                                grok_computer_use_gtk_parent::export(&window, &ticket)
                            });
                            if let Some((entered, release)) = held {
                                assert!(result.is_ok());
                                entered.send(()).unwrap();
                                release.recv_timeout(Duration::from_secs(10)).unwrap();
                            }
                            let _ = reply.send(result);
                        }
                        Ok(UiCommand::Hold(entered, release)) => {
                            entered.send(()).unwrap();
                            release.recv_timeout(Duration::from_secs(10)).unwrap();
                        }
                        Ok(UiCommand::HideAndHold(entered, release)) => {
                            window.hide(); // unmap invalidates the lease synchronously
                            entered.send(()).unwrap();
                            release.recv_timeout(Duration::from_secs(10)).unwrap();
                        }
                        Ok(UiCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                unsafe {
                    window.destroy();
                }
                while context.pending() {
                    context.iteration(false);
                }
            })
            .unwrap();
        booted.await.unwrap();
        Self {
            queue,
            thread: Some(thread),
        }
    }
    async fn hold(&self, hide: bool) -> mpsc::Sender<()> {
        let (entered, ack) = oneshot::channel();
        let (release, wait) = mpsc::channel();
        self.queue
            .send(if hide {
                UiCommand::HideAndHold(entered, wait)
            } else {
                UiCommand::Hold(entered, wait)
            })
            .unwrap();
        ack.await.unwrap();
        release
    }
}
impl Drop for Ui {
    fn drop(&mut self) {
        let _ = self.queue.send(UiCommand::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("original GTK owner did not join");
        }
    }
}

fn select(
    registry: &Arc<PortalRegistry>,
    fixture: &Fixture,
    ticket: &AuthorizationTicket,
    ui: &Ui,
    held: Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>,
) -> (PortalSelection, oneshot::Receiver<()>) {
    let address = fixture.bus.address.clone();
    let mut options = PortalOptions::new(ticket.run_id.clone(), SourceKind::Monitor);
    options.timeout = Duration::from_secs(5);
    let queue = ui.queue.clone();
    let queued = ticket.clone();
    let weak = Arc::downgrade(registry);
    let (dispatch, dispatched) = oneshot::channel();
    let selection = registry
        .select_owned(
            options,
            SelectionOwner {
                generation: 1,
                target_generation: 1,
                preparation: Some(ticket.clone()),
                policy: None,
                parent: Some(Box::new(move || {
                    Box::pin(async move {
                        assert!(!weak.upgrade().unwrap().is_idle(&queued.run_id));
                        let (reply, delivery) = oneshot::channel();
                        queue
                            .send(UiCommand::Export {
                                ticket: queued,
                                reply,
                                held,
                            })
                            .map_err(|_| "GTK queue closed")?;
                        let _ = dispatch.send(());
                        grok_computer_use_core::native_parent::retain_parent_dispatch(delivery)
                            .await
                            .map(|lease| Box::new(lease) as Box<dyn NativeParent>)
                    })
                })),
            },
            move |options| PortalSession::on_native_test_bus(options, address),
        )
        .unwrap();
    (selection, dispatched)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires private owned labwc, GTK, PipeWire and libeis fixtures"]
async fn gtk_parent_registry_joins_original_ui_and_native_input_owners() {
    let ui = Ui::start().await;
    // Actual queued GTK work must remain owned, both before export and after
    // native creation but before the lease crosses the thread boundary.
    for after_export in [false, true] {
        eprintln!("PARENT_REGISTRY_CASE queued-cancel after_export={after_export}");
        let fixture = Fixture::new(Mode::Normal).await;
        let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
        let broker = broker(&registry);
        let grants = SessionGrants::default();
        let ticket = grants.begin(&broker, "queued", None).unwrap();
        let (selection, release) = if after_export {
            let (entered, ack) = oneshot::channel();
            let (release, wait) = mpsc::channel();
            let (selection, dispatched) =
                select(&registry, &fixture, &ticket, &ui, Some((entered, wait)));
            dispatched.await.unwrap();
            ack.await.unwrap();
            (selection, release)
        } else {
            let release = ui.hold(false).await;
            let (selection, dispatched) = select(&registry, &fixture, &ticket, &ui, None);
            dispatched.await.unwrap();
            (selection, release)
        };
        let fence = grants.fence_fail_checked(&broker, &ticket, || {});
        assert!(ticket.is_preparation_cancelled());
        assert!(!registry.is_idle(&ticket.run_id));
        assert!(registry.forget(&selection).is_err());
        assert!(!fixture.shared.called("Start"));
        release.send(()).unwrap();
        closed(&registry, &selection).await;
        assert!(!fixture.shared.called("Start"));
        broker.finish_stop_cleanup(&fence.cleanup.unwrap()).unwrap();
        registry.forget(&selection).unwrap();
    }
    eprintln!("PARENT_REGISTRY_CASE live-input-and-parent-unmap");
    let native = Native::start().await;
    let eis = crate::tests::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "live", None).unwrap();
    let (selection, dispatched) = select(&registry, &fixture, &ticket, &ui, None);
    dispatched.await.unwrap();
    let target = ready(&registry, &selection).await;
    let parent = fixture
        .shared
        .parents
        .lock()
        .unwrap()
        .first()
        .unwrap()
        .clone();
    assert!(parent.starts_with("wayland:") && !parent.contains("fixture"));
    native.link(None).await;
    let generation = broker.authorize_target(&ticket.run_id, &target).unwrap();
    let metadata = broker
        .authorized_target_info(&ticket.run_id, SurfaceKind::Desktop)
        .unwrap();
    assert_eq!(metadata.target_id, target);
    assert_eq!(metadata.kind, "monitor");
    assert_eq!(metadata.app_name, "Wayland portal");
    assert_eq!(metadata.backend, "wayland-portal");
    grants.publish(&broker, &ticket, true, || {}).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let observation = loop {
        match broker.observe(&ticket.run_id) {
            Ok(image) => break image,
            Err(error) => {
                assert!(Instant::now() < deadline, "parented capture: {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    };
    let mut request = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: uuid::Uuid::new_v4().to_string(),
        run_id: ticket.run_id.clone(),
        target_id: target.clone(),
        target_generation: generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord { x: 0.75, y: 0.25 },
        parameters: json!({"button":"right"}),
    };
    let outcome = broker.act(request.clone());
    assert_eq!(outcome.kind, OutcomeKind::Applied, "{:?}", outcome.reason);
    eis.wait_event("button", 2).await;
    let before = inputs(&eis);
    assert_eq!(before.len(), 3);
    // No GTK timer can complete unexport while this exact owner is held.
    let release = ui.hold(true).await;
    assert!(!registry.target_alive(&target));
    assert!(!registry.input_available());
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    let metadata_error_after_unmap = broker
        .authorized_target_info(&ticket.run_id, SurfaceKind::Desktop)
        .unwrap_err()
        .to_string();
    request.action_id = uuid::Uuid::new_v4().to_string();
    assert_ne!(broker.act(request).kind, OutcomeKind::Applied);
    assert!(broker.observe(&ticket.run_id).is_err());
    eis.wait_event("disconnect", 1).await;
    assert_eq!(inputs(&eis), before);
    assert!(
        !registry.is_idle(&ticket.run_id),
        "EI disconnect cannot stand in for GTK release"
    );
    release.send(()).unwrap();
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    native.no_capture_node();
    broker.request_stop(&ticket.run_id).unwrap();
    registry.forget(&selection).unwrap();
    drop(ui); // join the exact GTK event thread before receipt publication
    fs::write(
        native.root.join("parent-registry.json"),
        serde_json::to_vec_pretty(&json!({
            "run":ticket.run_id, "target":target, "parent":parent, "observation":observation,
            "outcome":outcome, "inputs":before, "nativePeerRoot":eis.socket.parent().unwrap(),
            "queuedCancelBeforeExport":true, "lateExportJoined":true,
            "unmapImmediatelyDeniedInput":true, "uiAndNativeOwnersJoined":true,
            "authorizedMetadata": {
                "targetId":metadata.target_id, "title":metadata.title,
                "appName":metadata.app_name, "kind":metadata.kind, "backend":metadata.backend
            },
            "metadataErrorAfterUnmap":metadata_error_after_unmap,
            "appEnabled":false, "actualNativeGnome":false, "applicationEffectVerified":false
        }))
        .unwrap(),
    )
    .unwrap();
}
