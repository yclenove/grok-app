use super::*;
use crate::browser::{SharedTabOffer, TabDisconnect};
use crate::pairing::{ExtensionPairing, PairingSession, EXTENSION_ID};

fn fixture() -> (ExistingTabHost, PairingSession, String) {
    let host = ExistingTabHost::new();
    let challenge = host.begin_pairing_challenge();
    host.confirm_pairing_app_for(&challenge.nonce).unwrap();
    let session = host
        .complete_pairing_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    let origin = format!("chrome-extension://{EXTENSION_ID}");
    (host, session, origin)
}
fn grant(host: &ExistingTabHost, session: &PairingSession, origin: &str, tab: &str, sequence: u64) {
    host.offer_connected_tab(
        &connection(session),
        sequence,
        SharedTabOffer {
            pairing_token: &session.session_key,
            origin,
            extension_id: Some(EXTENSION_ID),
            tab_id: tab,
            title: "fixture",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 7,
            connection_generation: session.generation,
            focused: true,
        },
    )
    .unwrap();
    let selector = host
        .list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with(&format!("{tab}@")))
        .unwrap()
        .0;
    host.grant_picker_tab("owner", &format!("run-{tab}"), &selector)
        .unwrap();
}
fn connection(s: &PairingSession) -> PairingConnection {
    PairingConnection {
        instance_id: s.instance_id.clone(),
        connection_nonce: s.connection_nonce.clone(),
        generation: s.generation,
    }
}
fn result(request: ExtensionRequest) -> ExtensionResult {
    let ExtensionCommand::Observe { snapshot_id, .. } = &request.command;
    ExtensionResult {
        outcome: ExtensionOutcome::Observation {
            observation: ExtensionObservation {
                snapshot_id: snapshot_id.clone(),
                document_id: "0123456789abcdef0123456789abcdef".into(),
                title: "fixture".into(),
                url: "https://fixture.invalid/".into(),
                text: "visible fixture".into(),
                nodes: vec![],
                viewport_width: 800,
                viewport_height: 600,
                truncated: false,
                screenshot: None,
            },
        },
        request,
    }
}

#[test]
fn pairing_and_candidates_without_a_run_grant_cannot_enqueue() {
    let (host, session, origin) = fixture();
    assert!(host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_err());
    grant(&host, &session, &origin, "101", 1);
    assert!(host
        .enqueue_extension_observe("other", "run-101", "101", false)
        .is_err());
    assert!(host
        .enqueue_extension_observe("owner", "other", "101", false)
        .is_err());
    host.return_borrowed("run-101", "101").unwrap();
    assert!(host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_err());
}

