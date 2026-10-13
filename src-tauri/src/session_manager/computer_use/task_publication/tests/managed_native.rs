//! Actual source Node/Chromium → default App/ACP pin → Core transaction.
//! Session, Host decisions and ACP peer are synthetic, not live Grok/user consent.
use super::*;
use crate::computer_use::test_support::{remove_owned_root, BrowserSupervisor, SpawnRequest};
use std::path::PathBuf;

#[test]
fn managed_native_task_resume_uses_original_page_and_stop_fences_prepared_capture() {
    use grok_computer_use_core::broker::{HostExecutionAddress, HostTaskDecision};
    use std::time::Duration;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    for terminal in ["continue", "stop-after-capture"] {
        let f = runtime.block_on(TaskViewFixture::new_managed());
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
        let supervisor = BrowserSupervisor::spawn(SpawnRequest {
            node: manifest.join("resources/computer-use/seed/bin/node.exe"),
            script: manifest
                .join("../tools/computer-use-browser/server.mjs")
                .canonicalize()
                .unwrap(),
            profile_root: root.clone(),
            browser: Some(
                manifest.join("resources/computer-use/seed/chromium/chrome-win/chrome.exe"),
            ),
        })
        .unwrap();
        let result = (|| -> Result<(), String> {
            let broker = f.broker();
            supervisor.attach(&broker)?;
            let (mut approval, publish) =
                f.pending_managed_approval(manager.task_approval_pin("view-app", &endpoint));
            approval.publish_feature().map_err(|e| e.to_string())?;
            approval.prepare_execution().map_err(|e| e.to_string())?;
            approval.prepare_publication().map_err(|e| e.to_string())?;
            let outcome = publish(&mut approval)?;
            let original_claim = broker
                .claim_continuation("view-app")
                .ok_or("original approval continuation missing")?;
            broker
                .consume_continuation(&original_claim)
                .map_err(|e| e.to_string())?;
            let before = broker.observe(&outcome.run_id).map_err(|e| e.to_string())?;
            let view = f.original_host_task_view()?;
            let address = HostExecutionAddress {
                key: view.executions[0].key.clone(),
                run_id: outcome.run_id.clone(),
                generation: view.executions[0].generation.clone(),
            };
            f.client
                .computer_use_execution_call("view-app", "synthetic-view-agent", &address)?
                .pause()?;
            let paused = f.original_host_task_view()?;
            if !paused.executions[0].resume_available || paused.grants[0].executable {
                return Err("original managed task did not offer suspended recovery".into());
            }
            let address = HostExecutionAddress {
                generation: paused.executions[0].generation.clone(),
                ..address
            };
            if terminal == "continue" {
                runtime
                    .block_on(manager.resume_computer_use_task_execution("view-app", &address))?;
                let restored = f.original_host_task_view()?;
                if !restored.grants[0].executable
                    || !restored.executions[0].has_fresh_observation
                    || restored.executions[0].generation == address.generation
                    || restored.executions[0].target.as_ref().map(|t| &t.target_id)
                        != Some(&before.target_id)
                {
                    return Err("native recovery changed scope or omitted fresh observation".into());
                }
                let after = broker.observe(&outcome.run_id).map_err(|e| e.to_string())?;
                if after.snapshot_id == before.snapshot_id
                    || after.target_id != before.target_id
                    || after.target_generation == before.target_generation
                    || after.image.png_base64.as_ref().is_none_or(String::is_empty)
                {
                    return Err("native recovery reused an old frame/ref or lost screenshot".into());
                }
                let continuation = broker
                    .claim_continuation("view-app")
                    .ok_or("one resumed continuation missing")?;
                broker
                    .consume_continuation(&continuation)
                    .map_err(|e| e.to_string())?;
                if broker.claim_continuation("view-app").is_some() {
                    return Err("native recovery published a second continuation".into());
                }
            } else {
                let mut call = f.recovery(&address);
                call.prepare_native(Duration::from_secs(15))?;
                f.client.decide_computer_use_task(
                    "view-app",
                    "synthetic-view-agent",
                    &address.key,
                    HostTaskDecision::Stop,
                )?;
                if call.commit().is_ok()
                    || broker.executable_grant_count("view-app") != 0
                    || broker.claim_continuation("view-app").is_some()
                {
                    return Err("Stop allowed the prepared native recovery to publish".into());
                }
            }
            println!("managed default App/ACP native recovery: {terminal}; original page; fresh capture; no input");
            Ok(())
        })();
        drop(manager);
        runtime.block_on(f.finish());
        let shutdown = supervisor.shutdown();
        if result.is_ok() && shutdown.is_ok() && root.exists() {
            remove_owned_root(&root, &temporary);
        }
        assert!(
            shutdown.is_ok(),
            "original supervisor did not join: {shutdown:?}"
        );
        assert!(
            result.is_ok(),
            "native managed recovery at {terminal}: {result:?}; root={root:?}"
        );
        assert!(!root.exists());
    }
}

