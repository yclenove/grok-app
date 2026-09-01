//! Safe, source-agnostic remote image probing and download.
//!
//! Unlike the X/Imagine downloader, this path accepts arbitrary public HTTPS
//! origins, so every redirect and DNS resolution is checked through
//! `skin_net` before bytes are trusted.

use std::collections::HashMap;
use std::fs;
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};
use url::Url;

use crate::skin_net::{self, OriginPolicy};
use crate::wallpaper_remote_search::RemoteWallpaperSource;
use crate::wallpaper_source::{
    validate_fetched_media_bytes, validate_image_prefix, ValidatedImagePrefix,
    WallpaperFetchResult, WallpaperSearchCancellation, MAX_DOWNLOAD_BYTES,
};

const PROBE_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const BROWSER_IMAGE_USER_AGENT: &str =
    "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 GrokApp/WallpaperDiscovery";
const BROWSER_IMAGE_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9,*;q=0.5";

#[derive(Clone)]
struct ActiveMediaRequest {
    token: uuid::Uuid,
    cancellation: WallpaperSearchCancellation,
}

static ACTIVE_MEDIA: OnceLock<Mutex<HashMap<String, ActiveMediaRequest>>> = OnceLock::new();

fn active_media() -> &'static Mutex<HashMap<String, ActiveMediaRequest>> {
    ACTIVE_MEDIA.get_or_init(|| Mutex::new(HashMap::new()))
}

fn begin_media_request(request_id: &str) -> (uuid::Uuid, WallpaperSearchCancellation) {
    let token = uuid::Uuid::new_v4();
    let cancellation = WallpaperSearchCancellation::default();
    if let Some(previous) = active_media().lock().insert(
        request_id.to_string(),
        ActiveMediaRequest {
            token,
            cancellation: cancellation.clone(),
        },
    ) {
        previous.cancellation.cancel();
    }
    (token, cancellation)
}

fn finish_media_request(request_id: &str, token: uuid::Uuid) {
    let mut active = active_media().lock();
    if active
        .get(request_id)
        .is_some_and(|request| request.token == token)
    {
        active.remove(request_id);
    }
}

pub(crate) fn cancel_media_requests(request_ids: &[String]) -> usize {
    let mut active = active_media().lock();
    let mut cancelled = 0;
    for request_id in request_ids {
        if let Some(request) = active.remove(request_id) {
            request.cancellation.cancel();
            cancelled += 1;
        }
    }
    cancelled
}

pub(crate) fn cancel_all_media_requests() -> usize {
    let requests = active_media()
        .lock()
        .drain()
        .map(|(_, request)| request)
        .collect::<Vec<_>>();
    let count = requests.len();
    for request in requests {
        request.cancellation.cancel();
    }
    count
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteImageProbe {
    pub(crate) final_url: String,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) content_length: Option<u64>,
    pub(crate) content_fingerprint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteImageProbeFailure {
    Cancelled,
    Blocked,
    Timeout,
    Forbidden,
    NotFound,
    RateLimited,
    HttpClient,
    HttpServer,
    Network,
    InvalidImage,
    TooLarge,
    Redirect,
}

fn network_failure(error: &reqwest::Error) -> RemoteImageProbeFailure {
    if error.is_timeout() {
        RemoteImageProbeFailure::Timeout
    } else {
        RemoteImageProbeFailure::Network
    }
}

fn status_failure(status: reqwest::StatusCode) -> RemoteImageProbeFailure {
    match status.as_u16() {
        403 => RemoteImageProbeFailure::Forbidden,
        404 | 410 => RemoteImageProbeFailure::NotFound,
        429 => RemoteImageProbeFailure::RateLimited,
        400..=499 => RemoteImageProbeFailure::HttpClient,
        500..=599 => RemoteImageProbeFailure::HttpServer,
        _ => RemoteImageProbeFailure::Network,
    }
}

pub(crate) fn origin_referer(source_page: &Url) -> Option<String> {
    if source_page.scheme() != "https"
        || !source_page.username().is_empty()
        || source_page.password().is_some()
        || source_page.host_str().is_none()
    {
        return None;
    }
    Some(format!("{}/", source_page.origin().ascii_serialization()))
}

pub(crate) fn is_wallpaper_quality_candidate(probe: &RemoteImageProbe) -> bool {
    if let (Some(width), Some(height)) = (probe.width, probe.height) {
        let long = width.max(height);
        let short = width.min(height);
        let ratio = width as f64 / height as f64;
        long >= 900 && short >= 360 && (0.38..=3.2).contains(&ratio)
    } else {
        probe.content_length.is_some_and(|bytes| bytes >= 80 * 1024)
    }
}

#[derive(Clone)]
pub(crate) struct RemoteImageProber {
    client: reqwest::Client,
}

impl RemoteImageProber {
    pub(crate) fn new() -> Result<Self, String> {
        let client = crate::proxy::apply_to_reqwest(
            reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(Duration::from_secs(15))
                .redirect(Policy::none())
                .user_agent(BROWSER_IMAGE_USER_AGENT),
        )
        .build()
        .map_err(|_| "download_failed: network".to_string())?;
        Ok(Self { client })
    }

    pub(crate) async fn probe_image(
        &self,
        start: &str,
        referer_origin: Option<&str>,
        cancellation: &WallpaperSearchCancellation,
    ) -> Result<RemoteImageProbe, RemoteImageProbeFailure> {
        probe_image_with_client(&self.client, start, referer_origin, cancellation).await
    }
}

fn normalized_content_type(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn response_total_length(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit('/').next())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length())
}

