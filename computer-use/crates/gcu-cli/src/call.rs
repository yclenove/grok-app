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
            // One-shot artifact lease: retained for inspection, removed by the
            // next CLI invocation after one hour (or manually by the user).
            let _ = dir.keep();
        }
        writeln!(out, "{text}").map_err(|_| Failure::internal("Cannot write tool text"))?;
    }
    out.flush()
        .map_err(|_| Failure::internal("Cannot flush tool output"))?;
    Ok(code)
}
pub(crate) fn cleanup_artifacts() {
    let Ok(entries) = std::fs::read_dir(private::runtime_root()) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("gcu-call-")
            || private::check_dir(&entry.path()).is_err()
        {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600);
        if expired && std::fs::remove_dir_all(entry.path()).is_err() {
            tracing::warn!("Expired screenshot lease could not be removed");
        }
    }
}