#[test]
fn managed_native_publication_pins_original_session_and_request_transport() {
    // The native client owns a blocking HTTP runtime. Construct and drop it
    // outside Tokio, while the synthetic ACP peer runs on its retained runtime.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for terminal in ["original", "session-replaced", "transport-replaced"] {
        let f = runtime.block_on(TaskViewFixture::new_managed());
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let root = temporary.join(format!("grok-cu-managed-task-{}", uuid::Uuid::new_v4()));
        let supervisor = BrowserSupervisor::spawn(SpawnRequest {
            node: manifest.join("resources/computer-use/seed/bin/node.exe"),
            script: manifest
                .join("../tools/computer-use-browser/server.mjs")
                .canonicalize()
                .unwrap(),
            profile_root: root.clone(),
            browser: Some(
                manifest.join("resources/computer-use/seed/chromium/chrome-win/chrome.exe"),
            ),
        })
        .unwrap();
        let result = (|| -> Result<(), String> {
            let broker = f.broker();
            supervisor.attach(&broker)?;
            let (mut approval, publish) =
                f.pending_managed_approval(manager.task_approval_pin("view-app", &endpoint));
            approval.publish_feature().map_err(|e| e.to_string())?;
            approval.prepare_execution().map_err(|e| e.to_string())?;
            let pending = f
                .client
                .computer_use_task_view("view-app", "synthetic-view-agent")?;
            if !pending.grants.is_empty() || pending.executions.len() != 1 {
                return Err("resources did not retain one non-executable original owner".into());
            }
            approval.prepare_publication().map_err(|e| e.to_string())?;
            match terminal {
                "session-replaced" => {
                    manager.inner.lock().as_mut().unwrap().process_id = "replacement-process".into()
                }
                "transport-replaced" => f.replace_approval_transport_same_identity(),
                _ => {}
            }
            let outcome = publish(&mut approval);
            let expected_grants = usize::from(terminal == "original");
            if outcome.is_ok() != (terminal == "original")
                || broker.executable_grant_count("view-app") != expected_grants
                || broker.continuation_count("view-app") != expected_grants
            {
                return Err(format!(
                    "incorrect managed publication at {terminal}: error={:?}",
                    outcome.as_ref().err()
                ));
            }
            if let Ok(outcome) = outcome {
                if outcome.run_id != pending.executions[0].run_id {
                    return Err("publication changed original resource owner".into());
                }
                let obs = broker
                    .observe_with_screenshot(&outcome.run_id, false)
                    .map_err(|e| e.to_string())?;
                if obs.target_id != outcome.binding.target_id {
                    return Err("published run observed a different tab".into());
                }
            }
            println!("managed default App/ACP native publication: {terminal}; grants={expected_grants}; no input");
            Ok(())
        })();
        drop(manager);
        runtime.block_on(f.finish());
        let shutdown = supervisor.shutdown();
        if result.is_ok() && shutdown.is_ok() && root.exists() {
            remove_owned_root(&root, &temporary);
        }
        assert!(
            shutdown.is_ok(),
            "original supervisor did not join: {shutdown:?}"
        );
        assert!(
            result.is_ok(),
            "managed native publication failed at {terminal}: {result:?}"
        );
        assert!(!root.exists());
    }
}
