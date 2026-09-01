//! Read-only wallpaper search through the Grok Build Responses compatibility
//! endpoint. This module is Host-internal until the wallpaper search router
//! explicitly opts into it.

use std::future::Future;
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::account::{
    self, BuildOauthAccessToken, BuildOauthCredentialRevision, BuildOauthTokenError,
};
use crate::proxy;
use crate::wallpaper_source::{
    filter_reachable_gallery_items_cancellable, merge_rank_x_gallery_items_with_limit,
    parse_gallery_items, x_gallery_new_items, WallpaperGalleryItem, WallpaperSearchCancellation,
    WallpaperXSearchBatch, WallpaperXSearchRuntime, WallpaperXSearchStage,
};

mod more;
pub(crate) use more::search_more;

pub(crate) const RESPONSES_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/responses";
pub(crate) const RESPONSES_MODEL: &str = "grok-4.6";
pub(crate) const RESPONSES_EFFORT: &str = "low";
pub(crate) const RESPONSES_LANE_COUNT: usize = 3;
pub(crate) const RESPONSES_LANE_TARGET_COUNT: usize = 8;
pub(crate) const RESPONSES_LANE_MAX_X_SEARCH_CALLS: u32 = 3;

const RESPONSES_TIMEOUT: Duration = Duration::from_secs(90);
const RESPONSES_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const RESPONSES_MAX_RESULTS: usize = RESPONSES_LANE_COUNT * RESPONSES_LANE_TARGET_COUNT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponsesSearchErrorKind {
    OauthUnavailable,
    OauthExpired,
    BadRequest,
    Unauthorized,
    RateLimited,
    ServerError,
    Timeout,
    Tls,
    Network,
    Empty,
    InvalidJson,
    Protocol,
    SearchBudgetExceeded,
    Cancelled,
}

impl ResponsesSearchErrorKind {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::OauthUnavailable => "oauth_unavailable",
            Self::OauthExpired => "oauth_expired",
            Self::BadRequest => "responses_bad_request",
            Self::Unauthorized => "responses_unauthorized",
            Self::RateLimited => "responses_rate_limited",
            Self::ServerError => "responses_server_error",
            Self::Timeout => "responses_timeout",
            Self::Tls => "responses_tls",
            Self::Network => "responses_network",
            Self::Empty => "responses_empty",
            Self::InvalidJson => "responses_invalid_json",
            Self::Protocol => "responses_protocol",
            Self::SearchBudgetExceeded => "responses_search_budget_exceeded",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResponsesSearchError {
    pub(crate) kind: ResponsesSearchErrorKind,
    pub(crate) credential_revision: Option<BuildOauthCredentialRevision>,
    pub(crate) observed_search_calls: Option<u32>,
}

impl ResponsesSearchError {
    fn new(
        kind: ResponsesSearchErrorKind,
        credential_revision: Option<BuildOauthCredentialRevision>,
    ) -> Self {
        Self {
            kind,
            credential_revision,
            observed_search_calls: None,
        }
    }

    fn with_observed_search_calls(mut self, search_calls: u32) -> Self {
        self.observed_search_calls = Some(search_calls);
        self
    }

    pub(crate) fn code(&self) -> &'static str {
        self.kind.code()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResponsesSearchSuccess {
    pub(crate) items: Vec<WallpaperGalleryItem>,
    pub(crate) candidate_count: usize,
    pub(crate) valid_count: usize,
    pub(crate) search_calls: u32,
    pub(crate) model: &'static str,
    pub(crate) effort: &'static str,
    pub(crate) credential_revision: BuildOauthCredentialRevision,
}

struct ResponsesSearchRequest<'a> {
    query: &'a str,
    sort: Option<&'a str>,
    endpoint: &'a str,
    max_search_calls: u32,
    lane_index: usize,
    target_count: usize,
    excluded_ids: &'a [String],
    cancellation: &'a WallpaperSearchCancellation,
}

/// Run the fixed, read-only Responses preview request.
///
/// No endpoint, model, tool, or credential is accepted from the frontend.
pub(crate) async fn search(
    query: &str,
    sort: Option<&str>,
    runtime: &WallpaperXSearchRuntime,
) -> Result<ResponsesSearchSuccess, ResponsesSearchError> {
    if runtime.is_cancelled() {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Cancelled,
            None,
        ));
    }
    let auth = account::read_build_oauth_access_token().map_err(auth_error)?;
    let client = responses_client(RESPONSES_TIMEOUT, Some(auth.revision.clone()))?;
    run_parallel_lanes(runtime, auth.revision.clone(), |lane_index| {
        search_lane(query, sort, lane_index, &client, &auth, runtime)
    })
    .await
}

async fn search_lane(
    query: &str,
    sort: Option<&str>,
    lane_index: usize,
    client: &reqwest::Client,
    auth: &BuildOauthAccessToken,
    runtime: &WallpaperXSearchRuntime,
) -> Result<ResponsesSearchSuccess, ResponsesSearchError> {
    let mut result = search_with_client(
        ResponsesSearchRequest {
            query,
            sort,
            endpoint: RESPONSES_ENDPOINT,
            max_search_calls: RESPONSES_LANE_MAX_X_SEARCH_CALLS,
            lane_index,
            target_count: RESPONSES_LANE_TARGET_COUNT,
            excluded_ids: &[],
            cancellation: runtime.cancellation(),
        },
        client,
        auth.expose_to_build_proxy(),
        auth.revision.clone(),
    )
    .await?;
    runtime.report(WallpaperXSearchStage::Validating);
    result.items =
        filter_reachable_gallery_items_cancellable(result.items, Some(runtime.cancellation()))
            .await;
    if runtime.is_cancelled() {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Cancelled,
            Some(auth.revision.clone()),
        ));
    }
    if result.items.is_empty() {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Empty,
            Some(auth.revision.clone()),
        )
        .with_observed_search_calls(result.search_calls));
    }
    result.valid_count = result.items.len();
    Ok(result)
}