#[test]
fn requests_are_delivered_once_and_results_require_every_bound_field() {
    let (host, session, origin) = fixture();
    grant(&host, &session, &origin, "101", 1);
    let (request, receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    assert!(host
        .complete_extension_request(&origin, &session.session_key, result(request.clone()))
        .is_err());
    assert_eq!(
        host.poll_extension_request(&origin, &session.session_key, &connection(&session))
            .unwrap(),
        Some(request.clone())
    );
    assert!(host
        .poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap()
        .is_none());
    for field in [
        "protocol",
        "requestId",
        "sequence",
        "deadlineMs",
        "session",
        "runId",
        "tabId",
        "documentGeneration",
        "grantGeneration",
    ] {
        let mut altered = serde_json::to_value(&request).unwrap();
        if altered[field].is_number() {
            altered[field] = serde_json::json!(altered[field].as_u64().unwrap() + 1);
        } else {
            altered[field] = serde_json::json!("other");
        }
        let altered = serde_json::from_value(altered).unwrap();
        assert!(
            host.complete_extension_request(&origin, &session.session_key, result(altered))
                .is_err(),
            "{field}"
        );
    }
    host.complete_extension_request(&origin, &session.session_key, result(request.clone()))
        .unwrap();
    assert_eq!(receive.recv().unwrap().unwrap().text, "visible fixture");
    assert!(host
        .complete_extension_request(&origin, &session.session_key, result(request))
        .is_err());
}

#[test]
fn queue_is_bounded_and_only_one_request_per_tab_is_pending() {
    let (host, session, origin) = fixture();
    let mut receivers = vec![];
    for i in 1..=8 {
        let tab = i.to_string();
        grant(&host, &session, &origin, &tab, i);
        receivers.push(
            host.enqueue_extension_observe("owner", &format!("run-{tab}"), &tab, false)
                .unwrap()
                .1,
        );
    }
    assert!(host
        .enqueue_extension_observe("owner", "run-1", "1", false)
        .is_err());
    grant(&host, &session, &origin, "9", 9);
    assert!(host
        .enqueue_extension_observe("owner", "run-9", "9", false)
        .is_err());
    assert_eq!(host.inner.lock().extension_requests.len(), 8);
}

#[test]
fn deadline_navigation_return_revoke_and_disconnect_retire_pending_work() {
    for mode in [
        "deadline",
        "navigation",
        "return",
        "revoke",
        "disconnect",
        "unshare",
    ] {
        let (host, session, origin) = fixture();
        grant(&host, &session, &origin, "101", 1);
        let (request, receive) = host
            .enqueue_extension_observe("owner", "run-101", "101", false)
            .unwrap();
        host.poll_extension_request(&origin, &session.session_key, &connection(&session))
            .unwrap();
        match mode {
            "deadline" => host.inner.lock().extension_requests[0].deadline = Instant::now(),
            "navigation" => {
                host.note_user_navigation("101", "https://fixture.invalid/new")
                    .unwrap();
            }
            "return" => {
                host.return_borrowed("run-101", "101").unwrap();
            }
            "revoke" => host.revoke_pairing(),
            "disconnect" => {
                host.disconnect_tab("101", TabDisconnect::Stop).unwrap();
            }
            _ => {
                host.unoffer_connected_tab(
                    &origin,
                    &session.session_key,
                    &connection(&session),
                    2,
                    "101",
                    7,
                )
                .unwrap();
            }
        }
        assert!(
            host.complete_extension_request(&origin, &session.session_key, result(request))
                .is_err(),
            "{mode}"
        );
        assert!(
            matches!(
                receive.recv_timeout(Duration::from_millis(100)),
                Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "{mode}"
        );
        assert!(host.inner.lock().extension_requests.is_empty());
    }
}

#[test]
fn malformed_observation_does_not_complete_or_unblock_next_request() {
    let (host, session, origin) = fixture();
    grant(&host, &session, &origin, "101", 1);
    let (request, _receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    host.poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap();
    let mut reply = result(request);
    let ExtensionOutcome::Observation { observation } = &mut reply.outcome else {
        unreachable!()
    };
    observation.snapshot_id = "wrong".into();
    assert!(host
        .complete_extension_request(&origin, &session.session_key, reply)
        .is_err());
    assert_eq!(host.inner.lock().extension_requests.len(), 1);
}

#[test]
fn screenshot_request_is_exact_and_requires_a_valid_image() {
    for screenshot in [false, true] {
        let (host, session, origin) = fixture();
        grant(&host, &session, &origin, "101", 1);
        let (request, receive) = host
            .enqueue_extension_observe("owner", "run-101", "101", screenshot)
            .unwrap();
        host.poll_extension_request(&origin, &session.session_key, &connection(&session))
            .unwrap();
        let mut reply = result(request);
        let ExtensionOutcome::Observation { observation } = &mut reply.outcome else {
            unreachable!()
        };
        observation.screenshot = (!screenshot).then(crate::browser::extension_image::fixture_image);
        assert!(host
            .complete_extension_request(&origin, &session.session_key, reply.clone())
            .is_err());
        assert!(
            receive.try_recv().is_err(),
            "invalid image cannot release the consumer"
        );
        let ExtensionOutcome::Observation { observation } = &mut reply.outcome else {
            unreachable!()
        };
        observation.screenshot = screenshot.then(crate::browser::extension_image::fixture_image);
        host.complete_extension_request(&origin, &session.session_key, reply)
            .unwrap();
        assert_eq!(
            receive.recv().unwrap().unwrap().screenshot.is_some(),
            screenshot
        );
    }
}
