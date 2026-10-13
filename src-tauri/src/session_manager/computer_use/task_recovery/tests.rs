//! Synthetic session slots and exact native fixture, not installed App evidence.
use super::*;
use crate::acp_client::computer_use::TaskViewFixture;

fn manager(f: &TaskViewFixture) -> Arc<SessionManager> {
    let manager = Arc::new(SessionManager::new());
    *manager.inner.lock() = Some(super::super::tests::ready_live(
        "view-app",
        "synthetic-view-agent",
        "resume-process",
        f.client.clone(),
    ));
    manager
}

#[tokio::test]
async fn duplicate_user_resume_keeps_one_owned_capture_and_one_continuation() {
    let f = TaskViewFixture::new().await;
    let (fake, address) = f.native_paused_execution();
    let manager = manager(&f);
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    fake.arm_capture_hook(Box::new(move || {
        entered.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    let original = manager.clone();
    let original_address = address.clone();
    let worker = tokio::spawn(async move {
        original
            .resume_computer_use_task_execution("view-app", &original_address)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), started)
        .await
        .unwrap()
        .unwrap();
    let duplicate = manager
        .resume_computer_use_task_execution("view-app", &address)
        .await
        .unwrap();
    assert!(matches!(duplicate, TaskResumeOutcome::InProgress));
    release.send(()).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let TaskResumeOutcome::Resumed { address: resumed } = outcome else {
        panic!("resumed")
    };
    assert_eq!(resumed.run_id, address.run_id);
    assert_ne!(resumed.generation, address.generation);
    assert_eq!(fake.observe_calls(), 1);
    let claim = f.broker().claim_continuation("view-app").unwrap();
    f.broker().consume_continuation(&claim).unwrap();
    assert!(f.broker().claim_continuation("view-app").is_none());
    f.finish().await;
}

#[tokio::test]
async fn publication_accepts_exact_live_background_or_parked_slot_without_stealing_focus() {
    for slot in ["live", "background", "parked"] {
        let f = TaskViewFixture::new().await;
        let (_fake, address) = f.native_paused_execution();
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let mut call = f.recovery(&address);
        call.prepare_native(Duration::from_secs(1)).unwrap();
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
        assert!(
            manager
                .commit_task_resume("view-app", &endpoint, &mut call)
                .is_ok(),
            "{slot}"
        );
        assert_eq!(manager.inner.lock().is_some(), slot == "live");
        drop(call);
        assert!(f.broker().claim_continuation("view-app").is_some());
        manager.inner.lock().take();
        manager.background.lock().clear();
        manager.parked.lock().clear();
        f.finish().await;
    }
}

#[tokio::test]
async fn replacement_of_original_session_process_agent_or_acp_rejects_final_publication() {
    for replacement in ["process", "agent", "acp", "removed"] {
        let f = TaskViewFixture::new().await;
        let (_fake, address) = f.native_paused_execution();
        let manager = manager(&f);
        let endpoint = manager.task_picker_endpoint("view-app").unwrap();
        let mut call = f.recovery(&address);
        call.prepare_native(Duration::from_secs(1)).unwrap();
        if replacement == "removed" {
            manager.inner.lock().take();
        } else {
            let mut slot = manager.inner.lock();
            let live = slot.as_mut().unwrap();
            match replacement {
                "process" => live.process_id = "replacement-process".into(),
                "agent" => live.meta.agent_session_id = Some("replacement-agent".into()),
                _ => live.acp = None,
            }
        }
        assert!(
            manager
                .commit_task_resume("view-app", &endpoint, &mut call)
                .is_err(),
            "{replacement}"
        );
        assert!(f.broker().claim_continuation("view-app").is_none());
        drop(call);
        f.finish().await;
    }
}

#[tokio::test]
async fn resumed_continuation_waits_for_real_original_prompt_and_never_steals_other_chat() {
    let f = TaskViewFixture::new().await;
    let (_fake, address) = f.native_paused_execution();
    let manager = manager(&f);
    manager
        .resume_computer_use_task_execution("view-app", &address)
        .await
        .unwrap();
    let before = f.broker().claim_continuation("view-app").unwrap();
    let prompt = crate::acp_client::computer_use::PromptCallGuard::begin(
        &f.client,
        "synthetic-view-agent",
        Some("original-user-turn"),
    )
    .unwrap();
    assert!(manager.reserve_computer_use_delivery("view-app").is_none());
    assert_eq!(
        f.broker().claim_continuation("view-app").unwrap().id,
        before.id
    );
    drop(prompt);
    manager.try_park_live().unwrap();
    *manager.inner.lock() = Some(super::super::tests::ready_live(
        "other-chat",
        "other-agent",
        "other-process",
        f.client.clone(),
    ));
    let (endpoint, delivery, _turn) = manager.reserve_computer_use_delivery("view-app").unwrap();
    assert_eq!(endpoint.agent_session_id, "synthetic-view-agent");
    assert_eq!(delivery.app_session(), "view-app");
    assert_eq!(
        manager.inner.lock().as_ref().unwrap().app_session_id,
        "other-chat"
    );
    assert!(
        manager
            .background
            .lock()
            .get("view-app")
            .unwrap()
            .prompt_in_flight
    );
    assert!(manager.reserve_computer_use_delivery("view-app").is_none());
    manager.inner.lock().take();
    manager.background.lock().clear();
    crate::turn_lease::clear_lease("view-app");
    f.finish().await;
}
