//! Wallpaper X search router: stable CLI default plus an opt-in Responses
//! preview, bounded in-memory cache, progress events, and cooperative cancel.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::Emitter;

use crate::account::{self, BuildOauthCredentialRevision};
use crate::store;
use crate::wallpaper_source::{
    self, WallpaperCliSearchOutcome, WallpaperSearchCancellation, WallpaperSearchMeta,
    WallpaperSearchResult, WallpaperXSearchBatch, WallpaperXSearchRuntime, WallpaperXSearchStage,
};
use crate::wallpaper_x_responses::{
    self, ResponsesSearchError, ResponsesSearchErrorKind, ResponsesSearchSuccess,
};

mod more;
pub(crate) use more::search_more;

const CIRCUIT_FAILURE_THRESHOLD: u8 = 3;
const CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(10 * 60);
const CIRCUIT_OPEN_REASON: &str = "responses_circuit_open";
const CACHE_CAPACITY: usize = 32;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_CONTRACT_VERSION: u8 = 2;
const PRE_CANCEL_TTL: Duration = Duration::from_secs(30);
const PRE_CANCEL_CAPACITY: usize = 64;
pub(crate) const WALLPAPER_X_SEARCH_PROGRESS_EVENT: &str = "wallpaper://x-search-progress";
pub(crate) const WALLPAPER_X_SEARCH_BATCH_EVENT: &str = "wallpaper://x-search-batch";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WallpaperXSearchProgress {
    request_id: String,
    stage: WallpaperXSearchStage,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WallpaperXSearchBatchEvent {
    request_id: String,
    batch_index: usize,
    items: Vec<wallpaper_source::WallpaperGalleryItem>,
    accumulated_count: usize,
    done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SearchCacheKey {
    query: String,
    sort: String,
    requested_mode: String,
    contract_version: u8,
    credential_revision: Option<BuildOauthCredentialRevision>,
}

#[derive(Clone)]
struct SearchCacheEntry {
    inserted_at: Instant,
    result: WallpaperSearchResult,
    responses_continuation: ResponsesContinuationState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponsesContinuationState {
    Unavailable,
    Available,
    InFlight(u64),
    Consumed,
}

struct SearchCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<SearchCacheKey, SearchCacheEntry>,
    lru: VecDeque<SearchCacheKey>,
    next_continuation_lease: u64,
}

struct CachedSearchContext<'a> {
    request_id: &'a str,
    query: &'a str,
    sort: Option<&'a str>,
    requested_mode: &'a str,
    credential_revision: Option<BuildOauthCredentialRevision>,
    cache: &'a Mutex<SearchCache>,
    circuit: &'a Mutex<ResponsesCircuitBreaker>,
    runtime: &'a WallpaperXSearchRuntime,
}

impl SearchCache {
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl,
            entries: HashMap::new(),
            lru: VecDeque::new(),
            next_continuation_lease: 0,
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.entries.retain(|_, entry| {
            matches!(
                entry.responses_continuation,
                ResponsesContinuationState::InFlight(_)
            ) || now.duration_since(entry.inserted_at) < ttl
        });
        self.lru.retain(|key| self.entries.contains_key(key));
    }

    fn get(&mut self, key: &SearchCacheKey, now: Instant) -> Option<WallpaperSearchResult> {
        self.purge_expired(now);
        let entry = self.entries.get(key)?;
        let continuation_available =
            entry.responses_continuation == ResponsesContinuationState::Available;
        let mut result = entry.result.clone();
        if let Some(meta) = result.meta.as_mut() {
            meta.continuation_available = continuation_available;
        }
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.clone());
        Some(result)
    }

    fn insert(&mut self, key: SearchCacheKey, result: WallpaperSearchResult, now: Instant) -> bool {
        self.purge_expired(now);
        if self.capacity == 0 {
            return false;
        }
        if self.entries.get(&key).is_some_and(|entry| {
            matches!(
                entry.responses_continuation,
                ResponsesContinuationState::InFlight(_)
            )
        }) {
            return false;
        }
        self.entries.remove(&key);
        self.lru.retain(|candidate| candidate != &key);
        while self.entries.len() >= self.capacity {
            let Some(position) = self.lru.iter().position(|candidate| {
                self.entries.get(candidate).is_some_and(|entry| {
                    !matches!(
                        entry.responses_continuation,
                        ResponsesContinuationState::InFlight(_)
                    )
                })
            }) else {
                return false;
            };
            let Some(oldest) = self.lru.remove(position) else {
                return false;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key.clone(),
            SearchCacheEntry {
                inserted_at: now,
                responses_continuation: if is_responses_context(&result) {
                    ResponsesContinuationState::Available
                } else {
                    ResponsesContinuationState::Unavailable
                },
                result,
            },
        );
        self.lru.push_back(key);
        true
    }

    fn claim_responses_continuation(
        &mut self,
        key: &SearchCacheKey,
        now: Instant,
    ) -> Option<(u64, WallpaperSearchResult)> {
        self.purge_expired(now);
        let entry = self.entries.get(key)?;
        if entry.responses_continuation != ResponsesContinuationState::Available
            || !is_responses_context(&entry.result)
        {
            return None;
        }

        self.next_continuation_lease = self.next_continuation_lease.wrapping_add(1).max(1);
        let lease_id = self.next_continuation_lease;
        let entry = self.entries.get_mut(key)?;
        entry.responses_continuation = ResponsesContinuationState::InFlight(lease_id);
        let result = entry.result.clone();
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.clone());
        Some((lease_id, result))
    }

    fn resolve_responses_continuation(
        &mut self,
        key: &SearchCacheKey,
        lease_id: u64,
        consumed: bool,
        now: Instant,
    ) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        if entry.responses_continuation != ResponsesContinuationState::InFlight(lease_id) {
            return;
        }
        entry.responses_continuation = if consumed {
            ResponsesContinuationState::Consumed
        } else {
            entry.inserted_at = now;
            ResponsesContinuationState::Available
        };
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.clone());
    }
}

