use super::*;
use crate::broker::BrokerOptions;
use crate::fake::FakeAdapter;
use crate::pairing::{ExtensionPairing, PairingSession, EXTENSION_ID};

pub(super) fn fixture() -> (Arc<ComputerUseBroker>, IpcServer, PairingSession) {
    let broker = Arc::new(ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    ));
    let server = spawn(broker.clone()).unwrap();
    let session = pair(&broker);
    (broker, server, session)
}

fn pair(broker: &ComputerUseBroker) -> PairingSession {
    let challenge = broker.begin_pairing().unwrap();
    broker.confirm_pairing(&challenge.nonce).unwrap();
    broker
        .complete_pairing(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap()
}

pub(super) fn offer(session: &PairingSession) -> Value {
    json!({ "instanceId": session.instance_id, "connectionNonce": session.connection_nonce,
        "generation": session.generation, "sequence": 1, "tabId": "101", "title": "fixture",
        "url": "https://fixture.invalid/", "browserId": "chromium", "profileId": "connection",
        "documentGeneration": 7, "focused": true })
}

fn unoffer(session: &PairingSession, sequence: u64, document: u64) -> Value {
    json!({ "instanceId": session.instance_id, "connectionNonce": session.connection_nonce,
        "generation": session.generation, "sequence": sequence, "tabId": "101",
        "documentGeneration": document })
}

pub(super) fn post(server: &IpcServer, session: &PairingSession, route: &str, body: &Value) -> u16 {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}{route}", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .bearer_auth(&session.session_key)
        .json(body)
        .send()
        .unwrap_or_else(|error| {
            panic!(
                "POST {route} ({} JSON bytes) failed: {error:?}",
                body.to_string().len()
            )
        })
        .status()
        .as_u16()
}

#[test]
fn unshare_fences_borrowed_grant_and_late_offer_cannot_resurrect_it() {
    let (broker, server, session) = fixture();
    let body = offer(&session);
    assert_eq!(post(&server, &session, "/cu/tab-offer", &body), 200);
    let info = broker
        .tabs()
        .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
        .unwrap();
    assert_eq!(info.document_generation, 7);
    assert_eq!(info.connection_generation, session.generation);
    assert!(broker.tabs().act("run", "101").is_ok());
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/tab-unoffer",
            &unoffer(&session, 2, 7)
        ),
        200
    );
    assert!(broker.tabs().act("run", "101").is_err());
    assert!(broker.tabs().list_shared_candidates().is_empty());
    assert!(
        !broker.tabs().is_closed("101"),
        "unshare must never close the user's tab"
    );
    assert_ne!(post(&server, &session, "/cu/tab-offer", &body), 200);
    assert!(broker.tabs().list_shared_candidates().is_empty());
}

fn preflight(server: &IpcServer, route: &str) -> u16 {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .request(reqwest::Method::OPTIONS, format!("{}{route}", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .unwrap()
        .status()
        .as_u16()
}

fn saturate(server: &IpcServer, bucket: &str) {
    server.service.rate.lock().insert(
        bucket.into(),
        (0..RATE_MAX).map(|_| Instant::now()).collect(),
    );
}

#[test]
fn pairing_traffic_cannot_starve_tab_retirement() {
    let (broker, server, session) = fixture();
    assert_eq!(
        post(&server, &session, "/cu/tab-offer", &offer(&session)),
        200
    );
    broker
        .tabs()
        .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
        .unwrap();
    saturate(&server, "pairing");
    saturate(&server, "extension-lease");
    assert_eq!(preflight(&server, "/cu/tab-unoffer"), 204);
    let mut stale = session.clone();
    stale.session_key = "f".repeat(64);
    assert_eq!(
        post(&server, &stale, "/cu/tab-unoffer", &unoffer(&session, 2, 7)),
        403
    );
    assert_eq!(broker.tabs().list_shared_candidates().len(), 1);
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/tab-unoffer",
            &unoffer(&session, 2, 7)
        ),
        200
    );
    assert!(broker.tabs().list_shared_candidates().is_empty());
    assert!(broker.tabs().act("run", "101").is_err());
    assert!(!broker.tabs().is_closed("101"));
}

#[test]
fn pairing_traffic_cannot_starve_disconnect_or_heartbeat() {
    for route in [
        "/cu/extension-status",
        "/cu/extension-heartbeat",
        "/cu/extension-disconnect",
    ] {
        let (broker, server, session) = fixture();
        let body = json!({"instanceId":session.instance_id, "connectionNonce":session.connection_nonce,
            "generation":session.generation});
        saturate(&server, "pairing");
        assert_eq!(preflight(&server, route), 204, "{route}");
        assert_eq!(post(&server, &session, route, &body), 200, "{route}");
        assert_eq!(
            broker.tabs().pairing_session_key().is_some(),
            !route.ends_with("disconnect")
        );
    }
}

#[test]
fn pairing_traffic_cannot_starve_browser_lifecycle_registration() {
    let (_broker, server, session) = fixture();
    saturate(&server, "pairing");
    let body = json!({
        "connection": {
            "instanceId": session.instance_id,
            "connectionNonce": session.connection_nonce,
            "generation": session.generation
        },
        "currentId": "00000000-0000-4000-8000-000000000002",
        "previousId": null
    });
    let status = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}/cu/browser-restart", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .bearer_auth(&session.session_key)
        .json(&body)
        .send()
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 200);
}

#[test]
fn retirement_rate_stays_bounded_without_starving_lease_renewal() {
    let (_, server, session) = fixture();
    let body = json!({"instanceId":session.instance_id, "connectionNonce":session.connection_nonce,
        "generation":session.generation});
    saturate(&server, "extension-retirement");
    assert_eq!(preflight(&server, "/cu/extension-disconnect"), 429);
    assert_eq!(
        post(&server, &session, "/cu/extension-disconnect", &body),
        429
    );
    assert_eq!(preflight(&server, "/cu/extension-heartbeat"), 204);
    assert_eq!(
        post(&server, &session, "/cu/extension-heartbeat", &body),
        200
    );
}

