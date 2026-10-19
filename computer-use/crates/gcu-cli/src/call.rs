//! One-shot invocation, without a transport subprocess or authorization bypass.
use crate::{
    exit::{result_exit_code, Failure},
    private,
};
use gcu_mcp::Connection;
use serde_json::Value;
use std::io::Write;

pub(crate) async fn run(
    mut connection: Connection,
    tool: &str,
    args: &str,
    json: bool,
) -> Result<u8, Failure> {
    let value: Value = serde_json::from_str(args)
        .map_err(|_| Failure::arguments("--args must be a JSON object"))?;
    if !value.is_object() {
        return Err(Failure::arguments("--args must be a JSON object"));
    }
    gcu_mcp::validate_arguments(tool, value.clone()).map_err(|_| {
        Failure::arguments("Invalid tool or argument shape; check the MCP tool schema")
    })?;
    if tool == "press_key"
        && value
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(|key| gcu_protocol::keys::KeyChord::parse(key).is_err())
    {
        let result = connection.call(tool, value).await;
        let code = output(&connection, result, json).await?;
        connection.close().await?;
        return Ok(code);
    }
    // A one-shot process has no previous indices/geometry. Establish an
    // authorized fresh observation first, using the same session and router.
    if !matches!(
        tool,
        "list_apps" | "list_windows" | "capabilities" | "get_app_state" | "launch_app"
    ) {
        let mut observe = serde_json::json!({"app":value["app"],"include_screenshot":true});
        if let Some(window) = value.get("window") {
            observe["window"] = window.clone();
        }
        let preflight = connection.call("get_app_state", observe).await;
        if preflight.is_error == Some(true) {
            let code = output(&connection, preflight, json).await?;
            connection.close().await?;
            return Ok(code);
        }
    }
    let result = connection.call(tool, value).await;
    let code = output(&connection, result, json).await?;
    connection.close().await?;
    tracing::info!(tool, exit_code = code, "one-shot call completed");
    Ok(code)
}
async fn output(
    connection: &Connection,
    result: rmcp::model::CallToolResult,
    json: bool,
) -> Result<u8, Failure> {
    let files = if json {
        Vec::new()
    } else {
        connection.export_private_files().await?
    };
    private::work(move || output_sync(&result, json, files)).await
}
fn output_sync(
    result: &rmcp::model::CallToolResult,
    json: bool,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> Result<u8, Failure> {
    let code = result_exit_code(result);
    let mut out = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut out, result)
            .map_err(|_| Failure::internal("Cannot write JSON result"))?;
        writeln!(out).map_err(|_| Failure::internal("Cannot write JSON result"))?;
    } else {
        let value = serde_json::to_value(result)
            .map_err(|_| Failure::internal("Cannot render tool result"))?;
        let mut text = String::new();
        for content in value["content"].as_array().into_iter().flatten() {
            if let Some(t) = content.get("text").and_then(Value::as_str) {
                text.push_str(t);
                text.push('\n');
            }
        }
        // Copy only encoder-owned files, including small images that cannot be
        // attached inline and safe overflow text. Model text is never a path source.
        if !files.is_empty() {
            let dir = private::session_directory("gcu-call-")
                .map_err(|_| Failure::internal("Cannot create private screenshot session"))?;
            for (source, bytes) in files {
                let name = source
                    .file_name()
                    .ok_or_else(|| Failure::internal("Invalid session artifact name"))?;
                let path = dir.path().join(name);
                private::write_secret(&path, &bytes)
                    .map_err(|_| Failure::internal("Cannot save safe screenshot"))?;
                text = text.replace(
                    source.to_string_lossy().as_ref(),
                    path.to_string_lossy().as_ref(),
                );
            }
            // One-shot artifact lease: the expiry stamp is one hour from now.
            // The next CLI invocation deletes it even if the directory mtime moved.
            write_expiry(dir.path())
                .map_err(|_| Failure::internal("Cannot stamp screenshot lease"))?;
            // `TempDir::keep` disables deletion and returns the path. It does not
            // return an IO error; a missing directory still fails the one-shot call.
            let kept = dir.keep();
            if !kept.is_dir() {
                return Err(Failure::internal("Cannot retain screenshot lease"));
            }
        }
        writeln!(out, "{text}").map_err(|_| Failure::internal("Cannot write tool text"))?;
    }
    out.flush()
        .map_err(|_| Failure::internal("Cannot flush tool output"))?;
    Ok(code)
}
const KEEP_LEASE_SECS: u64 = 3600;