struct ResponsesContinuationLease<'a> {
    cache: &'a Mutex<SearchCache>,
    key: SearchCacheKey,
    lease_id: u64,
    previous: WallpaperSearchResult,
    resolved: bool,
}

impl<'a> ResponsesContinuationLease<'a> {
    fn claim(cache: &'a Mutex<SearchCache>, key: SearchCacheKey, now: Instant) -> Option<Self> {
        let (lease_id, previous) = cache.lock().claim_responses_continuation(&key, now)?;
        Some(Self {
            cache,
            key,
            lease_id,
            previous,
            resolved: false,
        })
    }

    fn previous(&self) -> &WallpaperSearchResult {
        &self.previous
    }

    fn consume_if_active(mut self, runtime: &WallpaperXSearchRuntime) -> bool {
        let consumed = runtime
            .cancellation()
            .commit_if_active(|| {
                self.cache.lock().resolve_responses_continuation(
                    &self.key,
                    self.lease_id,
                    true,
                    Instant::now(),
                );
            })
            .is_some();
        if !consumed {
            self.cache.lock().resolve_responses_continuation(
                &self.key,
                self.lease_id,
                false,
                Instant::now(),
            );
        }
        self.resolved = true;
        consumed
    }
}

impl Drop for ResponsesContinuationLease<'_> {
    fn drop(&mut self) {
        if self.resolved {
            return;
        }
        self.cache.lock().resolve_responses_continuation(
            &self.key,
            self.lease_id,
            false,
            Instant::now(),
        );
    }
}

fn response_cache() -> &'static Mutex<SearchCache> {
    static CACHE: OnceLock<Mutex<SearchCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(SearchCache::new(CACHE_CAPACITY, CACHE_TTL)))
}