#[test]
fn old_document_or_connection_cannot_remove_a_new_offer() {
    let (broker, server, old) = fixture();
    assert_eq!(post(&server, &old, "/cu/tab-offer", &offer(&old)), 200);
    broker
        .tabs()
        .grant_picker_tab(
            "old",
            "old-run",
            &broker.tabs().list_shared_candidates()[0].0,
        )
        .unwrap();
    let mut newer = offer(&old);
    newer["sequence"] = json!(3);
    newer["documentGeneration"] = json!(8);
    assert_eq!(post(&server, &old, "/cu/tab-offer", &newer), 200);
    assert!(
        broker.tabs().act("old-run", "101").is_err(),
        "new document must invalidate the old grant"
    );
    assert_ne!(
        post(&server, &old, "/cu/tab-unoffer", &unoffer(&old, 2, 7)),
        200
    );
    assert_ne!(
        post(&server, &old, "/cu/tab-unoffer", &unoffer(&old, 4, 7)),
        200
    );
    assert_eq!(broker.tabs().list_shared_candidates().len(), 1);
    let fresh = pair(&broker);
    assert_eq!(post(&server, &fresh, "/cu/tab-offer", &offer(&fresh)), 200);
    assert_eq!(
        post(&server, &old, "/cu/tab-unoffer", &unoffer(&old, 5, 7)),
        403
    );
    assert_eq!(broker.tabs().list_shared_candidates().len(), 1);
    assert_eq!(
        broker
            .tabs()
            .grant_picker_tab(
                "new",
                "new-run",
                &broker.tabs().list_shared_candidates()[0].0
            )
            .unwrap()
            .connection_generation,
        fresh.generation
    );
}

#[test]
fn malformed_shared_metadata_never_enters_picker() {
    for (field, value) in [
        ("tabId", json!("x".repeat(129))),
        ("url", json!("https://")),
        ("url", json!("https://user:secret@fixture.invalid/")),
        ("documentGeneration", json!(0)),
        ("sequence", json!(0)),
        ("profileId", json!("x".repeat(129))),
    ] {
        let (broker, server, session) = fixture();
        let mut body = offer(&session);
        body[field] = value;
        assert_ne!(
            post(&server, &session, "/cu/tab-offer", &body),
            200,
            "field {field}"
        );
        assert!(broker.tabs().list_shared_candidates().is_empty());
    }
}

fn connected_offer(
    broker: &ComputerUseBroker,
    session: &PairingSession,
    sequence: u64,
    tab: &str,
) -> Result<(), String> {
    let origin = format!("chrome-extension://{EXTENSION_ID}");
    broker.offer_connected_tab(
        &crate::pairing::PairingConnection {
            instance_id: session.instance_id.clone(),
            connection_nonce: session.connection_nonce.clone(),
            generation: session.generation,
        },
        sequence,
        crate::browser::SharedTabOffer {
            pairing_token: &session.session_key,
            origin: &origin,
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
}

#[test]
fn candidate_registry_has_a_fixed_capacity() {
    let (broker, _server, session) = fixture();
    for sequence in 1..=64 {
        connected_offer(&broker, &session, sequence, &sequence.to_string()).unwrap();
    }
    assert!(connected_offer(&broker, &session, 65, "65").is_err());
    assert_eq!(broker.tabs().list_shared_candidates().len(), 64);
    // Updating an existing tab is still possible at capacity.
    connected_offer(&broker, &session, 66, "1").unwrap();
    assert_eq!(broker.tabs().list_shared_candidates().len(), 64);
}

#[test]
fn feature_off_and_share_linearize_without_recreating_authority() {
    for _ in 0..16 {
        let (broker, _server, session) = fixture();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let task_broker = broker.clone();
        let task_barrier = barrier.clone();
        let task_session = session.clone();
        let task = std::thread::spawn(move || {
            task_barrier.wait();
            let _ = connected_offer(&task_broker, &task_session, 1, "101");
        });
        barrier.wait();
        broker.set_feature_enabled(false);
        task.join().unwrap();
        assert!(broker.tabs().list_shared_candidates().is_empty());
        assert!(broker.tabs().pairing_session_key().is_none());
        assert!(connected_offer(&broker, &session, 2, "101").is_err());
        assert_eq!(
            post(
                &_server,
                &session,
                "/cu/tab-unoffer",
                &unoffer(&session, 3, 7)
            ),
            403
        );
    }
}

#[test]
fn stale_picker_selection_cannot_authorize_a_replacement_document() {
    let (broker, server, session) = fixture();
    assert_eq!(
        post(&server, &session, "/cu/tab-offer", &offer(&session)),
        200
    );
    let old_selection = broker.tabs().list_shared_candidates()[0].0.clone();
    let mut newer = offer(&session);
    newer["sequence"] = json!(2);
    newer["documentGeneration"] = json!(8);
    assert_eq!(post(&server, &session, "/cu/tab-offer", &newer), 200);
    assert!(
        broker
            .tabs()
            .grant_picker_tab("owner", "run", &old_selection)
            .is_err(),
        "selecting an old picker row must not silently authorize the new document"
    );
    let fresh_selection = broker.tabs().list_shared_candidates()[0].0.clone();
    assert_ne!(old_selection, fresh_selection);
    let granted = broker
        .tabs()
        .grant_picker_tab("owner", "run", &fresh_selection)
        .unwrap();
    assert_eq!(granted.document_generation, 8);
}
