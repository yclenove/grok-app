//! Cancellable image-to-video generation for the wallpaper Imagine workspace.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::wallpaper_source::{
    self, LocalWallpaperMediaKind, WallpaperGalleryItem, WallpaperProvenance,
    WallpaperSearchCancellation, WallpaperSearchResult,
};

const VIDEO_TIMEOUT: Duration = Duration::from_secs(420);
const MAX_MOTION_PROMPT_CHARS: usize = 4_000;
const MAX_GENERATED_VIDEOS: usize = 4;
const PRE_CANCEL_TTL: Duration = Duration::from_secs(30);
const PRE_CANCEL_CAPACITY: usize = 64;

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

static REQUESTS: OnceLock<Mutex<RequestRegistry>> = OnceLock::new();

fn requests() -> &'static Mutex<RequestRegistry> {
    REQUESTS.get_or_init(|| Mutex::new(RequestRegistry::default()))
}

fn begin_request(request_id: &str) -> (uuid::Uuid, WallpaperSearchCancellation) {
    let mut registry = requests().lock();
    for (_, request) in registry.active.drain() {
        request.cancellation.cancel();
    }
    let token = uuid::Uuid::new_v4();
    let cancellation = WallpaperSearchCancellation::default();
    if registry.take_pre_cancel(request_id, Instant::now()) {
        cancellation.cancel();
    }
    registry.active.insert(
        request_id.to_string(),
        ActiveRequest {
            token,
            cancellation: cancellation.clone(),
        },
    );
    (token, cancellation)
}

fn finish_request(request_id: &str, token: uuid::Uuid) {
    let mut registry = requests().lock();
    if registry
        .active
        .get(request_id)
        .is_some_and(|request| request.token == token)
    {
        registry.active.remove(request_id);
    }
}

pub(crate) fn cancel(request_id: &str) -> Result<bool, String> {
    let request_id = normalized_request_id(request_id)?;
    let mut registry = requests().lock();
    if let Some(request) = registry.active.remove(&request_id) {
        drop(registry);
        request.cancellation.cancel();
    } else {
        registry.record_pre_cancel(&request_id, Instant::now());
    }
    Ok(true)
}

pub(crate) fn generate(
    request_id: &str,
    source_path: &str,
    motion_prompt: Option<&str>,
    duration: Option<u32>,
    resolution_name: Option<&str>,
) -> Result<WallpaperSearchResult, String> {
    let request_id = normalized_request_id(request_id)?;
    let (token, cancellation) = begin_request(&request_id);
    let result = generate_inner(
        source_path,
        motion_prompt,
        duration,
        resolution_name,
        &cancellation,
    );
    finish_request(&request_id, token);
    Ok(result)
}

fn generate_inner(
    source_path: &str,
    motion_prompt: Option<&str>,
    duration: Option<u32>,
    resolution_name: Option<&str>,
    cancellation: &WallpaperSearchCancellation,
) -> WallpaperSearchResult {
    if cancellation.is_cancelled() {
        return failure("cancelled");
    }
    let (duration, resolution_name) = match video_options(duration, resolution_name) {
        Ok(options) => options,
        Err(code) => return failure(code),
    };
    let motion_prompt = match normalized_motion_prompt(motion_prompt) {
        Ok(prompt) => prompt,
        Err(code) => return failure(code),
    };
    let root = wallpaper_source::wallpapers_root();
    let source = match validated_source_image(source_path, &root) {
        Ok(path) => path,
        Err(code) => return failure(code),
    };
    let cli = match wallpaper_source::require_cli_ready() {
        Ok(cli) => cli,
        Err(code) => return failure(&code),
    };

    let output_dir = video_output_dir(&root);
    if fs::create_dir_all(&output_dir).is_err() {
        return failure("imagine_failed");
    }
    let prompt = image_to_video_prompt(
        &source,
        &output_dir,
        motion_prompt.as_deref(),
        duration,
        resolution_name,
    );
    let schema = r#"{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "localPath": { "type": "string" },
          "kind": { "type": "string" },
          "prompt": { "type": "string" }
        },
        "required": ["localPath", "kind"]
      }
    }
  },
  "required": ["items"]
}"#;

    let stdout = match wallpaper_source::run_grok_headless_cancellable(
        &cli,
        &prompt,
        schema,
        18,
        VIDEO_TIMEOUT,
        Some(&output_dir),
        Some(cancellation),
    ) {
        Ok(stdout) => stdout,
        Err(code) => {
            cleanup_failed_output(&output_dir);
            return failure(match code.as_str() {
                "cancelled" => "cancelled",
                "timeout" => "timeout",
                "auth_required" => "auth_required",
                "cli_missing" => "cli_missing",
                _ => "imagine_failed",
            });
        }
    };

    if cancellation.is_cancelled() {
        cleanup_failed_output(&output_dir);
        return failure("cancelled");
    }
    let payload = wallpaper_source::parse_grok_wallpaper_payload(&stdout);
    let items = collect_generated_videos(payload.as_ref(), &output_dir, motion_prompt.as_deref());
    if items.is_empty() {
        cleanup_failed_output(&output_dir);
        return failure("imagine_failed");
    }

    WallpaperSearchResult {
        items,
        error_code: None,
        message: None,
        meta: None,
    }
}

