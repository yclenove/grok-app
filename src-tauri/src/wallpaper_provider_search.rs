//! Fixed-endpoint Openverse and Pexels wallpaper-library adapters.
//!
//! Provider credentials, paging cursors, raw responses, and query strings stay
//! inside the Host. Only validated image DTOs cross IPC.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use parking_lot::Mutex;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use url::Url;

use crate::skin_net::{self, OriginPolicy};
use crate::wallpaper_remote_media::{self, RemoteImageProber};
use crate::wallpaper_remote_search::{
    self as remote_search, RemoteSearchBatch, RemoteSearchResult, RemoteSearchRuntime,
    RemoteSearchStage, RemoteWallpaperSource,
};
use crate::wallpaper_source::{
    WallpaperGalleryItem, WallpaperProvenance, WallpaperSearchCancellation,
};

mod request_url;

use request_url::provider_request_url;
#[cfg(test)]
use request_url::{provider_url, PEXELS_CACHE_BUST_PARAM};

const PEXELS_LICENSE_URL: &str = "https://www.pexels.com/license/";
const CONTRACT_VERSION: u8 = 3;
const RESULT_LIMIT: usize = 20;
const OPENVERSE_PAGE_SIZE: usize = 20;
const OPENVERSE_PAGES_PER_BATCH: usize = 2;
const PEXELS_PAGE_SIZE: usize = 40;
const MAX_API_BYTES: usize = 2 * 1024 * 1024;
const MAX_CANDIDATES: usize = 40;
const MAX_CONCURRENT_PROBES: usize = 10;
const PROGRESS_BATCH_SIZE: usize = 4;
const HTTP_TIMEOUT: Duration = Duration::from_secs(25);
const IMAGE_PROBE_TIMEOUT: Duration = Duration::from_secs(12);
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_CAPACITY: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SearchKey {
    source: RemoteWallpaperSource,
    query: String,
    credential_revision: Option<String>,
    contract_version: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PageKey {
    search: SearchKey,
    first_page: usize,
}

#[derive(Clone)]
struct CacheEntry {
    key: PageKey,
    inserted: Instant,
    result: RemoteSearchResult,
    next_page: usize,
}

#[derive(Default)]
struct SearchCache {
    entries: VecDeque<CacheEntry>,
}

impl SearchCache {
    fn get(&mut self, key: &PageKey, now: Instant) -> Option<(RemoteSearchResult, usize)> {
        self.entries
            .retain(|entry| now.duration_since(entry.inserted) <= CACHE_TTL);
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index)?;
        let result = entry.result.clone();
        let next_page = entry.next_page;
        self.entries.push_front(entry);
        Some((result, next_page))
    }

    fn insert(&mut self, key: PageKey, result: RemoteSearchResult, next_page: usize, now: Instant) {
        self.entries.retain(|entry| entry.key != key);
        self.entries.push_front(CacheEntry {
            key,
            inserted: now,
            result,
            next_page,
        });
        self.entries.truncate(CACHE_CAPACITY);
    }
}

struct ProviderContext {
    search_key: SearchKey,
    api_key: Option<String>,
}

#[derive(Clone)]
struct ActiveRequest {
    token: uuid::Uuid,
    cancellation: WallpaperSearchCancellation,
}

#[derive(Clone)]
struct ProviderCandidate {
    upstream_id: String,
    image_url: String,
    source_url: String,
    source_name: &'static str,
    title: Option<String>,
    author_name: String,
    author_url: Option<String>,
    license: String,
    license_url: String,
}

struct ApiPage {
    candidates: Vec<ProviderCandidate>,
    has_more: bool,
}

struct CandidateBatch {
    candidates: Vec<ProviderCandidate>,
    has_more: bool,
    next_page: usize,
}