#[derive(Default)]
struct RequestRegistry {
    active: HashMap<String, WallpaperSearchCancellation>,
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

fn active_requests() -> &'static Mutex<RequestRegistry> {
    static REQUESTS: OnceLock<Mutex<RequestRegistry>> = OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(RequestRegistry::default()))
}

struct ActiveRequestGuard {
    request_id: String,
    cancellation: WallpaperSearchCancellation,
}

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        active_requests().lock().active.remove(&self.request_id);
    }
}

fn register_request(request_id: &str) -> Result<ActiveRequestGuard, String> {
    let cancellation = WallpaperSearchCancellation::default();
    let mut requests = active_requests().lock();
    if requests.active.contains_key(request_id) {
        return Err("wallpaper_request_in_use".into());
    }
    if requests.take_pre_cancel(request_id, Instant::now()) {
        cancellation.cancel();
    }
    requests
        .active
        .insert(request_id.to_string(), cancellation.clone());
    Ok(ActiveRequestGuard {
        request_id: request_id.to_string(),
        cancellation,
    })
}

pub(crate) fn request_id(raw: Option<&str>) -> Result<String, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(uuid::Uuid::new_v4().to_string());
    };
    uuid::Uuid::parse_str(raw).map_err(|_| "invalid_wallpaper_request_id".to_string())?;
    Ok(raw.to_string())
}

pub(crate) fn cancel(request_id: &str) -> bool {
    let mut requests = active_requests().lock();
    let cancellation = requests.active.get(request_id).cloned();
    if let Some(cancellation) = cancellation {
        drop(requests);
        cancellation.cancel();
        true
    } else {
        // IPC can deliver cancel just before the async search command reaches
        // registration. Keep a short, bounded tombstone so close/switch still
        // prevents that request from starting network or CLI work.
        requests.record_pre_cancel(request_id, Instant::now());
        true
    }
}

#[derive(Default)]
struct ResponsesCircuitBreaker {
    credential_revision: Option<BuildOauthCredentialRevision>,
    consecutive_failures: u8,
    open_until: Option<Instant>,
}

