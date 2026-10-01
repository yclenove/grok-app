use super::*;
use crate::browser::extension_protocol::ExtensionResult;
use crate::pairing::PairingConnection;
use axum::body::Body;
use axum::extract::{FromRequest, Request};

type Reply = (StatusCode, HeaderMap, Json<Value>);

fn gate(service: &Service, headers: &HeaderMap) -> Result<HeaderMap, Box<Reply>> {
    gate_for(service, headers, "extension-transport")
}

pub(super) fn gate_for(
    service: &Service,
    headers: &HeaderMap,
    bucket: &'static str,
) -> Result<HeaderMap, Box<Reply>> {
    let installed = service.broker.tabs().installed_extension_id();
    let cors = pairing_cors_for(headers, Some(&installed));
    if !pairing_headers_allow(headers, Some(&installed)) {
        return Err(Box::new((
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error": "extension origin unavailable"})),
        )));
    }
    let mut rate = service.rate.lock();
    // Poll/result traffic never consumes the stricter user pairing bucket.
    if !admit_rate(
        rate.entry(bucket.into()).or_default(),
        Instant::now(),
        40,
        RATE_WINDOW,
    ) {
        return Err(Box::new((
            StatusCode::TOO_MANY_REQUESTS,
            cors,
            Json(json!({"error": "rate limited"})),
        )));
    }
    Ok(cors)
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

pub(super) async fn poll(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(connection): Json<PairingConnection>,
) -> Reply {
    let cors = match gate(&service, &headers) {
        Ok(cors) => cors,
        Err(reply) => return *reply,
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let token = bearer_token(&headers).unwrap_or_default();
    let Ok(_slot) = service.extension_poll_slot.try_acquire() else {
        return (
            StatusCode::CONFLICT,
            cors,
            Json(json!({"error": "extension poll busy"})),
        );
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match service
            .broker
            .poll_extension_request(origin, token, &connection)
        {
            Ok(Some(request)) => {
                return (
                    StatusCode::OK,
                    cors,
                    Json(json!({"ok": true, "request": request})),
                )
            }
            Ok(None) if Instant::now() >= deadline => {
                return (
                    StatusCode::OK,
                    cors,
                    Json(json!({"ok": true, "request": null})),
                )
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(_) => {
                return (
                    StatusCode::FORBIDDEN,
                    cors,
                    Json(json!({"error": "extension connection unavailable"})),
                )
            }
        }
    }
}

pub(super) async fn result(
    State(service): State<Service>,
    headers: HeaderMap,
    request: Request,
) -> Reply {
    let mut cors = match gate(&service, &headers) {
        Ok(cors) => cors,
        Err(reply) => return *reply,
    };
    let Ok(slot) = service.extension_result_slot.try_acquire_owned() else {
        return (
            StatusCode::CONFLICT,
            cors,
            Json(json!({"error": "extension result busy"})),
        );
    };
    let (parts, body) = request.into_parts();
    let bytes = match super::result_body::read(body).await {
        Ok(bytes) => bytes,
        Err(rejection) => {
            if rejection.close {
                cors.insert("connection", "close".parse().unwrap());
            }
            return (
                rejection.status,
                cors,
                Json(json!({"error": "extension result body rejected"})),
            );
        }
    };
    // Retain Axum's exact content-type/schema checks and the result-only JSON
    // limit. Only bounded bytes reach this extractor; no oversize JSON is parsed.
    let result = match Json::<ExtensionResult>::from_request(
        Request::from_parts(parts, Body::from(bytes)),
        &(),
    )
    .await
    {
        Ok(Json(result)) => result,
        Err(rejection) => {
            return (
                rejection.status(),
                cors,
                Json(json!({"error": "extension result JSON rejected"})),
            );
        }
    };
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let token = bearer_token(&headers).unwrap_or_default().to_owned();
    // PNG decoding must not block the listener's two async threads. The permit
    // belongs to the blocking job even if the HTTP client disconnects mid-decode.
    match tokio::task::spawn_blocking(move || {
        let _slot = slot;
        service
            .broker
            .complete_extension_request(&origin, &token, result)
    })
    .await
    {
        Ok(Ok(())) => (StatusCode::OK, cors, Json(json!({"ok": true}))),
        _ => (
            StatusCode::FORBIDDEN,
            cors,
            Json(json!({"error": "extension result rejected"})),
        ),
    }
}