async fn read_prefix(
    response: &mut reqwest::Response,
    cancellation: &WallpaperSearchCancellation,
) -> Result<Vec<u8>, RemoteImageProbeFailure> {
    let mut bytes = Vec::with_capacity(PROBE_BYTES);
    while bytes.len() < PROBE_BYTES {
        let next = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(RemoteImageProbeFailure::Cancelled),
            chunk = response.chunk() => chunk,
        }
        .map_err(|error| network_failure(&error))?;
        let Some(chunk) = next else {
            break;
        };
        let remaining = PROBE_BYTES - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    Ok(bytes)
}

async fn probe_image_with_client(
    client: &reqwest::Client,
    start: &str,
    referer_origin: Option<&str>,
    cancellation: &WallpaperSearchCancellation,
) -> Result<RemoteImageProbe, RemoteImageProbeFailure> {
    if cancellation.is_cancelled() {
        return Err(RemoteImageProbeFailure::Cancelled);
    }
    let policy = OriginPolicy::AnyHttps;
    let mut current = skin_net::check_hop(start, &policy, skin_net::default_resolve)
        .map_err(|_| RemoteImageProbeFailure::Blocked)?;

    for hop in 0..=skin_net::MAX_REDIRECTS {
        let request = client
            .get(current.as_str())
            .header(
                reqwest::header::ACCEPT,
                "image/avif,image/webp,image/*,*/*;q=0.8",
            )
            .header(
                reqwest::header::ACCEPT_LANGUAGE,
                BROWSER_IMAGE_ACCEPT_LANGUAGE,
            )
            .header(
                reqwest::header::RANGE,
                format!("bytes=0-{}", PROBE_BYTES - 1),
            );
        let request = if let Some(referer) = referer_origin {
            request.header(reqwest::header::REFERER, referer)
        } else {
            request
        };
        let mut response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(RemoteImageProbeFailure::Cancelled),
            response = request.send() => response,
        }
        .map_err(|error| network_failure(&error))?;

        if response.status().is_redirection() {
            if hop == skin_net::MAX_REDIRECTS {
                return Err(RemoteImageProbeFailure::Redirect);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(RemoteImageProbeFailure::Redirect)?;
            let next = current
                .join(location)
                .map_err(|_| RemoteImageProbeFailure::Redirect)?;
            current = skin_net::check_hop(next.as_str(), &policy, skin_net::default_resolve)
                .map_err(|_| RemoteImageProbeFailure::Blocked)?;
            continue;
        }

        if !matches!(response.status().as_u16(), 200 | 206) {
            return Err(status_failure(response.status()));
        }
        let content_length = response_total_length(&response);
        if content_length.is_some_and(|length| length > MAX_DOWNLOAD_BYTES) {
            return Err(RemoteImageProbeFailure::TooLarge);
        }
        let content_type = normalized_content_type(&response);
        let prefix = read_prefix(&mut response, cancellation).await?;
        let ValidatedImagePrefix { dimensions, .. } = validate_image_prefix(&content_type, &prefix)
            .ok_or(RemoteImageProbeFailure::InvalidImage)?;
        let content_fingerprint = hex::encode(Sha256::digest(&prefix));
        let (width, height) = dimensions
            .map(|(width, height)| (Some(width), Some(height)))
            .unwrap_or((None, None));
        return Ok(RemoteImageProbe {
            final_url: current.to_string(),
            width,
            height,
            content_length,
            content_fingerprint,
        });
    }

    Err(RemoteImageProbeFailure::Redirect)
}