async fn run_parallel_lanes<F, Fut>(
    runtime: &WallpaperXSearchRuntime,
    credential_revision: BuildOauthCredentialRevision,
    run_lane: F,
) -> Result<ResponsesSearchSuccess, ResponsesSearchError>
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Result<ResponsesSearchSuccess, ResponsesSearchError>>,
{
    let mut lanes = FuturesUnordered::new();
    for lane_index in 1..=RESPONSES_LANE_COUNT {
        let lane = run_lane(lane_index);
        lanes.push(async move { (lane_index, lane.await) });
    }

    let mut items = Vec::new();
    let mut candidate_count = 0usize;
    let mut search_calls = 0u32;
    let mut errors = Vec::new();
    loop {
        let next = tokio::select! {
            biased;
            _ = runtime.cancellation().cancelled() => {
                return Err(ResponsesSearchError::new(
                    ResponsesSearchErrorKind::Cancelled,
                    Some(credential_revision.clone()),
                ));
            }
            next = lanes.next() => next,
        };
        let Some((lane_index, outcome)) = next else {
            break;
        };
        let is_last = lanes.is_empty();
        match outcome {
            Ok(lane) => {
                candidate_count = candidate_count.saturating_add(lane.candidate_count);
                search_calls = search_calls.saturating_add(lane.search_calls);
                let previous = items.clone();
                let mut combined = previous.clone();
                combined.extend(lane.items);
                items = merge_rank_x_gallery_items_with_limit(combined, RESPONSES_MAX_RESULTS);
                let batch_items = x_gallery_new_items(&previous, &items);
                runtime.report_batch(WallpaperXSearchBatch {
                    batch_index: lane_index,
                    items: batch_items,
                    accumulated_count: items.len(),
                    done: is_last,
                });
            }
            Err(error) if error.kind == ResponsesSearchErrorKind::Cancelled => {
                return Err(error);
            }
            Err(error) => {
                search_calls =
                    search_calls.saturating_add(error.observed_search_calls.unwrap_or_default());
                errors.push(error);
                if is_last {
                    runtime.report_batch(WallpaperXSearchBatch {
                        batch_index: lane_index,
                        items: Vec::new(),
                        accumulated_count: items.len(),
                        done: true,
                    });
                }
            }
        }
    }

    if runtime.is_cancelled() {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Cancelled,
            Some(credential_revision),
        ));
    }
    if items.is_empty() {
        return Err(select_parallel_error(errors, credential_revision));
    }

    Ok(ResponsesSearchSuccess {
        valid_count: items.len(),
        items,
        candidate_count,
        search_calls,
        model: RESPONSES_MODEL,
        effort: RESPONSES_EFFORT,
        credential_revision,
    })
}

fn select_parallel_error(
    errors: Vec<ResponsesSearchError>,
    credential_revision: BuildOauthCredentialRevision,
) -> ResponsesSearchError {
    errors
        .into_iter()
        .min_by_key(|error| parallel_error_priority(error.kind))
        .unwrap_or_else(|| {
            ResponsesSearchError::new(ResponsesSearchErrorKind::Empty, Some(credential_revision))
        })
}

fn parallel_error_priority(kind: ResponsesSearchErrorKind) -> u8 {
    match kind {
        ResponsesSearchErrorKind::Cancelled => 0,
        ResponsesSearchErrorKind::RateLimited => 1,
        ResponsesSearchErrorKind::SearchBudgetExceeded => 2,
        ResponsesSearchErrorKind::Unauthorized
        | ResponsesSearchErrorKind::OauthUnavailable
        | ResponsesSearchErrorKind::OauthExpired => 3,
        ResponsesSearchErrorKind::BadRequest
        | ResponsesSearchErrorKind::InvalidJson
        | ResponsesSearchErrorKind::Protocol => 4,
        ResponsesSearchErrorKind::ServerError
        | ResponsesSearchErrorKind::Timeout
        | ResponsesSearchErrorKind::Tls
        | ResponsesSearchErrorKind::Network => 5,
        ResponsesSearchErrorKind::Empty => 6,
    }
}

fn auth_error(error: BuildOauthTokenError) -> ResponsesSearchError {
    let kind = match error {
        BuildOauthTokenError::Unavailable => ResponsesSearchErrorKind::OauthUnavailable,
        BuildOauthTokenError::Expired => ResponsesSearchErrorKind::OauthExpired,
    };
    ResponsesSearchError::new(kind, None)
}

#[cfg(test)]
async fn search_with_credentials(
    request: ResponsesSearchRequest<'_>,
    timeout: Duration,
    credentials: Result<(&str, BuildOauthCredentialRevision), BuildOauthTokenError>,
) -> Result<ResponsesSearchSuccess, ResponsesSearchError> {
    let (token, credential_revision) = credentials.map_err(auth_error)?;
    let client = responses_client(timeout, Some(credential_revision.clone()))?;
    search_with_client(request, &client, token, credential_revision).await
}

