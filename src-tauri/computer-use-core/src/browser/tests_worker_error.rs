use crate::error::BrokerError;

use super::*;

#[test]
fn typed_worker_error_preserves_status_completion_and_generation() {
    let value = serde_json::json!({
        "ok": false,
        "futureEnvelopeField": "ignored",
        "error": {
            "status": 422,
            "code": "invalid_action",
            "completion": "not_started",
            "message": "action schema rejected",
            "currentPageGeneration": 7,
            "futureField": true
        }
    });
    let error = parse_worker_error_response(422, &value);
    assert_eq!(error.status, 422);
    assert_eq!(error.code, "invalid_action");
    assert_eq!(error.completion, WorkerCompletion::NotStarted);
    assert_eq!(error.current_page_generation, Some(7));

    let broker: BrokerError = error.clone().into();
    assert!(matches!(
        broker,
        BrokerError::BrowserWorker(inner) if inner == error
    ));
}

#[test]
fn worker_error_requires_an_explicit_false_ok_envelope() {
    let valid_error = serde_json::json!({
        "status": 409,
        "code": "stale_page",
        "completion": "not_started",
        "message": "page generation changed"
    });
    for value in [
        serde_json::json!({"ok": true, "error": valid_error.clone()}),
        serde_json::json!({"error": valid_error.clone()}),
        serde_json::json!({"ok": "false", "error": valid_error.clone()}),
        serde_json::json!({"ok": null, "error": valid_error.clone()}),
        serde_json::json!({"ok": false}),
        serde_json::json!({
            "ok": false,
            "error": {
                "status": 400,
                "code": "stale_page",
                "completion": "not_started",
                "message": "status does not match HTTP"
            }
        }),
    ] {
        let error = parse_worker_error_response(409, &value);
        assert_eq!(error.status, 409);
        assert_eq!(error.code, "invalid_worker_response");
        assert_eq!(error.completion, WorkerCompletion::Unknown);
        assert_eq!(error.current_page_generation, None);
    }
}

#[test]
fn worker_error_requires_every_typed_error_field() {
    for value in [
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "stale_page",
                "completion": "not_started",
                "message": "missing status"
            }
        }),
        serde_json::json!({
            "ok": false,
            "error": {
                "status": 409,
                "completion": "not_started",
                "message": "missing code"
            }
        }),
        serde_json::json!({
            "ok": false,
            "error": {
                "status": 409,
                "code": "stale_page",
                "message": "missing completion"
            }
        }),
        serde_json::json!({
            "ok": false,
            "error": {
                "status": 409,
                "code": "stale_page",
                "completion": "not_started"
            }
        }),
    ] {
        let error = parse_worker_error_response(409, &value);
        assert_eq!(error.status, 409);
        assert_eq!(error.code, "invalid_worker_response");
        assert_eq!(error.completion, WorkerCompletion::Unknown);
        assert_eq!(error.current_page_generation, None);
    }
}

#[test]
fn malformed_or_mismatched_worker_error_fails_closed_as_unknown() {
    for value in [
        serde_json::json!({
            "error": {
                "status": 409,
                "code": "stale_page",
                "message": "missing completion"
            }
        }),
        serde_json::json!({
            "error": {
                "status": 400,
                "code": "stale_page",
                "completion": "not_started",
                "message": "status does not match HTTP"
            }
        }),
        serde_json::json!({
            "error": {
                "status": 409,
                "code": "INVALID-CODE",
                "completion": "not_started",
                "message": "invalid stable code"
            }
        }),
        serde_json::json!({"error": "legacy string error"}),
    ] {
        let error = parse_worker_error_response(409, &value);
        assert_eq!(error.status, 409);
        assert_eq!(error.code, "invalid_worker_response");
        assert_eq!(error.completion, WorkerCompletion::Unknown);
        assert_eq!(error.current_page_generation, None);
    }
}

#[test]
fn transport_and_timeout_never_claim_not_started() {
    let transport = WorkerError::transport("connection reset");
    let timeout = WorkerError::timeout("deadline elapsed");
    assert_eq!(transport.status, 502);
    assert_eq!(timeout.status, 504);
    assert_eq!(transport.completion, WorkerCompletion::Unknown);
    assert_eq!(timeout.completion, WorkerCompletion::Unknown);
}
