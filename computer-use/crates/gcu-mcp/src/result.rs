//! Text/compact facts stay inside Grok's 20,000-byte prefix. Images stay safe.
#[path = "result_commit.rs"]
mod result_commit;
#[path = "result_guard.rs"]
mod result_guard;
#[path = "result_text.rs"]
mod result_text;

use result_text::{action_facts, append_action, facts, prefix, step_summary};

use crate::files::{blocking, SessionFiles, StagedFile};
use base64::{engine::general_purpose::STANDARD, Engine};
use gcu_core::{
    action::{ActionState, ToolOutcome},
    batch::BatchReport,
    redact::{EncodeOptions, RedactedImage},
    scope::ScopeLease,
    session::Session,
};
use gcu_protocol::{ErrorCode, ToolError};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::{io, path::PathBuf, sync::Arc};

pub(crate) use result_guard::FinishProbe;
pub(crate) use result_text::error;

const TEXT_LIMIT: usize = 19_000;
const COMBINED_LIMIT: usize = 20_000;
struct FinishIn {
    text: String,
    data: Value,
    images: Vec<ContentBlock>,
    failed: bool,
    stamp: result_guard::LiveStamp,
    created: Vec<StagedFile>,
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
    staged: Option<StagedFile>,
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
    /// Test-only one-shot refusal for a named finish check. Never armed in production.
    pub(crate) probe: result_guard::FinishProbe,
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
    pub async fn script(
        &self,
        output: &str,
        error: Option<ToolError>,
        produced: result_guard::LiveStamp,
    ) -> CallToolResult {
        let failed = error.is_some();
        let mut text = error
            .as_ref()
            .map(|item| format!("error: {}: {}\n", item.code, item.message))
            .unwrap_or_default();
        // Unscoped hosts keep the historical 2_000-byte prefix. Scoped output
        // stays whole so a spill file remains private until the commit point.
        let body = if self.scope.is_none() {
            prefix(output, 2_000)
        } else {
            output
        };
        text.push_str(body);
        let data = json!({
            "output": prefix(output, 2_000),
            "error": error,
        });
        self.finish_text(text, data, failed, produced).await
    }
    pub async fn discovery(
        &self,
        data: Value,
        produced: result_guard::LiveStamp,
    ) -> CallToolResult {
        // Discovery belongs in text; avoid repeating app/window lists in the
        // structured suffix. Long lists get a session-private file too.
        let text = serde_json::to_string_pretty(&data).unwrap_or_default();
        match self
            .finish(FinishIn {
                text,
                data: json!({"ok": true}),
                images: Vec::new(),
                failed: false,
                stamp: produced,
                created: Vec::new(),
                pending: None,
            })
            .await
        {
            Ok(result) => result,
            Err(_) => self.io_error(),
        }
    }
    async fn finish_text(
        &self,
        text: String,
        data: Value,
        failed: bool,
        produced: result_guard::LiveStamp,
    ) -> CallToolResult {
        match self
            .finish(FinishIn {
                text,
                data,
                images: Vec::new(),
                failed,
                stamp: produced,
                created: Vec::new(),
                pending: None,
            })
            .await
        {
            Ok(result) => result,
            Err(_) => self.io_error(),
        }
    }
    /// Epoch to carry from before a scoped read. Unscoped hosts stay off.
    ///
    /// Memory only. The lease lock is dropped before return. No timeout or
    /// cancellation. Do not call this after the read it is meant to cover.
    pub(crate) fn opened_stamp(&self) -> result_guard::LiveStamp {
        let Some(lease) = &self.scope else {
            return result_guard::LiveStamp::off();
        };
        match lease.current() {
            Some(scope) => result_guard::LiveStamp::epoch(scope.run_id(), scope.generation()),
            None => result_guard::LiveStamp::closed(),
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
                let mut encoded = match self.image(image).await {
                    Ok(encoded) => encoded,
                    Err(_) => {
                        created.clear();
                        return self.io_error();
                    }
                };
                let Some(staged) = encoded.staged.take() else {
                    created.clear();
                    return self.io_error();
                };
                encoded.path = match self.keep_private(staged, &stamp, "image-worker", &mut created)
                {
                    Ok(path) => path,
                    Err(fault) => {
                        return self.as_fault(fault, recovered_error.as_ref(), &recovered_data);
                    }
                };
                self.accept_image(&mut text, &mut images, &mut pending, state, encoded);
            }
            text.push_str(&state.text);
            if let Some(full) = state.overflow_text.take() {
                let staged = match self.save(full, "txt").await {
                    Ok(staged) => staged,
                    Err(_) => {
                        created.clear();
                        return self.io_error();
                    }
                };
                let path = match self.keep_private(staged, &stamp, "overflow-worker", &mut created)
                {
                    Ok(path) => path,
                    Err(fault) => {
                        return self.as_fault(fault, recovered_error.as_ref(), &recovered_data);
                    }
                };
                text.push_str(&format!(
                    "\n[truncated inline observation; full text: {}]\n",
                    path.to_string_lossy()
                ));
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
            let extension = if encoded.mime() == "image/jpeg" {
                "jpg"
            } else {
                "png"
            };
            let staged = files.stage(encoded.bytes(), extension)?;
            let path = staged.path().to_path_buf();
            Ok(EncodedFile {
                path,
                staged: Some(staged),
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
    async fn save(&self, text: String, extension: &'static str) -> io::Result<StagedFile> {
        let files = self.files.clone();
        blocking(move || files.stage(text.as_bytes(), extension)).await
    }
    fn as_fault(
        &self,
        fault: FinishFault,
        error: Option<&ToolError>,
        data: &Value,
    ) -> CallToolResult {
        match fault {
            FinishFault::Lost => self.assemble_cleared(error, data),
            FinishFault::Io => self.io_error(),
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

#[cfg(test)]
#[path = "result_round5_tests.rs"]
mod round5_tests;

#[cfg(test)]
#[path = "result_round6_tests.rs"]
mod round6_tests;