fn responses_client(
    timeout: Duration,
    credential_revision: Option<BuildOauthCredentialRevision>,
) -> Result<reqwest::Client, ResponsesSearchError> {
    proxy::apply_to_reqwest(
        reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(20)))
            // A Build bearer token is valid only for the fixed proxy endpoint.
            // Returning a 3xx is a protocol error; the credential is never
            // replayed to a redirect target.
            .redirect(reqwest::redirect::Policy::none()),
    )
    .build()
    .map_err(|_| ResponsesSearchError::new(ResponsesSearchErrorKind::Network, credential_revision))
}

async fn search_with_client(
    request: ResponsesSearchRequest<'_>,
    client: &reqwest::Client,
    token: &str,
    credential_revision: BuildOauthCredentialRevision,
) -> Result<ResponsesSearchSuccess, ResponsesSearchError> {
    let ResponsesSearchRequest {
        query,
        sort,
        endpoint,
        max_search_calls,
        lane_index,
        target_count,
        excluded_ids,
        cancellation,
    } = request;
    let error_revision = || Some(credential_revision.clone());

    let request = client.post(endpoint).bearer_auth(token);
    let request = crate::wallpaper_responses_client::apply_build_proxy_headers(request)
        .json(&responses_request(
            query,
            sort,
            max_search_calls,
            lane_index,
            target_count,
            excluded_ids,
        ))
        .send();
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            return Err(ResponsesSearchError::new(
                ResponsesSearchErrorKind::Cancelled,
                error_revision(),
            ));
        }
        response = request => response,
    }
    .map_err(|error| ResponsesSearchError::new(transport_error_kind(&error), error_revision()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(ResponsesSearchError::new(
            status_error_kind(status),
            error_revision(),
        ));
    }

    if response
        .content_length()
        .is_some_and(|length| length > RESPONSES_MAX_BODY_BYTES.try_into().unwrap_or(u64::MAX))
    {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Protocol,
            error_revision(),
        ));
    }

    let body = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            return Err(ResponsesSearchError::new(
                ResponsesSearchErrorKind::Cancelled,
                error_revision(),
            ));
        }
        body = read_bounded_body(response, error_revision()) => body?,
    };
    if body.is_empty() {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::Empty,
            error_revision(),
        ));
    }
    let payload: Value = serde_json::from_slice(&body).map_err(|_| {
        ResponsesSearchError::new(ResponsesSearchErrorKind::InvalidJson, error_revision())
    })?;
    let output = payload
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ResponsesSearchError::new(ResponsesSearchErrorKind::Protocol, error_revision())
        })?;
    let search_calls = count_x_search_calls(output);
    if search_calls > max_search_calls {
        return Err(ResponsesSearchError::new(
            ResponsesSearchErrorKind::SearchBudgetExceeded,
            error_revision(),
        )
        .with_observed_search_calls(search_calls));
    }

    let gallery = gallery_from_output(output)
        .map_err(|kind| ResponsesSearchError::new(kind, error_revision()))?;
    let candidate_count = gallery
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| {
            ResponsesSearchError::new(ResponsesSearchErrorKind::Protocol, error_revision())
        })?;
    let items = parse_gallery_items(&gallery, "x");
    let valid_count = items.len();

    Ok(ResponsesSearchSuccess {
        items,
        candidate_count,
        valid_count,
        search_calls,
        model: RESPONSES_MODEL,
        effort: RESPONSES_EFFORT,
        credential_revision,
    })
}

fn responses_request(
    query: &str,
    sort: Option<&str>,
    max_search_calls: u32,
    lane_index: usize,
    target_count: usize,
    excluded_ids: &[String],
) -> Value {
    json!({
        "model": RESPONSES_MODEL,
        "input": responses_prompt(
            query,
            sort,
            max_search_calls,
            lane_index,
            target_count,
            excluded_ids,
        ),
        "tools": [{ "type": "x_search" }],
        "tool_choice": "auto",
        "max_tool_calls": max_search_calls,
        "reasoning": {
            "effort": RESPONSES_EFFORT,
            "summary": "concise"
        },
        "text": {
            "format": {
                "type": "json_schema",
                "name": "wallpaper_gallery",
                "strict": true,
                "schema": gallery_schema(target_count)
            }
        },
        "store": false
    })
}