struct ValidatedPage {
    items: Vec<WallpaperGalleryItem>,
    has_more: bool,
    next_page: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderError {
    Cancelled,
    PexelsKeyMissing,
    PexelsKeyInvalid,
    RateLimited,
    Timeout,
    Network,
    ServiceUnavailable,
    Protocol,
}

impl ProviderError {
    fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::PexelsKeyMissing => "pexels_key_missing",
            Self::PexelsKeyInvalid => "pexels_key_invalid",
            Self::RateLimited => "provider_rate_limited",
            Self::Timeout => "provider_timeout",
            Self::Network => "provider_network",
            Self::ServiceUnavailable => "provider_service_unavailable",
            Self::Protocol => "provider_protocol",
        }
    }
}

static ACTIVE: OnceLock<Mutex<HashMap<String, ActiveRequest>>> = OnceLock::new();
static CACHE: OnceLock<Mutex<SearchCache>> = OnceLock::new();
static CONTINUATIONS: OnceLock<Mutex<HashMap<SearchKey, usize>>> = OnceLock::new();

fn active() -> &'static Mutex<HashMap<String, ActiveRequest>> {
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache() -> &'static Mutex<SearchCache> {
    CACHE.get_or_init(|| Mutex::new(SearchCache::default()))
}

fn continuations() -> &'static Mutex<HashMap<SearchKey, usize>> {
    CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn begin_request(request_id: &str) -> (uuid::Uuid, WallpaperSearchCancellation) {
    let mut requests = active().lock();
    for (_, request) in requests.drain() {
        request.cancellation.cancel();
    }
    let token = uuid::Uuid::new_v4();
    let cancellation = WallpaperSearchCancellation::default();
    requests.insert(
        request_id.to_string(),
        ActiveRequest {
            token,
            cancellation: cancellation.clone(),
        },
    );
    (token, cancellation)
}

fn finish_request(request_id: &str, token: uuid::Uuid) {
    let mut requests = active().lock();
    if requests
        .get(request_id)
        .is_some_and(|request| request.token == token)
    {
        requests.remove(request_id);
    }
}

pub(crate) fn cancel(request_id: &str) -> bool {
    let request = active().lock().remove(request_id);
    if let Some(request) = request {
        request.cancellation.cancel();
        true
    } else {
        false
    }
}

fn library_provider_query(query: &str) -> String {
    const CJK_WALLPAPER_TERMS: [&str; 4] = ["壁纸", "壁紙", "桌布", "배경화면"];

    let mut stripped = query.to_string();
    for term in CJK_WALLPAPER_TERMS {
        stripped = stripped.replace(term, " ");
    }

    let normalized = stripped
        .split_whitespace()
        .filter(|token| {
            let comparison = token
                .trim_matches(|ch: char| !ch.is_alphanumeric())
                .to_ascii_lowercase();
            !comparison.is_empty()
                && !matches!(
                    comparison.as_str(),
                    "wallpaper" | "wallpapers" | "4k" | "8k" | "uhd"
                )
        })
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.is_empty() {
        query.trim().to_string()
    } else {
        normalized
    }
}

fn provider_context(
    source: RemoteWallpaperSource,
    query: &str,
) -> Result<ProviderContext, ProviderError> {
    let (api_key, credential_revision) = match source {
        RemoteWallpaperSource::Openverse => (None, None),
        RemoteWallpaperSource::Pexels => {
            let key = crate::store::load_secrets()
                .pexels_api_key
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && value.len() <= 512)
                .ok_or(ProviderError::PexelsKeyMissing)?;
            HeaderValue::from_str(&key).map_err(|_| ProviderError::PexelsKeyInvalid)?;
            let revision = hex::encode(Sha256::digest(key.as_bytes()));
            (Some(key), Some(revision))
        }
        RemoteWallpaperSource::Web => return Err(ProviderError::Protocol),
    };
    Ok(ProviderContext {
        search_key: SearchKey {
            source,
            query: remote_search::normalized_query(query),
            credential_revision,
            contract_version: CONTRACT_VERSION,
        },
        api_key,
    })
}

