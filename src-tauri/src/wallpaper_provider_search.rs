//! Fixed-endpoint Openverse and Pexels wallpaper-library adapters.
//!
//! Provider credentials, paging cursors, raw responses, and query strings stay
//! inside the Host. Only validated image DTOs cross IPC.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use hyper::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use url::Url;

use crate::safe_https_client::{self, SafeHttpsClient, SafeHttpsError, SafeHttpsErrorKind};
use crate::skin_net::{self, OriginPolicy};
use crate::wallpaper_remote_media::{self, RemoteImageProber};
use crate::wallpaper_remote_search::{
    self as remote_search, RemoteSearchBatch, RemoteSearchResult, RemoteSearchRuntime,
    RemoteSearchStage, RemoteWallpaperSource,
};
use crate::wallpaper_source::{WallpaperGalleryItem, WallpaperSearchCancellation};

mod request_url;
mod response;

use request_url::provider_request_url;
#[cfg(test)]
use request_url::{provider_url, PEXELS_CACHE_BUST_PARAM};
use response::{parse_api_page, provider_item};
#[cfg(test)]
use response::{parse_openverse_page, parse_pexels_page, safe_url};

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
const PRE_CANCEL_TTL: Duration = Duration::from_secs(30);
const PRE_CANCEL_CAPACITY: usize = 64;
const MAX_EMPTY_VALIDATED_BATCHES: usize = 3;

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
    continuation: Option<ProviderContinuation>,
}

#[derive(Clone)]
struct ContinuationEntry {
    key: SearchKey,
    inserted: Instant,
    continuation: ProviderContinuation,
}

#[derive(Clone)]
struct ProviderContinuation {
    next_page: usize,
    buffered_items: Vec<WallpaperGalleryItem>,
    upstream_has_more: bool,
}

impl ProviderContinuation {
    fn is_available(&self) -> bool {
        self.upstream_has_more || !self.buffered_items.is_empty()
    }
}

#[derive(Default)]
struct SearchCache {
    entries: VecDeque<CacheEntry>,
    continuations: VecDeque<ContinuationEntry>,
}

impl SearchCache {
    fn purge_expired(&mut self, now: Instant) {
        self.entries
            .retain(|entry| now.duration_since(entry.inserted) <= CACHE_TTL);
        self.continuations
            .retain(|entry| now.duration_since(entry.inserted) <= CACHE_TTL);
    }

    fn get(
        &mut self,
        key: &PageKey,
        now: Instant,
    ) -> Option<(RemoteSearchResult, Option<ProviderContinuation>)> {
        self.purge_expired(now);
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index)?;
        let result = entry.result.clone();
        let continuation = entry.continuation.clone();
        self.entries.push_front(entry);
        Some((result, continuation))
    }

    fn insert(
        &mut self,
        key: PageKey,
        result: RemoteSearchResult,
        continuation: Option<ProviderContinuation>,
        now: Instant,
    ) {
        self.purge_expired(now);
        self.entries.retain(|entry| entry.key != key);
        self.entries.push_front(CacheEntry {
            key,
            inserted: now,
            result,
            continuation,
        });
        self.entries.truncate(CACHE_CAPACITY);
    }

    fn continuation(&mut self, key: &SearchKey, now: Instant) -> Option<ProviderContinuation> {
        self.purge_expired(now);
        let index = self
            .continuations
            .iter()
            .position(|entry| &entry.key == key)?;
        let entry = self.continuations.remove(index)?;
        let continuation = entry.continuation.clone();
        self.continuations.push_front(entry);
        Some(continuation)
    }

    fn update_continuation(
        &mut self,
        key: &SearchKey,
        continuation: Option<ProviderContinuation>,
        now: Instant,
    ) {
        self.purge_expired(now);
        self.continuations.retain(|entry| &entry.key != key);
        if let Some(continuation) = continuation.filter(ProviderContinuation::is_available) {
            self.continuations.push_front(ContinuationEntry {
                key: key.clone(),
                inserted: now,
                continuation,
            });
            self.continuations.truncate(CACHE_CAPACITY);
        }
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

#[derive(Default)]
struct RequestRegistry {
    active: HashMap<String, ActiveRequest>,
    pre_cancelled: VecDeque<(String, Instant)>,
}

impl RequestRegistry {
    fn purge_pre_cancelled(&mut self, now: Instant) {
        self.pre_cancelled
            .retain(|(_, created)| now.duration_since(*created) < PRE_CANCEL_TTL);
    }

    fn record_pre_cancel(&mut self, request_id: &str, now: Instant) {
        self.purge_pre_cancelled(now);
        self.pre_cancelled.retain(|(id, _)| id != request_id);
        while self.pre_cancelled.len() >= PRE_CANCEL_CAPACITY {
            self.pre_cancelled.pop_front();
        }
        self.pre_cancelled.push_back((request_id.to_string(), now));
    }

    fn take_pre_cancel(&mut self, request_id: &str, now: Instant) -> bool {
        self.purge_pre_cancelled(now);
        let found = self.pre_cancelled.iter().any(|(id, _)| id == request_id);
        self.pre_cancelled.retain(|(id, _)| id != request_id);
        found
    }
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
    buffered_items: Vec<WallpaperGalleryItem>,
    upstream_has_more: bool,
    next_page: usize,
}

impl ValidatedPage {
    fn continuation(&self) -> Option<ProviderContinuation> {
        let continuation = ProviderContinuation {
            next_page: self.next_page,
            buffered_items: self.buffered_items.clone(),
            upstream_has_more: self.upstream_has_more,
        };
        continuation.is_available().then_some(continuation)
    }

    fn has_more(&self) -> bool {
        self.upstream_has_more || !self.buffered_items.is_empty()
    }
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

static ACTIVE: OnceLock<Mutex<RequestRegistry>> = OnceLock::new();
static CACHE: OnceLock<Mutex<SearchCache>> = OnceLock::new();

fn active() -> &'static Mutex<RequestRegistry> {
    ACTIVE.get_or_init(|| Mutex::new(RequestRegistry::default()))
}

fn cache() -> &'static Mutex<SearchCache> {
    CACHE.get_or_init(|| Mutex::new(SearchCache::default()))
}