fn normalized_request_id(request_id: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(request_id.trim())
        .map(|id| id.to_string())
        .map_err(|_| "invalid_request_id".to_string())
}

fn normalized_motion_prompt(prompt: Option<&str>) -> Result<Option<String>, &'static str> {
    let prompt = prompt.map(str::trim).filter(|value| !value.is_empty());
    let Some(prompt) = prompt else {
        return Ok(None);
    };
    if prompt.chars().count() > MAX_MOTION_PROMPT_CHARS || prompt.contains('\0') {
        return Err("imagine_failed");
    }
    Ok(Some(prompt.to_string()))
}

fn video_options(
    duration: Option<u32>,
    resolution_name: Option<&str>,
) -> Result<(u32, &'static str), &'static str> {
    let duration = duration.unwrap_or(6);
    if !matches!(duration, 6 | 10) {
        return Err("imagine_failed");
    }
    let resolution = resolution_name.unwrap_or("480p").trim();
    let resolution = match resolution {
        "480p" => "480p",
        "720p" => "720p",
        _ => return Err("imagine_failed"),
    };
    Ok((duration, resolution))
}

fn validated_source_image(raw: &str, root: &Path) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(raw.trim());
    if raw.trim().is_empty()
        || !path.is_absolute()
        || !path.is_file()
        || !wallpaper_source::is_path_under_dir(&path, root)
    {
        return Err("imagine_source_invalid");
    }
    let path = path.canonicalize().map_err(|_| "imagine_source_invalid")?;
    wallpaper_source::validate_local_wallpaper_media(&path, LocalWallpaperMediaKind::Image)
        .map_err(|_| "imagine_source_invalid")?;
    Ok(path)
}

fn video_output_dir(root: &Path) -> PathBuf {
    root.join("imagine")
        .join(chrono::Local::now().format("%Y-%m-%d").to_string())
        .join(format!("video-{}", uuid::Uuid::new_v4().simple()))
}

fn image_to_video_prompt(
    source: &Path,
    output_dir: &Path,
    motion_prompt: Option<&str>,
    duration: u32,
    resolution_name: &str,
) -> String {
    let source = serde_json::to_string(&source.display().to_string()).unwrap_or_default();
    let output_dir = serde_json::to_string(&output_dir.display().to_string()).unwrap_or_default();
    let motion = motion_prompt
        .map(|value| serde_json::to_string(value).unwrap_or_default())
        .unwrap_or_else(|| "null".into());
    format!(
        r#"Animate the supplied wallpaper image with the built-in image_to_video tool.

Source image absolute path (JSON string): {source}
Optional motion/camera prompt (JSON string or null): {motion}
Duration: {duration} seconds
Resolution name: {resolution_name}
Required output directory (JSON string): {output_dir}

Requirements:
1. Call image_to_video exactly once. Do not call image_gen, web_search, or any unrelated tool.
2. Use the source image path exactly as supplied, duration {duration}, and resolution_name "{resolution_name}". Include the motion prompt only when it is not null.
3. After generation, copy the resulting MP4 or WebM into the required output directory. Keep the original generated file intact.
4. Return JSON with one items entry. localPath must be the absolute copied path inside the required output directory and kind must be "video".
5. Never invent a path and never return a remote URL."#
    )
}

