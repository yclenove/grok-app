//! User-triggered enrichment for an existing successful Responses gallery.
//!
//! The initial cache is used only as bounded Host-side continuation context.
//! This path bypasses cache delivery, never retries, and never falls back to
//! the CLI, so one click can spend at most one additional Responses request.

use super::*;

pub(crate) async fn search_more(
    app: &tauri::AppHandle,
    request_id: &str,
    query: &str,
    sort: Option<&str>,
) -> Result<WallpaperSearchResult, String> {
    let request = register_request(request_id)?;
    let runtime = load_more_runtime(app, request_id, request.cancellation.clone());
    let settings = store::load_settings();
    let requested_mode =
        store::normalize_wallpaper_x_search_mode(&settings.wallpaper_x_search_mode);
    let credential_revision = account::build_oauth_credential_revision();
    let started = Instant::now();
    runtime.report(WallpaperXSearchStage::Preparing);

    let mut result = if requested_mode != store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW {
        load_more_error_result(requested_mode, "load_more_unavailable", started, None, None)
    } else {
        let key = search_cache_key(query, sort, requested_mode, credential_revision.as_ref());
        let continuation = ResponsesContinuationLease::claim(response_cache(), key, Instant::now());
        match continuation {
            Some(continuation) => {
                let result = run_load_more_provider(
                    requested_mode,
                    credential_revision,
                    &continuation.previous().items,
                    query,
                    sort,
                    &runtime,
                    started,
                )
                .await;
                if runtime.is_cancelled() {
                    drop(continuation);
                    cancelled_result(
                        requested_mode,
                        "responses",
                        started,
                        result
                            .meta
                            .as_ref()
                            .and_then(|meta| meta.responses_duration_ms),
                        None,
                    )
                } else if consumes_responses_continuation(&result) {
                    // Commit at the cache lock after one last cancellation
                    // read. Failures and cancellation restore the lease.
                    if continuation.consume_if_active(&runtime) {
                        result
                    } else {
                        cancelled_result(
                            requested_mode,
                            "responses",
                            started,
                            result
                                .meta
                                .as_ref()
                                .and_then(|meta| meta.responses_duration_ms),
                            None,
                        )
                    }
                } else {
                    result
                }
            }
            None => {
                load_more_error_result(requested_mode, "load_more_unavailable", started, None, None)
            }
        }
    };

    if let Some(meta) = result.meta.as_mut() {
        meta.request_id = Some(request_id.to_string());
    }
    runtime.report(WallpaperXSearchStage::Done);
    Ok(result)
}

async fn run_load_more_provider(
    requested_mode: &str,
    credential_revision: Option<BuildOauthCredentialRevision>,
    previous: &[wallpaper_source::WallpaperGalleryItem],
    query: &str,
    sort: Option<&str>,
    runtime: &WallpaperXSearchRuntime,
    started: Instant,
) -> WallpaperSearchResult {
    if runtime.is_cancelled() {
        return cancelled_result(requested_mode, "responses", started, None, None);
    }
    if !responses_circuit()
        .lock()
        .allows_attempt(credential_revision.clone(), Instant::now())
    {
        return load_more_error_result(requested_mode, CIRCUIT_OPEN_REASON, started, None, None);
    }

    runtime.report(WallpaperXSearchStage::SearchingX);
    let responses_started = Instant::now();
    let outcome = wallpaper_x_responses::search_more(query, sort, previous, runtime).await;
    let responses_duration_ms = elapsed_ms(responses_started);
    match outcome {
        Ok(result) => {
            responses_circuit()
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
                    continuation_available: false,
                    search_calls: Some(result.search_calls),
                    candidate_count: result.candidate_count,
                    valid_count: result.valid_count,
                    model: Some(result.model.into()),
                    effort: Some(result.effort.into()),
                }),
            }
        }
        Err(error) if error.kind == ResponsesSearchErrorKind::Cancelled => cancelled_result(
            requested_mode,
            "responses",
            started,
            Some(responses_duration_ms),
            None,
        ),
        Err(error) => {
            // An empty enrichment batch is a valid "no more" outcome, not a
            // provider-health failure. Every other class follows the initial
            // circuit accounting, but none is retried or routed to the CLI.
            if error.kind != ResponsesSearchErrorKind::Empty && counts_toward_circuit(error.kind) {
                responses_circuit().lock().record_failure(
                    error.kind,
                    error.credential_revision.clone().or(credential_revision),
                    Instant::now(),
                );
            }
            load_more_error_result(
                requested_mode,
                load_more_error_code(error.kind),
                started,
                Some(responses_duration_ms),
                error.observed_search_calls,
            )
        }
    }
}

fn load_more_runtime(
    app: &tauri::AppHandle,
    request_id: &str,
    cancellation: WallpaperSearchCancellation,
) -> WallpaperXSearchRuntime {
    let progress_app = app.clone();
    let progress_request_id = request_id.to_string();
    let progress = Arc::new(move |stage| {
        let _ = progress_app.emit(
            WALLPAPER_X_SEARCH_PROGRESS_EVENT,
            WallpaperXSearchProgress {
                request_id: progress_request_id.clone(),
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
    WallpaperXSearchRuntime::new(cancellation, progress, batch)
}

fn consumes_responses_continuation(result: &WallpaperSearchResult) -> bool {
    result.error_code.is_none() || result.error_code.as_deref() == Some("empty")
}

fn load_more_error_code(kind: ResponsesSearchErrorKind) -> &'static str {
    if kind == ResponsesSearchErrorKind::Empty {
        "empty"
    } else {
        kind.code()
    }
}

fn load_more_error_result(
    requested_mode: &str,
    error_code: &str,
    started: Instant,
    responses_duration_ms: Option<u64>,
    search_calls: Option<u32>,
) -> WallpaperSearchResult {
    WallpaperSearchResult {
        items: Vec::new(),
        error_code: Some(error_code.into()),
        message: None,
        meta: Some(WallpaperSearchMeta {
            request_id: None,
            requested_mode: store::normalize_wallpaper_x_search_mode(requested_mode).into(),
            route_used: "responses".into(),
            fallback_reason: None,
            duration_ms: elapsed_ms(started),
            responses_duration_ms,
            cli_duration_ms: None,
            cache_hit: false,
            continuation_available: false,
            search_calls,
            candidate_count: 0,
            valid_count: 0,
            model: Some(wallpaper_x_responses::RESPONSES_MODEL.into()),
            effort: Some(wallpaper_x_responses::RESPONSES_EFFORT.into()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_x_load_more_empty_is_not_misreported_as_provider_failure() {
        assert_eq!(
            load_more_error_code(ResponsesSearchErrorKind::Empty),
            "empty"
        );
        assert_eq!(
            load_more_error_code(ResponsesSearchErrorKind::RateLimited),
            "responses_rate_limited"
        );
        assert!(!counts_toward_circuit(
            ResponsesSearchErrorKind::RateLimited
        ));

        let empty = load_more_error_result(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            "empty",
            Instant::now(),
            None,
            None,
        );
        let retryable = load_more_error_result(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            "responses_network",
            Instant::now(),
            None,
            None,
        );
        assert!(consumes_responses_continuation(&empty));
        assert!(!consumes_responses_continuation(&retryable));
    }
}
