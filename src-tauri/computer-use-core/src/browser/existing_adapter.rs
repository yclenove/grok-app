//! Run-scoped adapter over authenticated MV3 transport. No desktop/managed fallback.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use super::{ExistingTabHost, TabInfo};
use crate::adapter::{
    ActionSupport, AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter,
    DispatchRequest, SurfaceKind, TargetInfo,
};
use crate::browser::extension_action::ExistingActionInput;
use crate::browser::extension_action::{ActKind, ActionStatus, ExtensionActionCommand};
use crate::protocol::{
    ActionKind, ActionTarget, Observation, ObservationImage, ObservationNode, PROTOCOL_VERSION,
};

fn element_ref(req: &DispatchRequest) -> Result<&str, String> {
    match &req.target {
        ActionTarget::Element { element_ref } if !element_ref.trim().is_empty() => Ok(element_ref),
        ActionTarget::Element { .. } => Err("existing tab requires a non-empty elementRef".into()),
        ActionTarget::Coord { .. } => {
            Err("existing tab typed actions require a semantic element target".into())
        }
    }
}

fn decode_parameters<T: serde::de::DeserializeOwned>(
    action: ActionKind,
    parameters: &serde_json::Value,
) -> Result<T, String> {
    serde_json::from_value(parameters.clone())
        .map_err(|error| format!("invalid existing-tab {action:?} parameters: {error}"))
}

/// Convert the model-facing action contract into the deliberately smaller MV3
/// action protocol. This is the last boundary before the Host queue: no model
/// parameters or coordinate target is forwarded without an explicit mapping.
fn extension_command(req: &DispatchRequest) -> Result<ExtensionActionCommand, String> {
    let element_ref = element_ref(req)?.to_string();
    let snapshot_id = req.snapshot_id.clone();
    let kind = ActKind::Act;
    let command = match req.action {
        ActionKind::Click => Ok(ExtensionActionCommand::Click {
            kind,
            snapshot_id,
            element_ref,
            parameters: decode_parameters(req.action, &req.parameters)?,
        }),
        ActionKind::SetValue => Ok(ExtensionActionCommand::SetValue {
            kind,
            snapshot_id,
            element_ref,
            parameters: decode_parameters(req.action, &req.parameters)?,
        }),
        ActionKind::TypeText => {
            if req.parameters.get("via").is_some() {
                return Err("existing tab type_text does not support via=clipboard".into());
            }
            Ok(ExtensionActionCommand::TypeText {
                kind,
                snapshot_id,
                element_ref,
                parameters: decode_parameters(req.action, &req.parameters)?,
            })
        }
        ActionKind::Scroll => Ok(ExtensionActionCommand::Scroll {
            kind,
            snapshot_id,
            element_ref,
            parameters: decode_parameters(req.action, &req.parameters)?,
        }),
        ActionKind::Wait => Ok(ExtensionActionCommand::Wait {
            kind,
            snapshot_id,
            element_ref,
            parameters: decode_parameters(req.action, &req.parameters)?,
        }),
        ActionKind::Key | ActionKind::Drag => Err(format!(
            "existing tab action {:?} is not supported by the MV3 protocol",
            req.action
        )),
    }?;
    command
        .valid()
        .then_some(command)
        .ok_or_else(|| "existing tab action parameters are unsupported by the MV3 protocol".into())
}

fn node_actions(role: &str, disabled: bool) -> Vec<String> {
    if disabled {
        return Vec::new();
    }
    let mut actions = vec!["click".into(), "wait".into()];
    if matches!(role, "textbox" | "searchbox") {
        actions.extend(["set_value".into(), "type_text".into()]);
    }
    actions
}

pub struct ExistingBrowserAdapter {
    host: Arc<ExistingTabHost>,
    preview: AtomicBool,
}

