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
    WallpaperSearchResult, WallpaperXSearchRuntime, WallpaperXSearchStage,
};
use crate::wallpaper_x_responses::{
    self, ResponsesSearchError, ResponsesSearchErrorKind, ResponsesSearchSuccess,
};

const CIRCUIT_FAILURE_THRESHOLD: u8 = 3;
const CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(10 * 60);
const CIRCUIT_OPEN_REASON: &str = "responses_circuit_open";
const CACHE_CAPACITY: usize = 32;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_CONTRACT_VERSION: u8 = 1;
const PRE_CANCEL_TTL: Duration = Duration::from_secs(30);
const PRE_CANCEL_CAPACITY: usize = 64;
pub(crate) const WALLPAPER_X_SEARCH_PROGRESS_EVENT: &str = "wallpaper://x-search-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WallpaperXSearchProgress {
    request_id: String,
    stage: WallpaperXSearchStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SearchCacheKey {
    query: String,
    sort: String,
    requested_mode: String,
    contract_version: u8,
    credential_file_len: Option<u64>,
    credential_modified_ms: Option<u128>,
}

#[derive(Clone)]
struct SearchCacheEntry {
    inserted_at: Instant,
    result: WallpaperSearchResult,
}

struct SearchCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<SearchCacheKey, SearchCacheEntry>,
    lru: VecDeque<SearchCacheKey>,
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
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, entry| now.duration_since(entry.inserted_at) < ttl);
        self.lru.retain(|key| self.entries.contains_key(key));
    }

    fn get(&mut self, key: &SearchCacheKey, now: Instant) -> Option<WallpaperSearchResult> {
        self.purge_expired(now);
        let result = self.entries.get(key)?.result.clone();
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.clone());
        Some(result)
    }

    fn insert(&mut self, key: SearchCacheKey, result: WallpaperSearchResult, now: Instant) {
        self.purge_expired(now);
        self.entries.remove(&key);
        self.lru.retain(|candidate| candidate != &key);
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key.clone(),
            SearchCacheEntry {
                inserted_at: now,
                result,
            },
        );
        self.lru.push_back(key);
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
    let runtime = WallpaperXSearchRuntime::new(request.cancellation.clone(), progress);
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
        credential_file_len: credential_revision.map(|revision| revision.file_len),
        credential_modified_ms: credential_revision.and_then(|revision| revision.modified_ms),
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
    let key = search_cache_key(query, sort, requested_mode, credential_revision.as_ref());
    if let Some(result) = cache.lock().get(&key, Instant::now()) {
        runtime.report(WallpaperXSearchStage::Done);
        return prepare_cached_result(result, request_id, started);
    }
    if runtime.is_cancelled() {
        runtime.report(WallpaperXSearchStage::Done);
        return cancelled_result(requested_mode, "cli", started);
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
    if result.error_code.is_none() && !result.items.is_empty() && !runtime.is_cancelled() {
        cache
            .lock()
            .insert(key, prepare_cache_entry(result.clone()), Instant::now());
    }
    runtime.report(WallpaperXSearchStage::Done);
    result
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
        return finish_cli(cli().await, requested_mode, None, started);
    }

    if !circuit
        .lock()
        .allows_attempt(credential_revision.clone(), Instant::now())
    {
        runtime.report(WallpaperXSearchStage::FallingBack);
        return finish_cli(
            cli().await,
            requested_mode,
            Some(CIRCUIT_OPEN_REASON),
            started,
        );
    }

    runtime.report(WallpaperXSearchStage::SearchingX);
    match responses().await {
        Ok(result) if !result.items.is_empty() && result.valid_count > 0 => {
            if runtime.is_cancelled() {
                return cancelled_result(requested_mode, "responses", started);
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
                    cache_hit: false,
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
                return cancelled_result(requested_mode, "responses", started);
            }
            circuit.lock().record_failure(
                ResponsesSearchErrorKind::Empty,
                Some(result.credential_revision),
                Instant::now(),
            );
            runtime.report(WallpaperXSearchStage::FallingBack);
            if runtime.is_cancelled() {
                return cancelled_result(requested_mode, "responses", started);
            }
            finish_cli(
                cli().await,
                requested_mode,
                Some(ResponsesSearchErrorKind::Empty.code()),
                started,
            )
        }
        Err(error) if error.kind == ResponsesSearchErrorKind::Cancelled => {
            cancelled_result(requested_mode, "responses", started)
        }
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
                    cache_hit: false,
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
                return cancelled_result(requested_mode, "responses", started);
            }
            finish_cli(cli().await, requested_mode, Some(error.code()), started)
        }
    }
}

