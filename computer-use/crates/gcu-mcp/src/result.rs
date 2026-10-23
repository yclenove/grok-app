//! Text/compact facts stay inside Grok's 20,000-byte prefix. Images stay safe.
#[path = "result_guard.rs"]
mod result_guard;

use crate::files::{blocking, SessionFiles};
use base64::{engine::general_purpose::STANDARD, Engine};
use gcu_core::{
    action::{ActionState, ToolOutcome},
    batch::BatchReport,
    redact::{EncodeOptions, RedactedImage},
    scope::ScopeLease,
    session::Session,
};
use gcu_protocol::{result::Outcome, ActionResult, AppState, ErrorCode, ToolError};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::{io, path::PathBuf, sync::Arc};

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
fn step_summary(report: &BatchReport) -> String {
    let body = report
        .steps
        .iter()
        .map(|step| format!("{} {} {}", step.i, step.tool, outcome_name(step.outcome)))
        .collect::<Vec<_>>()
        .join("; ");
    format!("Batch: {body}.")
}
fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Done => "done",
        Outcome::Refused => "refused",
        Outcome::Failed => "failed",
        Outcome::Unknown => "unknown",
    }
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
fn append_action(text: &mut String, data: &Value) {
    let Some(action) = data.get("action") else {
        return;
    };
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
struct FinishIn {
    text: String,
    data: Value,
    images: Vec<ContentBlock>,
    failed: bool,
    stamp: result_guard::LiveStamp,
    created: Vec<PathBuf>,
    pending: Option<PendingShot>,
}
struct PendingShot {
    id: String,
    mime: String,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
    app_id: String,
    window_id: String,
    snapshot_id: String,
}
struct EncodedFile {
    path: PathBuf,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    mime: &'static str,
    scale: f64,
    reduced: bool,
    attach: bool,
}
#[derive(Debug)]
enum FinishFault {
    Lost,
    Io,
}
pub(crate) struct Results {
    pub files: Arc<SessionFiles>,
    pub session: Arc<Session>,
    pub encoding: EncodeOptions,
    pub images: Arc<crate::image_cache::ImageCache>,
    /// Fixture lease, when the router was scoped. `None` for an ordinary host.
    pub scope: Option<ScopeLease>,
}
impl Results {
    pub async fn batch(&self, report: BatchReport) -> CallToolResult {
        let mut data = json!({"steps": report.steps});
        if let Some(index) = report.failed_step {
            data["failed_step"] = json!(index);
        }
        if let Some(action) = report.failure.clone().or(report.last.clone()) {
            let mut action = action;
            action.notes.push(step_summary(&report));
            data["action"] = action_facts(&action);
        }
        let failed = report.failed_step.is_some() || report.error.is_some();
        let produced = match (&report.scope_run, report.scope_generation) {
            (Some(run), Some(generation)) => result_guard::LiveStamp::epoch(run, generation),
            _ => result_guard::LiveStamp::off(),
        };
        let mut result = self.build(report.state, report.error, data, produced).await;
        if failed {
            result.is_error = Some(true);
        }
        result
    }
    pub async fn outcome(&self, out: ToolOutcome) -> CallToolResult {
        let mut data = json!({"action":action_facts(&out.action)});
        if !out.windows.is_empty() {
            data["windows"] = json!(out.windows);
        }
        self.build(out.state, out.error, data, result_guard::LiveStamp::off())
            .await
    }
    pub async fn state(&self, state: ActionState) -> CallToolResult {
        self.build(Some(state), None, json!({}), result_guard::LiveStamp::off())
            .await
    }
    pub async fn failure(&self, error: ToolError) -> CallToolResult {
        self.build(None, Some(error), json!({}), result_guard::LiveStamp::off())
            .await
    }
    pub async fn script(&self, output: &str, error: Option<ToolError>) -> CallToolResult {
        let failed = error.is_some();
        let mut text = error
            .as_ref()
            .map(|item| format!("error: {}: {}\n", item.code, item.message))
            .unwrap_or_default();
        text.push_str(prefix(output, 2_000));
        let data = json!({
            "output": prefix(output, 2_000),
            "error": error,
        });
        match self
            .finish(FinishIn {
                text,
                data,
                images: Vec::new(),
                failed,
                stamp: result_guard::LiveStamp::off(),
                created: Vec::new(),
                pending: None,
            })
            .await
        {
            Ok(result) => result,
            Err(_) => self.io_error(),
        }
    }
    pub async fn discovery(&self, data: Value) -> CallToolResult {
        // Discovery belongs in text; avoid repeating app/window lists in the
        // structured suffix. Long lists get a session-private file too.
        let text = serde_json::to_string_pretty(&data).unwrap_or_default();
        match self
            .finish(FinishIn {
                text,
                data: json!({"ok": true}),
                images: Vec::new(),
                failed: false,
                stamp: result_guard::LiveStamp::off(),
                created: Vec::new(),
                pending: None,
            })
            .await
        {
            Ok(result) => result,
            Err(_) => self.io_error(),
        }
    }
    async fn build(
        &self,
        mut state: Option<ActionState>,
        error: Option<ToolError>,
        mut data: Value,
        produced: result_guard::LiveStamp,
    ) -> CallToolResult {
        let guard = result_guard::initial_block(
            self.scope.as_ref(),
            state.as_ref(),
            error.as_ref(),
            &data,
            &produced,
        );
        let recovered_error = error.clone();
        let recovered_data = data.clone();
        if guard.block {
            return self.assemble_cleared(recovered_error.as_ref(), &recovered_data);
        }
        let stamp = guard.stamp;
        let failed = error.is_some();
        let mut text = error
            .as_ref()
            .map(|item| format!("error: {}: {}\n", item.code, item.message))
            .unwrap_or_default();
        let mut images = Vec::new();
        let mut created = Vec::new();
        let mut pending = None;
        if let Some(state) = &mut state {
            if let Some(image) = state.image.take() {
                let encoded = match self.image(image).await {
                    Ok(encoded) => encoded,
                    Err(_) => {
                        self.discard_all(&created);
                        return self.io_error();
                    }
                };
                created.push(encoded.path.clone());
                if self.rejected(&stamp) {
                    self.discard_all(&created);
                    return self.assemble_cleared(recovered_error.as_ref(), &recovered_data);
                }
                self.accept_image(&mut text, &mut images, &mut pending, state, encoded);
            }
            text.push_str(&state.text);
            if let Some(full) = state.overflow_text.take() {
                match self.save(full, "txt").await {
                    Ok(path) => {
                        created.push(path.clone());
                        if self.rejected(&stamp) {
                            self.discard_all(&created);
                            return self
                                .assemble_cleared(recovered_error.as_ref(), &recovered_data);
                        }
                        text.push_str(&format!(
                            "\n[truncated inline observation; full text: {}]\n",
                            path.to_string_lossy()
                        ));
                    }
                    Err(_) => {
                        self.discard_all(&created);
                        return self.io_error();
                    }
                }
            }
            data["state"] = facts(&state.state);
        }
        if let Some(error) = error {
            let fresh = state
                .as_ref()
                .map(|item| facts(&item.state))
                .or_else(|| error.state.as_ref().map(facts));
            let mut value = json!({"code":error.code,"message":error.message,"hint":error.hint,"retryable":error.retryable});
            if let Some(fresh) = fresh {
                value["state"] = fresh;
            }
            data["error"] = value;
        }
        append_action(&mut text, &data);
        match self
            .finish(FinishIn {
                text,
                data,
                images,
                failed,
                stamp,
                created,
                pending,
            })
            .await
        {
            Ok(result) => result,
            Err(FinishFault::Lost) => {
                self.assemble_cleared(recovered_error.as_ref(), &recovered_data)
            }
            Err(FinishFault::Io) => self.io_error(),
        }
    }
    fn accept_image(
        &self,
        text: &mut String,
        images: &mut Vec<ContentBlock>,
        pending: &mut Option<PendingShot>,
        state: &mut ActionState,
        encoded: EncodedFile,
    ) {
        text.push_str(&format!(
            "Screenshot file: {}\n",
            encoded.path.to_string_lossy()
        ));
        if encoded.reduced {
            text.push_str("[image reduced to meet PNG byte budget]\n");
        }
        if encoded.attach {
            images.push(ContentBlock::image(
                STANDARD.encode(&encoded.bytes),
                encoded.mime,
            ));
        } else {
            text.push_str(
                "[image attachment omitted: short edge below 256; use screenshot file]\n",
            );
        }
        let app_id = state.state.app.id.clone();
        let window_id = state.state.window.id.clone();
        let snapshot_id = state.state.snapshot_id.clone();
        let Some(shot) = state.state.screenshot.as_mut() else {
            return;
        };
        let id = shot.id.clone();
        shot.width = encoded.width;
        shot.height = encoded.height;
        shot.scale_to_window = Some(encoded.scale);
        shot.mime = encoded.mime.into();
        *pending = Some(PendingShot {
            id,
            mime: encoded.mime.into(),
            width: encoded.width,
            height: encoded.height,
            bytes: encoded.bytes,
            app_id,
            window_id,
            snapshot_id,
        });
    }
    async fn image(&self, image: RedactedImage) -> io::Result<EncodedFile> {
        let files = self.files.clone();
        let encoding = self.encoding;
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
                .encode(encoding)
                .map_err(|_| io::Error::other("safe image encoding failed"))?;
            let path = files.write(
                encoded.bytes(),
                if encoded.mime() == "image/jpeg" {
                    "jpg"
                } else {
                    "png"
                },
            )?;
            Ok(EncodedFile {
                path,
                bytes: encoded.bytes().to_vec(),
                width: encoded.width(),
                height: encoded.height(),
                mime: encoded.mime(),
                scale: encoded.scale_to_window(),
                reduced: encoded.reduced(),
                attach: encoded.can_attach(),
            })
        })
        .await
    }
    async fn save(&self, text: String, extension: &'static str) -> io::Result<PathBuf> {
        let files = self.files.clone();
        blocking(move || files.write(text.as_bytes(), extension)).await
    }
    async fn finish(&self, mut input: FinishIn) -> Result<CallToolResult, FinishFault> {
        let mut json = serde_json::to_string(&input.data).map_err(|_| FinishFault::Io)?;
        if json.len() > 8_000 {
            let path = match self.save(json, "json").await {
                Ok(path) => path,
                Err(_) => {
                    self.discard_all(&input.created);
                    return Err(FinishFault::Io);
                }
            };
            input.created.push(path.clone());
            if self.rejected(&input.stamp) {
                self.discard_all(&input.created);
                return Err(FinishFault::Lost);
            }
            // Preserve error identity and the fresh snapshot ID in the prefix.
            let shown = path.to_string_lossy();
            let mut compact = json!({ "details_file": shown });
            if let Some(error) = input.data.get("error") {
                compact["error"] = json!({"code":error["code"],"message":prefix(error["message"].as_str().unwrap_or(""), 256),
                    "hint":"See details_file for full facts.","retryable":false});
                if let Some(id) = error.pointer("/state/snapshot_id") {
                    compact["error"]["state"] = json!({"snapshot_id":id});
                }
            }
            if let Some(action) = input.data.get("action") {
                compact["action"] = action.clone();
            }
            if let Some(steps) = input.data.get("steps") {
                compact["steps"] = steps.clone();
            }
            if let Some(index) = input.data.get("failed_step") {
                compact["failed_step"] = index.clone();
            }
            input.data = compact;
            json = serde_json::to_string(&input.data).map_err(|_| FinishFault::Io)?;
        }
        let budget = TEXT_LIMIT.min(COMBINED_LIMIT.saturating_sub(json.len() + 64));
        if input.text.len() > budget {
            let path = match self.save(input.text.clone(), "txt").await {
                Ok(path) => path,
                Err(_) => {
                    self.discard_all(&input.created);
                    return Err(FinishFault::Io);
                }
            };
            input.created.push(path.clone());
            if self.rejected(&input.stamp) {
                self.discard_all(&input.created);
                return Err(FinishFault::Lost);
            }
            let marker = format!(
                "\n[truncated to 20 KB result budget; full text: {}]\n",
                path.to_string_lossy()
            );
            input.text = format!(
                "{}{}",
                prefix(&input.text, budget.saturating_sub(marker.len())),
                marker
            );
        }
        if self.rejected(&input.stamp) {
            self.discard_all(&input.created);
            return Err(FinishFault::Lost);
        }
        if let Some(shot) = input.pending.take() {
            self.images.insert(
                shot.id,
                gcu_host_protocol::messages::Image {
                    mime: shot.mime,
                    width: shot.width,
                    height: shot.height,
                    bytes: shot.bytes,
                },
            );
            self.session.screenshot_delivered(
                &shot.app_id,
                &shot.window_id,
                &shot.snapshot_id,
                shot.width,
                shot.height,
            );
        }
        input.images.insert(0, ContentBlock::text(input.text));
        let mut result = CallToolResult::success(input.images);
        result.structured_content = Some(input.data);
        result.is_error = Some(input.failed);
        Ok(result)
    }
    fn rejected(&self, stamp: &result_guard::LiveStamp) -> bool {
        stamp.blocks(self.scope.as_ref())
    }
    fn discard_all(&self, paths: &[PathBuf]) {
        for path in paths {
            self.files.discard(path);
        }
    }
    fn assemble_cleared(&self, error: Option<&ToolError>, data: &Value) -> CallToolResult {
        let mut data = data.clone();
        result_guard::scrub(&mut data);
        let mut text = String::new();
        if let Some(error) = error {
            text.push_str(&format!("error: {}: {}\n", error.code, error.message));
            data["error"] = json!({
                "code": error.code,
                "message": error.message,
                "hint": error.hint,
                "retryable": error.retryable,
            });
        }
        append_action(&mut text, &data);
        let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
        result.structured_content = Some(data);
        result.is_error = Some(error.is_some());
        result
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

#[cfg(test)]
#[path = "result_scope_tests.rs"]
mod scope_tests;