fn responses_prompt(
    query: &str,
    sort: Option<&str>,
    max_search_calls: u32,
    lane_index: usize,
    target_count: usize,
    excluded_ids: &[String],
) -> String {
    let sort = match sort.unwrap_or("top") {
        "latest" | "Latest" => "Latest",
        _ => "Top",
    };
    let lane_guidance = match lane_index {
        1 => "Concurrent batch 1 of 3. Primary batch: search the strongest direct interpretation of the topic and prioritize immediately recognizable wallpaper candidates.",
        2 => "Concurrent batch 2 of 3. Visual-variation batch: avoid repeating the primary lane; emphasize alternate composition, lighting, season, or medium.",
        3 => "Concurrent batch 3 of 3. Discovery batch: use different viewpoint, palette, time or weather, cultural framing, or bilingual keywords; avoid the obvious direct and visual-variation queries.",
        4 => "User-requested load-more batch: search fresh long-tail variants with a different viewpoint, palette, setting, time, weather, cultural framing, or bilingual keywords. Do not repeat the initial three batches.",
        _ => "Concurrent wallpaper batch: use complementary direct and visual/style query variants.",
    };
    let exclusion_guidance = if excluded_ids.is_empty() {
        String::new()
    } else {
        format!(
            "\nExclude candidates carrying these opaque media/post ids from the existing validated gallery: {}",
            excluded_ids.join(", ")
        )
    };
    format!(
        r#"You collect high-quality still images from X (Twitter) for a wallpaper picker.

User topic: {query}
Sort preference: {sort}

{lane_guidance}{exclusion_guidance}

Use X search only. Use no more than {max_search_calls} x_search call(s) in this request. Search useful query variants with image filters. Prefer real photography or polished AI art suitable as wallpaper. Prefer posts that include both a prompt and attached images when relevant. Skip memes, screenshots, text cards, avatars, emoji packs, ads, blurry thumbnails, videos, and placeholder links.

Return {target_count} distinct items when possible. Return fewer only when you cannot confirm enough candidates; never pad, duplicate, or guess metadata. fullUrl must be a direct HTTPS full-size image CDN URL, preferably https://pbs.twimg.com/media/... with name=orig. postUrl must be a confirmed canonical https://x.com/<user>/status/<id>; omit it rather than guessing. Include mediaIndex 1-4 only when the status media position is known. Return metadata only and do not download files."#
    )
}

