use super::tests_sharing::{fixture, post};
use super::*;

#[test]
fn extension_poll_rejects_wrong_connection_and_arbitrary_script_fields() {
    let (_broker, server, session) = fixture();
    let mut body = json!({"instanceId": session.instance_id,
        "connectionNonce": session.connection_nonce, "generation": session.generation});
    body["generation"] = json!(session.generation + 1);
    assert_eq!(post(&server, &session, "/cu/extension-poll", &body), 403);
    body["generation"] = json!(session.generation);
    body["script"] = json!("untrusted source");
    assert_eq!(post(&server, &session, "/cu/extension-poll", &body), 422);
}

fn connection(session: &crate::pairing::PairingSession) -> Value {
    json!({"instanceId": session.instance_id,"connectionNonce": session.connection_nonce,"generation": session.generation})
}
fn http(
    server: &IpcServer,
    session: &crate::pairing::PairingSession,
    route: &str,
    body: &Value,
) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("{}{route}", server.url))
        .header(
            "origin",
            format!("chrome-extension://{}", crate::pairing::EXTENSION_ID),
        )
        .bearer_auth(&session.session_key)
        .json(body)
        .send()
        .unwrap()
}

#[test]
fn real_http_observation_is_bounded_and_replay_or_forged_binding_cannot_complete() {
    let (broker, server, session) = fixture();
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/tab-offer",
            &super::tests_sharing::offer(&session)
        ),
        200
    );
    let selector = broker.tabs().list_shared_candidates()[0].0.clone();
    broker
        .tabs()
        .grant_picker_tab("owner", "run", &selector)
        .unwrap();
    let observing = broker.clone();
    let task = std::thread::spawn(move || {
        observing
            .tabs()
            .observe_existing_document("owner", "run", "101", true)
    });
    let polled = http(
        &server,
        &session,
        "/cu/extension-poll",
        &connection(&session),
    );
    assert_eq!(polled.status().as_u16(), 200);
    let polled: Value = polled.json().unwrap();
    assert!(!polled.to_string().contains(&session.session_key));
    let request = polled["request"].clone();
    assert!(request.is_object());
    let body = json!({"request":request,"outcome":{"kind":"observation","observation":{
        "snapshotId":request["command"]["snapshotId"],"documentId":"0123456789abcdef0123456789abcdef",
        "title":"fixture","url":"https://fixture.invalid/","text":"界".repeat(32000),
        "nodes":[],"viewportWidth":800,"viewportHeight":600,"truncated":false,
        "screenshot": crate::browser::extension_image::fixture_noisy_image()}}});
    assert!(
        body.to_string().len() > 128 * 1024,
        "result route needs its own bounded body limit"
    );
    let mut forged = body.clone();
    forged["request"]["session"] = json!("another-session");
    assert_eq!(
        post(&server, &session, "/cu/extension-result", &forged),
        403
    );
    let mut oversize = body.clone();
    oversize["outcome"]["observation"]["text"] = json!("x".repeat(769 * 1024));
    assert_eq!(
        post(&server, &session, "/cu/extension-result", &oversize),
        413
    );
    assert_eq!(post(&server, &session, "/cu/extension-result", &body), 200);
    assert_eq!(task.join().unwrap().unwrap().text.chars().count(), 32000);
    assert_eq!(post(&server, &session, "/cu/extension-result", &body), 403);
}

#[test]
fn feature_off_interrupts_a_live_long_poll_without_waiting_for_its_deadline() {
    let (broker, server, session) = fixture();
    let server = Arc::new(server);
    let poll_server = server.clone();
    let task = std::thread::spawn(move || {
        http(
            &poll_server,
            &session,
            "/cu/extension-poll",
            &connection(&session),
        )
        .status()
        .as_u16()
    });
    let wait = Instant::now() + Duration::from_secs(2);
    while server.service.extension_poll_slot.available_permits() != 0 {
        assert!(Instant::now() < wait, "poll did not enter product handler");
        std::thread::sleep(Duration::from_millis(5));
    }
    let cancelled = Instant::now();
    broker.set_feature_enabled(false);
    assert_eq!(task.join().unwrap(), 403);
    assert!(cancelled.elapsed() < Duration::from_millis(750));
}

#[test]
fn feature_off_releases_a_pending_observer_and_rejects_the_late_browser_result() {
    let (broker, server, session) = fixture();
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/tab-offer",
            &super::tests_sharing::offer(&session)
        ),
        200
    );
    let selector = broker.tabs().list_shared_candidates()[0].0.clone();
    broker
        .tabs()
        .grant_picker_tab("owner", "run", &selector)
        .unwrap();
    let observing = broker.clone();
    let task = std::thread::spawn(move || {
        observing
            .tabs()
            .observe_existing_document("owner", "run", "101", false)
    });
    let polled: Value = http(
        &server,
        &session,
        "/cu/extension-poll",
        &connection(&session),
    )
    .json()
    .unwrap();
    assert!(
        polled["request"].is_object(),
        "request must actually be dispatched before feature-off"
    );
    let start = Instant::now();
    broker.set_feature_enabled(false);
    assert!(task.join().unwrap().is_err());
    assert!(start.elapsed() < Duration::from_millis(750));
    let reply = json!({"request": polled["request"], "outcome": {"kind": "rejected", "reason": "cancelled"}});
    assert_eq!(post(&server, &session, "/cu/extension-result", &reply), 403);
    assert!(broker.tabs().list_shared_candidates().is_empty());
}
