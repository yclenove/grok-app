//! Real source Node/Chromium → Rust client/Host/adapter identity contract.
//! Isolated profile and synthetic setup, not task approval or installed App.
use super::*;
use crate::computer_use::browser_supervisor::{BrowserSupervisor, SpawnRequest};
use grok_computer_use_core::adapter::{ComputerUseAdapter, NativeScopeIdentity};
use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
use grok_computer_use_core::browser::ManagedBrowserAdapter;
use grok_computer_use_core::execution::ActionCancellation;

fn require(value: bool, message: &str) -> Result<(), String> {
    value.then_some(()).ok_or_else(|| message.to_owned())
}

#[test]
fn unbound_client_cannot_query_scope_even_if_a_worker_is_reachable() {
    let client = LoopbackPlaywrightWorker::new("http://127.0.0.1:1", "synthetic-unused");
    let error = client.scope_identity("run", "profile", "page").unwrap_err();
    assert_eq!(error.code, "scope_identity_unavailable");
    assert_eq!(
        error.completion,
        grok_computer_use_core::browser::WorkerCompletion::NotStarted
    );
}

#[test]
fn native_worker_host_scope_identity_preserves_instances_and_retires_on_stop() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    let root = temporary.join(format!("grok-cu-app-scope-{}", uuid::Uuid::new_v4()));
    let broker = ComputerUseBroker::new(
        Arc::new(crate::computer_use::test_support::CountingAdapter::default()),
        BrokerOptions {
            lease_path: root.join("unused-desktop.lease"),
            ..Default::default()
        },
    );
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
        let host = broker.tabs();
        let request = host
            .admit_managed_request("scope-run", ActionCancellation::default())
            .map_err(|error| error.to_string())?;
        let scoped = Arc::new(
            host.for_managed_request(&request)
                .map_err(|error| error.to_string())?,
        );
        let tab = scoped
            .open_managed_profile("scope-session", "scope-run", "scope-profile")
            .map_err(|error| error.to_string())?;
        let adapter = ManagedBrowserAdapter::new(scoped.clone());
        let original = adapter.scope_identity(&tab.tab_id)?;
        let NativeScopeIdentity::ManagedPage { identity } = &original else {
            return Err("managed scope did not produce a real worker witness".into());
        };
        require(
            identity.owner == "scope-run" && identity.profile == "scope-profile",
            "wrong original owner/profile",
        )?;
        require(
            adapter.scope_identity(&tab.tab_id)? == original,
            "same instance identity changed",
        )?;
        require(
            matches!(
                broker.require_owner("scope-session", "scope-run"),
                Err(grok_computer_use_core::error::BrokerError::RunNotFound)
            ),
            "metadata preparation opened a Broker execution run",
        )?;
        require(
            adapter
                .scope_target_info("another-run", &tab.tab_id)
                .is_err(),
            "another run borrowed metadata",
        )?;
        require(
            adapter
                .scope_identity("managed:scope-profile:missing-page")
                .is_err(),
            "unknown page accepted",
        )?;

        let new_tab = scoped
            .new_managed_tab("scope-run", "scope-profile", "scope-new-tab")
            .map_err(|error| error.to_string())?;
        let second_id = format!("managed:scope-profile:{}", new_tab.page_id);
        let NativeScopeIdentity::ManagedPage { identity: second } =
            adapter.scope_identity(&second_id)?
        else {
            return Err("new tab did not produce a worker witness".into());
        };
        require(
            second.worker_instance_id == identity.worker_instance_id
                && second.context_instance_id == identity.context_instance_id
                && second.page_id != identity.page_id,
            "new tab conflated context/page identity",
        )?;
        host.pause_managed_run("scope-run")
            .map_err(|error| error.to_string())?;
        require(
            adapter.scope_identity(&tab.tab_id).is_err(),
            "paused request still queried scope",
        )?;
        host.cancel_run("scope-run")
            .map_err(|error| error.to_string())?;
        require(
            adapter.scope_identity(&tab.tab_id).is_err()
                && adapter.scope_identity(&second_id).is_err(),
            "stopped context retained scope",
        )?;
        require(
            host.managed_run_idle("scope-run"),
            "original native context cleanup not confirmed",
        )?;
        println!("scope native: original worker/context/page identities verified; pause/stop fenced; no execution grant or input");
        Ok(())
    })();
    let shutdown = supervisor.shutdown();
    // The original kernel Job and Child join are the owner proof. Numeric PID
    // lookup or a success response alone never authorizes deleting the root.
    let removal = if result.is_ok() && shutdown.is_ok() && root.exists() {
        super::profile_cleanup::remove_fixture_root(&root, &temporary)
            .map_err(|error| error.to_string())
    } else {
        Ok(())
    };
    assert!(
        shutdown.is_ok(),
        "owned shutdown failed; profile retained: {shutdown:?}"
    );
    assert!(removal.is_ok(), "owned profile cleanup failed: {removal:?}");
    assert!(result.is_ok(), "native scope contract failed: {result:?}");
    assert!(!root.exists());
    println!("scope native: original supervisor shutdown/join confirmed; disposable profile/runtime removed");
}
