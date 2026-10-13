//! Actual App session/ACP/Core boundary, explicit synthetic Host/native fixture.
use super::*;
use crate::acp_client::computer_use::TaskViewFixture;
#[cfg(windows)]
mod existing_joint;
mod existing_preparation;

#[cfg(windows)]
mod managed_cleanup;
#[cfg(windows)]
mod managed_native;
#[cfg(windows)]
mod managed_owned;

fn manager(f: &TaskViewFixture) -> Arc<SessionManager> {
    manager_from_client(f.client.clone())
}

fn manager_from_client(client: Arc<crate::acp_client::AcpClient>) -> Arc<SessionManager> {
    let manager = Arc::new(SessionManager::new());
    *manager.inner.lock() = Some(super::super::tests::ready_live(
        "view-app",
        "synthetic-view-agent",
        "approval-process",
        client,
    ));
    manager
}

#[tokio::test]
async fn approval_publishes_once_in_original_live_background_and_parked_slots() {
    for slot in ["live", "background", "parked"] {
        let f = TaskViewFixture::new().await;
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let (fake, mut approval, publish) =
            f.pending_approval(manager.task_approval_pin("view-app", &endpoint));
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        let native_manager = manager.clone();
        fake.arm_liveness_hook(Box::new(move || {
            assert!(
                native_manager.inner.try_lock().is_some(),
                "native preparation held App lock"
            );
            assert!(native_manager.background.try_lock().is_some());
            assert!(native_manager.parked.try_lock().is_some());
        }));
        approval.prepare_publication().unwrap();
        fake.arm_liveness_hook(Box::new(|| panic!("final session/ACP pin queried native")));
        match slot {
            "background" => {
                let live = manager.inner.lock().take().unwrap();
                manager.background.lock().insert("view-app".into(), live);
            }
            "parked" => {
                manager.try_park_live().unwrap();
            }
            _ => {}
        }
        publish(&mut approval).unwrap();
        assert!(publish(&mut approval).is_err());
        assert_eq!(f.broker().executable_grant_count("view-app"), 1);
        assert_eq!(f.broker().continuation_count("view-app"), 1);
        assert_eq!(fake.observe_calls(), 0);
        assert!(fake.executions().is_empty());
        assert_eq!(manager.inner.lock().is_some(), slot == "live");
        drop(approval);
        drop(publish);
        manager.inner.lock().take();
        manager.background.lock().clear();
        manager.parked.lock().clear();
        f.finish().await;
    }
}

#[tokio::test]
async fn replacement_before_approval_publication_leaves_zero_grants_and_continuations() {
    for replacement in ["process", "agent", "acp", "removed"] {
        let f = TaskViewFixture::new().await;
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let (fake, mut approval, publish) =
            f.pending_approval(manager.task_approval_pin("view-app", &endpoint));
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.prepare_publication().unwrap();
        if replacement == "removed" {
            manager.inner.lock().take();
        } else {
            let mut slot = manager.inner.lock();
            let current = slot.as_mut().unwrap();
            match replacement {
                "process" => current.process_id = "replacement-process".into(),
                "agent" => current.meta.agent_session_id = Some("replacement-agent".into()),
                _ => current.acp = None,
            }
        }
        assert!(publish(&mut approval).is_err(), "{replacement}");
        assert_eq!(
            f.broker().executable_grant_count("view-app"),
            0,
            "{replacement}"
        );
        assert_eq!(f.broker().continuation_count("view-app"), 0);
        assert_eq!(fake.observe_calls(), 0);
        drop(approval);
        drop(publish);
        assert!(!f.broker().feature_enabled());
        f.finish().await;
    }
}

#[tokio::test]
async fn replacing_acp_transport_with_identical_live_bearer_cannot_publish_original_approval() {
    let f = TaskViewFixture::new().await;
    let manager = manager(&f);
    let endpoint = manager.task_picker_endpoint("view-app").unwrap();
    let (_, mut approval, publish) =
        f.pending_approval(manager.task_approval_pin("view-app", &endpoint));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    approval.prepare_publication().unwrap();
    f.replace_approval_transport_same_identity();
    assert!(manager.endpoint_is_current("view-app", &endpoint));
    assert!(f
        .client
        .computer_use_task_view("view-app", "synthetic-view-agent")
        .is_ok());
    assert!(publish(&mut approval).is_err());
    assert_eq!(f.broker().executable_grant_count("view-app"), 0);
    assert_eq!(f.broker().continuation_count("view-app"), 0);
    drop(approval);
    drop(publish);
    f.finish().await;
}

#[tokio::test]
async fn a_valid_app_pin_cannot_publish_another_brokers_opaque_approval() {
    let f = TaskViewFixture::new().await;
    let foreign = TaskViewFixture::new().await;
    let manager = manager(&f);
    let endpoint = manager.task_picker_endpoint("view-app").unwrap();
    let (_, original, publish) =
        f.pending_approval(manager.task_approval_pin("view-app", &endpoint));
    let (_, mut other, _) =
        foreign.pending_approval(Arc::new(|_, _| Err("unused synthetic pin".into())));
    other.publish_feature().unwrap();
    other.prepare_execution().unwrap();
    other.prepare_publication().unwrap();
    assert!(publish(&mut other).is_err());
    assert_eq!(foreign.broker().executable_grant_count("view-app"), 0);
    assert_eq!(foreign.broker().continuation_count("view-app"), 0);
    assert_eq!(f.broker().executable_grant_count("view-app"), 0);
    drop(original);
    drop(other);
    drop(publish);
    f.finish().await;
    foreign.finish().await;
}

#[tokio::test]
async fn stop_or_manager_drop_after_native_prepare_cannot_publish_an_approval() {
    for terminal in ["stop", "manager-drop"] {
        let f = TaskViewFixture::new().await;
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let (_, mut approval, publish) =
            f.pending_approval(manager.task_approval_pin("view-app", &endpoint));
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.prepare_publication().unwrap();
        let mut live_manager = Some(manager);
        if terminal == "stop" {
            approval.cancel_handle().fence();
        } else {
            live_manager.take();
        }
        assert!(publish(&mut approval).is_err());
        assert_eq!(f.broker().executable_grant_count("view-app"), 0);
        assert_eq!(f.broker().continuation_count("view-app"), 0);
        drop(approval);
        drop(publish);
        drop(live_manager);
        f.finish().await;
    }
}
