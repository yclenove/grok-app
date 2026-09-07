use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::wallpaper_source::WallpaperGalleryItem;

fn revision(value: u64) -> BuildOauthCredentialRevision {
    BuildOauthCredentialRevision::for_test(value, Some(value.into()), value as u8)
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
        metadata: None,
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

fn cached_responses_result() -> WallpaperSearchResult {
    let mut result = finish_cli(
        cli_success(),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        None,
        Instant::now(),
        None,
        0,
    );
    result.meta.as_mut().expect("responses meta").route_used = "responses".into();
    prepare_cache_entry(result)
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

#[tokio::test]
async fn wallpaper_x_search_pre_cancel_wins_over_a_cached_success() {
    let requested_mode = store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW;
    let credential_revision = revision(1);
    let cache = Mutex::new(SearchCache::new(32, Duration::from_secs(600)));
    let key = search_cache_key(
        "misty mountains",
        Some("top"),
        requested_mode,
        Some(&credential_revision),
    );
    let cached = prepare_cache_entry(finish_cli(
        cli_success(),
        requested_mode,
        None,
        Instant::now(),
        None,
        1,
    ));
    cache.lock().insert(key, cached, Instant::now());

    let runtime = WallpaperXSearchRuntime::quiet();
    runtime.cancellation().cancel();
    let responses_calls = Arc::new(AtomicUsize::new(0));
    let cli_calls = Arc::new(AtomicUsize::new(0));
    let observed_responses_calls = Arc::clone(&responses_calls);
    let observed_cli_calls = Arc::clone(&cli_calls);
    let result = search_with_cache_and_providers(
        CachedSearchContext {
            request_id: "request-cancelled",
            query: "misty mountains",
            sort: Some("top"),
            requested_mode,
            credential_revision: Some(credential_revision),
            cache: &cache,
            circuit: &Mutex::new(ResponsesCircuitBreaker::default()),
            runtime: &runtime,
        },
        move || async move {
            observed_responses_calls.fetch_add(1, Ordering::SeqCst);
            Ok(responses_success())
        },
        move || async move {
            observed_cli_calls.fetch_add(1, Ordering::SeqCst);
            cli_success()
        },
    )
    .await;

    assert_eq!(result.error_code.as_deref(), Some("cancelled"));
    assert!(result.items.is_empty());
    assert_eq!(responses_calls.load(Ordering::SeqCst), 0);
    assert_eq!(cli_calls.load(Ordering::SeqCst), 0);
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

#[test]
fn wallpaper_x_responses_continuation_claim_is_atomic() {
    let cache = Arc::new(Mutex::new(SearchCache::new(32, Duration::from_secs(600))));
    let key = search_cache_key(
        "misty mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&revision(1)),
    );
    cache
        .lock()
        .insert(key.clone(), cached_responses_result(), Instant::now());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let claims = (0..8)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let key = key.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                cache
                    .lock()
                    .claim_responses_continuation(&key, Instant::now())
                    .is_some()
            })
        })
        .collect::<Vec<_>>();

    assert_eq!(
        claims
            .into_iter()
            .map(|claim| claim.join().expect("continuation claimant panicked"))
            .filter(|claimed| *claimed)
            .count(),
        1
    );
}

#[test]
fn wallpaper_x_responses_continuation_restores_on_drop_and_consumes_once() {
    let cache = Mutex::new(SearchCache::new(32, Duration::from_secs(600)));
    let key = search_cache_key(
        "misty mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&revision(1)),
    );
    cache
        .lock()
        .insert(key.clone(), cached_responses_result(), Instant::now());

    let failed_attempt = ResponsesContinuationLease::claim(&cache, key.clone(), Instant::now())
        .expect("first continuation claim");
    assert!(ResponsesContinuationLease::claim(&cache, key.clone(), Instant::now()).is_none());
    drop(failed_attempt);

    let successful_attempt = ResponsesContinuationLease::claim(&cache, key.clone(), Instant::now())
        .expect("retry continuation claim");
    assert!(successful_attempt.consume_if_active(&WallpaperXSearchRuntime::quiet()));
    assert!(ResponsesContinuationLease::claim(&cache, key.clone(), Instant::now()).is_none());
    let cached = cache
        .lock()
        .get(&key, Instant::now())
        .expect("cached result");
    assert!(!cached.meta.expect("cache metadata").continuation_available);
}

