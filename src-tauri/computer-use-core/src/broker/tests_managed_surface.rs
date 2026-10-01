use super::gates;
use super::*;
use crate::browser::RecordingBrowserWorker;
use crate::fake::FakeAdapter;
use crate::protocol::{ActionKind, ActionRequest, ActionTarget, OutcomeKind, PROTOCOL_VERSION};

fn open_managed(
    broker: &ComputerUseBroker,
    worker: &Arc<RecordingBrowserWorker>,
    root: &std::path::Path,
    session: &str,
    run_id: &str,
    profile: &str,
) -> (String, u64) {
    broker.tabs().set_profile_root(root.join("profiles"));
    broker.tabs().set_staging_root(root.join("staging"));
    broker.tabs().set_worker(worker.clone());
    broker.register_managed_browser_adapter().unwrap();
    broker.open_run(session, run_id).unwrap();
    let generation = broker
        .authorize_on_surface(
            session,
            run_id,
            SurfaceKind::ManagedBrowser,
            &format!("managed-profile:{profile}"),
        )
        .unwrap();
    let target = broker
        .tabs()
        .list_managed_for_run(run_id)
        .into_iter()
        .next()
        .expect("managed target")
        .tab_id;
    (target, generation)
}

fn click_request(
    run_id: &str,
    target_id: &str,
    target_generation: u64,
    observation: &Observation,
) -> ActionRequest {
    ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: format!("click-{run_id}"),
        run_id: run_id.into(),
        target_id: target_id.into(),
        target_generation,
        snapshot_id: observation.snapshot_id.clone(),
        geometry_revision: observation.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Element {
            element_ref: observation.nodes[0].node_ref.clone(),
        },
        parameters: serde_json::json!({}),
    }
}

#[test]
fn generic_managed_surface_reuses_host_worker_grant_ledger_and_cleanup() {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    let root = std::env::temp_dir().join(format!("cu-managed-adapter-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    let (target, target_generation) =
        open_managed(&broker, &worker, &root, "session-a", "run-a", "profile-a");

    let observation = broker.observe("run-a").unwrap();
    assert_eq!(observation.target_id, target);
    assert_eq!(observation.target_generation, target_generation);
    assert_eq!(observation.geometry_revision, 1);
    assert_eq!(observation.nodes[0].node_ref, "rec-ref");
    assert_eq!(desktop.observe_calls(), 0);

    let outcome = broker.act(click_request(
        "run-a",
        &target,
        target_generation,
        &observation,
    ));
    assert_eq!(outcome.kind, OutcomeKind::Applied, "{outcome:?}");
    assert!(outcome.executed);
    assert_eq!(*worker.act_calls.lock(), 1);
    assert!(desktop.executions().is_empty());

    assert_eq!(broker.request_stop("run-a").unwrap(), StopState::Stopped);
    assert_eq!(*worker.cancel_run_calls.lock(), 1);
    assert!(broker.tabs().managed_target_info(&target).is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn authorized_metadata_reads_managed_tab_grant_not_profile_picker() {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    let root = std::env::temp_dir().join(format!("cu-managed-readback-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    let (target, _) = open_managed(&broker, &worker, &root, "chat", "owned-run", "profile");
    let candidates = broker
        .list_targets_for_surface(SurfaceKind::ManagedBrowser)
        .unwrap();
    assert!(candidates
        .iter()
        .all(|row| row.target_id.starts_with("managed-profile:")));
    let row = broker
        .authorized_target_info("owned-run", SurfaceKind::ManagedBrowser)
        .unwrap();
    assert_eq!(row.target_id, target);
    assert_eq!(row.kind, "managed-tab");
    assert_eq!(row.backend, "playwright-managed");
    assert_eq!(desktop.observe_calls(), 0);
    assert!(desktop.executions().is_empty());
    broker.request_stop("owned-run").unwrap();
    assert!(broker
        .authorized_target_info("owned-run", SurfaceKind::ManagedBrowser)
        .is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_surface_is_run_scoped_and_does_not_take_desktop_lease() {
    let mut desktop_options = gates::enabled_opts(100);
    desktop_options.instance_id = "desktop-owner".into();
    let lease_path = desktop_options.lease_path.clone();
    let desktop = Arc::new(FakeAdapter::new());
    let desktop_broker = ComputerUseBroker::new(desktop.clone(), desktop_options);
    desktop_broker
        .open_run("session-desktop", "run-desktop")
        .unwrap();
    let desktop_target = desktop.fixture_id();
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

    let mut managed_options = gates::enabled_opts(100);
    managed_options.instance_id = "managed-owner".into();
    managed_options.lease_path = lease_path;
    let managed_desktop = Arc::new(FakeAdapter::new());
    let managed_broker = ComputerUseBroker::new(managed_desktop.clone(), managed_options);
    let root = std::env::temp_dir().join(format!("cu-managed-lease-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    let (target, target_generation) = open_managed(
        &managed_broker,
        &worker,
        &root,
        "session-managed",
        "run-managed",
        "profile-managed",
    );
    managed_broker
        .open_run("session-other", "run-other")
        .unwrap();

    let cross_run = managed_broker
        .authorize_on_surface(
            "session-other",
            "run-other",
            SurfaceKind::ManagedBrowser,
            &target,
        )
        .expect_err("another run must not claim the managed tab");
    assert!(cross_run.to_string().contains("another run"), "{cross_run}");

    let observation = managed_broker.observe("run-managed").unwrap();
    let outcome = managed_broker.act(click_request(
        "run-managed",
        &target,
        target_generation,
        &observation,
    ));
    assert_eq!(outcome.kind, OutcomeKind::Applied, "{outcome:?}");
    assert!(managed_desktop.executions().is_empty());

    managed_broker.request_stop("run-managed").unwrap();
    desktop_broker.request_stop("run-desktop").unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_preview_reaches_worker_with_separate_purpose_and_keeps_model_authority() {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(desktop.clone(), gates::enabled_opts(100));
    let root = std::env::temp_dir().join(format!("cu-managed-preview-{}", Uuid::new_v4()));
    let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
    let (target, generation) = open_managed(
        &broker,
        &worker,
        &root,
        "session-preview",
        "run-preview",
        "profile-preview",
    );
    let model = broker
        .observe_with_screenshot("run-preview", false)
        .unwrap();
    let preview = broker.observe_preview("run-preview").unwrap();
    assert_ne!(preview.snapshot_id, model.snapshot_id);
    assert_eq!(
        broker.act_defaults("run-preview").unwrap().2,
        model.snapshot_id
    );
    assert_eq!(
        broker
            .tabs()
            .tab_write_identity("run-preview", &target)
            .unwrap()
            .snapshot_id
            .as_deref(),
        Some(model.snapshot_id.as_str())
    );
    assert_eq!(
        *worker.capture_options.lock(),
        [(true, false), (false, true)]
    );
    let result = broker.act(click_request("run-preview", &target, generation, &model));
    assert_eq!(result.kind, OutcomeKind::Applied, "{result:?}");
    assert_eq!(*worker.act_calls.lock(), 1);
    assert!(desktop.executions().is_empty());
    broker.request_stop("run-preview").unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