pub(crate) async fn search(
    app: &AppHandle,
    source: RemoteWallpaperSource,
    request_id: &str,
    query: &str,
) -> Result<RemoteSearchResult, String> {
    let query = remote_search::validate_query(query)?;
    let provider_query = library_provider_query(&query);
    let started = Instant::now();
    let context = match provider_context(source, &query) {
        Ok(context) => context,
        Err(error) => {
            return Ok(RemoteSearchResult::error(
                source.as_str(),
                error.code(),
                elapsed_ms(started),
            ));
        }
    };
    let (token, cancellation) = begin_request(request_id);
    let runtime = remote_search::runtime(app, request_id, source, cancellation);
    runtime.report(RemoteSearchStage::Preparing);
    let page_key = PageKey {
        search: context.search_key.clone(),
        first_page: 1,
    };

    if let Some((mut result, next_page)) = cache().lock().get(&page_key, Instant::now()) {
        result.cache_hit = true;
        result.duration_ms = elapsed_ms(started);
        update_continuation(&context.search_key, result.has_more, next_page);
        emit_cached_batch(&runtime, &result);
        runtime.report(RemoteSearchStage::Done);
        finish_request(request_id, token);
        return Ok(result);
    }

    let page = search_page(
        source,
        &provider_query,
        1,
        context.api_key.as_deref(),
        &runtime,
    )
    .await;
    let mut result = result_from_page(source, page.as_ref(), started);
    if runtime.is_cancelled() {
        result = RemoteSearchResult::error(source.as_str(), "cancelled", elapsed_ms(started));
    } else if let Ok(page) = page {
        update_continuation(&context.search_key, page.has_more, page.next_page);
        if !page.items.is_empty() {
            cache()
                .lock()
                .insert(page_key, result.clone(), page.next_page, Instant::now());
        }
    } else {
        continuations().lock().remove(&context.search_key);
    }
    runtime.report(RemoteSearchStage::Done);
    finish_request(request_id, token);
    Ok(result)
}

pub(crate) async fn search_more(
    app: &AppHandle,
    source: RemoteWallpaperSource,
    request_id: &str,
    query: &str,
) -> Result<RemoteSearchResult, String> {
    let query = remote_search::validate_query(query)?;
    let provider_query = library_provider_query(&query);
    let started = Instant::now();
    let context = match provider_context(source, &query) {
        Ok(context) => context,
        Err(error) => {
            return Ok(RemoteSearchResult::error(
                source.as_str(),
                error.code(),
                elapsed_ms(started),
            ));
        }
    };
    let Some(first_page) = continuations().lock().get(&context.search_key).copied() else {
        return Ok(RemoteSearchResult::error(
            source.as_str(),
            "provider_no_continuation",
            elapsed_ms(started),
        ));
    };
    let (token, cancellation) = begin_request(request_id);
    let runtime = remote_search::runtime(app, request_id, source, cancellation);
    runtime.report(RemoteSearchStage::LoadingMore);
    let page_key = PageKey {
        search: context.search_key.clone(),
        first_page,
    };

    if let Some((mut result, next_page)) = cache().lock().get(&page_key, Instant::now()) {
        result.cache_hit = true;
        result.duration_ms = elapsed_ms(started);
        update_continuation(&context.search_key, result.has_more, next_page);
        emit_cached_batch(&runtime, &result);
        runtime.report(RemoteSearchStage::Done);
        finish_request(request_id, token);
        return Ok(result);
    }

    let page = search_page(
        source,
        &provider_query,
        first_page,
        context.api_key.as_deref(),
        &runtime,
    )
    .await;
    let mut result = result_from_page(source, page.as_ref(), started);
    if runtime.is_cancelled() {
        result = RemoteSearchResult::error(source.as_str(), "cancelled", elapsed_ms(started));
    } else if let Ok(page) = page {
        update_continuation(&context.search_key, page.has_more, page.next_page);
        if !page.items.is_empty() {
            cache()
                .lock()
                .insert(page_key, result.clone(), page.next_page, Instant::now());
        }
    }
    runtime.report(RemoteSearchStage::Done);
    finish_request(request_id, token);
    Ok(result)
}

