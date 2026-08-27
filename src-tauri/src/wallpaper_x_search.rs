//! Wallpaper X search router: stable CLI default plus an opt-in Responses
//! preview with one-way fallback and a small process-local circuit breaker.

use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::account::{self, BuildOauthCredentialRevision};
use crate::store;
use crate::wallpaper_source::{
    self, WallpaperCliSearchOutcome, WallpaperSearchMeta, WallpaperSearchResult,
};
use crate::wallpaper_x_responses::{
    self, ResponsesSearchError, ResponsesSearchErrorKind, ResponsesSearchSuccess,
};

const CIRCUIT_FAILURE_THRESHOLD: u8 = 3;
const CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(10 * 60);
const CIRCUIT_OPEN_REASON: &str = "responses_circuit_open";

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
    kind != ResponsesSearchErrorKind::RateLimited
}

pub(crate) async fn search(query: &str, sort: Option<&str>) -> WallpaperSearchResult {
    let settings = store::load_settings();
    let requested_mode =
        store::normalize_wallpaper_x_search_mode(&settings.wallpaper_x_search_mode);
    let credential_revision = account::build_oauth_credential_revision();

    route_with_providers(
        requested_mode,
        credential_revision,
        responses_circuit(),
        || async {
            let mut result = wallpaper_x_responses::search(query, sort).await?;
            result.items = wallpaper_source::filter_reachable_gallery_items(result.items).await;
            result.valid_count = result.items.len();
            Ok(result)
        },
        || wallpaper_source::x_search_cli_outcome(query, sort),
    )
    .await
}

async fn route_with_providers<R, RFut, C, CFut>(
    requested_mode: &str,
    credential_revision: Option<BuildOauthCredentialRevision>,
    circuit: &Mutex<ResponsesCircuitBreaker>,
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
        return finish_cli(cli().await, requested_mode, None, started);
    }

    if !circuit
        .lock()
        .allows_attempt(credential_revision.clone(), Instant::now())
    {
        return finish_cli(
            cli().await,
            requested_mode,
            Some(CIRCUIT_OPEN_REASON),
            started,
        );
    }

    match responses().await {
        Ok(result) if !result.items.is_empty() && result.valid_count > 0 => {
            circuit
                .lock()
                .record_success(result.credential_revision.clone());
            WallpaperSearchResult {
                items: result.items,
                error_code: None,
                message: None,
                meta: Some(WallpaperSearchMeta {
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
            circuit.lock().record_failure(
                ResponsesSearchErrorKind::Empty,
                Some(result.credential_revision),
                Instant::now(),
            );
            finish_cli(
                cli().await,
                requested_mode,
                Some(ResponsesSearchErrorKind::Empty.code()),
                started,
            )
        }
        Err(error) if !should_fallback(error.kind) => WallpaperSearchResult {
            items: Vec::new(),
            error_code: Some(error.code().into()),
            message: None,
            meta: Some(WallpaperSearchMeta {
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
        },
        Err(error) => {
            circuit.lock().record_failure(
                error.kind,
                error.credential_revision.clone().or(credential_revision),
                Instant::now(),
            );
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
    async fn wallpaper_x_search_rate_limit_never_double_spends_on_cli() {
        let cli_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&cli_calls);
        let result = route_with_providers(
            store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
            Some(revision(1)),
            &Mutex::new(ResponsesCircuitBreaker::default()),
            || async { Err(responses_error(ResponsesSearchErrorKind::RateLimited)) },
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                cli_success()
            },
        )
        .await;
        assert_eq!(cli_calls.load(Ordering::SeqCst), 0);
        assert!(result.items.is_empty());
        assert_eq!(result.error_code.as_deref(), Some("responses_rate_limited"));
        assert_eq!(result.meta.expect("route meta").route_used, "responses");
    }

    #[tokio::test]
    async fn wallpaper_x_search_circuit_opens_after_three_failures_and_skips_http() {
        let circuit = Mutex::new(ResponsesCircuitBreaker::default());
        for _ in 0..3 {
            let _ = route_with_providers(
                store::WALLPAPER_X_SEARCH_MODE_RESPONSES_PREVIEW,
                Some(revision(1)),
                &circuit,
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