fn begin_request(request_id: &str) -> (uuid::Uuid, WallpaperSearchCancellation) {
    let mut requests = active().lock();
    for (_, request) in requests.active.drain() {
        request.cancellation.cancel();
    }
    let token = uuid::Uuid::new_v4();
    let cancellation = WallpaperSearchCancellation::default();
    if requests.take_pre_cancel(request_id, Instant::now()) {
        cancellation.cancel();
    }
    requests.active.insert(
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
        .active
        .get(request_id)
        .is_some_and(|request| request.token == token)
    {
        requests.active.remove(request_id);
    }
}

pub(crate) fn cancel(request_id: &str) -> bool {
    let mut requests = active().lock();
    let request = requests.active.remove(request_id);
    if let Some(request) = request {
        drop(requests);
        request.cancellation.cancel();
    } else {
        requests.record_pre_cancel(request_id, Instant::now());
    }
    true
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

    if runtime.is_cancelled() {
        finish_request(request_id, token);
        return Ok(RemoteSearchResult::error(
            source.as_str(),
            "cancelled",
            elapsed_ms(started),
        ));
    }
    let cached = { cache().lock().get(&page_key, Instant::now()) };
    if let Some((mut result, continuation)) = cached {
        result.cache_hit = true;
        result.duration_ms = elapsed_ms(started);
        cache()
            .lock()
            .update_continuation(&context.search_key, continuation, Instant::now());
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
        let continuation = page.continuation();
        cache().lock().update_continuation(
            &context.search_key,
            continuation.clone(),
            Instant::now(),
        );
        if !page.items.is_empty() {
            cache()
                .lock()
                .insert(page_key, result.clone(), continuation, Instant::now());
        }
    } else {
        cache()
            .lock()
            .update_continuation(&context.search_key, None, Instant::now());
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
    let (token, cancellation) = begin_request(request_id);
    let runtime = remote_search::runtime(app, request_id, source, cancellation);
    runtime.report(RemoteSearchStage::LoadingMore);
    if runtime.is_cancelled() {
        finish_request(request_id, token);
        return Ok(RemoteSearchResult::error(
            source.as_str(),
            "cancelled",
            elapsed_ms(started),
        ));
    }
    let continuation = {
        cache()
            .lock()
            .continuation(&context.search_key, Instant::now())
    };
    let Some(continuation) = continuation else {
        finish_request(request_id, token);
        return Ok(RemoteSearchResult::error(
            source.as_str(),
            "provider_no_continuation",
            elapsed_ms(started),
        ));
    };
    if let Some((items, next_continuation)) = take_buffered_page(continuation.clone()) {
        let mut result = RemoteSearchResult::success(
            source.as_str(),
            items,
            next_continuation.is_some(),
            elapsed_ms(started),
        );
        result.cache_hit = true;
        cache()
            .lock()
            .update_continuation(&context.search_key, next_continuation, Instant::now());
        emit_cached_batch(&runtime, &result);
        runtime.report(RemoteSearchStage::Done);
        finish_request(request_id, token);
        return Ok(result);
    }
    let first_page = continuation.next_page;
    let page_key = PageKey {
        search: context.search_key.clone(),
        first_page,
    };

    let cached = { cache().lock().get(&page_key, Instant::now()) };
    if let Some((mut result, continuation)) = cached {
        result.cache_hit = true;
        result.duration_ms = elapsed_ms(started);
        cache()
            .lock()
            .update_continuation(&context.search_key, continuation, Instant::now());
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
        let continuation = page.continuation();
        cache().lock().update_continuation(
            &context.search_key,
            continuation.clone(),
            Instant::now(),
        );
        if !page.items.is_empty() {
            cache()
                .lock()
                .insert(page_key, result.clone(), continuation, Instant::now());
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

fn take_buffered_page(
    mut continuation: ProviderContinuation,
) -> Option<(Vec<WallpaperGalleryItem>, Option<ProviderContinuation>)> {
    if continuation.buffered_items.is_empty() {
        return None;
    }
    let remainder = if continuation.buffered_items.len() > RESULT_LIMIT {
        continuation.buffered_items.split_off(RESULT_LIMIT)
    } else {
        Vec::new()
    };
    let items = std::mem::replace(&mut continuation.buffered_items, remainder);
    let next = continuation.is_available().then_some(continuation);
    Some((items, next))
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
            page.has_more(),
            elapsed_ms(started),
        ),
        Ok(page) => {
            RemoteSearchResult::empty(source.as_str(), page.has_more(), elapsed_ms(started))
        }
        Err(error) => RemoteSearchResult::error(source.as_str(), error.code(), elapsed_ms(started)),
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
    let mut page = first_page;
    for _ in 0..MAX_EMPTY_VALIDATED_BATCHES {
        let batch = fetch_candidate_batch(source, query, page, api_key, runtime).await?;
        runtime.report(RemoteSearchStage::ValidatingImages);
        let mut items = validate_candidates(source, batch.candidates, runtime).await?;
        let buffered_items = if items.len() > RESULT_LIMIT {
            items.split_off(RESULT_LIMIT)
        } else {
            Vec::new()
        };
        let validated = ValidatedPage {
            items,
            buffered_items,
            upstream_has_more: batch.has_more,
            next_page: batch.next_page,
        };
        if !validated.items.is_empty() || !validated.has_more() {
            return Ok(validated);
        }
        page = validated.next_page;
        runtime.report(RemoteSearchStage::LoadingMore);
    }
    Ok(ValidatedPage {
        items: Vec::new(),
        buffered_items: Vec::new(),
        upstream_has_more: true,
        next_page: page,
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
    let client = Arc::new(provider_client());
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

fn provider_client() -> SafeHttpsClient {
    safe_https_client::shared()
}

async fn fetch_api_page(
    client: &SafeHttpsClient,
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
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("GrokApp/WallpaperProviderSearch"),
    );
    if source == RemoteWallpaperSource::Pexels {
        let key = api_key.ok_or(ProviderError::PexelsKeyMissing)?;
        let mut value = HeaderValue::from_str(key).map_err(|_| ProviderError::PexelsKeyInvalid)?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
        response = client.get(&checked, headers, HTTP_TIMEOUT) => {
            response.map_err(|error| classify_transport_error(&error))?
        },
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
    mut response: crate::safe_https_client::SafeHttpsResponse,
    cancellation: &WallpaperSearchCancellation,
) -> Result<Vec<u8>, ProviderError> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            chunk = response.chunk() => chunk.map_err(|error| classify_transport_error(&error))?,
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

fn classify_transport_error(error: &SafeHttpsError) -> ProviderError {
    match error.kind() {
        SafeHttpsErrorKind::Timeout => ProviderError::Timeout,
        SafeHttpsErrorKind::Blocked | SafeHttpsErrorKind::Network => ProviderError::Network,
    }
}

async fn validate_candidates(
    source: RemoteWallpaperSource,
    candidates: Vec<ProviderCandidate>,
    runtime: &RemoteSearchRuntime,
) -> Result<Vec<WallpaperGalleryItem>, ProviderError> {
    let prober = Arc::new(RemoteImageProber::new());
    let cancellation = runtime.cancellation().clone();
    let mut probes = buffered_in_input_order(candidates, MAX_CONCURRENT_PROBES, |candidate| {
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
    });

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
        if items.len() < RESULT_LIMIT {
            pending.push(item.clone());
        }
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
    }
    runtime.report_batch(RemoteSearchBatch {
        batch_index,
        items: pending,
        accumulated_count: items.len().min(RESULT_LIMIT),
        done: true,
    });
    Ok(items)
}

fn buffered_in_input_order<I, F, Fut>(
    inputs: I,
    concurrency: usize,
    probe: F,
) -> impl futures_util::Stream<Item = Fut::Output>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Fut,
    Fut: std::future::Future,
{
    futures_util::stream::iter(inputs)
        .map(probe)
        .buffered(concurrency.max(1))
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests;