fn emit_cached_batch(runtime: &RemoteSearchRuntime, result: &RemoteSearchResult) {
    runtime.report_batch(RemoteSearchBatch {
        batch_index: 1,
        items: result.items.clone(),
        accumulated_count: result.items.len(),
        done: true,
    });
}

fn result_from_page(
    source: RemoteWallpaperSource,
    page: Result<&ValidatedPage, &ProviderError>,
    started: Instant,
) -> RemoteSearchResult {
    match page {
        Ok(page) if !page.items.is_empty() => RemoteSearchResult::success(
            source.as_str(),
            page.items.clone(),
            page.has_more,
            elapsed_ms(started),
        ),
        Ok(_) => RemoteSearchResult::error(source.as_str(), "empty", elapsed_ms(started)),
        Err(error) => RemoteSearchResult::error(source.as_str(), error.code(), elapsed_ms(started)),
    }
}

fn update_continuation(key: &SearchKey, has_more: bool, next_page: usize) {
    if has_more {
        continuations().lock().insert(key.clone(), next_page);
    } else {
        continuations().lock().remove(key);
    }
}

async fn search_page(
    source: RemoteWallpaperSource,
    query: &str,
    first_page: usize,
    api_key: Option<&str>,
    runtime: &RemoteSearchRuntime,
) -> Result<ValidatedPage, ProviderError> {
    if runtime.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    runtime.report(if first_page == 1 {
        RemoteSearchStage::SearchingProvider
    } else {
        RemoteSearchStage::LoadingMore
    });
    let batch = fetch_candidate_batch(source, query, first_page, api_key, runtime).await?;
    runtime.report(RemoteSearchStage::ValidatingImages);
    let items = validate_candidates(source, batch.candidates, runtime).await?;
    Ok(ValidatedPage {
        items,
        has_more: batch.has_more,
        next_page: batch.next_page,
    })
}

async fn fetch_candidate_batch(
    source: RemoteWallpaperSource,
    query: &str,
    first_page: usize,
    api_key: Option<&str>,
    runtime: &RemoteSearchRuntime,
) -> Result<CandidateBatch, ProviderError> {
    let pages_per_batch = match source {
        RemoteWallpaperSource::Openverse => OPENVERSE_PAGES_PER_BATCH,
        RemoteWallpaperSource::Pexels => 1,
        RemoteWallpaperSource::Web => return Err(ProviderError::Protocol),
    };
    let client = Arc::new(provider_client()?);
    let mut requests = FuturesUnordered::new();
    for page in first_page..first_page + pages_per_batch {
        let client = Arc::clone(&client);
        let cancellation = runtime.cancellation().clone();
        let api_key = api_key.map(str::to_string);
        let query = query.to_string();
        requests.push(async move {
            let result = fetch_api_page(
                client.as_ref(),
                source,
                &query,
                page,
                api_key.as_deref(),
                &cancellation,
            )
            .await;
            (page, result)
        });
    }

    let mut completed = HashMap::new();
    while let Some((page, result)) = tokio::select! {
        biased;
        _ = runtime.cancellation().cancelled() => return Err(ProviderError::Cancelled),
        result = requests.next() => result,
    } {
        completed.insert(page, result);
    }

    let mut candidates = Vec::new();
    let mut has_more = false;
    let mut next_page = first_page;
    for page_number in first_page..first_page + pages_per_batch {
        let Some(result) = completed.remove(&page_number) else {
            break;
        };
        match result {
            Ok(page) => {
                candidates.extend(page.candidates);
                next_page = page_number + 1;
                has_more = page.has_more;
                if !has_more {
                    break;
                }
            }
            Err(error) if page_number == first_page => return Err(error),
            Err(_) => {
                has_more = true;
                next_page = page_number;
                break;
            }
        }
    }
    candidates.truncate(MAX_CANDIDATES);
    Ok(CandidateBatch {
        candidates,
        has_more,
        next_page,
    })
}

