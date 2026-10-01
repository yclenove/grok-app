//! Real HTTP framing and control isolation, without desktop/browser side effects.
use std::io::{Cursor, Read, Write};
use std::net::TcpStream;

use super::tests_sharing::{fixture, post};
use super::*;
use crate::browser::extension_protocol::EXTENSION_RESULT_BYTES;
use crate::pairing::{PairingSession, EXTENSION_ID};

fn request(
    client: &reqwest::blocking::Client,
    server: &IpcServer,
    session: &PairingSession,
) -> reqwest::blocking::RequestBuilder {
    client
        .post(format!("{}/cu/extension-result", server.url))
        .header("origin", format!("chrome-extension://{EXTENSION_ID}"))
        .bearer_auth(&session.session_key)
}

#[test]
fn chunked_oversize_keeps_413_cors_and_json_checks_on_the_next_request() {
    let (_broker, server, session) = fixture();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    // Body::new(reader) has no known Content-Length: Hyper must send chunks.
    let response = request(&client, &server, &session)
        .header("content-type", "application/json")
        .body(reqwest::blocking::Body::new(Cursor::new(vec![
            b'x';
            EXTENSION_RESULT_BYTES + 100 * 1024
        ])))
        .send()
        .unwrap();
    assert_eq!(response.status().as_u16(), 413);
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        format!("chrome-extension://{EXTENSION_ID}")
    );
    assert_eq!(
        response.json::<Value>().unwrap()["error"],
        "extension result body rejected"
    );
    for (body, media, expected) in [
        ("{}", "application/json", 422),
        ("{", "application/json", 400),
        ("{}", "text/plain", 415),
    ] {
        let response = request(&client, &server, &session)
            .header("content-type", media)
            .body(body)
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
        assert_eq!(
            response.json::<Value>().unwrap()["error"],
            "extension result JSON rejected"
        );
    }
}

#[test]
fn stalled_upload_cannot_block_stop_or_heartbeat_and_releases_admission() {
    let (broker, server, session) = fixture();
    broker.open_run("upload-owner", "upload-run").unwrap();
    broker
        .authorize_target("upload-run", "fake:window:1")
        .unwrap();
    let tool_token = server
        .credential_for_session("upload-owner", "upload-run")
        .unwrap();
    let authority = server.url.strip_prefix("http://").unwrap();
    let mut stalled = TcpStream::connect(authority).unwrap();
    stalled
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stalled
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    write!(stalled,
        "POST /cu/extension-result HTTP/1.1\r\nHost: {authority}\r\nOrigin: chrome-extension://{EXTENSION_ID}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 1024\r\n\r\n{{",
        session.session_key
    ).unwrap();
    stalled.flush().unwrap();
    let admitted = Instant::now() + Duration::from_secs(1);
    while server.service.extension_result_slot.available_permits() != 0 {
        assert!(
            Instant::now() < admitted,
            "upload did not enter bounded reader"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        post(&server, &session, "/cu/extension-result", &json!({})),
        409
    );
    let start = Instant::now();
    assert_eq!(
        post(
            &server,
            &session,
            "/cu/extension-heartbeat",
            &json!({
                "instanceId": session.instance_id,
                "connectionNonce": session.connection_nonce,
                "generation": session.generation
            })
        ),
        200
    );
    let stopped = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(750))
        .build()
        .unwrap()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(tool_token)
        .json(&json!({"name": "computer_stop"}))
        .send()
        .unwrap();
    assert_eq!(stopped.status().as_u16(), 200);
    assert_eq!(stopped.json::<Value>().unwrap()["isError"], false);
    assert!(start.elapsed() < Duration::from_millis(750));
    assert_eq!(server.service.extension_result_slot.available_permits(), 0);
    let mut response = String::new();
    stalled.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 408 "),
        "expected bounded timeout, not reset"
    );
    assert!(response.to_ascii_lowercase().contains("connection: close"));
    assert_eq!(server.service.extension_result_slot.available_permits(), 1);
    assert_eq!(
        post(&server, &session, "/cu/extension-result", &json!({})),
        422
    );
}
