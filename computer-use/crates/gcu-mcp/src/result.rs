//! Text/compact facts stay inside Grok's 20,000-byte prefix. Images stay safe.
use crate::files::{blocking, SessionFiles};
use base64::{engine::general_purpose::STANDARD, Engine};
use gcu_core::{
    action::{ActionState, ToolOutcome},
    redact::{EncodeOptions, RedactedImage},
    session::Session,
};
use gcu_protocol::{ActionResult, AppState, ErrorCode, ToolError};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::{io, sync::Arc};

const TEXT_LIMIT: usize = 19_000;
const COMBINED_LIMIT: usize = 20_000;

pub(crate) fn error(code: ErrorCode) -> ToolError {
    ToolError {
        code,
        message: code.as_str().replace('_', " "),
        hint: "Use fresh state and obtain user authorization when required.".into(),
        retryable: false,
        state: None,
    }
}
fn prefix(text: &str, budget: usize) -> &str {
    let mut n = text.len().min(budget);
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    &text[..n]
}
fn facts(state: &AppState) -> Value {
    json!({"app_id":state.app.id,"window_id":state.window.id,"snapshot_id":state.snapshot_id,
        "screenshot":state.screenshot,"focused":state.focused,"truncated":state.truncated})
}
fn action_facts(action: &ActionResult) -> Value {
    let mut action = action.clone();
    if let Some(gcu_protocol::result::ActionTarget::Element { label, .. }) = &mut action.target {
        *label = prefix(label, 256).into();
    }
    if let Some(gcu_protocol::result::ActionTarget::Coordinates { screenshot_id, .. }) =
        &mut action.target
    {
        *screenshot_id = prefix(screenshot_id, 256).into();
    }
    if let Some(verify) = &mut action.verify {
        verify.expect = prefix(&verify.expect, 256).into();
    }
    action.notes.truncate(4);
    for note in &mut action.notes {
        *note = prefix(note, 256).into();
    }
    json!(action)
}
pub(crate) struct Results {
    pub files: Arc<SessionFiles>,
    pub session: Arc<Session>,
}
impl Results {
    pub async fn outcome(&self, out: ToolOutcome) -> CallToolResult {
        let mut data = json!({"action":action_facts(&out.action)});
        if !out.windows.is_empty() {
            data["windows"] = json!(out.windows);
        }
        self.build(out.state, out.error, data).await
    }
    pub async fn state(&self, state: ActionState) -> CallToolResult {
        self.build(Some(state), None, json!({})).await
    }
    pub async fn failure(&self, error: ToolError) -> CallToolResult {
        self.build(None, Some(error), json!({})).await
    }
    pub async fn discovery(&self, data: Value) -> CallToolResult {
        // Discovery belongs in text; avoid repeating app/window lists in the
        // structured suffix. Long lists get a session-private file too.
        let text = serde_json::to_string_pretty(&data).unwrap_or_default();
        match self
            .finish(text, json!({"ok":true}), Vec::new(), false)
            .await
        {
            Ok(r) => r,
            Err(_) => self.io_error(),
        }
    }
    async fn build(
        &self,
        mut state: Option<ActionState>,
        error: Option<ToolError>,
        mut data: Value,
    ) -> CallToolResult {
        let failed = error.is_some();
        let mut text = error
            .as_ref()
            .map(|e| format!("error: {}: {}\n", e.code, e.message))
            .unwrap_or_default();
        let mut images = Vec::new();
        if let Some(state) = &mut state {
            if let Some(image) = state.image.take() {
                match self.image(image).await {
                    Ok((encoded, path)) => {
                        text.push_str(&format!("Screenshot file: {path}\n"));
                        if encoded.reduced() {
                            text.push_str("[image reduced to meet PNG byte budget]\n");
                        }
                        if encoded.can_attach() {
                            images.push(ContentBlock::image(
                                STANDARD.encode(encoded.bytes()),
                                encoded.mime(),
                            ));
                        } else {
                            text.push_str("[image attachment omitted: short edge below 256; use screenshot file]\n");
                        }
                        if let Some(shot) = &mut state.state.screenshot {
                            shot.width = encoded.width();
                            shot.height = encoded.height();
                            shot.scale_to_window = Some(encoded.scale_to_window());
                            shot.mime = encoded.mime().into();
                        }
                        self.session.screenshot_delivered(
                            &state.state.app.id,
                            &state.state.window.id,
                            &state.state.snapshot_id,
                            encoded.width(),
                            encoded.height(),
                        );
                    }
                    Err(_) => return self.io_error(),
                }
            }
            text.push_str(&state.text);
            if let Some(full) = state.overflow_text.take() {
                match self.save(full, "txt").await {
                    Ok(path) => text.push_str(&format!(
                        "\n[truncated inline observation; full text: {path}]\n"
                    )),
                    Err(_) => return self.io_error(),
                }
            }
            data["state"] = facts(&state.state);
        }
        if let Some(error) = error {
            let fresh = state
                .as_ref()
                .map(|s| facts(&s.state))
                .or_else(|| error.state.as_ref().map(facts));
            let mut value = json!({"code":error.code,"message":error.message,"hint":error.hint,"retryable":error.retryable});
            if let Some(fresh) = fresh {
                value["state"] = fresh;
            }
            data["error"] = value;
        }
        if let Some(action) = data.get("action") {
            text.push_str(&format!(
                "\nAction: {} — {} ({}).",
                action["tool"].as_str().unwrap_or(""),
                action["outcome"].as_str().unwrap_or(""),
                action["effect"].as_str().unwrap_or("")
            ));
            if let Some(notes) = action["notes"].as_array() {
                for note in notes {
                    if let Some(note) = note.as_str() {
                        text.push('\n');
                        text.push_str(note);
                    }
                }
            }
        }
        match self.finish(text, data, images, failed).await {
            Ok(r) => r,
            Err(_) => self.io_error(),
        }
    }
    async fn image(
        &self,
        image: RedactedImage,
    ) -> io::Result<(gcu_core::redact::EncodedImage, String)> {
        let files = self.files.clone();
        blocking(move || {
            // Cropping the entire safe image uses the core's bounded enlargement
            // path; no raw Capture can enter this function.
            let enlarged = if image.width().min(image.height()) < 256 {
                image
                    .approval_crop(gcu_core::model::Rect {
                        x: image.origin().x,
                        y: image.origin().y,
                        w: f64::from(image.width()) * image.scale_to_window(),
                        h: f64::from(image.height()) * image.scale_to_window(),
                    })
                    .ok()
            } else {
                None
            };
            let image = enlarged.as_ref().unwrap_or(&image);
            let encoded = image
                .encode(EncodeOptions::default())
                .map_err(|_| io::Error::other("safe image encoding failed"))?;
            let path = files.write(encoded.bytes(), "png")?;
            Ok((encoded, path.to_string_lossy().into_owned()))
        })
        .await
    }
    async fn save(&self, text: String, extension: &'static str) -> io::Result<String> {
        let files = self.files.clone();
        blocking(move || {
            files
                .write(text.as_bytes(), extension)
                .map(|p| p.to_string_lossy().into_owned())
        })
        .await
    }
    async fn finish(
        &self,
        mut text: String,
        mut data: Value,
        mut images: Vec<ContentBlock>,
        failed: bool,
    ) -> io::Result<CallToolResult> {
        let mut json = serde_json::to_string(&data).map_err(io::Error::other)?;
        if json.len() > 8_000 {
            let path = self.save(json, "json").await?;
            // Preserve error identity and the fresh snapshot ID in the prefix.
            let mut compact = json!({"details_file":path});
            if let Some(error) = data.get("error") {
                compact["error"] = json!({"code":error["code"],"message":prefix(error["message"].as_str().unwrap_or(""), 256),
                    "hint":"See details_file for full facts.","retryable":false});
                if let Some(id) = error.pointer("/state/snapshot_id") {
                    compact["error"]["state"] = json!({"snapshot_id":id});
                }
            }
            if let Some(action) = data.get("action") {
                compact["action"] = action.clone();
            }
            data = compact;
            json = serde_json::to_string(&data).map_err(io::Error::other)?;
        }
        let budget = TEXT_LIMIT.min(COMBINED_LIMIT.saturating_sub(json.len() + 64));
        if text.len() > budget {
            let path = self.save(text.clone(), "txt").await?;
            let marker = format!("\n[truncated to 20 KB result budget; full text: {path}]\n");
            text = format!(
                "{}{}",
                prefix(&text, budget.saturating_sub(marker.len())),
                marker
            );
        }
        images.insert(0, ContentBlock::text(text));
        let mut result = CallToolResult::success(images);
        result.structured_content = Some(data);
        result.is_error = Some(failed);
        Ok(result)
    }
    fn io_error(&self) -> CallToolResult {
        let error = error(ErrorCode::Internal);
        let mut result = CallToolResult::error(vec![ContentBlock::text(format!(
            "error: {}: {}",
            error.code, error.message
        ))]);
        result.structured_content = Some(json!({"error":error}));
        result
    }
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