fn provider_client() -> Result<reqwest::Client, ProviderError> {
    crate::proxy::apply_to_reqwest(
        reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .connect_timeout(Duration::from_secs(12))
            .redirect(Policy::none())
            .user_agent("GrokApp/WallpaperProviderSearch"),
    )
    .build()
    .map_err(|_| ProviderError::Network)
}

async fn fetch_api_page(
    client: &reqwest::Client,
    source: RemoteWallpaperSource,
    query: &str,
    page: usize,
    api_key: Option<&str>,
    cancellation: &WallpaperSearchCancellation,
) -> Result<ApiPage, ProviderError> {
    let url = provider_request_url(source, query, page)?;
    let checked = skin_net::check_hop(
        url.as_str(),
        &OriginPolicy::AnyHttps,
        skin_net::default_resolve,
    )
    .map_err(|_| ProviderError::Network)?;
    let mut request = client.get(checked).header(ACCEPT, "application/json");
    if source == RemoteWallpaperSource::Pexels {
        let key = api_key.ok_or(ProviderError::PexelsKeyMissing)?;
        let value = HeaderValue::from_str(key).map_err(|_| ProviderError::PexelsKeyInvalid)?;
        request = request.header(AUTHORIZATION, value);
    }
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
        response = request.send() => response.map_err(|error| classify_reqwest_error(&error))?,
    };
    let status = response.status();
    if status.is_redirection() {
        return Err(ProviderError::Protocol);
    }
    match status.as_u16() {
        200..=299 => {}
        401 | 403 if source == RemoteWallpaperSource::Pexels => {
            return Err(ProviderError::PexelsKeyInvalid)
        }
        408 | 504 => return Err(ProviderError::Timeout),
        429 => return Err(ProviderError::RateLimited),
        500..=599 => return Err(ProviderError::ServiceUnavailable),
        _ => return Err(ProviderError::Protocol),
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "application/json" && !content_type.ends_with("+json") {
        return Err(ProviderError::Protocol);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_API_BYTES as u64)
    {
        return Err(ProviderError::Protocol);
    }
    let bytes = read_bounded_body(response, cancellation).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ProviderError::Protocol)?;
    parse_api_page(source, &value, page)
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    cancellation: &WallpaperSearchCancellation,
) -> Result<Vec<u8>, ProviderError> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            chunk = response.chunk() => chunk.map_err(|error| classify_reqwest_error(&error))?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        if bytes.len() + chunk.len() > MAX_API_BYTES {
            return Err(ProviderError::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn classify_reqwest_error(error: &reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Network
    }
}

fn parse_api_page(
    source: RemoteWallpaperSource,
    value: &Value,
    page: usize,
) -> Result<ApiPage, ProviderError> {
    match source {
        RemoteWallpaperSource::Openverse => parse_openverse_page(value, page),
        RemoteWallpaperSource::Pexels => parse_pexels_page(value, page),
        RemoteWallpaperSource::Web => Err(ProviderError::Protocol),
    }
}

fn parse_openverse_page(value: &Value, page: usize) -> Result<ApiPage, ProviderError> {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Protocol)?;
    let page_count = value.get("page_count").and_then(Value::as_u64);
    let has_more = page_count
        .map(|count| (page as u64) < count)
        .unwrap_or(results.len() >= OPENVERSE_PAGE_SIZE);
    let mut candidates = Vec::new();
    for raw in results.iter().take(OPENVERSE_PAGE_SIZE) {
        if raw.get("mature").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(image_url) = raw.get("url").and_then(Value::as_str).and_then(safe_url) else {
            continue;
        };
        let Some(source_url) = raw
            .get("foreign_landing_url")
            .and_then(Value::as_str)
            .and_then(safe_url)
        else {
            continue;
        };
        let Some(author_name) = clean_text(raw.get("creator").and_then(Value::as_str), 160) else {
            continue;
        };
        let Some(license_url) = raw
            .get("license_url")
            .and_then(Value::as_str)
            .and_then(safe_url)
        else {
            continue;
        };
        let Some(license) = openverse_license_label(
            raw.get("license").and_then(Value::as_str),
            raw.get("license_version").and_then(Value::as_str),
        ) else {
            continue;
        };
        let upstream_id = clean_text(raw.get("id").and_then(Value::as_str), 160)
            .unwrap_or_else(|| opaque_id(&image_url));
        candidates.push(ProviderCandidate {
            upstream_id,
            image_url,
            source_url,
            source_name: "Openverse",
            title: clean_text(raw.get("title").and_then(Value::as_str), 240),
            author_name,
            author_url: raw
                .get("creator_url")
                .and_then(Value::as_str)
                .and_then(safe_url),
            license,
            license_url,
        });
    }
    Ok(ApiPage {
        candidates,
        has_more,
    })
}