fn collect_generated_videos(
    payload: Option<&Value>,
    output_dir: &Path,
    motion_prompt: Option<&str>,
) -> Vec<WallpaperGalleryItem> {
    let mut candidates = Vec::new();
    if let Some(items) = payload
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
    {
        for item in items {
            let raw = item
                .get("localPath")
                .or_else(|| item.get("local_path"))
                .and_then(Value::as_str);
            if let Some(path) = raw.and_then(local_path_from_value) {
                candidates.push(path);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(output_dir) {
        candidates.extend(entries.flatten().map(|entry| entry.path()));
    }

    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for candidate in candidates {
        if items.len() >= MAX_GENERATED_VIDEOS || !candidate.is_file() {
            continue;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !wallpaper_source::is_path_under_dir(&canonical, output_dir)
            || !seen.insert(canonical.clone())
        {
            continue;
        }
        let Ok(media) = wallpaper_source::validate_local_wallpaper_media(
            &canonical,
            LocalWallpaperMediaKind::Video,
        ) else {
            continue;
        };
        let extension = canonical
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension != media.extension {
            continue;
        }

        crate::path_scope::grant_path(&canonical);
        let path = crate::process_util::strip_extended_path_prefix(&canonical.to_string_lossy());
        let mut digest = Sha256::new();
        digest.update(path.as_bytes());
        digest.update(media.bytes.to_le_bytes());
        let id = hex::encode(digest.finalize());
        items.push(WallpaperGalleryItem {
            id: format!("imagine-video-{}", &id[..24]),
            thumb_url: format!("file://{path}"),
            full_url: format!("file://{path}"),
            kind: "video".into(),
            width: None,
            height: None,
            source: "imagine".into(),
            username: None,
            post_url: None,
            text_preview: None,
            likes: None,
            local_path: Some(path),
            prompt: motion_prompt.map(str::to_string),
            provenance: WallpaperProvenance::empty(),
            status_id: None,
            media_index: None,
            media_quality: None,
            media_fingerprint: None,
        });
    }
    items
}

fn local_path_from_value(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("file://") {
        return url::Url::parse(raw).ok()?.to_file_path().ok();
    }
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

fn failure(code: &str) -> WallpaperSearchResult {
    WallpaperSearchResult {
        items: Vec::new(),
        error_code: Some(code.to_string()),
        message: None,
        meta: None,
    }
}

fn cleanup_failed_output(output_dir: &Path) {
    let root = wallpaper_source::wallpapers_root();
    let owned_name = output_dir
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with("video-") && value.len() == 38);
    if owned_name && wallpaper_source::is_path_under_dir(output_dir, &root.join("imagine")) {
        let _ = fs::remove_dir_all(output_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grok-app-wallpaper-video-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_fake_mp4(path: &Path) {
        let mut bytes = vec![0_u8; 128];
        bytes[4..8].copy_from_slice(b"ftyp");
        bytes[8..12].copy_from_slice(b"isom");
        fs::write(path, bytes).unwrap();
    }

    fn write_fake_jpeg(path: &Path) {
        let mut bytes = vec![0_u8; 64];
        bytes[..4].copy_from_slice(&[0xff, 0xd8, 0xff, 0xdb]);
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn video_options_accept_only_supported_contract_values() {
        assert_eq!(video_options(None, None).unwrap(), (6, "480p"));
        assert_eq!(video_options(Some(10), Some("720p")).unwrap(), (10, "720p"));
        assert!(video_options(Some(7), Some("480p")).is_err());
        assert!(video_options(Some(6), Some("1080p")).is_err());
    }

    #[test]
    fn source_image_must_be_real_and_inside_wallpaper_root() {
        let root = temp_dir("source-root");
        let inside = root.join("source.jpg");
        write_fake_jpeg(&inside);
        let outside_root = temp_dir("source-outside");
        let outside = outside_root.join("source.jpg");
        write_fake_jpeg(&outside);

        assert!(validated_source_image(&inside.display().to_string(), &root).is_ok());
        assert_eq!(
            validated_source_image(&outside.display().to_string(), &root),
            Err("imagine_source_invalid")
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside_root);
    }

    #[test]
    fn output_collection_rejects_non_video_and_out_of_dir_paths() {
        let output = temp_dir("outputs");
        let accepted = output.join("result.mp4");
        write_fake_mp4(&accepted);
        let wrong_kind = output.join("result.jpg");
        write_fake_jpeg(&wrong_kind);
        let outside_root = temp_dir("outputs-outside");
        let outside = outside_root.join("outside.mp4");
        write_fake_mp4(&outside);
        let payload = serde_json::json!({
            "items": [
                { "localPath": accepted.display().to_string(), "kind": "video" },
                { "localPath": wrong_kind.display().to_string(), "kind": "video" },
                { "localPath": outside.display().to_string(), "kind": "video" }
            ]
        });

        let items = collect_generated_videos(Some(&payload), &output, Some("slow orbit"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "video");
        assert_eq!(items[0].prompt.as_deref(), Some("slow orbit"));
        assert_eq!(
            items[0].local_path.as_deref(),
            Some(accepted.display().to_string()).as_deref()
        );

        let _ = fs::remove_dir_all(output);
        let _ = fs::remove_dir_all(outside_root);
    }

    #[test]
    fn cancellation_is_sticky_when_it_arrives_before_generation() {
        let request_id = uuid::Uuid::new_v4().to_string();
        assert!(cancel(&request_id).unwrap());
        let (token, cancellation) = begin_request(&request_id);
        assert!(cancellation.is_cancelled());
        finish_request(&request_id, token);
    }
}
