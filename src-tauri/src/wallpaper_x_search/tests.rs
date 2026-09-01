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
        provenance: crate::wallpaper_source::WallpaperProvenance::empty(),
        status_id: None,
        media_index: None,
        media_quality: None,
        media_fingerprint: None,
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
        observed_search_calls: None,
    }
}

#[test]
fn wallpaper_x_search_batch_event_serializes_safe_camel_case_payload() {
    let mut batch_item = item("batch");
    batch_item.status_id = Some("host-only-status".into());
    batch_item.media_index = Some(2);
    let payload = serde_json::to_value(WallpaperXSearchBatchEvent {
        request_id: "request-1".into(),
        batch_index: 2,
        items: vec![batch_item],
        accumulated_count: 7,
        done: false,
    })
    .expect("serialize batch event");

    assert_eq!(payload["requestId"], "request-1");
    assert_eq!(payload["batchIndex"], 2);
    assert_eq!(payload["accumulatedCount"], 7);
    assert_eq!(payload["done"], false);
    assert_eq!(payload["items"][0]["id"], "batch");
    assert!(payload["items"][0].get("statusId").is_none());
    assert!(payload["items"][0].get("mediaIndex").is_none());
    assert!(payload.get("request_id").is_none());
}

#[test]
fn wallpaper_x_search_terminal_batch_only_reports_cached_responses_results() {
    let captured = Arc::new(Mutex::new(Vec::<WallpaperXSearchBatch>::new()));
    let captured_batches = Arc::clone(&captured);
    let cancellation = WallpaperSearchCancellation::default();
    let runtime = WallpaperXSearchRuntime::new(
        cancellation.clone(),
        Arc::new(|_| {}),
        Arc::new(move |batch| captured_batches.lock().push(batch)),
    );
    let mut responses = finish_cli(
        cli_success(),
        "responses_preview",
        None,
        Instant::now(),
        None,
        0,
    );
    responses.meta.as_mut().expect("responses meta").route_used = "responses".into();

    report_terminal_responses_batch(&runtime, &responses);
    let batches = captured.lock();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].batch_index, 1);
    assert_eq!(batches[0].items[0].id, "cli");
    assert_eq!(batches[0].accumulated_count, 1);
    assert!(batches[0].done);
    drop(batches);

    let cli = finish_cli(cli_success(), "cli", None, Instant::now(), None, 0);
    report_terminal_responses_batch(&runtime, &cli);
    assert_eq!(captured.lock().len(), 1, "CLI never emits a batch event");

    cancellation.cancel();
    report_terminal_responses_batch(&runtime, &responses);
    assert_eq!(captured.lock().len(), 1, "cancelled requests stay silent");
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
        assert_eq!(meta.responses_duration_ms, None);
        assert!(meta.cli_duration_ms.is_some());
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
    assert!(meta.responses_duration_ms.is_some());
    assert_eq!(meta.cli_duration_ms, None);
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
        assert!(meta.responses_duration_ms.is_some());
        assert!(meta.cli_duration_ms.is_some());
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
        let meta = result.meta.expect("route meta");
        assert_eq!(meta.route_used, "responses");
        assert!(meta.responses_duration_ms.is_some());
        assert_eq!(meta.cli_duration_ms, None);
    }
}

#[tokio::test]
async fn wallpaper_x_search_fallback_reports_split_and_total_timings() {
    let result = route_with_providers(
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(revision(1)),
        &Mutex::new(ResponsesCircuitBreaker::default()),
        &WallpaperXSearchRuntime::quiet(),
        || async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Err(responses_error(ResponsesSearchErrorKind::Network))
        },
        || async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cli_success()
        },
    )
    .await;

    let meta = result.meta.expect("route meta");
    let responses_ms = meta.responses_duration_ms.expect("responses timing");
    let cli_ms = meta.cli_duration_ms.expect("cli timing");
    assert!(responses_ms >= 10, "responses_ms={responses_ms}");
    assert!(cli_ms >= 10, "cli_ms={cli_ms}");
    assert!(
        meta.duration_ms >= responses_ms.saturating_add(cli_ms),
        "total={} responses={responses_ms} cli={cli_ms}",
        meta.duration_ms
    );
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
    assert_eq!(meta.responses_duration_ms, None);
    assert_eq!(meta.cli_duration_ms, None);
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

    let cached = prepare_cache_entry(finish_cli(cli_success(), "cli", None, now, None, 0));
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