fn parse_pexels_page(value: &Value, page: usize) -> Result<ApiPage, ProviderError> {
    let results = value
        .get("photos")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Protocol)?;
    let total = value.get("total_results").and_then(Value::as_u64);
    let per_page = value
        .get("per_page")
        .and_then(Value::as_u64)
        .unwrap_or(PEXELS_PAGE_SIZE as u64)
        .max(1);
    let has_more = total
        .map(|count| (page as u64).saturating_mul(per_page) < count)
        .unwrap_or_else(|| value.get("next_page").is_some_and(|next| !next.is_null()));
    let mut candidates = Vec::new();
    for raw in results.iter().take(PEXELS_PAGE_SIZE) {
        let Some(image_url) = raw
            .get("src")
            .and_then(|src| src.get("original").or_else(|| src.get("large2x")))
            .and_then(Value::as_str)
            .and_then(safe_url)
        else {
            continue;
        };
        let Some(source_url) = raw.get("url").and_then(Value::as_str).and_then(safe_url) else {
            continue;
        };
        let Some(author_name) = clean_text(raw.get("photographer").and_then(Value::as_str), 160)
        else {
            continue;
        };
        let upstream_id = raw
            .get("id")
            .and_then(|id| {
                id.as_u64()
                    .map(|value| value.to_string())
                    .or_else(|| clean_text(id.as_str(), 160))
            })
            .unwrap_or_else(|| opaque_id(&image_url));
        candidates.push(ProviderCandidate {
            upstream_id,
            image_url,
            source_url,
            source_name: "Pexels",
            title: clean_text(raw.get("alt").and_then(Value::as_str), 240),
            author_name,
            author_url: raw
                .get("photographer_url")
                .and_then(Value::as_str)
                .and_then(safe_url),
            license: "Pexels License".into(),
            license_url: PEXELS_LICENSE_URL.into(),
        });
    }
    Ok(ApiPage {
        candidates,
        has_more,
    })
}

fn safe_url(raw: &str) -> Option<String> {
    if raw.len() > 2_048 {
        return None;
    }
    let mut url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.port().is_some_and(|port| port != 443)
    {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn clean_text(value: Option<&str>, limit: usize) -> Option<String> {
    let value = value?.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        None
    } else {
        Some(value.chars().take(limit).collect())
    }
}

fn openverse_license_label(code: Option<&str>, version: Option<&str>) -> Option<String> {
    let code = code?.trim().to_ascii_lowercase();
    if code.is_empty()
        || !code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return None;
    }
    let version = version.map(str::trim).filter(|value| {
        !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_digit() || character == '.')
    });
    let base = match code.as_str() {
        "cc0" => "CC0".to_string(),
        "pdm" => "Public Domain Mark".to_string(),
        other => format!("CC {}", other.to_ascii_uppercase()),
    };
    Some(match version {
        Some(version) => format!("{base} {version}"),
        None => base,
    })
}