fn raw_tab_id(target: &str) -> Option<&str> {
    let raw = target.strip_prefix("tab:")?;
    let number = raw.parse::<u64>().ok()?;
    (number <= crate::protocol::JS_MAX_SAFE_INTEGER && number.to_string() == raw).then_some(raw)
}

impl ExistingBrowserAdapter {
    pub fn new(host: Arc<ExistingTabHost>) -> Self {
        Self {
            host,
            preview: AtomicBool::new(false),
        }
    }

    fn require_target(&self, run: &str, target: &str) -> Result<TabInfo, String> {
        let raw = raw_tab_id(target).ok_or("invalid existing-tab target")?;
        let info = self
            .host
            .existing_target_info(raw)
            .ok_or("existing tab is no longer shared or authorized")?;
        if info.run_id != run {
            return Err("existing tab belongs to another run".into());
        }
        Ok(info)
    }

    fn target_info(info: TabInfo) -> TargetInfo {
        TargetInfo {
            target_id: format!("tab:{}", info.tab_id),
            title: info.title,
            app_name: "existing-tabs".into(),
            kind: "existing-tab".into(),
            pid: None,
            backend: "chromium-extension".into(),
            execution_mode: "borrow".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: info.document_generation,
            display_id: "existing-tabs".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: format!("{}/{}", info.session, info.run_id),
        }
    }
}