fn write_expiry(dir: &std::path::Path) -> std::io::Result<()> {
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .saturating_add(std::time::Duration::from_secs(KEEP_LEASE_SECS))
        .as_secs();
    write_deadline(dir, deadline)
}

fn write_deadline(dir: &std::path::Path, deadline: u64) -> std::io::Result<()> {
    use gcu_host_protocol::owner_only::{self, SyncPolicy};
    let body = format!("{deadline}\n");
    owner_only::atomic_write(&dir.join("expiry"), body.as_bytes(), SyncPolicy::Required)
}

pub(crate) fn cleanup_artifacts() {
    cleanup_root(&private::runtime_root(), std::time::SystemTime::now());
}

fn cleanup_root(root: &std::path::Path, now: std::time::SystemTime) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            warn_lease(root, &error);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn_lease(root, &error);
                continue;
            }
        };
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("gcu-call-") {
            continue;
        }
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) => {
                warn_lease(&path, &error);
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            warn_lease(
                &path,
                &std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "screenshot lease is not a real directory",
                ),
            );
            continue;
        }
        if private::check_dir(&path).is_err() {
            warn_lease(
                &path,
                &std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "screenshot lease permissions are unsafe",
                ),
            );
            continue;
        }
        let expired = match lease_expired(&path, &meta, now) {
            Ok(expired) => expired,
            Err(error) => {
                warn_lease(&path, &error);
                continue;
            }
        };
        if !expired {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(&path) {
            if error.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            warn_lease(&path, &error);
            // A partial delete can unlink the stamp and leave the directory.
            // Restore an already-due stamp so the next start retries.
            if let Err(stamp) = write_deadline(&path, 1) {
                if stamp.kind() != std::io::ErrorKind::NotFound {
                    warn_lease(&path, &stamp);
                }
            }
        }
    }
}

fn lease_expired(
    path: &std::path::Path,
    meta: &std::fs::Metadata,
    now: std::time::SystemTime,
) -> std::io::Result<bool> {
    use gcu_host_protocol::owner_only;
    match owner_only::read_no_follow(&path.join("expiry"), 64) {
        Ok(bytes) => Ok(stamp_expired(&bytes, now)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(aged_out(meta, now)),
        Err(error) => Err(error),
    }
}

fn stamp_expired(bytes: &[u8], now: std::time::SystemTime) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return true;
    };
    let Ok(deadline) = text.trim().parse::<u64>() else {
        return true;
    };
    let Ok(elapsed) = now.duration_since(std::time::UNIX_EPOCH) else {
        return true;
    };
    elapsed.as_secs() >= deadline
}

fn aged_out(meta: &std::fs::Metadata, now: std::time::SystemTime) -> bool {
    meta.modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age.as_secs() > KEEP_LEASE_SECS)
}

fn warn_lease(path: &std::path::Path, error: &std::io::Error) {
    gcu_host_protocol::owner_only::warn_meta("keep_lease", path, error);
    #[cfg(test)]
    {
        warned_leases()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.display().to_string());
    }
}

#[cfg(test)]
fn warned_leases() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use gcu_host_protocol::owner_only::{self, SyncPolicy};
    use std::os::unix::fs::PermissionsExt;
    use std::time::SystemTime;

    fn scratch() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gcu-lease-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn keep_lease_expires_on_the_stamp_even_if_mtime_is_fresh() {
        let root = scratch();
        let dir = root.join("gcu-call-stamped");
        owner_only::ensure_dir(&dir).unwrap();
        std::fs::write(dir.join("shot.png"), b"png").unwrap();
        owner_only::atomic_write(&dir.join("expiry"), b"1\n", SyncPolicy::Required).unwrap();
        cleanup_root(&root, SystemTime::now());
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keep_lease_delete_failure_warns_and_retries() {
        let root = scratch();
        let dir = root.join("gcu-call-stuck");
        owner_only::ensure_dir(&dir).unwrap();
        let nest = dir.join("nest");
        std::fs::create_dir(&nest).unwrap();
        std::fs::write(nest.join("shot.png"), b"png").unwrap();
        std::fs::set_permissions(&nest, std::fs::Permissions::from_mode(0o000)).unwrap();
        owner_only::atomic_write(&dir.join("expiry"), b"1\n", SyncPolicy::Required).unwrap();
        let now = SystemTime::now();
        cleanup_root(&root, now);
        assert!(dir.exists());
        assert!(warned_leases()
            .lock()
            .unwrap()
            .contains(&dir.display().to_string()));
        std::fs::set_permissions(&nest, std::fs::Permissions::from_mode(0o700)).unwrap();
        cleanup_root(&root, now);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
