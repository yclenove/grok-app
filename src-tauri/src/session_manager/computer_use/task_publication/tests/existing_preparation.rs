//! Original App endpoint and ACP picker, synthetic session/explicit confirmation.
use super::*;

#[tokio::test]
async fn existing_preparation_read_and_confirm_pin_original_app_process_agent_and_acp() {
    for replacement in ["process", "agent", "acp", "removed"] {
        let f = TaskViewFixture::new_existing().await;
        let manager = manager(&f);
        let key = f.request_key();
        let started = manager
            .begin_computer_use_task_picker("view-app", &key, "existing-tabs")
            .unwrap();
        let started = serde_json::to_value(started).unwrap();
        let id = started["pickerId"].as_str().unwrap();
        let preparation = manager
            .computer_use_task_picker_preparation("view-app", &key, id)
            .unwrap();
        let preparation = serde_json::to_value(preparation).unwrap();
        let nonce = preparation["pairing"]["nonce"].as_str().unwrap();
        assert_eq!(preparation["ready"], false);
        assert_eq!(preparation["pairingConfirmed"], false);
        manager
            .confirm_computer_use_task_picker_pairing("view-app", &key, id, nonce)
            .unwrap();
        let confirmed = serde_json::to_value(
            manager
                .computer_use_task_picker_preparation("view-app", &key, id)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(confirmed["pairingConfirmed"], true);
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
        // The original process is not a new owner merely because it shares
        // the session key. An unavailable endpoint must never be bootstrapped.
        assert!(
            manager
                .computer_use_task_picker_preparation("view-app", &key, id)
                .is_err(),
            "{replacement}"
        );
        assert!(
            manager
                .confirm_computer_use_task_picker_pairing("view-app", &key, id, nonce)
                .is_err(),
            "{replacement}"
        );
        assert!(!f.broker().feature_enabled());
        assert_eq!(f.broker().executable_grant_count("view-app"), 0);
        assert_eq!(f.broker().continuation_count("view-app"), 0);
        manager.inner.lock().take();
        f.finish().await;
    }
}

#[tokio::test]
async fn existing_preparation_remains_task_bound_in_live_background_and_parked_app_slots() {
    for slot in ["live", "background", "parked"] {
        let f = TaskViewFixture::new_existing().await;
        let manager = manager(&f);
        let key = f.request_key();
        let started = serde_json::to_value(
            manager
                .begin_computer_use_task_picker("view-app", &key, "existing-tabs")
                .unwrap(),
        )
        .unwrap();
        let id = started["pickerId"].as_str().unwrap();
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
        let view = serde_json::to_value(
            manager
                .computer_use_task_picker_preparation("view-app", &key, id)
                .unwrap(),
        )
        .unwrap();
        let nonce = view["pairing"]["nonce"].as_str().unwrap();
        manager
            .confirm_computer_use_task_picker_pairing("view-app", &key, id, nonce)
            .unwrap();
        assert!(manager
            .cancel_computer_use_task_picker("view-app", &key, id)
            .unwrap());
        assert!(manager
            .computer_use_task_picker_preparation("view-app", &key, id)
            .is_err());
        assert!(manager
            .confirm_computer_use_task_picker_pairing("view-app", &key, id, nonce)
            .is_err());
        assert!(f.broker().tabs().pending_pairing_challenge().is_none());
        assert!(!f.broker().feature_enabled());
        assert_eq!(f.broker().executable_grant_count("view-app"), 0);
        manager.inner.lock().take();
        manager.background.lock().clear();
        manager.parked.lock().clear();
        f.finish().await;
    }
}
