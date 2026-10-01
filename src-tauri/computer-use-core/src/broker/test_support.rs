//! Shared Fake/IPC fixtures for named broker contract tests.
use super::*;
use crate::fake::FakeAdapter;

pub(super) fn wait_args(
    broker: &ComputerUseBroker,
    run: &str,
    name: &str,
    timeout: u64,
) -> serde_json::Value {
    if broker.model_snapshot_id(run).is_none() {
        broker.observe(run).expect("fixture model observation");
    }
    let (target, generation, snapshot, geometry) =
        broker.act_defaults(run).expect("fixture identity");
    serde_json::json!({
        "version":PROTOCOL_VERSION,"actionId":format!("wait-{}", Uuid::new_v4()),
        "runId":run,"targetId":target,"targetGeneration":generation,"snapshotId":snapshot,
        "geometryRevision":geometry,"elementRef":"n1","nameEquals":name,"timeoutMs":timeout,
    })
}

pub(super) fn ipc_fixture() -> Result<(crate::ipc::IpcServer, String), String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!("cu-ipc-{}.lock", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    ));
    broker
        .open_run("session-a", "run-a")
        .map_err(|e| e.to_string())?;
    broker
        .authorize_target("run-a", &fake.fixture_id())
        .map_err(|e| e.to_string())?;
    let server = crate::ipc::spawn(broker).map_err(|e| e.to_string())?;
    let token = server
        .issue_session("session-a", "run-a")
        .map_err(|e| e.to_string())?;
    Ok((server, token))
}
