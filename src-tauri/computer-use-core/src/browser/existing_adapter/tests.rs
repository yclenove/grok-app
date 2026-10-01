use super::*;
use crate::adapter::{ActionScope, DispatchRequest};
use crate::broker::{BrokerOptions, ComputerUseBroker};
use crate::browser::extension_action::ExtensionActionCommand;
use crate::browser::{extension_protocol::*, SharedTabOffer};
use crate::fake::FakeAdapter;
use crate::pairing::{ExtensionPairing, PairingConnection, PairingSession, EXTENSION_ID};
use crate::protocol::{ActionKind, ActionTarget, OutcomeKind, PROTOCOL_VERSION};
use serde_json::json;
use std::time::{Duration, Instant};

fn dispatch(action: ActionKind, parameters: serde_json::Value) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "run".into(),
        action_id: "action".into(),
        generation: 1,
        target_id: "tab:101".into(),
        target_generation: 7,
        snapshot_id: "00000000-0000-4000-8000-000000000001".into(),
        geometry_revision: 7,
        action,
        target: ActionTarget::Element {
            element_ref: "00000000-0000-4000-8000-000000000001-1".into(),
        },
        parameters,
        scope: ActionScope::Directed,
    }
}

#[test]
fn action_mapping_keeps_the_existing_tab_protocol_strict() {
    let click = super::extension_command(&dispatch(ActionKind::Click, json!({}))).unwrap();
    assert!(matches!(click, ExtensionActionCommand::Click { .. }));

    let fill =
        super::extension_command(&dispatch(ActionKind::SetValue, json!({"text":"你好"}))).unwrap();
    assert!(matches!(fill, ExtensionActionCommand::SetValue { .. }));

    let wait = super::extension_command(&dispatch(
        ActionKind::Wait,
        json!({"nameEquals":"完成","timeoutMs":1000}),
    ))
    .unwrap();
    assert!(matches!(wait, ExtensionActionCommand::Wait { .. }));

    for (action, parameters) in [
        (ActionKind::Click, json!({"button":"right"})),
        (ActionKind::Click, json!({"count":2})),
        (ActionKind::TypeText, json!({"text":"x","via":"clipboard"})),
    ] {
        assert!(super::extension_command(&dispatch(action, parameters)).is_err());
    }
    assert!(super::extension_command(&dispatch(ActionKind::Key, json!({"key":"Enter"}))).is_err());
    assert!(
        super::extension_command(&dispatch(ActionKind::Drag, json!({"toX":1,"toY":1}))).is_err()
    );
    assert!(super::extension_command(&DispatchRequest {
        target: ActionTarget::Coord { x: 1.0, y: 1.0 },
        ..dispatch(ActionKind::Click, json!({}))
    })
    .is_err());
}

#[test]
fn observation_nodes_publish_only_actions_the_extension_can_execute() {
    assert_eq!(super::node_actions("button", false), vec!["click", "wait"]);
    assert_eq!(
        super::node_actions("textbox", false),
        vec!["click", "wait", "set_value", "type_text"]
    );
    assert!(super::node_actions("textbox", true).is_empty());
}

