//! Read-only CLI preflight. This never consumes an action or grants authority.
use super::*;
use crate::protocol::ActionRequest;

impl ComputerUseBroker {
    pub fn preflight_tool_permission(
        &self,
        token: &str,
        tool: &str,
        args: &serde_json::Value,
    ) -> Result<(), BrokerError> {
        if !args.is_object() || crate::protocol::forbidden_permission_param(args).is_some() {
            return Err(BrokerError::Schema("invalid tool permission input".into()));
        }
        if tool == "computer_status" && self.ungranted_access_status(token, args)?.is_some() {
            return Ok(());
        }
        if tool == "computer_list_targets" {
            return self.approved_task_targets(token, args).map(|_| ());
        }
        let app_webview_navigation =
            tool == "computer_navigate" && args.get("targetGeneration").is_some();
        let app_webview_download =
            tool == "computer_download" && args.get("targetGeneration").is_some();
        let claim = crate::tools_claim::execution_claim(args)?;
        let claimed_target = if app_webview_navigation || app_webview_download {
            None
        } else {
            claim.target_id
        };
        let permit = self.begin_model_call_claiming(token, tool, claim.run_id, claimed_target)?;
        if tool == "computer_upload" {
            let intent = crate::tools_upload::parse(args)?;
            return self.preflight_webview_upload(&intent, &permit);
        }
        if tool == "computer_open_target" {
            if claim.target_id.is_none() {
                return Err(BrokerError::IdentityMismatch("targetId"));
            }
            return self.preflight_scope_switch(&permit);
        }
        if app_webview_navigation {
            match self.replay_webview_navigation_for_permit(&permit, args) {
                Ok(Some(_)) | Err(BrokerError::Adapter(_)) => return Ok(()),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        if app_webview_download {
            match self.replay_webview_download_for_permit(&permit, args) {
                Ok(Some(_)) | Err(BrokerError::Adapter(_)) => return Ok(()),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        self.recheck_model_call(&permit)?;
        if matches!(tool, "computer_act" | "computer_wait") {
            let request: ActionRequest = if tool == "computer_wait" {
                crate::tools_wait::parse_wait_action(args.clone())
            } else {
                serde_json::from_value(args.clone())
            }
            .map_err(|_| BrokerError::Schema("invalid action request".into()))?;
            request.validate_schema().map_err(BrokerError::Schema)?;
            if request.run_id != permit.run_id {
                return Err(BrokerError::IdentityMismatch("runId"));
            }
            let inner = self.inner.lock();
            inner
                .consent
                .recheck_permit(&permit, inner.feature_enabled, inner.feature_epoch)?;
            let run = inner
                .runs
                .get(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.paused || run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            super::action_claim::validate_action_observation(run, &request)?;
        }
        if app_webview_navigation {
            let request = crate::tools::parse_webview_navigation_request(
                &permit.run_id,
                Some(&permit.target_id),
                args,
            )?;
            if request.target_id != permit.target_id {
                return Err(BrokerError::IdentityMismatch("targetId"));
            }
            let inner = self.inner.lock();
            inner
                .consent
                .recheck_permit(&permit, inner.feature_enabled, inner.feature_epoch)?;
            let run = inner
                .runs
                .get(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.paused || run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if !run.target.as_ref().is_some_and(|target| {
                target.surface == SurfaceKind::WebView
                    && target.target_id == request.target_id
                    && target.target_generation == request.target_generation
                    && !target.closed
            }) {
                return Err(BrokerError::IdentityMismatch("targetGeneration"));
            }
            drop(inner);
            return self.recheck_model_call(&permit);
        }
        if app_webview_download {
            let request = crate::tools::parse_webview_download_intent(
                &permit.run_id,
                Some(&permit.target_id),
                args,
            )?;
            let inner = self.inner.lock();
            inner
                .consent
                .recheck_permit(&permit, inner.feature_enabled, inner.feature_epoch)?;
            let run = inner
                .runs
                .get(&permit.run_id)
                .ok_or(BrokerError::RunNotFound)?;
            if run.paused || run.stop != StopState::Running {
                return Err(BrokerError::StopRequested);
            }
            if !run.target.as_ref().is_some_and(|target| {
                target.surface == SurfaceKind::WebView
                    && target.target_id == request.target_id
                    && target.target_generation == request.target_generation
                    && !target.closed
            }) {
                return Err(BrokerError::IdentityMismatch("targetGeneration"));
            }
            if run.snapshot_id.as_deref() != Some(&request.snapshot_id) {
                return Err(BrokerError::IdentityMismatch("snapshotId"));
            }
            if !run.element_refs.contains(&request.element_ref) {
                return Err(BrokerError::IdentityMismatch("elementRef"));
            }
            drop(inner);
            return self.recheck_model_call(&permit);
        }
        if matches!(
            tool,
            "browser_act"
                | "computer_download"
                | "computer_navigate"
                | "browser_observe"
                | "browser_open"
        ) {
            let tab = args
                .get("tabId")
                .and_then(serde_json::Value::as_str)
                .ok_or(BrokerError::IdentityMismatch("tabId"))?;
            let identity = self.tabs.tab_write_identity(&permit.run_id, tab)?;
            if tool != "browser_open"
                && args
                    .get("pageGeneration")
                    .and_then(serde_json::Value::as_u64)
                    != Some(identity.page_generation)
            {
                return Err(BrokerError::IdentityMismatch("pageGeneration"));
            }
            if matches!(tool, "browser_act" | "computer_download") {
                if identity.snapshot_id.as_deref()
                    != args.get("snapshotId").and_then(serde_json::Value::as_str)
                    || identity.snapshot_id.is_none()
                {
                    return Err(BrokerError::IdentityMismatch("snapshotId"));
                }
                if let Some(reference) = args.get("elementRef").and_then(serde_json::Value::as_str)
                {
                    if !identity.refs.contains(reference) {
                        return Err(BrokerError::IdentityMismatch("elementRef"));
                    }
                }
            }
        }
        self.recheck_model_call(&permit)
    }
}