impl ResponsesCircuitBreaker {
    fn reset_for_network_change(&mut self) {
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    fn reset_for_revision(&mut self, revision: Option<BuildOauthCredentialRevision>) {
        if self.credential_revision == revision {
            return;
        }
        self.credential_revision = revision;
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    fn allows_attempt(
        &mut self,
        revision: Option<BuildOauthCredentialRevision>,
        now: Instant,
    ) -> bool {
        self.reset_for_revision(revision);
        match self.open_until {
            Some(until) if now < until => false,
            Some(_) => {
                self.open_until = None;
                self.consecutive_failures = 0;
                true
            }
            None => true,
        }
    }

    fn record_success(&mut self, revision: BuildOauthCredentialRevision) {
        self.credential_revision = Some(revision);
        self.consecutive_failures = 0;
        self.open_until = None;
    }

    fn record_failure(
        &mut self,
        kind: ResponsesSearchErrorKind,
        revision: Option<BuildOauthCredentialRevision>,
        now: Instant,
    ) {
        self.reset_for_revision(revision);
        if kind == ResponsesSearchErrorKind::BadRequest {
            self.consecutive_failures = CIRCUIT_FAILURE_THRESHOLD;
            self.open_until = Some(now + CIRCUIT_OPEN_DURATION);
            return;
        }
        if !counts_toward_circuit(kind) {
            return;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= CIRCUIT_FAILURE_THRESHOLD {
            self.open_until = Some(now + CIRCUIT_OPEN_DURATION);
        }
    }
}

fn responses_circuit() -> &'static Mutex<ResponsesCircuitBreaker> {
    static CIRCUIT: OnceLock<Mutex<ResponsesCircuitBreaker>> = OnceLock::new();
    CIRCUIT.get_or_init(|| Mutex::new(ResponsesCircuitBreaker::default()))
}

pub(crate) fn reset_responses_circuit_for_network_change() {
    responses_circuit().lock().reset_for_network_change();
}

fn counts_toward_circuit(kind: ResponsesSearchErrorKind) -> bool {
    matches!(
        kind,
        ResponsesSearchErrorKind::ServerError
            | ResponsesSearchErrorKind::Timeout
            | ResponsesSearchErrorKind::Tls
            | ResponsesSearchErrorKind::Network
            | ResponsesSearchErrorKind::Empty
            | ResponsesSearchErrorKind::InvalidJson
            | ResponsesSearchErrorKind::Protocol
            | ResponsesSearchErrorKind::ToolNotCalled
            | ResponsesSearchErrorKind::SearchBudgetExceeded
    )
}

fn should_fallback(kind: ResponsesSearchErrorKind) -> bool {
    !matches!(
        kind,
        ResponsesSearchErrorKind::RateLimited
            | ResponsesSearchErrorKind::SearchBudgetExceeded
            | ResponsesSearchErrorKind::Cancelled
    )
}

pub(crate) async fn search(
    app: &tauri::AppHandle,
    request_id: &str,
    query: &str,
    sort: Option<&str>,
) -> Result<WallpaperSearchResult, String> {
    let request = register_request(request_id)?;
    let event_app = app.clone();
    let event_request_id = request_id.to_string();
    let progress = Arc::new(move |stage| {
        let _ = event_app.emit(
            WALLPAPER_X_SEARCH_PROGRESS_EVENT,
            WallpaperXSearchProgress {
                request_id: event_request_id.clone(),
                stage,
            },
        );
    });
    let batch_app = app.clone();
    let batch_request_id = request_id.to_string();
    let batch = Arc::new(move |batch: WallpaperXSearchBatch| {
        let _ = batch_app.emit(
            WALLPAPER_X_SEARCH_BATCH_EVENT,
            WallpaperXSearchBatchEvent {
                request_id: batch_request_id.clone(),
                batch_index: batch.batch_index,
                items: batch.items,
                accumulated_count: batch.accumulated_count,
                done: batch.done,
            },
        );
    });
    let runtime = WallpaperXSearchRuntime::new(request.cancellation.clone(), progress, batch);
    let settings = store::load_settings();
    let requested_mode =
        store::normalize_wallpaper_x_search_mode(&settings.wallpaper_x_search_mode);
    let credential_revision = account::build_oauth_credential_revision();

    Ok(search_with_cache_and_providers(
        CachedSearchContext {
            request_id,
            query,
            sort,
            requested_mode,
            credential_revision,
            cache: response_cache(),
            circuit: responses_circuit(),
            runtime: &runtime,
        },
        || wallpaper_x_responses::search(query, sort, &runtime),
        || wallpaper_source::x_search_cli_outcome(query, sort, Some(&runtime)),
    )
    .await)
}

fn search_cache_key(
    query: &str,
    sort: Option<&str>,
    requested_mode: &str,
    credential_revision: Option<&BuildOauthCredentialRevision>,
) -> SearchCacheKey {
    let requested_mode = store::normalize_wallpaper_x_search_mode(requested_mode);
    let credential_revision = (requested_mode == store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW)
        .then_some(credential_revision)
        .flatten();
    SearchCacheKey {
        query: query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        sort: match sort.unwrap_or("top") {
            "latest" | "Latest" => "latest".into(),
            _ => "top".into(),
        },
        requested_mode: requested_mode.into(),
        contract_version: CACHE_CONTRACT_VERSION,
        credential_revision: credential_revision.cloned(),
    }
}

fn prepare_cached_result(
    mut result: WallpaperSearchResult,
    request_id: &str,
    started: Instant,
) -> WallpaperSearchResult {
    if let Some(meta) = result.meta.as_mut() {
        meta.request_id = Some(request_id.to_string());
        meta.cache_hit = true;
        meta.duration_ms = elapsed_ms(started);
        // Provider timings belong to the original request that populated the
        // cache, not this lookup. Do not present stale work as a cache cost.
        meta.responses_duration_ms = None;
        meta.cli_duration_ms = None;
    }
    result
}

fn prepare_cache_entry(mut result: WallpaperSearchResult) -> WallpaperSearchResult {
    if let Some(meta) = result.meta.as_mut() {
        meta.request_id = None;
        meta.cache_hit = false;
    }
    result
}

fn is_responses_context(result: &WallpaperSearchResult) -> bool {
    result.error_code.is_none()
        && !result.items.is_empty()
        && result
            .meta
            .as_ref()
            .is_some_and(|meta| meta.route_used == "responses")
}

async fn search_with_cache_and_providers<R, RFut, C, CFut>(
    context: CachedSearchContext<'_>,
    responses: R,
    cli: C,
) -> WallpaperSearchResult
where
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result<ResponsesSearchSuccess, ResponsesSearchError>>,
    C: FnOnce() -> CFut,
    CFut: Future<Output = WallpaperCliSearchOutcome>,
{
    let CachedSearchContext {
        request_id,
        query,
        sort,
        requested_mode,
        credential_revision,
        cache,
        circuit,
        runtime,
    } = context;
    let started = Instant::now();
    runtime.report(WallpaperXSearchStage::Preparing);
    if runtime.is_cancelled() {
        runtime.report(WallpaperXSearchStage::Done);
        return cancelled_result(requested_mode, "cli", started, None, None);
    }
    let key = search_cache_key(query, sort, requested_mode, credential_revision.as_ref());
    if let Some(result) = cache.lock().get(&key, Instant::now()) {
        if runtime.is_cancelled() {
            runtime.report(WallpaperXSearchStage::Done);
            return cancelled_result(requested_mode, "cli", started, None, None);
        }
        let result = prepare_cached_result(result, request_id, started);
        report_terminal_responses_batch(runtime, &result);
        runtime.report(WallpaperXSearchStage::Done);
        return result;
    }
    if runtime.is_cancelled() {
        runtime.report(WallpaperXSearchStage::Done);
        return cancelled_result(requested_mode, "cli", started, None, None);
    }

    let mut result = route_with_providers(
        requested_mode,
        credential_revision,
        circuit,
        runtime,
        responses,
        cli,
    )
    .await;
    if let Some(meta) = result.meta.as_mut() {
        meta.request_id = Some(request_id.to_string());
    }
    if result.error_code.is_none() && !result.items.is_empty() {
        let inserted = runtime.cancellation().commit_if_active(|| {
            cache
                .lock()
                .insert(key, prepare_cache_entry(result.clone()), Instant::now())
        });
        match inserted {
            Some(inserted) => {
                if let Some(meta) = result.meta.as_mut() {
                    meta.continuation_available &= inserted;
                }
            }
            None => {
                let meta = result.meta.as_ref();
                let route_used = meta.map_or("cli", |meta| meta.route_used.as_str());
                let responses_duration_ms = meta.and_then(|meta| meta.responses_duration_ms);
                let cli_duration_ms = meta.and_then(|meta| meta.cli_duration_ms);
                result = cancelled_result(
                    requested_mode,
                    route_used,
                    started,
                    responses_duration_ms,
                    cli_duration_ms,
                );
            }
        }
    }
    runtime.report(WallpaperXSearchStage::Done);
    result
}

fn report_terminal_responses_batch(
    runtime: &WallpaperXSearchRuntime,
    result: &WallpaperSearchResult,
) {
    let is_responses = result
        .meta
        .as_ref()
        .is_some_and(|meta| meta.route_used == "responses");
    if !is_responses || result.error_code.is_some() || result.items.is_empty() {
        return;
    }
    runtime.report_batch(WallpaperXSearchBatch {
        batch_index: 1,
        items: result.items.clone(),
        accumulated_count: result.items.len(),
        done: true,
    });
}

async fn route_with_providers<R, RFut, C, CFut>(
    requested_mode: &str,
    credential_revision: Option<BuildOauthCredentialRevision>,
    circuit: &Mutex<ResponsesCircuitBreaker>,
    runtime: &WallpaperXSearchRuntime,
    responses: R,
    cli: C,
) -> WallpaperSearchResult
where
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result<ResponsesSearchSuccess, ResponsesSearchError>>,
    C: FnOnce() -> CFut,
    CFut: Future<Output = WallpaperCliSearchOutcome>,
{
    let started = Instant::now();
    let requested_mode = store::normalize_wallpaper_x_search_mode(requested_mode);

    // `auto` stays protocol-reserved until QA explicitly enables it. Both it
    // and the public default execute the established CLI route.
    if requested_mode != store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW {
        runtime.report(WallpaperXSearchStage::SearchingX);
        return run_cli(cli, requested_mode, None, started, None).await;
    }

    if !circuit
        .lock()
        .allows_attempt(credential_revision.clone(), Instant::now())
    {
        runtime.report(WallpaperXSearchStage::FallingBack);
        return run_cli(
            cli,
            requested_mode,
            Some(CIRCUIT_OPEN_REASON),
            started,
            None,
        )
        .await;
    }

    runtime.report(WallpaperXSearchStage::SearchingX);
    let responses_started = Instant::now();
    let responses_result = responses().await;
    let responses_duration_ms = elapsed_ms(responses_started);
    match responses_result {
        Ok(result) if !result.items.is_empty() && result.valid_count > 0 => {
            if runtime.is_cancelled() {
                return cancelled_result(
                    requested_mode,
                    "responses",
                    started,
                    Some(responses_duration_ms),
                    None,
                );
            }
            circuit
                .lock()
                .record_success(result.credential_revision.clone());
            WallpaperSearchResult {
                items: result.items,
                error_code: None,
                message: None,
                meta: Some(WallpaperSearchMeta {
                    request_id: None,
                    requested_mode: requested_mode.into(),
                    route_used: "responses".into(),
                    fallback_reason: None,
                    duration_ms: elapsed_ms(started),
                    responses_duration_ms: Some(responses_duration_ms),
                    cli_duration_ms: None,
                    cache_hit: false,
                    continuation_available: true,
                    search_calls: Some(result.search_calls),
                    candidate_count: result.candidate_count,
                    valid_count: result.valid_count,
                    model: Some(result.model.into()),
                    effort: Some(result.effort.into()),
                }),
            }
        }
        Ok(result) => {
            if runtime.is_cancelled() {
                return cancelled_result(
                    requested_mode,
                    "responses",
                    started,
                    Some(responses_duration_ms),
                    None,
                );
            }
            circuit.lock().record_failure(
                ResponsesSearchErrorKind::Empty,
                Some(result.credential_revision),
                Instant::now(),
            );
            runtime.report(WallpaperXSearchStage::FallingBack);
            if runtime.is_cancelled() {
                return cancelled_result(
                    requested_mode,
                    "responses",
                    started,
                    Some(responses_duration_ms),
                    None,
                );
            }
            run_cli(
                cli,
                requested_mode,
                Some(ResponsesSearchErrorKind::Empty.code()),
                started,
                Some(responses_duration_ms),
            )
            .await
        }
        Err(error) if error.kind == ResponsesSearchErrorKind::Cancelled => cancelled_result(
            requested_mode,
            "responses",
            started,
            Some(responses_duration_ms),
            None,
        ),
        Err(error) if !should_fallback(error.kind) => {
            if counts_toward_circuit(error.kind) {
                circuit.lock().record_failure(
                    error.kind,
                    error.credential_revision.clone().or(credential_revision),
                    Instant::now(),
                );
            }
            WallpaperSearchResult {
                items: Vec::new(),
                error_code: Some(error.code().into()),
                message: None,
                meta: Some(WallpaperSearchMeta {
                    request_id: None,
                    requested_mode: requested_mode.into(),
                    route_used: "responses".into(),
                    fallback_reason: None,
                    duration_ms: elapsed_ms(started),
                    responses_duration_ms: Some(responses_duration_ms),
                    cli_duration_ms: None,
                    cache_hit: false,
                    continuation_available: false,
                    search_calls: None,
                    candidate_count: 0,
                    valid_count: 0,
                    model: Some(wallpaper_x_responses::RESPONSES_MODEL.into()),
                    effort: Some(wallpaper_x_responses::RESPONSES_EFFORT.into()),
                }),
            }
        }
        Err(error) => {
            circuit.lock().record_failure(
                error.kind,
                error.credential_revision.clone().or(credential_revision),
                Instant::now(),
            );
            runtime.report(WallpaperXSearchStage::FallingBack);
            if runtime.is_cancelled() {
                return cancelled_result(
                    requested_mode,
                    "responses",
                    started,
                    Some(responses_duration_ms),
                    None,
                );
            }
            run_cli(
                cli,
                requested_mode,
                Some(error.code()),
                started,
                Some(responses_duration_ms),
            )
            .await
        }
    }
}

async fn run_cli<C, CFut>(
    cli: C,
    requested_mode: &str,
    fallback_reason: Option<&str>,
    started: Instant,
    responses_duration_ms: Option<u64>,
) -> WallpaperSearchResult
where
    C: FnOnce() -> CFut,
    CFut: Future<Output = WallpaperCliSearchOutcome>,
{
    let cli_started = Instant::now();
    let outcome = cli().await;
    finish_cli(
        outcome,
        requested_mode,
        fallback_reason,
        started,
        responses_duration_ms,
        elapsed_ms(cli_started),
    )
}

fn finish_cli(
    mut outcome: WallpaperCliSearchOutcome,
    requested_mode: &str,
    fallback_reason: Option<&str>,
    started: Instant,
    responses_duration_ms: Option<u64>,
    cli_duration_ms: u64,
) -> WallpaperSearchResult {
    outcome.result.meta = Some(WallpaperSearchMeta {
        request_id: None,
        requested_mode: requested_mode.into(),
        route_used: "cli".into(),
        fallback_reason: fallback_reason.map(str::to_string),
        duration_ms: elapsed_ms(started),
        responses_duration_ms,
        cli_duration_ms: Some(cli_duration_ms),
        cache_hit: false,
        continuation_available: false,
        // Grok Build does not currently expose a reliable hosted X call count
        // or selected official model through this headless result contract.
        search_calls: None,
        candidate_count: outcome.candidate_count,
        valid_count: outcome.valid_count,
        model: None,
        effort: Some("low".into()),
    });
    outcome.result
}

fn cancelled_result(
    requested_mode: &str,
    route_used: &str,
    started: Instant,
    responses_duration_ms: Option<u64>,
    cli_duration_ms: Option<u64>,
) -> WallpaperSearchResult {
    WallpaperSearchResult {
        items: Vec::new(),
        error_code: Some("cancelled".into()),
        message: None,
        meta: Some(WallpaperSearchMeta {
            request_id: None,
            requested_mode: store::normalize_wallpaper_x_search_mode(requested_mode).into(),
            route_used: route_used.into(),
            fallback_reason: None,
            duration_ms: elapsed_ms(started),
            responses_duration_ms,
            cli_duration_ms,
            cache_hit: false,
            continuation_available: false,
            search_calls: None,
            candidate_count: 0,
            valid_count: 0,
            model: (route_used == "responses")
                .then(|| wallpaper_x_responses::RESPONSES_MODEL.into()),
            effort: Some("low".into()),
        }),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