fn finish_cli(
    mut outcome: WallpaperCliSearchOutcome,
    requested_mode: &str,
    fallback_reason: Option<&str>,
    started: Instant,
) -> WallpaperSearchResult {
    outcome.result.meta = Some(WallpaperSearchMeta {
        request_id: None,
        requested_mode: requested_mode.into(),
        route_used: "cli".into(),
        fallback_reason: fallback_reason.map(str::to_string),
        duration_ms: elapsed_ms(started),
        cache_hit: false,
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
            cache_hit: false,
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
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::wallpaper_source::WallpaperGalleryItem;

    fn revision(value: u64) -> BuildOauthCredentialRevision {
        BuildOauthCredentialRevision {
            file_len: value,
            modified_ms: Some(value.into()),
        }
    }

    fn item(id: &str) -> WallpaperGalleryItem {
        WallpaperGalleryItem {
            id: id.into(),
            thumb_url: format!("https://pbs.twimg.com/media/{id}.jpg?name=orig"),
            full_url: format!("https://pbs.twimg.com/media/{id}.jpg?name=orig"),
            kind: "image".into(),
            width: None,
            height: None,
            source: "x".into(),
            username: None,
            post_url: None,
            text_preview: None,
            likes: None,
            local_path: None,
            prompt: None,
            status_id: None,
            media_index: None,
            media_quality: None,
        }
    }

    fn cli_success() -> WallpaperCliSearchOutcome {
        WallpaperCliSearchOutcome {
            result: WallpaperSearchResult {
                items: vec![item("cli")],
                error_code: None,
                message: None,
                meta: None,
            },
            candidate_count: 2,
            valid_count: 1,
        }
    }

    fn responses_success() -> ResponsesSearchSuccess {
        ResponsesSearchSuccess {
            items: vec![item("responses")],
            candidate_count: 3,
            valid_count: 1,
            search_calls: 2,
            model: wallpaper_x_responses::RESPONSES_MODEL,
            effort: wallpaper_x_responses::RESPONSES_EFFORT,
            credential_revision: revision(1),
        }
    }

    fn responses_error(kind: ResponsesSearchErrorKind) -> ResponsesSearchError {
        ResponsesSearchError {
            kind,
            credential_revision: Some(revision(1)),
        }
    }

    #[tokio::test]
    async fn wallpaper_x_search_cli_default_and_reserved_auto_never_try_responses() {
        for mode in [
            store::WALLPAPER_X_SEARCH_MODE_CLI,
            store::WALLPAPER_X_SEARCH_MODE_AUTO,
        ] {
            let responses_calls = Arc::new(AtomicUsize::new(0));
            let calls = Arc::clone(&responses_calls);
            let result = route_with_providers(
                mode,
                Some(revision(1)),
                &Mutex::new(ResponsesCircuitBreaker::default()),
                &WallpaperXSearchRuntime::quiet(),
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(responses_success())
                },
                || async { cli_success() },
            )
            .await;
            assert_eq!(responses_calls.load(Ordering::SeqCst), 0);
            let meta = result.meta.expect("route meta");
            assert_eq!(meta.requested_mode, mode);
            assert_eq!(meta.route_used, "cli");
            assert_eq!(meta.fallback_reason, None);
        }
    }

    #[tokio::test]
    async fn wallpaper_x_search_preview_reports_actual_responses_route() {
        let result = route_with_providers(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(revision(1)),
            &Mutex::new(ResponsesCircuitBreaker::default()),
            &WallpaperXSearchRuntime::quiet(),
            || async { Ok(responses_success()) },
            || async { cli_success() },
        )
        .await;
        assert_eq!(result.items[0].id, "responses");
        let meta = result.meta.expect("route meta");
        assert_eq!(meta.route_used, "responses");
        assert_eq!(meta.search_calls, Some(2));
        assert_eq!(meta.candidate_count, 3);
        assert_eq!(meta.valid_count, 1);
        assert_eq!(meta.model.as_deref(), Some("grok-4.6"));
        assert_eq!(meta.effort.as_deref(), Some("low"));
    }

    #[tokio::test]
    async fn wallpaper_x_search_preview_falls_back_once_with_stable_reason() {
        for kind in [
            ResponsesSearchErrorKind::OauthUnavailable,
            ResponsesSearchErrorKind::OauthExpired,
            ResponsesSearchErrorKind::Unauthorized,
            ResponsesSearchErrorKind::BadRequest,
            ResponsesSearchErrorKind::ServerError,
            ResponsesSearchErrorKind::Timeout,
            ResponsesSearchErrorKind::InvalidJson,
        ] {
            let cli_calls = Arc::new(AtomicUsize::new(0));
            let calls = Arc::clone(&cli_calls);
            let result = route_with_providers(
                store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
                Some(revision(1)),
                &Mutex::new(ResponsesCircuitBreaker::default()),
                &WallpaperXSearchRuntime::quiet(),
                move || async move { Err(responses_error(kind)) },
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    cli_success()
                },
            )
            .await;
            assert_eq!(cli_calls.load(Ordering::SeqCst), 1);
            assert_eq!(result.items[0].id, "cli");
            let meta = result.meta.expect("route meta");
            assert_eq!(meta.route_used, "cli");
            assert_eq!(meta.fallback_reason.as_deref(), Some(kind.code()));
        }
    }

    #[tokio::test]
    async fn wallpaper_x_search_rate_limit_and_budget_overrun_never_double_spend_on_cli() {
        for kind in [
            ResponsesSearchErrorKind::RateLimited,
            ResponsesSearchErrorKind::SearchBudgetExceeded,
        ] {
            let cli_calls = Arc::new(AtomicUsize::new(0));
            let calls = Arc::clone(&cli_calls);
            let result = route_with_providers(
                store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
                Some(revision(1)),
                &Mutex::new(ResponsesCircuitBreaker::default()),
                &WallpaperXSearchRuntime::quiet(),
                move || async move { Err(responses_error(kind)) },
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    cli_success()
                },
            )
            .await;
            assert_eq!(cli_calls.load(Ordering::SeqCst), 0);
            assert!(result.items.is_empty());
            assert_eq!(result.error_code.as_deref(), Some(kind.code()));
            assert_eq!(result.meta.expect("route meta").route_used, "responses");
        }
    }

    #[tokio::test]
    async fn wallpaper_x_search_cache_hits_without_new_provider_work() {
        let cache = Mutex::new(SearchCache::new(32, Duration::from_secs(600)));
        let circuit = Mutex::new(ResponsesCircuitBreaker::default());
        let cli_calls = Arc::new(AtomicUsize::new(0));

        let first_calls = Arc::clone(&cli_calls);
        let first_runtime = WallpaperXSearchRuntime::quiet();
        let first = search_with_cache_and_providers(
            CachedSearchContext {
                request_id: "request-1",
                query: "  Misty   Mountains ",
                sort: Some("top"),
                requested_mode: store::WALLPAPER_X_SEARCH_MODE_CLI,
                credential_revision: Some(revision(1)),
                cache: &cache,
                circuit: &circuit,
                runtime: &first_runtime,
            },
            || async { Ok(responses_success()) },
            move || async move {
                first_calls.fetch_add(1, Ordering::SeqCst);
                cli_success()
            },
        )
        .await;
        assert!(!first.meta.as_ref().expect("first meta").cache_hit);

        let second_calls = Arc::clone(&cli_calls);
        let second_runtime = WallpaperXSearchRuntime::quiet();
        let second = search_with_cache_and_providers(
            CachedSearchContext {
                request_id: "request-2",
                query: "misty mountains",
                sort: Some("top"),
                requested_mode: store::WALLPAPER_X_SEARCH_MODE_CLI,
                credential_revision: Some(revision(9)),
                cache: &cache,
                circuit: &circuit,
                runtime: &second_runtime,
            },
            || async { Ok(responses_success()) },
            move || async move {
                second_calls.fetch_add(1, Ordering::SeqCst);
                cli_success()
            },
        )
        .await;

        assert_eq!(cli_calls.load(Ordering::SeqCst), 1);
        let meta = second.meta.expect("cached meta");
        assert!(meta.cache_hit);
        assert_eq!(meta.request_id.as_deref(), Some("request-2"));
        assert_eq!(meta.route_used, "cli");
    }

    #[test]
    fn wallpaper_x_search_cache_obeys_ttl_lru_mode_and_credential_revision() {
        let now = Instant::now();
        let mut cache = SearchCache::new(2, Duration::from_secs(10));
        let cli_a = search_cache_key("A", Some("top"), "cli", Some(&revision(1)));
        let cli_b = search_cache_key("B", Some("top"), "cli", Some(&revision(1)));
        let cli_c = search_cache_key("C", Some("top"), "cli", Some(&revision(1)));
        let preview_a = search_cache_key(
            "A",
            Some("top"),
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(&revision(1)),
        );
        let preview_a_new_credential = search_cache_key(
            "A",
            Some("top"),
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(&revision(2)),
        );
        assert_ne!(cli_a, preview_a);
        assert_ne!(preview_a, preview_a_new_credential);

        let cached = prepare_cache_entry(finish_cli(cli_success(), "cli", None, now));
        cache.insert(cli_a.clone(), cached.clone(), now);
        cache.insert(cli_b.clone(), cached.clone(), now);
        assert!(cache.get(&cli_a, now).is_some(), "touch A as most recent");
        cache.insert(cli_c.clone(), cached.clone(), now);
        assert!(cache.get(&cli_b, now).is_none(), "least-recent B evicted");
        assert!(cache.get(&cli_a, now).is_some());
        assert!(cache.get(&cli_c, now + Duration::from_secs(10)).is_none());
    }

    #[tokio::test]
    async fn wallpaper_x_search_cancellation_never_falls_back() {
        let runtime = WallpaperXSearchRuntime::quiet();
        let cancellation = runtime.cancellation().clone();
        let cli_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&cli_calls);
        let result = route_with_providers(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(revision(1)),
            &Mutex::new(ResponsesCircuitBreaker::default()),
            &runtime,
            move || async move {
                cancellation.cancel();
                Err(responses_error(ResponsesSearchErrorKind::ServerError))
            },
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                cli_success()
            },
        )
        .await;
        assert_eq!(cli_calls.load(Ordering::SeqCst), 0);
        assert_eq!(result.error_code.as_deref(), Some("cancelled"));
    }

    #[test]
    fn wallpaper_x_search_request_registry_cancels_and_cleans_up() {
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(request_id(Some(&id)).expect("valid request id"), id);
        assert!(request_id(Some("not-a-uuid")).is_err());
        let request = register_request(&id).expect("register request");
        assert!(!request.cancellation.is_cancelled());
        assert!(cancel(&id));
        assert!(request.cancellation.is_cancelled());
        drop(request);
        assert!(cancel(&id), "early cancel is retained for an IPC race");
        let pre_cancelled = register_request(&id).expect("register pre-cancelled request");
        assert!(pre_cancelled.cancellation.is_cancelled());
        drop(pre_cancelled);
    }

    #[tokio::test]
    async fn wallpaper_x_search_circuit_opens_after_three_failures_and_skips_http() {
        let circuit = Mutex::new(ResponsesCircuitBreaker::default());
        for _ in 0..3 {
            let _ = route_with_providers(
                store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
                Some(revision(1)),
                &circuit,
                &WallpaperXSearchRuntime::quiet(),
                || async { Err(responses_error(ResponsesSearchErrorKind::ServerError)) },
                || async { cli_success() },
            )
            .await;
        }

        let responses_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&responses_calls);
        let result = route_with_providers(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(revision(1)),
            &circuit,
            &WallpaperXSearchRuntime::quiet(),
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(responses_success())
            },
            || async { cli_success() },
        )
        .await;
        assert_eq!(responses_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            result.meta.expect("route meta").fallback_reason.as_deref(),
            Some(CIRCUIT_OPEN_REASON)
        );
    }

    #[test]
    fn wallpaper_x_search_circuit_resets_on_expiry_success_or_token_change() {
        let now = Instant::now();
        let mut circuit = ResponsesCircuitBreaker::default();
        circuit.record_failure(ResponsesSearchErrorKind::BadRequest, Some(revision(1)), now);
        assert!(!circuit.allows_attempt(Some(revision(1)), now));
        assert!(circuit.allows_attempt(Some(revision(1)), now + CIRCUIT_OPEN_DURATION));

        circuit.record_failure(ResponsesSearchErrorKind::BadRequest, Some(revision(1)), now);
        assert!(circuit.allows_attempt(Some(revision(2)), now));

        circuit.record_failure(
            ResponsesSearchErrorKind::ServerError,
            Some(revision(2)),
            now,
        );
        circuit.record_success(revision(2));
        assert!(circuit.allows_attempt(Some(revision(2)), now));
        assert_eq!(circuit.consecutive_failures, 0);
    }
}