#[test]
fn wallpaper_x_responses_continuation_survives_ttl_and_lru_while_in_flight() {
    let now = Instant::now();
    let mut cache = SearchCache::new(1, Duration::from_secs(10));
    let leased_key = search_cache_key(
        "misty mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&revision(1)),
    );
    let other_key = search_cache_key("forest", Some("top"), "cli", Some(&revision(1)));
    cache.insert(leased_key.clone(), cached_responses_result(), now);
    let (lease_id, _) = cache
        .claim_responses_continuation(&leased_key, now)
        .expect("continuation claim");

    let after_ttl = now + Duration::from_secs(11);
    let cli = prepare_cache_entry(finish_cli(cli_success(), "cli", None, now, None, 0));
    assert!(!cache.insert(other_key.clone(), cli, after_ttl));
    assert_eq!(
        cache.entries.len(),
        1,
        "in-flight leases keep the cache bounded"
    );
    assert!(!cache.entries.contains_key(&other_key));
    cache.resolve_responses_continuation(&leased_key, lease_id, false, after_ttl);

    assert!(
        cache
            .claim_responses_continuation(&leased_key, after_ttl)
            .is_some(),
        "retry lease remains available after a long failed request"
    );
}

#[tokio::test]
async fn wallpaper_x_responses_only_advertises_continuation_when_cache_accepts_it() {
    let cache = Mutex::new(SearchCache::new(1, Duration::from_secs(600)));
    let circuit = Mutex::new(ResponsesCircuitBreaker::default());
    let occupied_key = search_cache_key(
        "occupied",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&revision(1)),
    );
    assert!(cache.lock().insert(
        occupied_key.clone(),
        cached_responses_result(),
        Instant::now(),
    ));
    let occupied = ResponsesContinuationLease::claim(&cache, occupied_key, Instant::now())
        .expect("occupy the only cache slot");
    let runtime = WallpaperXSearchRuntime::quiet();

    let result = search_with_cache_and_providers(
        CachedSearchContext {
            request_id: "request-cache-full",
            query: "fresh query",
            sort: Some("top"),
            requested_mode: store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            credential_revision: Some(revision(1)),
            cache: &cache,
            circuit: &circuit,
            runtime: &runtime,
        },
        || async { Ok(responses_success()) },
        || async { cli_success() },
    )
    .await;

    assert_eq!(result.error_code, None);
    assert!(
        !result
            .meta
            .expect("responses metadata")
            .continuation_available
    );
    assert_eq!(cache.lock().entries.len(), 1);
    drop(occupied);
}

#[test]
fn wallpaper_x_responses_continuation_cancel_restores_instead_of_consuming() {
    let cache = Mutex::new(SearchCache::new(32, Duration::from_secs(600)));
    let key = search_cache_key(
        "misty mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&revision(1)),
    );
    cache
        .lock()
        .insert(key.clone(), cached_responses_result(), Instant::now());
    let lease = ResponsesContinuationLease::claim(&cache, key.clone(), Instant::now())
        .expect("continuation claim");
    let runtime = WallpaperXSearchRuntime::quiet();
    runtime.cancellation().cancel();

    assert!(!lease.consume_if_active(&runtime));
    assert!(ResponsesContinuationLease::claim(&cache, key, Instant::now()).is_some());
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

#[test]
fn wallpaper_x_search_circuit_resets_when_proxy_settings_change() {
    let now = Instant::now();
    let credential = revision(1);
    let mut circuit = ResponsesCircuitBreaker::default();
    circuit.record_failure(
        ResponsesSearchErrorKind::BadRequest,
        Some(credential.clone()),
        now,
    );
    assert!(!circuit.allows_attempt(Some(credential.clone()), now));

    circuit.reset_for_network_change();

    assert!(circuit.allows_attempt(Some(credential), now));
    assert_eq!(circuit.consecutive_failures, 0);
    assert_eq!(circuit.open_until, None);
}

#[test]
fn wallpaper_x_cache_and_circuit_reset_for_same_metadata_content_replacement() {
    let first = BuildOauthCredentialRevision::for_test(10, Some(20), 1);
    let replacement = BuildOauthCredentialRevision::for_test(10, Some(20), 2);
    let first_key = search_cache_key(
        "mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&first),
    );
    let replacement_key = search_cache_key(
        "mountains",
        Some("top"),
        store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
        Some(&replacement),
    );
    assert_ne!(first_key, replacement_key);

    let now = Instant::now();
    let mut circuit = ResponsesCircuitBreaker::default();
    circuit.record_failure(
        ResponsesSearchErrorKind::BadRequest,
        Some(first.clone()),
        now,
    );
    assert!(!circuit.allows_attempt(Some(first), now));
    assert!(circuit.allows_attempt(Some(replacement), now));
}
