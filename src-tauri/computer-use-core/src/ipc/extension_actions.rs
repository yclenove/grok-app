//! Bounded v2 action-only routes. No grant or enqueue route is exposed.
use super::*;
use crate::browser::extension_action::{
    ActionDispatch, ActionNegotiation, ActionResult, ACTION_PACKET_BYTES,
};
use crate::pairing::PairingConnection;
use axum::extract::rejection::JsonRejection;

type Reply = (StatusCode, HeaderMap, Json<Value>);

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/negotiate", post(negotiate).options(preflight))
        .route("/poll", post(poll).options(preflight))
        .route("/claim", post(claim).options(preflight))
        .route("/result", post(result).options(preflight))
        .layer(DefaultBodyLimit::max(ACTION_PACKET_BYTES))
}

fn gate(service: &Service, headers: &HeaderMap) -> Result<HeaderMap, Box<Reply>> {
    super::extension_transport::gate_for(service, headers, "extension-actions")
}

async fn preflight(State(service): State<Service>, headers: HeaderMap) -> (StatusCode, HeaderMap) {
    match gate(&service, &headers) {
        Ok(cors) => (StatusCode::NO_CONTENT, cors),
        Err(reply) => {
            let (status, cors, _) = *reply;
            (status, cors)
        }
    }
}

fn handle<T>(
    service: Service,
    headers: HeaderMap,
    body: Result<Json<T>, JsonRejection>,
    operation: impl FnOnce(&ComputerUseBroker, &str, &str, T) -> Result<Value, String>,
) -> Reply {
    let cors = match gate(&service, &headers) {
        Ok(cors) => cors,
        Err(reply) => return *reply,
    };
    let value = match body {
        Ok(Json(value)) => value,
        Err(error) => {
            return (
                error.status(),
                cors,
                Json(json!({"error":"invalid action request"})),
            )
        }
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    match operation(
        &service.broker,
        origin,
        bearer_token(&headers).unwrap_or_default(),
        value,
    ) {
        Ok(value) => (StatusCode::OK, cors, Json(value)),
        Err(_) => (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error":"action unavailable"})),
        ),
    }
}

async fn negotiate(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ActionNegotiation>, JsonRejection>,
) -> Reply {
    handle(
        service,
        headers,
        body,
        |broker, origin, token, negotiation| {
            broker.negotiate_existing_actions(origin, token, negotiation)?;
            Ok(json!({"ok":true, "protocol":2, "completionProtocol":2}))
        },
    )
}

async fn poll(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<PairingConnection>, JsonRejection>,
) -> Reply {
    // Immediate bounded poll; no extra held long-poll slot or detached task.
    handle(
        service,
        headers,
        body,
        |broker, origin, token, connection| {
            broker
                .poll_existing_action(origin, token, &connection)
                .map(|dispatch| json!({"ok":true, "dispatch":dispatch}))
        },
    )
}

async fn claim(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ActionDispatch>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, token, dispatch| {
        broker.claim_existing_action(origin, token, &dispatch)?;
        Ok(json!({"ok":true}))
    })
}

async fn result(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ActionResult>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, token, result| {
        broker.complete_existing_action(origin, token, result)?;
        Ok(json!({"ok":true}))
    })
}