fn fixture() -> (
    Arc<ComputerUseBroker>,
    Arc<FakeAdapter>,
    PairingSession,
    String,
) {
    let desktop = Arc::new(FakeAdapter::new());
    let broker = Arc::new(ComputerUseBroker::new(
        desktop.clone(),
        BrokerOptions {
            feature_enabled: true,
            ..Default::default()
        },
    ));
    broker.register_existing_browser_adapter().unwrap();
    broker.open_run("owner", "run").unwrap();
    let host = broker.tabs();
    let challenge = host.begin_pairing_challenge();
    host.confirm_pairing_app_for(&challenge.nonce).unwrap();
    let session = host
        .complete_pairing_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    let origin = format!("chrome-extension://{EXTENSION_ID}");
    host.offer_connected_tab(
        &connection(&session),
        1,
        SharedTabOffer {
            pairing_token: &session.session_key,
            origin: &origin,
            extension_id: Some(EXTENSION_ID),
            tab_id: "101",
            title: "Fixture",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 7,
            connection_generation: session.generation,
            focused: true,
        },
    )
    .unwrap();
    let candidate = broker
        .list_targets_for_surface(SurfaceKind::ExistingTab)
        .unwrap()
        .remove(0);
    broker
        .authorize_on_surface(
            "owner",
            "run",
            SurfaceKind::ExistingTab,
            &candidate.target_id,
        )
        .unwrap();
    (broker, desktop, session, origin)
}
fn connection(s: &PairingSession) -> PairingConnection {
    PairingConnection {
        instance_id: s.instance_id.clone(),
        connection_nonce: s.connection_nonce.clone(),
        generation: s.generation,
    }
}
fn poll(broker: &ComputerUseBroker, session: &PairingSession, origin: &str) -> ExtensionRequest {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(request) = broker
            .tabs()
            .poll_extension_request(origin, &session.session_key, &connection(session))
            .unwrap()
        {
            return request;
        }
        assert!(
            Instant::now() < until,
            "adapter never reached the extension transport"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn complete(
    broker: &ComputerUseBroker,
    session: &PairingSession,
    origin: &str,
    request: ExtensionRequest,
) {
    let ExtensionCommand::Observe {
        snapshot_id,
        screenshot,
        ..
    } = &request.command;
    let observation = ExtensionObservation {
        snapshot_id: snapshot_id.clone(),
        document_id: "0123456789abcdef0123456789abcdef".into(),
        title: "Fixture".into(),
        url: "https://fixture.invalid/".into(),
        text: "Visible document 你好".into(),
        nodes: vec![ExtensionNode {
            element_ref: format!("{snapshot_id}-1"),
            role: "button".into(),
            name: "Fixture control".into(),
            disabled: false,
        }],
        viewport_width: 800,
        viewport_height: 600,
        truncated: false,
        screenshot: screenshot.then(crate::browser::extension_image::fixture_image),
    };
    broker
        .tabs()
        .complete_extension_request(
            origin,
            &session.session_key,
            ExtensionResult {
                request,
                outcome: ExtensionOutcome::Observation { observation },
            },
        )
        .unwrap();
}

fn complete_action(
    broker: &ComputerUseBroker,
    session: &PairingSession,
    origin: &str,
    dispatch: &crate::browser::extension_action::ActionDispatch,
    status: crate::browser::extension_action::ActionStatus,
) {
    broker
        .tabs()
        .claim_existing_action(origin, &session.session_key, dispatch)
        .unwrap();
    // The extension settles the physical receipt only after its click/input
    // handler has returned.  The adapter's business waiter is not released
    // until this proof and the result are both accepted.
    broker
        .tabs()
        .settle_existing_completion(origin, &dispatch.proof)
        .unwrap();
    broker
        .tabs()
        .complete_existing_action(
            origin,
            &session.session_key,
            crate::browser::extension_action::ActionResult {
                request: dispatch.request.clone(),
                proof: dispatch.proof.clone(),
                outcome: crate::browser::extension_action::ActionOutcome {
                    status,
                    detail: "fixture_result".into(),
                },
            },
        )
        .unwrap();
}

fn reject_action(
    broker: &ComputerUseBroker,
    session: &PairingSession,
    origin: &str,
    dispatch: &crate::browser::extension_action::ActionDispatch,
) {
    broker
        .tabs()
        .complete_existing_action(
            origin,
            &session.session_key,
            crate::browser::extension_action::ActionResult {
                request: dispatch.request.clone(),
                proof: dispatch.proof.clone(),
                outcome: crate::browser::extension_action::ActionOutcome {
                    status: crate::browser::extension_action::ActionStatus::Rejected,
                    detail: "fixture_rejected".into(),
                },
            },
        )
        .unwrap();
}

#[test]
fn mcp_act_reaches_existing_adapter_and_waits_for_verified_extension_result() {
    let (broker, desktop, session, origin) = fixture();
    broker
        .tabs()
        .negotiate_existing_actions(
            &origin,
            &session.session_key,
            crate::browser::extension_action::ActionNegotiation {
                protocol: 2,
                completion_protocol: 2,
                connection: connection(&session),
                actions: vec![
                    crate::browser::extension_action::ActionName::Click,
                    crate::browser::extension_action::ActionName::SetValue,
                    crate::browser::extension_action::ActionName::TypeText,
                    crate::browser::extension_action::ActionName::Scroll,
                    crate::browser::extension_action::ActionName::Wait,
                ],
            },
        )
        .unwrap();

    let observing = broker.clone();
    let observe_task = std::thread::spawn(move || observing.observe_with_screenshot("run", false));
    let observe_request = poll(&broker, &session, &origin);
    complete(&broker, &session, &origin, observe_request);
    let observation = observe_task.join().unwrap().unwrap();
    let node = observation.nodes.first().unwrap().node_ref.clone();

    let binding = crate::ipc::SessionBinding {
        session_id: "owner".into(),
        run_id: "run".into(),
    };
    let args = serde_json::json!({
        "version": PROTOCOL_VERSION,
        "actionId": "existing-click-1",
        "runId": "run",
        "targetId": "tab:101",
        "targetGeneration": observation.target_generation,
        "snapshotId": observation.snapshot_id,
        "geometryRevision": observation.geometry_revision,
        "action": "click",
        "target": {"elementRef": node},
        "parameters": {}
    });
    let action_broker = broker.clone();
    let action_task = std::thread::spawn(move || {
        crate::tools::dispatch(&action_broker, &binding, "computer_act", args)
    });

    let dispatch = loop {
        if let Some(dispatch) = broker
            .tabs()
            .poll_existing_action(&origin, &session.session_key, &connection(&session))
            .unwrap()
        {
            break dispatch;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(matches!(
        dispatch.request.command,
        crate::browser::extension_action::ExtensionActionCommand::Click { .. }
    ));
    complete_action(
        &broker,
        &session,
        &origin,
        &dispatch,
        crate::browser::extension_action::ActionStatus::Verified,
    );

    let reply = action_task.join().unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    let outcome: crate::protocol::ActionOutcome =
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(outcome.kind, OutcomeKind::Verified, "{outcome:?}");
    assert!(outcome.executed);
    assert_eq!(desktop.observe_calls(), 0);

    let drive_verified = |action_id: &str, action: &str, parameters: serde_json::Value| {
        let args = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "actionId": action_id,
            "runId": "run",
            "targetId": "tab:101",
            "targetGeneration": observation.target_generation,
            "snapshotId": observation.snapshot_id,
            "geometryRevision": observation.geometry_revision,
            "action": action,
            "target": {"elementRef": node},
            "parameters": parameters
        });
        let action_broker = broker.clone();
        let action_task = std::thread::spawn(move || {
            crate::tools::dispatch(
                &action_broker,
                &crate::ipc::SessionBinding {
                    session_id: "owner".into(),
                    run_id: "run".into(),
                },
                "computer_act",
                args,
            )
        });
        let dispatch = loop {
            if let Some(dispatch) = broker
                .tabs()
                .poll_existing_action(&origin, &session.session_key, &connection(&session))
                .unwrap()
            {
                break dispatch;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        complete_action(
            &broker,
            &session,
            &origin,
            &dispatch,
            crate::browser::extension_action::ActionStatus::Verified,
        );
        let reply = action_task.join().unwrap();
        assert_eq!(reply["isError"], false, "{reply}");
        let outcome: crate::protocol::ActionOutcome =
            serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(outcome.kind, OutcomeKind::Verified, "{outcome:?}");
    };
    drive_verified(
        "existing-set-value",
        "set_value",
        serde_json::json!({"text":"新值"}),
    );
    drive_verified(
        "existing-type-text",
        "type_text",
        serde_json::json!({"text":"追加"}),
    );
    drive_verified(
        "existing-scroll",
        "scroll",
        serde_json::json!({"delta":240}),
    );
    drive_verified(
        "existing-wait",
        "wait",
        serde_json::json!({"nameEquals":"Fixture action","timeoutMs":1000}),
    );

    let rejected_args = serde_json::json!({
        "version": PROTOCOL_VERSION,
        "actionId": "existing-click-rejected",
        "runId": "run",
        "targetId": "tab:101",
        "targetGeneration": observation.target_generation,
        "snapshotId": observation.snapshot_id,
        "geometryRevision": observation.geometry_revision,
        "action": "click",
        "target": {"elementRef": node},
        "parameters": {}
    });
    let rejected_broker = broker.clone();
    let rejected_task = std::thread::spawn(move || {
        crate::tools::dispatch(
            &rejected_broker,
            &crate::ipc::SessionBinding {
                session_id: "owner".into(),
                run_id: "run".into(),
            },
            "computer_act",
            rejected_args,
        )
    });
    let rejected_dispatch = loop {
        if let Some(dispatch) = broker
            .tabs()
            .poll_existing_action(&origin, &session.session_key, &connection(&session))
            .unwrap()
        {
            break dispatch;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    reject_action(&broker, &session, &origin, &rejected_dispatch);
    let rejected_reply = rejected_task.join().unwrap();
    assert_eq!(rejected_reply["isError"], true, "{rejected_reply}");
    let rejected: crate::protocol::ActionOutcome =
        serde_json::from_str(rejected_reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rejected.kind, OutcomeKind::Rejected, "{rejected:?}");
    assert!(!rejected.executed);
}

#[test]
fn existing_tab_listing_never_enumerates_the_desktop() {
    let (broker, desktop, _session, _origin) = fixture();
    let calls = desktop.list_calls();
    desktop.set_listing_error(true);
    let reply = crate::tools::dispatch(
        &broker,
        &crate::ipc::SessionBinding {
            session_id: "owner".into(),
            run_id: "run".into(),
        },
        "computer_list_targets",
        json!({}),
    );
    assert_eq!(reply["isError"], false, "{reply}");
    let listed: serde_json::Value =
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["targetId"], "tab:101");
    assert_eq!(desktop.list_calls(), calls);
}

#[test]
fn claimed_completion_keeps_broker_stop_requested_until_receipt_settles() {
    use crate::broker::StopState;
    let (broker, desktop, session, origin) = fixture();
    let observing = broker.clone();
    let task = std::thread::spawn(move || observing.observe_with_screenshot("run", false));
    let request = poll(&broker, &session, &origin);
    complete(&broker, &session, &origin, request);
    let observation = task.join().unwrap().unwrap();
    let proof = broker
        .tabs()
        .offer_existing_completion(
            "owner",
            "run",
            "101",
            &observation.snapshot_id,
            crate::execution::ActionCancellation::default(),
        )
        .unwrap();
    broker
        .claim_existing_completion(&origin, &session.session_key, &proof)
        .unwrap();
    assert_eq!(
        broker.request_stop("run").unwrap(),
        StopState::StopRequested
    );
    assert_eq!(broker.stop_state("run").unwrap(), StopState::StopRequested);
    assert!(!broker.tabs().is_closed("101"));
    broker
        .tabs()
        .settle_existing_completion(&origin, &proof)
        .unwrap();
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
    assert_eq!(desktop.observe_calls(), 0);
}

#[test]
fn mcp_observation_routes_numeric_tab_through_existing_adapter_and_preserves_text() {
    let (broker, desktop, session, origin) = fixture();
    assert_eq!(broker.authorized_target("run").unwrap().0, "tab:101");
    assert_eq!(
        broker.capabilities_for_run("run").unwrap().backend_id,
        "chromium-extension"
    );
    let observing = broker.clone();
    let task = std::thread::spawn(move || {
        crate::tools::dispatch(
            &observing,
            &crate::ipc::SessionBinding {
                session_id: "owner".into(),
                run_id: "run".into(),
            },
            "computer_observe",
            json!({"screenshot":false}),
        )
    });
    let request = poll(&broker, &session, &origin);
    assert!(matches!(
        request.command,
        ExtensionCommand::Observe {
            screenshot: false,
            preview: false,
            ..
        }
    ));
    complete(&broker, &session, &origin, request);
    let reply = task.join().unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    let observed: serde_json::Value =
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(observed["targetId"], "tab:101");
    assert_eq!(observed["text"], "Visible document 你好");
    assert!(!observed.to_string().contains("pngBase64"));
    assert_eq!(desktop.observe_calls(), 0);

    let old = broker
        .tabs()
        .tab_write_identity("run", "101")
        .unwrap()
        .snapshot_id;
    let observing = broker.clone();
    let preview = std::thread::spawn(move || observing.observe_preview("run"));
    let request = poll(&broker, &session, &origin);
    assert!(matches!(
        request.command,
        ExtensionCommand::Observe { preview: true, .. }
    ));
    complete(&broker, &session, &origin, request);
    assert!(preview.join().unwrap().is_ok());
    assert_eq!(
        broker
            .tabs()
            .tab_write_identity("run", "101")
            .unwrap()
            .snapshot_id,
        old
    );
    assert_eq!(broker.act_defaults("run").unwrap().2, old.unwrap());
}

#[test]
fn stop_fence_cancels_pending_observation_before_cleanup_and_late_result_is_rejected() {
    let (broker, desktop, session, origin) = fixture();
    let observing = broker.clone();
    let task = std::thread::spawn(move || observing.observe_with_screenshot("run", false));
    let request = poll(&broker, &session, &origin);
    let started = Instant::now();
    let ticket = broker.fence_stop("run").unwrap().unwrap();
    assert!(task.join().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_millis(750));
    assert!(broker
        .tabs()
        .complete_extension_request(
            &origin,
            &session.session_key,
            ExtensionResult {
                request,
                outcome: ExtensionOutcome::Rejected {
                    reason: ExtensionRejection::Cancelled
                }
            }
        )
        .is_err());
    broker.finish_stop_cleanup(&ticket).unwrap();
    assert!(broker.tabs().existing_operations_idle("run"));
    assert!(!broker.tabs().is_closed("101"));
    assert_eq!(desktop.observe_calls(), 0);
}

#[test]
fn stopped_run_cannot_leave_a_new_picker_borrow() {
    let (broker, _, _, _) = fixture();
    broker.tabs().return_borrowed("run", "101").unwrap();
    broker.fence_stop("run").unwrap();
    let candidate = broker
        .list_targets_for_surface(SurfaceKind::ExistingTab)
        .unwrap()
        .remove(0);
    assert!(broker
        .authorize_on_surface(
            "owner",
            "run",
            SurfaceKind::ExistingTab,
            &candidate.target_id
        )
        .is_err());
    assert!(
        broker.tabs().existing_target_info("101").is_none(),
        "failed authorization leaked a borrow"
    );
}

#[test]
fn picker_authorization_requires_the_runs_actual_session_owner() {
    let (broker, _, _, _) = fixture();
    broker.tabs().return_borrowed("run", "101").unwrap();
    let candidate = broker
        .list_targets_for_surface(SurfaceKind::ExistingTab)
        .unwrap()
        .remove(0);
    assert!(
        broker
            .authorize_on_surface(
                "other-owner",
                "run",
                SurfaceKind::ExistingTab,
                &candidate.target_id
            )
            .is_err(),
        "cross-session authorization succeeded"
    );
    assert!(broker.tabs().existing_target_info("101").is_none());
}

#[test]
fn picker_rollback_distinguishes_reused_and_replacement_grants() {
    let (broker, _, _, _) = fixture();
    let host = broker.tabs();
    let selector = host.list_shared_candidates().remove(0).0;
    let (old, acquired) = host.acquire_picker_tab("owner", "run", &selector).unwrap();
    assert!(
        !acquired,
        "reauthorizing a borrowed tab must not own its rollback"
    );
    host.return_borrowed("run", "101").unwrap();
    let (current, acquired) = host.acquire_picker_tab("owner", "run", &selector).unwrap();
    assert!(acquired);
    assert!(
        !host.release_existing_grant(&old),
        "old cleanup revoked a replacement grant"
    );
    assert_eq!(
        host.existing_target_info("101").unwrap().generation,
        current.generation
    );
    assert!(host.release_existing_grant(&current));
    assert!(
        !host.release_existing_grant(&current),
        "cleanup must be idempotent"
    );
    assert!(host.existing_target_info("101").is_none());
    assert!(!host.is_closed("101"));
}
