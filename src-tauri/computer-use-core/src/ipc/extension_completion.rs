//! Cleanup receipts remain usable after revoke; claiming always needs current authority.
use super::*;
use crate::browser::extension_completion::CompletionProof;
use axum::extract::rejection::JsonRejection;

type Reply = (StatusCode, HeaderMap, Json<Value>);

fn gate(service: &Service, headers: &HeaderMap) -> Result<HeaderMap, Box<Reply>> {
    // Observation polling cannot starve cleanup. The bucket is fixed and bounded.
    super::extension_transport::gate_for(service, headers, "extension-completion")
}

pub(super) async fn preflight(
    State(service): State<Service>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap) {
    match gate(&service, &headers) {
        Ok(cors) => (StatusCode::NO_CONTENT, cors),
        Err(reply) => {
            let (status, cors, _) = *reply;
            (status, cors)
        }
    }
}

enum Operation {
    Claim,
    Status,
    Settle,
    Retirement,
    HostLifetime,
}

fn handle(
    service: Service,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
    operation: Operation,
) -> Reply {
    let cors = match gate(&service, &headers) {
        Ok(cors) => cors,
        Err(reply) => return *reply,
    };
    let proof = match body {
        Ok(Json(proof)) => proof,
        Err(error) => {
            // Serde's diagnostic may quote submitted values, including credential-shaped data.
            return (
                error.status(),
                cors,
                Json(json!({"error": "invalid completion request"})),
            );
        }
    };
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let result = match operation {
        Operation::Claim => service
            .broker
            .claim_existing_completion(origin, bearer_token(&headers).unwrap_or_default(), &proof)
            .map(|()| json!({"ok": true})),
        Operation::Status => service
            .broker
            .tabs()
            .existing_completion_status(origin, &proof)
            .map(|status| json!({"ok": true, "status": status})),
        Operation::Settle => service
            .broker
            .tabs()
            .settle_existing_completion(origin, &proof)
            .map(|()| json!({"ok": true})),
        Operation::Retirement => service
            .broker
            .tabs()
            .existing_completion_retirement(origin, &proof)
            .map(|state| json!({"ok": true, "state": state})),
        Operation::HostLifetime => service
            .broker
            .tabs()
            .existing_completion_status(origin, &proof)
            .and_then(|_| {
                crate::process_lifetime::HostLifetime::capture(
                    &proof.binding.connection.instance_id,
                )
            })
            .map(|lifetime| json!({"ok":true,"lifetime":lifetime})),
    };
    match result {
        Ok(value) => (StatusCode::OK, cors, Json(value)),
        Err(_) => (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error": "completion unavailable"})),
        ),
    }
}

pub(super) async fn claim(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, Operation::Claim)
}

pub(super) async fn status(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, Operation::Status)
}

pub(super) async fn settle(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, Operation::Settle)
}

pub(super) async fn retirement(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, Operation::Retirement)
}

pub(super) async fn host_lifetime(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<CompletionProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, Operation::HostLifetime)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostRetirementRequest {
    connection: crate::pairing::PairingConnection,
    lifetime: crate::process_lifetime::HostLifetime,
}

/// Only kernel facts about the client's saved witness. Never authenticates or settles old proof.
pub(super) async fn host_retirement(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<HostRetirementRequest>, JsonRejection>,
) -> Reply {
    let cors = match gate(&service, &headers) {
        Ok(cors) => cors,
        Err(reply) => return *reply,
    };
    let Ok(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            cors,
            Json(json!({"error":"invalid process retirement request"})),
        );
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !service.broker.feature_enabled()
        || body.lifetime.validate().is_err()
        || service
            .broker
            .tabs()
            .check_pairing_connection(
                origin,
                bearer_token(&headers).unwrap_or_default(),
                &body.connection,
                false,
            )
            .is_err()
    {
        return (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error":"process retirement unavailable"})),
        );
    }
    let state = body.lifetime.retirement();
    (
        StatusCode::OK,
        cors,
        Json(json!({"ok":true,"state":state,"lifetime":body.lifetime})),
    )
}
