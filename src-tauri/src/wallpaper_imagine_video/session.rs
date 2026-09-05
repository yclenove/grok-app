//! Accept one completed, correctly parameterized tool call in our fresh session.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::wallpaper_source::{self, LocalWallpaperMediaKind, WallpaperSearchCancellation};
use serde_json::Value;

const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 200 * 1024 * 1024;

pub(super) struct VideoInvocation<'a> {
    pub session_id: &'a str,
    pub source: &'a Path,
    pub motion: Option<&'a str>,
    pub duration: u32,
    pub resolution: &'a str,
}

fn bounded_text(path: &Path) -> Result<String, &'static str> {
    let file = fs::File::open(path).map_err(|_| "imagine_failed")?;
    let mut text = String::new();
    file.take(MAX_LOG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "imagine_failed")?;
    if text.len() as u64 > MAX_LOG_BYTES {
        return Err("imagine_failed");
    }
    Ok(text)
}

fn validate_input(input: &Value, expected: &VideoInvocation<'_>) -> Result<(), &'static str> {
    let raw = input
        .get("image")
        .and_then(Value::as_str)
        .ok_or("imagine_failed")?;
    let path = Path::new(raw);
    if !path.is_absolute()
        || path.canonicalize().ok() != expected.source.canonicalize().ok()
        || !expected.source.is_file()
        || input.get("duration").and_then(Value::as_u64) != Some(u64::from(expected.duration))
        || input.get("resolution_name").and_then(Value::as_str) != Some(expected.resolution)
        || input
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            != expected.motion
    {
        return Err("imagine_failed");
    }
    Ok(())
}

fn audit(raw: &str, expected: &VideoInvocation<'_>) -> Result<(), &'static str> {
    let mut call_id: Option<String> = None;
    let mut completed = false;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|_| "imagine_failed")?;
        let Some(update) = value.pointer("/params/update") else {
            continue;
        };
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !matches!(kind, "tool_call" | "tool_call_update") {
            continue;
        }
        if value.pointer("/params/sessionId").and_then(Value::as_str) != Some(expected.session_id) {
            return Err("imagine_failed");
        }
        let id = update
            .get("toolCallId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("imagine_failed")?;
        let name = update
            .pointer("/_meta/x.ai~1tool/name")
            .or_else(|| update.get("toolName"))
            .or_else(|| update.get("tool_name"));
        if kind == "tool_call" {
            if call_id.is_some()
                || name.or_else(|| update.get("title")).and_then(Value::as_str)
                    != Some("image_to_video")
            {
                return Err("imagine_failed");
            }
            validate_input(update.get("rawInput").ok_or("imagine_failed")?, expected)?;
            call_id = Some(id.to_string());
        } else if call_id.as_deref() != Some(id) || completed {
            return Err("imagine_failed");
        }
        if let Some(name) = name {
            if name.as_str() != Some("image_to_video") {
                return Err("imagine_failed");
            }
        }
        if let Some(input) = update.get("rawInput").filter(|v| !v.is_null()) {
            validate_input(input, expected)?;
        }
        match update.get("status").and_then(Value::as_str) {
            Some("completed") => completed = true,
            Some("failed" | "cancelled") => return Err("imagine_failed"),
            _ => {}
        }
    }
    if call_id.is_none() || !completed {
        return Err("imagine_failed");
    }
    Ok(())
}

pub(super) fn copy_result(
    expected: &VideoInvocation<'_>,
    cwd: &Path,
    cancellation: &WallpaperSearchCancellation,
) -> Result<PathBuf, &'static str> {
    let dir = crate::paths::find_agent_session_dir(
        expected.session_id,
        Some(&cwd.to_string_lossy()),
        "shared",
    )
    .ok_or("imagine_failed")?;
    let sessions = crate::paths::resolve_agent_grok_home("shared").join("sessions");
    if !wallpaper_source::is_path_under_dir(&dir, &sessions) {
        return Err("imagine_failed");
    }
    copy_from_session(&dir, expected, cwd, cancellation)
}

fn copy_from_session(
    dir: &Path,
    expected: &VideoInvocation<'_>,
    output: &Path,
    cancellation: &WallpaperSearchCancellation,
) -> Result<PathBuf, &'static str> {
    let log = dir.join("updates.jsonl");
    if !wallpaper_source::is_path_under_dir(&log, dir) {
        return Err("imagine_failed");
    }
    // Older name-only events cannot establish parameters; fail closed.
    audit(&bounded_text(&log)?, expected)?;
    let videos = dir.join("videos");
    if !wallpaper_source::is_path_under_dir(&videos, dir) {
        return Err("imagine_failed");
    }
    let candidates: Vec<_> = fs::read_dir(&videos)
        .map_err(|_| "imagine_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "imagine_failed")?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "mp4" | "webm"
            )
        })
        .collect();
    if candidates.len() != 1 {
        return Err("imagine_failed");
    }
    let path = candidates[0].canonicalize().map_err(|_| "imagine_failed")?;
    if !wallpaper_source::is_path_under_dir(&path, &videos) {
        return Err("imagine_failed");
    }
    let media =
        wallpaper_source::validate_local_wallpaper_media(&path, LocalWallpaperMediaKind::Video)
            .map_err(|_| "imagine_failed")?;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        != media.extension
    {
        return Err("imagine_failed");
    }
    let target = output.join(format!("result.{}", media.extension));
    let mut input = fs::File::open(path).map_err(|_| "imagine_failed")?;
    let mut dest = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|_| "imagine_failed")?;
    let mut total = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err("cancelled");
        }
        let count = input.read(&mut buffer).map_err(|_| "imagine_failed")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_VIDEO_BYTES {
            return Err("imagine_failed");
        }
        dest.write_all(&buffer[..count])
            .map_err(|_| "imagine_failed")?;
    }
    if total != media.bytes {
        return Err("imagine_failed");
    }
    drop(dest);
    wallpaper_source::validate_local_wallpaper_media(&target, LocalWallpaperMediaKind::Video)
        .map_err(|_| "imagine_failed")?;
    Ok(target)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
