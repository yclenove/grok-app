use super::{computer_use_retry_cleanup_impl, ComputerPairingChallengeDto, SessionManager};

include!("computer_use_target_tests.rs");

#[test]
fn app_preparation_gate_rejects_cancelled_queue_and_discards_late_bound_target() {
    use crate::computer_use::test_support::CountingAdapter;
    use grok_computer_use_core::{
        broker::{BrokerOptions, ComputerUseBroker},
        session_grants::SessionGrants,
    };
    let broker = ComputerUseBroker::new(
        std::sync::Arc::new(CountingAdapter::default()),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    );
    let grants = SessionGrants::default();
    let before = grants.begin(&broker, "before-bind", None).unwrap();
    grants.fence_fail_checked(&broker, &before, || {});
    assert!(super::computer_use_prepare_target(
        &before,
        || panic!("cancelled queued work performed native bind"),
        || panic!("unstarted bind must not discard another target")
    )
    .is_err());
    let during = grants.begin(&broker, "during-bind", None).unwrap();
    let discarded = std::cell::Cell::new(0);
    let result = super::computer_use_prepare_target(
        &during,
        || {
            grants.fence_fail_checked(&broker, &during, || {});
            Ok("late-run-owned-target".into())
        },
        || discarded.set(discarded.get() + 1),
    );
    assert!(result.is_err());
    assert_eq!(discarded.get(), 1);
    let live = grants.begin(&broker, "live-bind", None).unwrap();
    assert_eq!(
        super::computer_use_prepare_target(
            &live,
            || Ok("valid-target".into()),
            || panic!("valid target was discarded")
        )
        .unwrap(),
        "valid-target"
    );
    grants.publish(&broker, &live, true, || {}).unwrap();
    assert!(super::computer_use_prepare_target(
        &live,
        || panic!("completed ticket restarted preparation"),
        || {}
    )
    .is_err());
}

#[test]
fn app_target_picker_keeps_owned_run_but_allows_initial_discovery() {
    use crate::computer_use::sessions;
    use crate::computer_use::test_support::CountingAdapter;
    use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
    // This contract is for enumerated desktop targets. A preview build running
    // under WSLg/Wayland must not silently turn the fixture into portal mode.
    fn picker(
        broker: &ComputerUseBroker,
        session: Option<&str>,
        run: Option<&str>,
        surface: Option<&str>,
    ) -> Result<Vec<super::ComputerTargetDto>, String> {
        super::computer_use_picker_targets_with_mode(broker, session, run, surface, "targets")
    }
    let adapter = std::sync::Arc::new(CountingAdapter::default());
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    );
    let chat = format!("picker-command-chat-{}", uuid::Uuid::new_v4());
    let initial = picker(&broker, Some(&chat), None, Some("desktop")).unwrap();
    assert_eq!(initial.len(), 1);
    assert!(adapter.listed_runs().is_empty());
    let ticket = sessions::begin(&broker, &chat, None, "picker-attempt", 1).unwrap();
    let scoped = picker(&broker, Some(&chat), Some(&ticket.run_id), Some("desktop")).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].target_id, initial[0].target_id);
    assert_eq!(
        adapter.listed_runs().as_slice(),
        std::slice::from_ref(&ticket.run_id)
    );
    assert!(broker.authorized_target(&ticket.run_id).is_err());
    for (session, requested, surface) in [
        (None, None, "desktop"),
        (Some("   "), None, "desktop"),
        (Some("other-chat"), Some(ticket.run_id.as_str()), "desktop"),
        (Some(chat.as_str()), Some("unknown"), "desktop"),
        (
            Some(chat.as_str()),
            Some(ticket.run_id.as_str()),
            "unknown-surface",
        ),
    ] {
        assert!(picker(&broker, session, requested, Some(surface)).is_err());
    }
    // Global chat/run bookkeeping alone is insufficient: the supplied
    // broker must independently confirm that run's chat ownership.
    let other_broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    );
    other_broker.open_run("other-chat", &ticket.run_id).unwrap();
    assert!(picker(
        &other_broker,
        Some(&chat),
        Some(&ticket.run_id),
        Some("desktop")
    )
    .is_err());
    broker.set_feature_enabled(false);
    assert!(
        picker(&broker, Some(&chat), Some(&ticket.run_id), Some("desktop"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(adapter.listed_runs(), [ticket.run_id]);
    sessions::forget_session(&chat);
}

#[test]
fn app_pairing_dto_contains_user_code_but_never_session_key() {
    let dto = ComputerPairingChallengeDto {
        nonce: "n".into(),
        instance_id: "i".into(),
        verification_code: "ABCDE-FGHIJ-KLMNO-PQRST".into(),
        endpoint: "http://127.0.0.1:12345".into(),
        installed_extension_id: "e".into(),
        expires_at_ms: 1,
    };
    let value = serde_json::to_value(&dto).expect("dto json");
    assert!(value.get("secret").is_none());
    assert!(value.get("pairingSecret").is_none());
    assert!(value.get("pairing_secret").is_none());
    assert_eq!(value["nonce"], "n");
    assert_eq!(value["instanceId"], "i");
    assert!(value["verificationCode"].is_string());
    assert!(value.get("sessionKey").is_none());
}

#[tokio::test]
async fn cleanup_retry_command_validates_input_and_returns_absent_catalog_dto() {
    let manager = SessionManager::new();
    let missing = computer_use_retry_cleanup_impl(&manager, None)
        .await
        .expect_err("missing chat must fail closed");
    assert_eq!(missing, "select a local chat first");
    let blank = computer_use_retry_cleanup_impl(&manager, Some("   ".into()))
        .await
        .expect_err("blank chat must fail closed");
    assert_eq!(blank, "select a local chat first");

    let session = format!("cleanup-command-dto-{}", uuid::Uuid::new_v4());
    manager
        .reconcile_session_mcp(&session)
        .await
        .expect("a disconnected desired-absent catalog is already clean");
    let dto = computer_use_retry_cleanup_impl(&manager, Some(session))
        .await
        .expect("settled desired-absent retry is idempotent");
    assert_eq!(dto.desired_generation, 0);
    assert_eq!(dto.applied_generation, Some(0));
    assert!(!dto.desired_present);
    assert!(!dto.pending);
    assert!(!dto.cleanup_pending);
    assert_eq!(dto.last_error, None);
}
