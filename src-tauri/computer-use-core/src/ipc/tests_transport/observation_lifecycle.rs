use super::*;
use crate::pairing::EXTENSION_ID;

const PREFIX: &str = "/cu/extension-observation";

fn cleanup(server: &IpcServer, operation: &str, body: &Value) -> reqwest::blocking::Response {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(format!("{}{PREFIX}/{operation}", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .json(body)
        .send()
        .unwrap()
}

#[test]
fn observation_protocol_negotiation_refuses_legacy_or_cross_purpose_channels() {
    let (_broker, server, session) = fixture();
    let route = format!("{PREFIX}/negotiate");
    let body = json!({"protocol":2,"completionProtocol":2,"purpose":"observation","connection":connection(&session)});
    assert_eq!(cleanup(&server, "negotiate", &body).status(), 403);
    let reply = http(&server, &session, &route, &body);
    assert_eq!(reply.status(), 200);
    assert_eq!(
        reply.json::<Value>().unwrap(),
        json!({"ok":true,"protocol":2,"completionProtocol":2,"purpose":"observation"})
    );
    for field in ["protocol", "completionProtocol"] {
        let mut old = body.clone();
        old[field] = json!(1);
        assert_eq!(post(&server, &session, &route, &old), 403);
    }
    let mut wrong = body.clone();
    wrong["purpose"] = json!("action");
    assert_eq!(post(&server, &session, &route, &wrong), 422);
    wrong = body.clone();
    wrong["connection"]["generation"] = json!(session.generation + 1);
    assert_eq!(post(&server, &session, &route, &wrong), 403);
    wrong = body;
    wrong["script"] = json!("untrusted");
    assert_eq!(post(&server, &session, &route, &wrong), 422);
}

#[test]
fn real_http_claimed_reader_survives_revoke_until_cleanup_without_pair_bearer() {
    for feature_off in [false, true] {
        let (broker, server, session) = fixture();
        assert_eq!(
            post(
                &server,
                &session,
                "/cu/tab-offer",
                &super::super::tests_sharing::offer(&session)
            ),
            200
        );
        broker
            .tabs()
            .grant_picker_tab("owner", "run", &broker.tabs().list_shared_candidates()[0].0)
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
        let request = &polled["request"];
        assert_eq!(request["protocol"], 2);
        let proof = &request["completion"];
        assert_eq!(
            post(&server, &session, &format!("{PREFIX}/claim"), proof),
            200
        );
        assert_eq!(
            post(&server, &session, &format!("{PREFIX}/claim"), proof),
            403
        );
        let before: Value = cleanup(&server, "status", proof).json().unwrap();
        assert_eq!(
            before["status"],
            json!({"phase":"claimed","cancelRequested":false})
        );
        let lifetime = cleanup(&server, "host-lifetime", proof);
        assert_eq!(lifetime.status(), 200);
        assert_eq!(
            lifetime.json::<Value>().unwrap()["lifetime"]["pid"],
            std::process::id()
        );
        if feature_off {
            broker.set_feature_enabled(false);
        } else {
            broker.tabs().revoke_pairing();
        }
        assert!(task.join().unwrap().is_err());
        assert!(!broker.tabs().existing_operations_idle("run"));
        let cancelled: Value = cleanup(&server, "status", proof).json().unwrap();
        assert_eq!(
            cancelled["status"],
            json!({"phase":"claimed","cancelRequested":true})
        );
        assert_eq!(
            post(
                &server,
                &session,
                "/cu/extension-completion/settle",
                &proof["proof"]
            ),
            403
        );
        assert!(!broker.tabs().existing_operations_idle("run"));
        let mut changed = proof.clone();
        changed["proof"]["binding"]["documentGeneration"] = json!(8);
        assert_eq!(cleanup(&server, "settle", &changed).status(), 403);
        let late = json!({"request":request,"outcome":{"kind":"rejected","reason":"cancelled"}});
        assert_eq!(post(&server, &session, "/cu/extension-result", &late), 403);
        // The original synthetic read has physically ended, after the held checks.
        assert_eq!(cleanup(&server, "settle", proof).status(), 200);
        assert!(broker.tabs().existing_operations_idle("run"));
        assert_eq!(
            cleanup(&server, "retirement", proof)
                .json::<Value>()
                .unwrap()["state"],
            "settled"
        );
        assert_eq!(cleanup(&server, "settle", proof).status(), 200);
        assert_eq!(
            post(&server, &session, &format!("{PREFIX}/claim"), proof),
            403
        );
    }
}

#[test]
fn malformed_cleanup_responses_never_echo_submitted_credentials() {
    let (_broker, server, _) = fixture();
    let secret = "credential-shaped-test-value";
    let reply = cleanup(
        &server,
        "settle",
        &json!({"purpose":"observation","proof":{"completionKey":secret},"extra":secret}),
    );
    assert_eq!(reply.status(), 422);
    assert!(!reply.text().unwrap().contains(secret));
    let reply = cleanup(&server, "settle", &json!({"padding":"x".repeat(4097)}));
    assert_eq!(reply.status(), 413);
    let forbidden = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}{PREFIX}/status", server.url))
        .header("origin", "https://fixture.invalid")
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(forbidden.status(), 403);
}