impl ComputerUseAdapter for ExistingBrowserAdapter {
    fn backend_id(&self) -> &'static str {
        "chromium-extension"
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::for_surface(SurfaceKind::ExistingTab, self.backend_id());
        caps.observe_ax = true;
        caps.observe_screenshot = true;
        caps.semantic_click = true;
        caps.coordinate_click = false;
        caps.unicode_text = true;
        caps.chinese_ime = false;
        caps.click = ActionSupport::SEMANTIC;
        caps.set_value = ActionSupport::SEMANTIC;
        caps.type_text = ActionSupport::SEMANTIC;
        caps.key = ActionSupport::NONE;
        caps.scroll = ActionSupport::SEMANTIC;
        caps.drag = ActionSupport::NONE;
        caps.wait = ActionSupport::SEMANTIC;
        caps.notes = vec![
            "Explicitly shared current tab; screenshots require a browser activeTab grant".into(),
            "Typed semantic actions require the negotiated MV3 v2 action transport".into(),
            "Coordinate, key, drag, clipboard and arbitrary evaluate actions are unavailable"
                .into(),
        ];
        caps
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(Vec::new())
    }
    fn list_targets_for_run(&self, run: &str) -> Result<Vec<TargetInfo>, String> {
        Ok(self
            .host
            .list_for_run(run)
            .into_iter()
            .filter(|info| info.user_owned && info.borrowed)
            .filter_map(|info| self.host.existing_target_info(&info.tab_id))
            .map(Self::target_info)
            .collect())
    }
    fn target_alive(&self, target: &str) -> bool {
        raw_tab_id(target)
            .and_then(|raw| self.host.existing_target_info(raw))
            .is_some()
    }
    fn observe(&self, _target: &str) -> Result<Observation, String> {
        Err("existing tab requires a run-scoped observation".into())
    }
    fn observe_for_run(&self, run: &str, target: &str) -> Result<Observation, String> {
        self.capture_for_run(run, target, CaptureOptions::model(true))
    }
    fn capture_for_run(
        &self,
        run: &str,
        target: &str,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        let info = self.require_target(run, target)?;
        let observed = self
            .host
            .capture_existing_document(&info.session, run, &info.tab_id, options)
            .map_err(|e| e.to_string())?;
        let current = self.require_target(run, target)?;
        if current.generation != info.generation
            || current.document_generation != info.document_generation
        {
            return Err("existing tab grant changed during observation".into());
        }
        let image = match observed.screenshot {
            Some(image) => ObservationImage {
                width: image.width,
                height: image.height,
                content_id: format!("extension-{}", observed.snapshot_id),
                png_base64: Some(image.png_base64),
            },
            None => ObservationImage {
                width: 0,
                height: 0,
                content_id: String::new(),
                png_base64: None,
            },
        };
        Ok(Observation {
            version: PROTOCOL_VERSION,
            run_id: run.into(),
            target_id: target.into(),
            target_generation: info.generation,
            snapshot_id: observed.snapshot_id,
            captured_at: chrono::Utc::now().to_rfc3339(),
            geometry_revision: info.document_generation,
            coordinate_space: "image-pixels".into(),
            crop_width: image.width,
            crop_height: image.height,
            image,
            nodes: observed
                .nodes
                .into_iter()
                .map(|node| {
                    let actions = node_actions(&node.role, node.disabled);
                    ObservationNode {
                        node_ref: node.element_ref,
                        role: node.role,
                        name: node.name,
                        actions,
                        truncated: false,
                        x: None,
                        y: None,
                        width: None,
                        height: None,
                    }
                })
                .collect(),
            text: observed.text,
            truncated: observed.truncated,
            crop_x: 0,
            crop_y: 0,
            scale: 1.0,
            dpi: 96.0,
            origin_x: 0,
            origin_y: 0,
            topology_revision: info.document_generation,
        })
    }
    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        req.admit(self)?;
        let info = self.require_target(&req.run_id, &req.target_id)?;
        let command = extension_command(req)?;
        let outcome = self
            .host
            .execute_existing_action(ExistingActionInput {
                session: &info.session,
                run: &req.run_id,
                tab: &info.tab_id,
                grant_generation: info.generation,
                document_generation: info.document_generation,
                command,
                cancellation: req.cancellation.clone(),
            })
            .map_err(|error| error.to_string())?;
        match outcome.status {
            ActionStatus::Applied => Ok(AdapterActResult {
                applied: true,
                outcome: None,
                postcondition_ok: false,
                verifiable: false,
                detail: format!(
                    "existing tab action applied; observe again to verify ({})",
                    outcome.detail
                ),
            }),
            ActionStatus::Verified => Ok(AdapterActResult {
                applied: true,
                outcome: None,
                postcondition_ok: true,
                verifiable: true,
                detail: format!("existing tab action verified ({})", outcome.detail),
            }),
            ActionStatus::Rejected => Ok(AdapterActResult {
                applied: false,
                outcome: Some(crate::protocol::OutcomeKind::Rejected),
                postcondition_ok: false,
                verifiable: true,
                detail: format!(
                    "existing tab action rejected before a side effect: {}",
                    outcome.detail
                ),
            }),
            ActionStatus::Unknown => Err(format!(
                "existing tab action outcome is unknown; observe before retrying: {}",
                outcome.detail
            )),
        }
    }
    fn abort(&self, run: &str, _generation: u64) -> Result<(), String> {
        self.host.cancel_existing_operations(run);
        Ok(())
    }
    fn is_idle(&self, run: &str) -> bool {
        self.host.existing_operations_idle(run)
    }
    fn start_periodic_preview(&self, target: &str) {
        if self.target_alive(target) {
            self.preview.store(true, Ordering::SeqCst);
        }
    }
    fn stop_periodic_preview(&self) {
        self.preview.store(false, Ordering::SeqCst);
    }
    fn periodic_preview_active(&self) -> bool {
        self.preview.load(Ordering::SeqCst)
    }
    fn current_geometry_revision_for(&self, target: &str) -> u64 {
        raw_tab_id(target)
            .and_then(|raw| self.host.existing_target_info(raw))
            .map(|info| info.document_generation)
            .unwrap_or(0)
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        self.require_target(run, target).map(|_| ())
    }
    fn release_target_for_run(&self, run: &str, target: &str) {
        if let Ok(info) = self.require_target(run, target) {
            self.host.release_existing_grant(&info);
        }
    }
    fn worker_attached(&self) -> bool {
        self.host.extension_connected()
    }
}

#[cfg(test)]
mod tests;