async fn validate_candidates(
    source: RemoteWallpaperSource,
    candidates: Vec<ProviderCandidate>,
    runtime: &RemoteSearchRuntime,
) -> Result<Vec<WallpaperGalleryItem>, ProviderError> {
    let prober = Arc::new(RemoteImageProber::new().map_err(|_| ProviderError::Network)?);
    let cancellation = runtime.cancellation().clone();
    let mut probes = futures_util::stream::iter(candidates)
        .map(|candidate| {
            let prober = Arc::clone(&prober);
            let cancellation = cancellation.clone();
            async move {
                let referer = Url::parse(&candidate.source_url)
                    .ok()
                    .as_ref()
                    .and_then(wallpaper_remote_media::origin_referer);
                let probe = tokio::time::timeout(
                    IMAGE_PROBE_TIMEOUT,
                    prober.probe_image(&candidate.image_url, referer.as_deref(), &cancellation),
                )
                .await
                .ok()
                .and_then(Result::ok)?;
                if !wallpaper_remote_media::is_wallpaper_quality_candidate(&probe) {
                    return None;
                }
                Some(provider_item(source, candidate, probe))
            }
        })
        .buffer_unordered(MAX_CONCURRENT_PROBES);

    let mut items = Vec::new();
    let mut pending = Vec::new();
    let mut seen_urls = HashSet::new();
    let mut seen_fingerprints = HashSet::new();
    let mut batch_index = 1;
    loop {
        let next = tokio::select! {
            biased;
            _ = runtime.cancellation().cancelled() => return Err(ProviderError::Cancelled),
            next = probes.next() => next,
        };
        let Some(item) = next else {
            break;
        };
        let Some(item) = item else {
            continue;
        };
        let fingerprint = item.media_fingerprint.clone().unwrap_or_default();
        if !seen_urls.insert(item.full_url.clone())
            || (!fingerprint.is_empty() && !seen_fingerprints.insert(fingerprint))
        {
            continue;
        }
        pending.push(item.clone());
        items.push(item);
        if pending.len() >= PROGRESS_BATCH_SIZE && items.len() < RESULT_LIMIT {
            runtime.report_batch(RemoteSearchBatch {
                batch_index,
                items: std::mem::take(&mut pending),
                accumulated_count: items.len(),
                done: false,
            });
            batch_index += 1;
        }
        if items.len() == RESULT_LIMIT {
            break;
        }
    }
    runtime.report_batch(RemoteSearchBatch {
        batch_index,
        items: pending,
        accumulated_count: items.len(),
        done: true,
    });
    Ok(items)
}

fn provider_item(
    source: RemoteWallpaperSource,
    candidate: ProviderCandidate,
    probe: wallpaper_remote_media::RemoteImageProbe,
) -> WallpaperGalleryItem {
    wallpaper_remote_media::register_media_source(source, &probe.final_url, &candidate.source_url);
    let mut digest = Sha256::new();
    digest.update(source.as_str().as_bytes());
    digest.update(candidate.upstream_id.as_bytes());
    digest.update(probe.content_fingerprint.as_bytes());
    let identity = hex::encode(digest.finalize());
    WallpaperGalleryItem {
        id: format!("{}-{}", source.as_str(), &identity[..24]),
        thumb_url: probe.final_url.clone(),
        full_url: probe.final_url,
        kind: "image".into(),
        width: probe.width,
        height: probe.height,
        source: source.as_str().into(),
        username: None,
        post_url: None,
        text_preview: candidate.title,
        likes: None,
        local_path: None,
        prompt: None,
        provenance: WallpaperProvenance {
            source_url: Some(candidate.source_url),
            source_name: Some(candidate.source_name.into()),
            author_name: Some(candidate.author_name),
            author_url: candidate.author_url,
            license: Some(candidate.license),
            license_url: Some(candidate.license_url),
        },
        status_id: None,
        media_index: None,
        media_quality: None,
        media_fingerprint: Some(probe.content_fingerprint),
    }
}

fn opaque_id(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests;
