//! Observation-only lifecycle. Cleanup survives feature, Pair and grant revoke.
use super::*;
use crate::browser::extension_completion::COMPLETION_REQUEST_BYTES;
use crate::browser::extension_observation::{ObservationProof, ObservationPurpose};
use axum::extract::rejection::JsonRejection;

type Reply = (StatusCode, HeaderMap, Json<Value>);

pub(super) fn routes() -> Router<Service> {
    Router::new()
        .route("/negotiate", post(negotiate).options(preflight))
        .route("/claim", post(claim).options(preflight))
        .route("/status", post(status).options(preflight))
        .route("/settle", post(settle).options(preflight))
        .route("/retirement", post(retirement).options(preflight))
        .route("/host-lifetime", post(host_lifetime).options(preflight))
        .layer(DefaultBodyLimit::max(COMPLETION_REQUEST_BYTES))
}

fn gate(service: &Service, headers: &HeaderMap) -> Result<HeaderMap, Box<Reply>> {
    super::extension_transport::gate_for(service, headers, "extension-observation")
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
                Json(json!({"error":"invalid observation lifecycle request"})),
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
            Json(json!({"error":"observation lifecycle unavailable"})),
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Negotiation {
    protocol: u32,
    completion_protocol: u32,
    purpose: ObservationPurpose,
    connection: crate::pairing::PairingConnection,
}

async fn negotiate(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<Negotiation>, JsonRejection>,
) -> Reply {
    handle(
        service,
        headers,
        body,
        |broker, origin, token, negotiation| {
            if negotiation.protocol != crate::browser::extension_protocol::EXTENSION_PROTOCOL
                || negotiation.completion_protocol != 2
                || negotiation.purpose != ObservationPurpose::Observation
            {
                return Err("observation lifecycle protocol unavailable".into());
            }
            broker.negotiate_extension_observations(origin, token, &negotiation.connection)?;
            Ok(json!({"ok":true,"protocol":2,"completionProtocol":2,"purpose":"observation"}))
        },
    )
}

async fn claim(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ObservationProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, token, proof| {
        broker.claim_extension_observation(origin, token, &proof)?;
        Ok(json!({"ok":true}))
    })
}

async fn status(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ObservationProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, _, proof| {
        broker
            .tabs()
            .extension_observation_status(origin, &proof)
            .map(|status| json!({"ok":true,"status":status}))
    })
}

async fn settle(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ObservationProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, _, proof| {
        broker.tabs().settle_extension_observation(origin, &proof)?;
        Ok(json!({"ok":true}))
    })
}

async fn retirement(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ObservationProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, _, proof| {
        broker
            .tabs()
            .extension_observation_retirement(origin, &proof)
            .map(|state| json!({"ok":true,"state":state}))
    })
}

async fn host_lifetime(
    State(service): State<Service>,
    headers: HeaderMap,
    body: Result<Json<ObservationProof>, JsonRejection>,
) -> Reply {
    handle(service, headers, body, |broker, origin, _, proof| {
        broker.tabs().extension_observation_status(origin, &proof)?;
        let lifetime = crate::process_lifetime::HostLifetime::capture(
            &proof.proof.binding.connection.instance_id,
        )?;
        Ok(json!({"ok":true,"lifetime":lifetime}))
    })
}
