//! Actual source Node/Chromium with an explicit synthetic Host choice.
//! Not installed App, user consent, or a recording-worker substitute.
use super::*;
use crate::computer_use::browser_supervisor::{BrowserSupervisor, SpawnRequest};
use grok_computer_use_core::adapter::{ComputerUseAdapter, NativeScopeIdentity};
use grok_computer_use_core::broker::{AccessApprovalReady, BrokerOptions, ComputerUseBroker};
use grok_computer_use_core::browser::ManagedBrowserAdapter;

fn require(value: bool, message: &str) -> Result<(), String> {
    value.then_some(()).ok_or_else(|| message.to_owned())
}

#[test]
fn original_managed_task_reservation_creates_real_bound_before_executing() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(crate::computer_use::test_support::CountingAdapter::default()),
        BrokerOptions {
            feature_enabled: false,
            lease_path: root.join("unused-desktop.lease"),
            ..Default::default()
        },
    ));
    let transport = broker
        .issue_transport_credential("managed-task-app")
        .unwrap();
    broker
        .bind_agent_session("managed-task-app", "synthetic-managed-agent")
        .unwrap();
    let request = broker
        .submit_access(
            transport.token(),
            json!({
                "action":"request", "purpose":"synthetic isolated native task",
                "scopeIntent":"one independent browser tab", "surfaces":["managed-browser"],
                "capabilities":["observe"]
            }),
        )
        .unwrap();
    let request_id = request["requestId"].as_str().unwrap();
    assert!(!root.exists(), "model request started a runtime/profile");
    assert!(!broker.feature_enabled());
    let supervisor = BrowserSupervisor::spawn(SpawnRequest {
        node: manifest.join("resources/computer-use/seed/bin/node.exe"),
        script: manifest
            .join("../tools/computer-use-browser/server.mjs")
            .canonicalize()
            .unwrap(),
        profile_root: root.clone(),
        browser: Some(manifest.join("resources/computer-use/seed/chromium/chrome-win/chrome.exe")),
    })
    .unwrap();
    let result = (|| -> Result<(), String> {
        supervisor.attach(&broker)?;
        let ready = broker
            .reserve_managed_access_approval(transport.token(), request_id, 1)
            .map_err(|error| format!("original managed resource reservation rejected: {error}"))?;
        let AccessApprovalReady::Prepare(mut approval) = ready else {
            return Err("first managed decision already committed".into());
        };
        require(!broker.feature_enabled(), "reservation enabled execution")?;
        require(
            broker
                .reserve_managed_access_approval(transport.token(), request_id, 1)
                .is_err(),
            "duplicate gesture reserved another browser",
        )?;
        approval
            .publish_feature()
            .map_err(|error| error.to_string())?;
        approval
            .prepare_execution()
            .map_err(|error| error.to_string())?;
        let view = broker
            .host_task_view(transport.token())
            .map_err(|error| error.to_string())?;
        require(
            view.grants.is_empty(),
            "resource preparation published an execution grant",
        )?;
        require(
            view.executions.len() == 1,
            "missing original non-executable preparation owner",
        )?;
        let pending_run = &view.executions[0].run_id;
        require(
            broker
                .require_owner("managed-task-app", pending_run)
                .is_err(),
            "pending resource run admitted legacy execution",
        )?;
        let pending_observe = grok_computer_use_core::tools::dispatch_with_transport(
            &broker,
            transport.token(),
            "computer_observe",
            json!({"runId":pending_run,"screenshot":false}),
            None,
        );
        require(
            pending_observe["isError"] == true,
            "model observed a page before final commit",
        )?;
        approval
            .prepare_publication()
            .map_err(|error| error.to_string())?;
        let outcome = approval
            .commit_for_transport(transport.token())
            .map_err(|error| error.to_string())?;
        let run = outcome.run_id;
        require(
            run == *pending_run,
            "Bound/execution replaced the resource reservation run",
        )?;
        require(
            broker.require_owner("managed-task-app", &run).is_ok(),
            "committed owner rejected",
        )?;
        let scoped = broker
            .tabs()
            .admit_managed_request(
                &run,
                grok_computer_use_core::execution::ActionCancellation::default(),
            )
            .map_err(|error| error.to_string())?;
        let adapter = ManagedBrowserAdapter::new(Arc::new(
            broker
                .tabs()
                .for_managed_request(&scoped)
                .map_err(|error| error.to_string())?,
        ));
        let NativeScopeIdentity::ManagedPage { identity } =
            adapter.scope_identity(&outcome.binding.target_id)?
        else {
            return Err("task used a synthetic/legacy proof".into());
        };
        require(
            identity.owner == run,
            "context borrowed another run's identity",
        )?;
        require(
            adapter
                .scope_target_info("another-task", &outcome.binding.target_id)
                .is_err(),
            "another task borrowed this page",
        )?;
        let observed = grok_computer_use_core::tools::dispatch_with_transport(
            &broker,
            transport.token(),
            "computer_observe",
            json!({"runId":run,"screenshot":false}),
            None,
        );
        require(
            observed["isError"] == false,
            "fresh model observation after commit failed",
        )?;
        require(
            approval.commit_authority().is_err(),
            "double commit accepted",
        )?;
        println!("managed task native: original reservation/context/page bound; one commit; fresh observation; no input");
        Ok(())
    })();
    broker.fence_task_consent("managed-task-app");
    let cleanup = broker
        .take_cleanup_tickets()
        .into_iter()
        .try_for_each(|ticket| {
            broker
                .finish_stop_cleanup(&ticket)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    let shutdown = supervisor.shutdown();
    if result.is_ok() && cleanup.is_ok() && shutdown.is_ok() && root.exists() {
        remove_owned_root(&root, &temporary);
    }
    assert!(
        cleanup.is_ok(),
        "resource cleanup failed; retained root: {cleanup:?}"
    );
    assert!(
        shutdown.is_ok(),
        "original supervisor did not join: {shutdown:?}"
    );
    assert!(
        result.is_ok(),
        "native task producer contract failed: {result:?}"
    );
    assert!(!root.exists());
}

pub(crate) fn remove_owned_root(root: &std::path::Path, temporary: &std::path::Path) {
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("grok-cu-managed-task-"));
    super::profile_cleanup::remove_fixture_root(root, temporary).unwrap();
}