pub(crate) async fn fetch_image_bytes(
    start: &str,
    cancellation: Option<&WallpaperSearchCancellation>,
) -> Result<(String, String, Vec<u8>), String> {
    let fetch = skin_net::safe_https_get_browser_image_response(
        start,
        OriginPolicy::AnyHttps,
        MAX_DOWNLOAD_BYTES,
    );
    let response = if let Some(cancellation) = cancellation {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err("cancelled".into()),
            response = fetch => response?,
        }
    } else {
        fetch.await?
    };
    let content_type = response
        .content_type
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    validate_image_prefix(&content_type, &response.bytes)
        .ok_or_else(|| "download_failed: invalid image".to_string())?;
    Ok((response.final_url.to_string(), content_type, response.bytes))
}

pub(crate) async fn fetch_and_store_image(
    start: &str,
    source: RemoteWallpaperSource,
    request_id: &str,
) -> Result<WallpaperFetchResult, String> {
    let (token, cancellation) = begin_media_request(request_id);
    let result = async {
        let (final_url, content_type, bytes) =
            fetch_image_bytes(start, Some(&cancellation)).await?;
        if cancellation.is_cancelled() {
            return Err("cancelled".into());
        }
        save_image(&final_url, source, &content_type, bytes)
    }
    .await;
    finish_media_request(request_id, token);
    result
}

fn save_image(
    final_url: &str,
    source: RemoteWallpaperSource,
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<WallpaperFetchResult, String> {
    let (mime, extension) = validate_fetched_media_bytes(content_type, &bytes)?;
    if !mime.starts_with("image/") {
        return Err("download_failed: image required".into());
    }
    let mut digest = Sha256::new();
    digest.update(final_url.as_bytes());
    digest.update(&bytes);
    let hash = format!("{:x}", digest.finalize());
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let directory = crate::wallpaper_source::wallpapers_root()
        .join(source.as_str())
        .join(day);
    fs::create_dir_all(&directory).map_err(|error| format!("write: {error}"))?;
    let name = format!(
        "{}-{}.{}",
        chrono::Local::now().format("%H%M%S%3f"),
        &hash[..16],
        extension
    );
    let path = directory.join(&name);
    let temporary = directory.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, &bytes).map_err(|error| format!("write: {error}"))?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("write: {error}"));
    }
    crate::path_scope::grant_path(&path);
    Ok(WallpaperFetchResult {
        path: path.display().to_string(),
        mime: mime.to_string(),
        bytes: bytes.len() as u64,
        name,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::paths::APP_HOME_ENV_LOCK;

    use super::*;

    #[test]
    fn content_range_total_shape_is_stable() {
        let parsed = "bytes 0-65535/1048576"
            .rsplit('/')
            .next()
            .and_then(|value| value.parse::<u64>().ok());
        assert_eq!(parsed, Some(1_048_576));
    }

    #[test]
    fn image_referer_contains_only_the_source_origin() {
        let source = Url::parse("https://photos.example/private/path?query=secret").unwrap();
        assert_eq!(
            origin_referer(&source).as_deref(),
            Some("https://photos.example/")
        );
        assert!(origin_referer(&Url::parse("http://photos.example/path").unwrap()).is_none());
    }

    #[test]
    fn probe_http_failures_have_stable_categories() {
        assert_eq!(
            status_failure(reqwest::StatusCode::FORBIDDEN),
            RemoteImageProbeFailure::Forbidden
        );
        assert_eq!(
            status_failure(reqwest::StatusCode::NOT_FOUND),
            RemoteImageProbeFailure::NotFound
        );
        assert_eq!(
            status_failure(reqwest::StatusCode::TOO_MANY_REQUESTS),
            RemoteImageProbeFailure::RateLimited
        );
        assert_eq!(
            status_failure(reqwest::StatusCode::BAD_GATEWAY),
            RemoteImageProbeFailure::HttpServer
        );
    }

    #[test]
    fn stored_remote_images_stay_in_their_source_directory() {
        let _environment = APP_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let home = std::env::temp_dir().join(format!(
            "grok-app-remote-image-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        // SAFETY: test-only mutation serialized by APP_HOME_ENV_LOCK.
        unsafe { std::env::set_var("GROK_APP_HOME", &home) };
        let mut png = vec![0u8; 128];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let saved = save_image(
            "https://images.example.test/wallpaper.png",
            RemoteWallpaperSource::Web,
            "image/png",
            png,
        )
        .expect("save remote image");
        assert!(Path::new(&saved.path).is_file());
        assert!(Path::new(&saved.path).starts_with(home.join("wallpapers").join("web")));

        unsafe { std::env::remove_var("GROK_APP_HOME") };
        let _ = fs::remove_dir_all(home);
    }
}
