//! Formal HTTP + original picker/reservation. User and extension are synthetic.
use super::*;
use crate::adapter::SurfaceKind;
use crate::broker::{AccessApprovalReady, BrokerOptions, HostScopePickerCall};
use crate::fake::FakeAdapter;
use crate::pairing::{
    ExtensionPairing, PairingConnection, PairingProof, PairingSession, EXTENSION_ID,
};
use crate::task_consent::HostRequestKey;

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    desktop: Arc<FakeAdapter>,
    server: IpcServer,
    token: String,
    key: HostRequestKey,
}

impl Fixture {
    fn new() -> Self {
        let desktop = Arc::new(FakeAdapter::new());
        let broker = Arc::new(ComputerUseBroker::new(
            desktop.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-http-first-use-{}.lease", uuid::Uuid::new_v4())),
                ..Default::default()
            },
        ));
        broker.register_existing_browser_adapter().unwrap();
        let server = spawn(broker.clone()).unwrap();
        let token = server.issue_transport_credential("http-first-use").unwrap();
        broker
            .bind_transport_agent(&token, "synthetic-http-agent")
            .unwrap();
        let f = Self {
            key: request(&broker, &token),
            broker,
            desktop,
            server,
            token,
        };
        f.no_execution();
        f
    }

    fn picker(&self) -> HostScopePickerCall {
        self.broker
            .begin_task_scope_picker(&self.token, &self.key, SurfaceKind::ExistingTab)
            .unwrap()
    }

    fn http(&self, route: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        let mut call = client
            .post(format!("{}{route}", self.server.url))
            .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
            .json(&body);
        if let Some(token) = token {
            call = call.bearer_auth(token);
        }
        let response = call.send().unwrap();
        (response.status().as_u16(), response.json().unwrap())
    }

    fn pair(&self, picker: &HostScopePickerCall) -> PairingSession {
        let challenge = picker.pairing_challenge().unwrap().unwrap();
        let (status, public) = self.http("/cu/pairing-challenge", None, json!({}));
        assert_eq!(status, 200);
        assert_eq!(public["nonce"], challenge.nonce);
        assert!(!public.to_string().contains(&challenge.verification_code));
        assert!(public.get("verificationCode").is_none());
        let proof = ExtensionPairing::proof_for(&challenge, "00000000-0000-4000-8000-000000000001");
        let body = proof_body(&proof);
        assert_eq!(self.http("/cu/pairing-confirm", None, body.clone()).0, 403);
        assert!(picker.confirm_existing_pairing("foreign-nonce").is_err());
        assert!(self.broker.confirm_pairing(&challenge.nonce).is_err());
        picker.confirm_existing_pairing(&challenge.nonce).unwrap();
        let (status, response) = self.http("/cu/pairing-confirm", None, body.clone());
        assert_eq!(status, 200, "{response}");
        assert_eq!(self.http("/cu/pairing-confirm", None, body).0, 403);
        self.no_execution();
        PairingSession {
            session_key: response["sessionKey"].as_str().unwrap().into(),
            instance_id: response["instanceId"].as_str().unwrap().into(),
            connection_nonce: response["connectionNonce"].as_str().unwrap().into(),
            generation: response["generation"].as_u64().unwrap(),
        }
    }

    fn share(&self, session: &PairingSession) {
        let mut offer = super::tests_sharing::offer(session);
        offer["documentId"] = json!("0123456789abcdef0123456789abcdef");
        assert_eq!(
            self.http("/cu/tab-offer", Some(&session.session_key), offer)
                .0,
            200
        );
    }

    fn no_execution(&self) {
        assert!(!self.broker.feature_enabled());
        assert_eq!(self.broker.test_run_count(), 0);
        assert_eq!(self.broker.executable_grant_count("http-first-use"), 0);
        assert_eq!(self.broker.continuation_count("http-first-use"), 0);
        assert_eq!(self.desktop.list_calls(), 0);
        assert_eq!(self.desktop.observe_calls(), 0);
        assert!(self.desktop.executions().is_empty());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("http-first-use");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

fn request(broker: &ComputerUseBroker, token: &str) -> HostRequestKey {
    broker.submit_access(token, json!({"action":"request", "purpose":"synthetic HTTP first use",
        "surfaces":["existing-tabs"], "scopeIntent":"explicit user Share", "capabilities":["observe","input"]})).unwrap();
    broker.host_task_view(token).unwrap().request.unwrap().key
}

fn proof_body(proof: &PairingProof) -> Value {
    json!({"protocol":proof.protocol,"nonce":proof.nonce,"instance":proof.instance,"ext":proof.ext,
        "expiresAt":proof.expires_at,"connectionNonce":proof.connection_nonce,"response":proof.response})
}

fn connection(session: &PairingSession) -> PairingConnection {
    PairingConnection {
        instance_id: session.instance_id.clone(),
        connection_nonce: session.connection_nonce.clone(),
        generation: session.generation,
    }
}

#[test]
fn existing_first_use_http_pair_share_original_reservation_commit_and_stop() {
    let f = Fixture::new();
    assert_eq!(f.http("/cu/pairing-challenge", None, json!({})).0, 403);
    let call = f.picker();
    let session = f.pair(&call);
    let conn = serde_json::to_value(connection(&session)).unwrap();
    let (status, reply) = f.http(
        "/cu/extension-status",
        Some(&session.session_key),
        conn.clone(),
    );
    assert_eq!(status, 200);
    assert_eq!(reply["executionReady"], false);
    assert_eq!(
        f.http(
            "/cu/extension-heartbeat",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        200
    );
    assert_eq!(
        f.http(
            "/cu/browser-restart",
            Some(&session.session_key),
            json!({"connection":conn,
        "currentId":"00000000-0000-4000-8000-000000000009","previousId":null,"cleanups":[]})
        )
        .0,
        200
    );
    f.share(&session);
    f.no_execution();
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert_eq!(
        f.http(
            "/cu/extension-poll",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        403
    );
    assert_eq!(
        f.http(
            "/cu/extension-actions/poll",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        403
    );
    assert_eq!(
        f.http(
            "/cu/extension-actions/negotiate",
            Some(&session.session_key),
            json!({"protocol":2,
        "completionProtocol":2,"connection":conn,"actions":["click"]})
        )
        .0,
        403
    );
    assert!(f
        .broker
        .execution_run(&f.token, "computer_observe")
        .is_err());
    let picker = call.discover().unwrap();
    let views = picker.views().unwrap();
    assert_eq!(views.len(), 1);
    let selected = picker.select(&[views[0].candidate_id.clone()]).unwrap();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, selected)
        .unwrap()
    else {
        panic!("original reservation");
    };
    assert_eq!(
        f.http(
            "/cu/extension-status",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        200,
        "connection metadata must bridge the original reservation-to-enable gap"
    );
    let mut late_share = super::tests_sharing::offer(&session);
    late_share["sequence"] = json!(2);
    late_share["documentId"] = json!("0123456789abcdef0123456789abcdef");
    assert_eq!(
        f.http("/cu/tab-offer", Some(&session.session_key), late_share)
            .0,
        403
    );
    assert_eq!(f.broker.executable_grant_count("http-first-use"), 0);
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    assert_eq!(outcome.binding.target_id, "tab:101");
    assert_eq!(f.broker.executable_grant_count("http-first-use"), 1);
    assert_eq!(
        f.http("/cu/extension-status", Some(&session.session_key), conn)
            .1["executionReady"],
        true
    );
    drop(picker);
    assert_eq!(f.broker.executable_grant_count("http-first-use"), 1);
    f.broker.fence_task_consent("http-first-use");
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
    assert!(!f.broker.tabs().is_closed("101"));
    assert_eq!(f.desktop.observe_calls(), 0);
}

#[test]
fn existing_first_use_http_cancel_retains_passive_pair_but_closes_metadata_and_execution() {
    let f = Fixture::new();
    let call = f.picker();
    let session = f.pair(&call);
    f.share(&session);
    call.cancel_handle().fence();
    assert!(call.pairing_challenge().is_err());
    let conn = serde_json::to_value(connection(&session)).unwrap();
    let before = f.broker.tabs().stored_pairing_lease_for_test();
    let (status, reply) = f.http(
        "/cu/extension-status",
        Some(&session.session_key),
        conn.clone(),
    );
    assert_eq!(status, 200);
    assert_eq!(reply, json!({"ok":true,"executionReady":false}));
    let after = f.broker.tabs().stored_pairing_lease_for_test();
    assert!(after.0 && after.1 <= before.1 && !after.1.is_zero());
    assert_eq!(after.2, 1);
    for route in [
        "/cu/extension-heartbeat",
        "/cu/extension-poll",
        "/cu/extension-actions/poll",
    ] {
        assert_eq!(
            f.http(route, Some(&session.session_key), conn.clone()).0,
            403,
            "{route}"
        );
    }
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    let replacement = f.picker();
    assert!(replacement.pairing_challenge().unwrap().is_none());
    drop(call);
    assert_eq!(
        f.http("/cu/extension-status", Some(&session.session_key), conn)
            .0,
        200
    );
    f.no_execution();
}

#[test]
fn existing_first_use_http_passive_status_survives_stop_drop_and_preparation_expiry() {
    for terminal in ["stop", "drop", "preparation-expiry"] {
        let f = Fixture::new();
        let call = f.picker();
        let session = f.pair(&call);
        f.share(&session);
        match terminal {
            "stop" => {
                f.broker
                    .decide_host_task(&f.token, &f.key, crate::broker::HostTaskDecision::Stop)
                    .unwrap();
            }
            "preparation-expiry" => f.broker.expire_existing_share_preparation_for_test(),
            _ => {}
        }
        drop(call);
        let conn = serde_json::to_value(connection(&session)).unwrap();
        let before = f.broker.tabs().stored_pairing_lease_for_test();
        for _ in 0..2 {
            let (status, reply) = f.http(
                "/cu/extension-status",
                Some(&session.session_key),
                conn.clone(),
            );
            assert_eq!(status, 200, "{terminal}");
            assert_eq!(reply, json!({"ok":true,"executionReady":false}));
        }
        let after = f.broker.tabs().stored_pairing_lease_for_test();
        assert!(after.0 && after.1 <= before.1 && !after.1.is_zero());
        assert_eq!(after.2, 1);
        for route in [
            "/cu/extension-heartbeat",
            "/cu/extension-poll",
            "/cu/extension-actions/poll",
        ] {
            assert_eq!(
                f.http(route, Some(&session.session_key), conn.clone()).0,
                403
            );
        }
        let offer = super::tests_sharing::offer(&session);
        assert_eq!(
            f.http("/cu/tab-offer", Some(&session.session_key), offer).0,
            403
        );
        f.no_execution();
    }
}

#[test]
fn existing_first_use_http_passive_status_requires_exact_credentials_and_origin() {
    let f = Fixture::new();
    let call = f.picker();
    let session = f.pair(&call);
    f.share(&session);
    drop(call);
    let conn = serde_json::to_value(connection(&session)).unwrap();
    for token in [None, Some("wrong")] {
        assert_eq!(f.http("/cu/extension-status", token, conn.clone()).0, 403);
    }
    for field in ["instanceId", "connectionNonce", "generation"] {
        let mut wrong = conn.clone();
        wrong[field] = if field == "generation" {
            json!(session.generation + 1)
        } else {
            json!("foreign")
        };
        assert_eq!(
            f.http("/cu/extension-status", Some(&session.session_key), wrong)
                .0,
            403
        );
    }
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}/cu/extension-status", f.server.url))
        .header("origin", "chrome-extension://foreign")
        .bearer_auth(&session.session_key)
        .json(&conn)
        .send()
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        f.http(
            "/cu/extension-status",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        200
    );
    f.no_execution();
}

#[test]
fn existing_first_use_http_passive_status_rejects_expired_replaced_and_revoked_pair() {
    let f = Fixture::new();
    let call = f.picker();
    let session = f.pair(&call);
    f.share(&session);
    drop(call);
    let conn = serde_json::to_value(connection(&session)).unwrap();
    f.broker.tabs().expire_pairing_connection_for_test();
    assert_eq!(
        f.http(
            "/cu/extension-status",
            Some(&session.session_key),
            conn.clone()
        )
        .0,
        403
    );
    let replacement = f.picker();
    // The first Pair already tested pre-confirmation and replay rejection.
    // Complete this new challenge once, within the production rate limit.
    let challenge = replacement.pairing_challenge().unwrap().unwrap();
    replacement
        .confirm_existing_pairing(&challenge.nonce)
        .unwrap();
    let proof = ExtensionPairing::proof_for(&challenge, "00000000-0000-4000-8000-000000000002");
    let (status, fresh) = f.http("/cu/pairing-confirm", None, proof_body(&proof));
    assert_eq!(status, 200);
    drop(replacement);
    assert_eq!(
        f.http("/cu/extension-status", Some(&session.session_key), conn)
            .0,
        403
    );
    let fresh_key = fresh["sessionKey"].as_str().unwrap();
    let fresh_conn = json!({"instanceId":fresh["instanceId"],"connectionNonce":fresh["connectionNonce"],"generation":fresh["generation"]});
    assert_eq!(
        f.http("/cu/extension-status", Some(fresh_key), fresh_conn.clone())
            .0,
        200
    );
    f.broker.set_feature_enabled(false);
    assert_eq!(
        f.http("/cu/extension-status", Some(fresh_key), fresh_conn)
            .0,
        403
    );
    f.no_execution();
}

#[test]
fn existing_first_use_http_stale_credentials_documents_and_cleanup_do_not_widen_preparation() {
    let f = Fixture::new();
    let call = f.picker();
    let session = f.pair(&call);
    let conn = serde_json::to_value(connection(&session)).unwrap();
    assert_eq!(
        f.http("/cu/extension-status", Some(&"f".repeat(64)), conn.clone())
            .0,
        403
    );
    let mut stale = conn.clone();
    stale["generation"] = json!(session.generation + 1);
    assert_eq!(
        f.http("/cu/extension-status", Some(&session.session_key), stale)
            .0,
        403
    );
    let mut offer = super::tests_sharing::offer(&session);
    offer["documentId"] = json!("not-a-document");
    assert_eq!(
        f.http("/cu/tab-offer", Some(&session.session_key), offer).0,
        403
    );
    assert!(f.broker.tabs().list_shared_candidates().is_empty());
    f.share(&session);
    let cleanups = json!([{"version":1,"browserId":"00000000-0000-4000-8000-000000000008",
        "requestId":"00000000-0000-4000-8000-000000000007","documentId":"0123456789abcdef0123456789abcdef","phase":"cleanup"}]);
    assert_eq!(f.http("/cu/browser-restart", Some(&session.session_key), json!({"connection":conn,
        "currentId":"00000000-0000-4000-8000-000000000009","previousId":"00000000-0000-4000-8000-000000000008",
        "cleanups":cleanups})).0, 403);
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
    f.no_execution();
}

#[test]
fn existing_first_use_http_original_approval_fence_closes_the_metadata_bridge() {
    for stop in [false, true] {
        let f = Fixture::new();
        let call = f.picker();
        let session = f.pair(&call);
        f.share(&session);
        let picker = call.discover().unwrap();
        let chosen = vec![picker.views().unwrap()[0].candidate_id.clone()];
        let AccessApprovalReady::Prepare(mut approval) = f
            .broker
            .reserve_selected_access_approval(&f.token, picker.select(&chosen).unwrap())
            .unwrap()
        else {
            panic!("original approval");
        };
        let conn = serde_json::to_value(connection(&session)).unwrap();
        let (status, reply) = f.http(
            "/cu/extension-status",
            Some(&session.session_key),
            conn.clone(),
        );
        assert_eq!(status, 200);
        assert_eq!(reply["executionReady"], false);
        if stop {
            f.broker
                .decide_host_task(&f.token, &f.key, crate::broker::HostTaskDecision::Stop)
                .unwrap();
        } else {
            approval.cancel_handle().fence();
        }
        assert_eq!(
            f.http("/cu/extension-heartbeat", Some(&session.session_key), conn)
                .0,
            403
        );
        assert!(approval.publish_feature().is_err());
        approval.abort();
        assert!(f.broker.tabs().existing_target_info("101").is_none());
        assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
        f.no_execution();
    }
}