fn gallery_schema(target_count: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "maxItems": target_count,
                "items": {
                    "type": "object",
                    "properties": {
                        "fullUrl": { "type": "string" },
                        "thumbUrl": { "type": "string" },
                        "username": { "type": "string" },
                        "postUrl": { "type": "string" },
                        "textPreview": { "type": "string" },
                        "likes": { "type": "number" },
                        "kind": { "type": "string" },
                        "width": { "type": "number" },
                        "height": { "type": "number" },
                        "mediaIndex": { "type": "number" }
                    },
                    "required": ["fullUrl"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["items"],
        "additionalProperties": false
    })
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    credential_revision: Option<BuildOauthCredentialRevision>,
) -> Result<Vec<u8>, ResponsesSearchError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        ResponsesSearchError::new(transport_error_kind(&error), credential_revision.clone())
    })? {
        if body.len().saturating_add(chunk.len()) > RESPONSES_MAX_BODY_BYTES {
            return Err(ResponsesSearchError::new(
                ResponsesSearchErrorKind::Protocol,
                credential_revision,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn status_error_kind(status: StatusCode) -> ResponsesSearchErrorKind {
    match status.as_u16() {
        400 => ResponsesSearchErrorKind::BadRequest,
        401 | 403 => ResponsesSearchErrorKind::Unauthorized,
        429 => ResponsesSearchErrorKind::RateLimited,
        500..=599 => ResponsesSearchErrorKind::ServerError,
        _ => ResponsesSearchErrorKind::Protocol,
    }
}

fn transport_error_kind(error: &reqwest::Error) -> ResponsesSearchErrorKind {
    classify_transport_error(error.is_timeout(), error)
}

fn classify_transport_error(
    is_timeout: bool,
    error: &(dyn std::error::Error + 'static),
) -> ResponsesSearchErrorKind {
    if is_timeout {
        return ResponsesSearchErrorKind::Timeout;
    }
    let mut source = Some(error);
    while let Some(cause) = source {
        let message = cause.to_string().to_ascii_lowercase();
        if message.contains("tls")
            || message.contains("certificate")
            || message.contains("cert ")
            || message.contains("handshake")
        {
            return ResponsesSearchErrorKind::Tls;
        }
        source = cause.source();
    }
    ResponsesSearchErrorKind::Network
}

fn count_x_search_calls(output: &[Value]) -> u32 {
    output
        .iter()
        .filter(|item| {
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if item_type.contains("x_search")
                || item_type.contains("xsearch")
                // Only x_search is registered in this fixed request. Current
                // Build responses may emit an unnamed custom_tool_call.
                || item_type.contains("custom_tool")
            {
                return true;
            }
            item_type.contains("tool_call")
                && ["name", "tool_name", "type"]
                    .iter()
                    .filter_map(|field| item.get(field).and_then(Value::as_str))
                    .any(|name| name.eq_ignore_ascii_case("x_search"))
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn gallery_from_output(output: &[Value]) -> Result<Value, ResponsesSearchErrorKind> {
    let mut saw_invalid_json = false;
    for item in output.iter().rev() {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content.iter().rev() {
            if part.get("type").and_then(Value::as_str) != Some("output_text") {
                continue;
            }
            let Some(text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(text) {
                Ok(value) if value.get("items").and_then(Value::as_array).is_some() => {
                    return Ok(value);
                }
                Ok(_) => return Err(ResponsesSearchErrorKind::Protocol),
                Err(_) => saw_invalid_json = true,
            }
        }
    }
    if saw_invalid_json {
        Err(ResponsesSearchErrorKind::InvalidJson)
    } else {
        Err(ResponsesSearchErrorKind::Empty)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::body::{Body, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::Router;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::wallpaper_source::X_SEARCH_FIRST_ROUND_CALLS;

    #[derive(Clone)]
    struct MockState {
        status: StatusCode,
        response_body: Arc<String>,
        delay: Duration,
        requests: Arc<AtomicUsize>,
        authorization_ok: Arc<AtomicBool>,
        request_body: Arc<Mutex<Option<Value>>>,
    }

    struct FailingDnsResolver;

    #[derive(Debug)]
    struct SyntheticTransportError(&'static str);

    impl std::fmt::Display for SyntheticTransportError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for SyntheticTransportError {}

    impl reqwest::dns::Resolve for FailingDnsResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            Box::pin(async {
                let error: Box<dyn std::error::Error + Send + Sync> = Box::new(
                    std::io::Error::new(std::io::ErrorKind::NotFound, "synthetic DNS failure"),
                );
                Err::<reqwest::dns::Addrs, _>(error)
            })
        }
    }

    async fn spawn_raw_response(
        response: &'static [u8],
        delay: Duration,
    ) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind raw server");
        let address = listener.local_addr().expect("raw server address");
        let task = tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let _ = stream.write_all(response).await;
        });
        (address, task)
    }

    async fn mock_responses(
        State(state): State<MockState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        state.requests.fetch_add(1, Ordering::SeqCst);
        state.authorization_ok.store(
            headers
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some("Bearer test-token"),
            Ordering::SeqCst,
        );
        if let Ok(value) = serde_json::from_slice::<Value>(&body) {
            *state.request_body.lock().expect("request body lock") = Some(value);
        }
        if !state.delay.is_zero() {
            tokio::time::sleep(state.delay).await;
        }
        (state.status, state.response_body.as_str().to_string()).into_response()
    }

    async fn spawn_mock(
        status: StatusCode,
        body: impl Into<String>,
        delay: Duration,
    ) -> (String, MockState, JoinHandle<()>) {
        let state = MockState {
            status,
            response_body: Arc::new(body.into()),
            delay,
            requests: Arc::new(AtomicUsize::new(0)),
            authorization_ok: Arc::new(AtomicBool::new(false)),
            request_body: Arc::new(Mutex::new(None)),
        };
        let app = Router::new()
            .route("/v1/responses", post(mock_responses))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let address = listener.local_addr().expect("mock address");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}/v1/responses"), state, task)
    }

    fn test_revision() -> BuildOauthCredentialRevision {
        BuildOauthCredentialRevision {
            file_len: 123,
            modified_ms: Some(456),
        }
    }

    fn success_payload(search_calls: usize) -> String {
        let mut output = Vec::new();
        for _ in 0..search_calls {
            output.push(json!({ "type": "x_search_call", "status": "completed" }));
        }
        output.push(json!({
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": serde_json::to_string(&json!({
                    "items": [{
                        "fullUrl": "https://pbs.twimg.com/media/test.jpg?name=small",
                        "thumbUrl": "https://pbs.twimg.com/media/test.jpg?name=small",
                        "username": "alice",
                        "postUrl": "https://x.com/alice/status/1234567890123456789",
                        "kind": "image"
                    }]
                })).expect("gallery json")
            }]
        }));
        json!({
            "model": RESPONSES_MODEL,
            "output": output
        })
        .to_string()
    }

    async fn test_search(
        endpoint: &str,
        timeout: Duration,
    ) -> Result<ResponsesSearchSuccess, ResponsesSearchError> {
        let cancellation = WallpaperSearchCancellation::default();
        search_with_credentials(
            ResponsesSearchRequest {
                query: "misty mountains",
                sort: Some("top"),
                endpoint,
                max_search_calls: X_SEARCH_FIRST_ROUND_CALLS,
                lane_index: 1,
                target_count: 16,
                excluded_ids: &[],
                cancellation: &cancellation,
            },
            timeout,
            Ok(("test-token", test_revision())),
        )
        .await
    }

    fn lane_item(id: &str) -> WallpaperGalleryItem {
        WallpaperGalleryItem {
            id: id.into(),
            thumb_url: format!("https://pbs.twimg.com/media/{id}.jpg?name=small"),
            full_url: format!("https://pbs.twimg.com/media/{id}.jpg?name=orig"),
            kind: "image".into(),
            width: None,
            height: None,
            source: "x".into(),
            username: None,
            post_url: Some(format!("https://x.com/example/status/{id}")),
            text_preview: None,
            likes: None,
            local_path: None,
            prompt: None,
            provenance: crate::wallpaper_source::WallpaperProvenance::empty(),
            status_id: Some(id.into()),
            media_index: Some(1),
            media_quality: None,
            media_fingerprint: None,
        }
    }

    fn lane_success(ids: &[&str], search_calls: u32) -> ResponsesSearchSuccess {
        ResponsesSearchSuccess {
            items: ids.iter().map(|id| lane_item(id)).collect(),
            candidate_count: ids.len(),
            valid_count: ids.len(),
            search_calls,
            model: RESPONSES_MODEL,
            effort: RESPONSES_EFFORT,
            credential_revision: test_revision(),
        }
    }

    #[tokio::test]
    async fn wallpaper_x_responses_parallel_lanes_emit_completion_order_and_dedupe() {
        let captured = Arc::new(Mutex::new(Vec::<WallpaperXSearchBatch>::new()));
        let captured_batches = Arc::clone(&captured);
        let runtime = WallpaperXSearchRuntime::new(
            WallpaperSearchCancellation::default(),
            Arc::new(|_| {}),
            Arc::new(move |batch| {
                captured_batches
                    .lock()
                    .expect("batch capture lock")
                    .push(batch);
            }),
        );

        let result = run_parallel_lanes(&runtime, test_revision(), |lane_index| async move {
            match lane_index {
                1 => {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    Ok(lane_success(&["a", "shared"], 2))
                }
                2 => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Ok(lane_success(&["b", "shared"], 2))
                }
                _ => {
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    Err(ResponsesSearchError::new(
                        ResponsesSearchErrorKind::Empty,
                        Some(test_revision()),
                    )
                    .with_observed_search_calls(1))
                }
            }
        })
        .await
        .expect("two useful lanes should return partial success");

        assert_eq!(result.candidate_count, 4);
        assert_eq!(result.valid_count, 3);
        assert_eq!(result.search_calls, 5);
        let mut ids = result
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(ids, ["a", "b", "shared"]);

        let batches = captured.lock().expect("batch capture lock");
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].batch_index, 2);
        assert_eq!(batches[0].items.len(), 2);
        assert_eq!(batches[0].accumulated_count, 2);
        assert!(!batches[0].done);
        assert_eq!(batches[1].batch_index, 1);
        assert_eq!(batches[1].items.len(), 1);
        assert_eq!(batches[1].items[0].id, "a");
        assert_eq!(batches[1].accumulated_count, 3);
        assert!(batches[1].done);
    }

    #[tokio::test]
    async fn wallpaper_x_responses_parallel_cancellation_stops_all_lanes_without_batches() {
        let captured = Arc::new(Mutex::new(Vec::<WallpaperXSearchBatch>::new()));
        let captured_batches = Arc::clone(&captured);
        let cancellation = WallpaperSearchCancellation::default();
        let runtime = WallpaperXSearchRuntime::new(
            cancellation.clone(),
            Arc::new(|_| {}),
            Arc::new(move |batch| {
                captured_batches
                    .lock()
                    .expect("batch capture lock")
                    .push(batch);
            }),
        );
        let run = run_parallel_lanes(&runtime, test_revision(), |_| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(lane_success(&["late"], 1))
        });
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancellation.cancel();
        };
        let (result, ()) = tokio::join!(run, cancel);

        assert_eq!(
            result.expect_err("cancelled lanes must fail").kind,
            ResponsesSearchErrorKind::Cancelled
        );
        assert!(
            captured.lock().expect("batch capture lock").is_empty(),
            "cancelled lanes must not emit late batches"
        );
    }

    #[test]
    fn wallpaper_x_responses_parallel_error_priority_prevents_cli_double_spend() {
        let error = select_parallel_error(
            vec![
                ResponsesSearchError::new(ResponsesSearchErrorKind::Network, Some(test_revision())),
                ResponsesSearchError::new(
                    ResponsesSearchErrorKind::RateLimited,
                    Some(test_revision()),
                ),
            ],
            test_revision(),
        );
        assert_eq!(error.kind, ResponsesSearchErrorKind::RateLimited);
    }

    #[tokio::test]
    async fn wallpaper_x_responses_success_parses_gallery_and_fixed_contract() {
        assert_eq!(
            count_x_search_calls(&[json!({ "type": "custom_tool_call" })]),
            1
        );
        let (endpoint, state, task) =
            spawn_mock(StatusCode::OK, success_payload(2), Duration::ZERO).await;
        let result = test_search(&endpoint, Duration::from_secs(2))
            .await
            .expect("responses success");
        task.abort();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.candidate_count, 1);
        assert_eq!(result.valid_count, 1);
        assert_eq!(result.search_calls, 2);
        assert_eq!(result.model, "grok-4.6");
        assert_eq!(result.effort, "low");
        assert!(result.items[0].full_url.contains("name=orig"));
        assert_eq!(state.requests.load(Ordering::SeqCst), 1);
        assert!(state.authorization_ok.load(Ordering::SeqCst));

        let request = state
            .request_body
            .lock()
            .expect("request body lock")
            .clone()
            .expect("captured request");
        assert_eq!(request.get("model"), Some(&json!("grok-4.6")));
        assert_eq!(request.pointer("/reasoning/effort"), Some(&json!("low")));
        assert_eq!(request.get("store"), Some(&json!(false)));
        assert_eq!(request.get("max_tool_calls"), Some(&json!(2)));
        assert_eq!(request.pointer("/tools/0/type"), Some(&json!("x_search")));
        assert_eq!(
            request.pointer("/text/format/type"),
            Some(&json!("json_schema"))
        );
        assert_eq!(request.pointer("/text/format/strict"), Some(&json!(true)));
        assert_eq!(
            request.pointer("/text/format/schema/additionalProperties"),
            Some(&json!(false))
        );
        assert!(request
            .get("input")
            .and_then(Value::as_str)
            .is_some_and(|prompt| prompt.contains("no more than 2 x_search call")));
        let visual_lane = responses_request(
            "misty mountains",
            Some("top"),
            RESPONSES_LANE_MAX_X_SEARCH_CALLS,
            2,
            RESPONSES_LANE_TARGET_COUNT,
            &[],
        );
        assert_eq!(visual_lane.get("max_tool_calls"), Some(&json!(3)));
        assert_eq!(
            visual_lane.pointer("/text/format/schema/properties/items/maxItems"),
            Some(&json!(8))
        );
        assert!(visual_lane
            .get("input")
            .and_then(Value::as_str)
            .is_some_and(|prompt| {
                prompt.contains("Visual-variation batch")
                    && prompt.contains("Return 8 distinct items")
            }));
        assert_eq!(
            RESPONSES_LANE_COUNT as u32 * RESPONSES_LANE_MAX_X_SEARCH_CALLS,
            9
        );
        let excluded = vec!["twimg:seen-id".into(), "status:12345678:1".into()];
        let load_more = responses_request(
            "misty mountains",
            Some("top"),
            RESPONSES_LANE_MAX_X_SEARCH_CALLS,
            4,
            RESPONSES_LANE_TARGET_COUNT,
            &excluded,
        );
        assert!(load_more
            .get("input")
            .and_then(Value::as_str)
            .is_some_and(|prompt| {
                prompt.contains("User-requested load-more batch")
                    && prompt.contains("twimg:seen-id")
                    && prompt.contains("status:12345678:1")
            }));
    }

    #[tokio::test]
    async fn wallpaper_x_responses_classifies_http_failures() {
        for (status, expected) in [
            (
                StatusCode::BAD_REQUEST,
                ResponsesSearchErrorKind::BadRequest,
            ),
            (
                StatusCode::UNAUTHORIZED,
                ResponsesSearchErrorKind::Unauthorized,
            ),
            (
                StatusCode::FORBIDDEN,
                ResponsesSearchErrorKind::Unauthorized,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                ResponsesSearchErrorKind::RateLimited,
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponsesSearchErrorKind::ServerError,
            ),
        ] {
            let (endpoint, _state, task) = spawn_mock(status, "redacted", Duration::ZERO).await;
            let error = test_search(&endpoint, Duration::from_secs(2))
                .await
                .expect_err("http status must fail");
            task.abort();
            assert_eq!(error.kind, expected);
            assert_eq!(error.code(), expected.code());
        }
    }

    #[tokio::test]
    async fn wallpaper_x_responses_classifies_timeout_invalid_json_and_empty() {
        let (endpoint, _state, task) = spawn_mock(
            StatusCode::OK,
            success_payload(1),
            Duration::from_millis(150),
        )
        .await;
        let error = test_search(&endpoint, Duration::from_millis(25))
            .await
            .expect_err("request must time out");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::Timeout);

        let (endpoint, _state, task) = spawn_mock(StatusCode::OK, "not-json", Duration::ZERO).await;
        let error = test_search(&endpoint, Duration::from_secs(2))
            .await
            .expect_err("invalid json must fail");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::InvalidJson);

        let (endpoint, _state, task) = spawn_mock(StatusCode::OK, "", Duration::ZERO).await;
        let error = test_search(&endpoint, Duration::from_secs(2))
            .await
            .expect_err("empty response must fail");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::Empty);
    }

    #[tokio::test]
    async fn wallpaper_x_responses_classifies_proxy_dns_and_connection_failures_as_network() {
        let (proxy_address, proxy_task) = spawn_raw_response(
            b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Duration::ZERO,
        )
        .await;
        let proxy_client = reqwest::Client::builder()
            .no_proxy()
            .proxy(
                reqwest::Proxy::all(format!("http://{proxy_address}"))
                    .expect("synthetic proxy URL"),
            )
            .timeout(Duration::from_secs(1))
            .build()
            .expect("proxy client");
        let proxy_error = proxy_client
            .get("https://wallpaper-search.invalid")
            .send()
            .await
            .expect_err("proxy must reject the CONNECT request");
        proxy_task.abort();
        assert_eq!(
            transport_error_kind(&proxy_error),
            ResponsesSearchErrorKind::Network,
            "proxy error: {proxy_error:?}"
        );

        let dns_client = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(FailingDnsResolver))
            .timeout(Duration::from_secs(1))
            .build()
            .expect("DNS client");
        let dns_error = dns_client
            .get("http://wallpaper-search.invalid")
            .send()
            .await
            .expect_err("synthetic DNS must fail");
        assert_eq!(
            transport_error_kind(&dns_error),
            ResponsesSearchErrorKind::Network
        );

        let (reset_address, reset_task) = spawn_raw_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\npartial",
            Duration::ZERO,
        )
        .await;
        let reset_client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reset client");
        let reset_error = match reset_client
            .get(format!("http://{reset_address}"))
            .send()
            .await
        {
            Ok(response) => response
                .bytes()
                .await
                .expect_err("truncated response body must fail"),
            Err(error) => error,
        };
        reset_task.abort();
        assert_eq!(
            transport_error_kind(&reset_error),
            ResponsesSearchErrorKind::Network
        );
    }

    #[tokio::test]
    async fn wallpaper_x_responses_classifies_tls_markers_and_timeout_transport_failures() {
        for message in [
            "TLS handshake failed",
            "invalid peer certificate: UnknownIssuer",
        ] {
            assert_eq!(
                classify_transport_error(false, &SyntheticTransportError(message)),
                ResponsesSearchErrorKind::Tls
            );
        }
        assert_eq!(
            classify_transport_error(false, &SyntheticTransportError("connection reset by peer")),
            ResponsesSearchErrorKind::Network
        );

        let (timeout_address, timeout_task) =
            spawn_raw_response(b"HTTP/1.1 204 No Content\r\n\r\n", Duration::from_secs(1)).await;
        let timeout_error = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(25))
            .build()
            .expect("timeout client")
            .get(format!("http://{timeout_address}"))
            .send()
            .await
            .expect_err("delayed response must time out");
        timeout_task.abort();
        assert_eq!(
            transport_error_kind(&timeout_error),
            ResponsesSearchErrorKind::Timeout
        );
    }

    #[tokio::test]
    async fn wallpaper_x_responses_cancellation_aborts_inflight_http() {
        let (endpoint, state, task) =
            spawn_mock(StatusCode::OK, success_payload(1), Duration::from_secs(10)).await;
        let cancellation = WallpaperSearchCancellation::default();
        let cancellation_for_request = cancellation.clone();
        let request = tokio::spawn(async move {
            search_with_credentials(
                ResponsesSearchRequest {
                    query: "misty mountains",
                    sort: Some("top"),
                    endpoint: &endpoint,
                    max_search_calls: X_SEARCH_FIRST_ROUND_CALLS,
                    lane_index: 1,
                    target_count: 16,
                    excluded_ids: &[],
                    cancellation: &cancellation_for_request,
                },
                Duration::from_secs(30),
                Ok(("test-token", test_revision())),
            )
            .await
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            while state.requests.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mock request did not start");
        cancellation.cancel();
        let error = tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .expect("cancelled request did not return")
            .expect("request task panicked")
            .expect_err("cancelled request must fail");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::Cancelled);
    }

    #[tokio::test]
    async fn wallpaper_x_responses_rejects_bad_structured_output_and_tool_overrun() {
        let invalid_output = json!({
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "not-json" }]
            }]
        })
        .to_string();
        let (endpoint, _state, task) =
            spawn_mock(StatusCode::OK, invalid_output, Duration::ZERO).await;
        let error = test_search(&endpoint, Duration::from_secs(2))
            .await
            .expect_err("bad structured output must fail");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::InvalidJson);

        let (endpoint, _state, task) =
            spawn_mock(StatusCode::OK, success_payload(4), Duration::ZERO).await;
        let error = test_search(&endpoint, Duration::from_secs(2))
            .await
            .expect_err("tool overrun must fail");
        task.abort();
        assert_eq!(error.kind, ResponsesSearchErrorKind::SearchBudgetExceeded);
    }

    #[tokio::test]
    async fn wallpaper_x_responses_oauth_failure_sends_no_request() {
        let (endpoint, state, task) =
            spawn_mock(StatusCode::OK, success_payload(1), Duration::ZERO).await;
        for (oauth_error, expected) in [
            (
                BuildOauthTokenError::Unavailable,
                ResponsesSearchErrorKind::OauthUnavailable,
            ),
            (
                BuildOauthTokenError::Expired,
                ResponsesSearchErrorKind::OauthExpired,
            ),
        ] {
            let cancellation = WallpaperSearchCancellation::default();
            let error = search_with_credentials(
                ResponsesSearchRequest {
                    query: "misty mountains",
                    sort: Some("top"),
                    endpoint: &endpoint,
                    max_search_calls: X_SEARCH_FIRST_ROUND_CALLS,
                    lane_index: 1,
                    target_count: 16,
                    excluded_ids: &[],
                    cancellation: &cancellation,
                },
                Duration::from_secs(2),
                Err(oauth_error),
            )
            .await
            .expect_err("oauth failure must stop before HTTP");
            assert_eq!(error.kind, expected);
        }
        task.abort();
        assert_eq!(state.requests.load(Ordering::SeqCst), 0);
    }

    #[derive(Clone)]
    struct RedirectState {
        target: String,
        source_requests: Arc<AtomicUsize>,
        target_requests: Arc<AtomicUsize>,
        target_saw_authorization: Arc<AtomicBool>,
    }

    async fn redirect_source(State(state): State<RedirectState>) -> Response {
        state.source_requests.fetch_add(1, Ordering::SeqCst);
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
        response.headers_mut().insert(
            reqwest::header::LOCATION,
            HeaderValue::from_str(&state.target).expect("redirect target header"),
        );
        response
    }

    async fn redirect_target(State(state): State<RedirectState>, headers: HeaderMap) -> Response {
        state.target_requests.fetch_add(1, Ordering::SeqCst);
        state.target_saw_authorization.store(
            headers.contains_key(reqwest::header::AUTHORIZATION),
            Ordering::SeqCst,
        );
        (StatusCode::OK, success_payload(1)).into_response()
    }

    #[tokio::test]
    async fn wallpaper_x_responses_never_replays_authorization_across_redirect() {
        let target_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect target");
        let target_address = target_listener.local_addr().expect("target address");
        let state = RedirectState {
            target: format!("http://{target_address}/target"),
            source_requests: Arc::new(AtomicUsize::new(0)),
            target_requests: Arc::new(AtomicUsize::new(0)),
            target_saw_authorization: Arc::new(AtomicBool::new(false)),
        };
        let target_app = Router::new()
            .route("/target", post(redirect_target))
            .with_state(state.clone());
        let target_task = tokio::spawn(async move {
            let _ = axum::serve(target_listener, target_app).await;
        });

        let source_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect source");
        let source_address = source_listener.local_addr().expect("source address");
        let source_app = Router::new()
            .route("/v1/responses", post(redirect_source))
            .with_state(state.clone());
        let source_task = tokio::spawn(async move {
            let _ = axum::serve(source_listener, source_app).await;
        });

        let error = test_search(
            &format!("http://{source_address}/v1/responses"),
            Duration::from_secs(2),
        )
        .await
        .expect_err("redirect must be a protocol failure");
        source_task.abort();
        target_task.abort();

        assert_eq!(error.kind, ResponsesSearchErrorKind::Protocol);
        assert_eq!(state.source_requests.load(Ordering::SeqCst), 1);
        assert_eq!(state.target_requests.load(Ordering::SeqCst), 0);
        assert!(!state.target_saw_authorization.load(Ordering::SeqCst));
    }
}
