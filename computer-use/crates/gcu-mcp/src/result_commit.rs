//! One commit for files, the image cache, and screenshot delivery.
//!
//! Staging and sealing happen first. Nothing is exported until
//! [`Results::finish`] passes the lease's commit point.

use super::result_text::prefix;
use super::{FinishFault, FinishIn, Results, COMBINED_LIMIT, TEXT_LIMIT};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;
use std::path::PathBuf;

impl Results {
    /// Seal `staged` under its export name without registering it.
    ///
    /// A probe or a stale stamp drops `staged` and everything already in
    /// `created`. The rename is outside the lease lock. No await, timeout, or
    /// cancellation. [`Self::finish`] admits the sealed names later, or their
    /// guards delete them.
    pub(super) fn keep_private(
        &self,
        mut staged: super::StagedFile,
        stamp: &super::result_guard::LiveStamp,
        point: &'static str,
        created: &mut Vec<super::StagedFile>,
    ) -> Result<PathBuf, FinishFault> {
        if self.probe.take(point) || stamp.blocks(self.scope.as_ref()) {
            drop(staged);
            created.clear();
            return Err(FinishFault::Lost);
        }
        match self.files.seal(&mut staged) {
            Ok(path) => {
                created.push(staged);
                Ok(path)
            }
            Err(_) => {
                drop(staged);
                created.clear();
                Err(FinishFault::Io)
            }
        }
    }

    pub(super) async fn finish(&self, mut input: FinishIn) -> Result<CallToolResult, FinishFault> {
        let mut json = serde_json::to_string(&input.data).map_err(|_| FinishFault::Io)?;
        if json.len() > 8_000 {
            let staged = match self.save(json, "json").await {
                Ok(staged) => staged,
                Err(_) => {
                    input.created.clear();
                    return Err(FinishFault::Io);
                }
            };
            let path = self.keep_private(staged, &input.stamp, "json-spill", &mut input.created)?;
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
            let staged = match self.save(input.text.clone(), "txt").await {
                Ok(staged) => staged,
                Err(_) => {
                    input.created.clear();
                    return Err(FinishFault::Io);
                }
            };
            let path = self.keep_private(staged, &input.stamp, "text-spill", &mut input.created)?;
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
        if self.probe.take("commit") || input.stamp.blocks(self.scope.as_ref()) {
            input.created.clear();
            return Err(FinishFault::Lost);
        }
        self.commit_outputs(&mut input)?;
        input.images.insert(0, ContentBlock::text(input.text));
        let mut result = CallToolResult::success(input.images);
        result.structured_content = Some(input.data);
        result.is_error = Some(input.failed);
        Ok(result)
    }

    /// Lock all output stores before the lease, then register one prepared batch.
    ///
    /// Lock order is snapshot, image cache, file ledger, lease. No output lock
    /// crosses an await. The lease critical section contains only infallible
    /// memory registration; readers cannot see a partially registered batch.
    fn commit_outputs(&self, input: &mut FinishIn) -> Result<(), FinishFault> {
        if let Some(lease) = &self.scope {
            self.images.bind_lease(lease.clone());
            self.files.bind_lease(lease.clone());
        }
        let authority = input.stamp.epochs().to_vec();
        let pairs: Vec<(&str, u64)> = authority
            .iter()
            .map(|(run, generation)| (run.as_str(), *generation))
            .collect();
        if let Some(lease) = &self.scope {
            if input.stamp.is_closed() || pairs.is_empty() {
                return Err(FinishFault::Lost);
            }
            lease.commit_before_gate().enter();
        }
        {
            let shot = input.pending.take();
            let mut delivery = shot.as_ref().and_then(|shot| {
                self.session.prepare_screenshot_delivery(
                    &shot.app_id,
                    &shot.window_id,
                    &shot.snapshot_id,
                    shot.width,
                    shot.height,
                )
            });
            let mut image = shot.map(|shot| {
                self.images.admission(
                    shot.id,
                    gcu_host_protocol::messages::Image {
                        mime: shot.mime,
                        width: shot.width,
                        height: shot.height,
                        bytes: shot.bytes,
                    },
                )
            });
            // Every fallible operation, lock wait, allocation and test pause
            // finishes before taking the lease. Refusal drops private guards.
            if image.is_some() && self.probe.take("cache-commit") {
                return Err(FinishFault::Lost);
            }
            let mut files = self
                .files
                .admission(&input.created)
                .map_err(|_| FinishFault::Io)?;
            let epoch = self.scope.as_ref().and_then(|_| {
                pairs
                    .first()
                    .map(|(run, generation)| ((*run).to_owned(), *generation))
            });
            let mut publish = || {
                if let Some(delivery) = &mut delivery {
                    delivery.commit();
                }
                if let Some(image) = &mut image {
                    image.stamp(epoch.clone());
                    image.commit();
                }
                files.stamp(epoch.clone());
                files.commit(&mut input.created);
            };
            if let Some(lease) = &self.scope {
                lease
                    .commit_if_current(&pairs, publish)
                    .map_err(|_| FinishFault::Lost)?;
            } else {
                publish();
            }
        }
        // Output locks and the lease have all been released. This function does
        // not undo a commit. A later get or export hides a row whose stamped
        // generation is no longer current.
        if let Some(lease) = &self.scope {
            lease.commit_after_gate().enter();
        }
        Ok(())
    }
}
