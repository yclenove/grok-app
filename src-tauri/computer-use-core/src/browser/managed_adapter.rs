//! Generic Computer Use adapter over the Broker-owned managed-browser Host.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{ExistingTabHost, ManagedLocator, ManagedObservation, ManagedTabAction, TabInfo};
use crate::adapter::{
    ActionSupport, AdapterActResult, Capabilities, CaptureOptions, ComputerUseAdapter,
    DispatchRequest, SurfaceKind, TargetInfo,
};
use crate::protocol::{
    ActionKind, ActionTarget, Observation, ObservationImage, ObservationNode, PROTOCOL_VERSION,
};

pub struct ManagedBrowserAdapter {
    host: Arc<ExistingTabHost>,
    preview_active: AtomicBool,
}

impl ManagedBrowserAdapter {
    pub fn new(host: Arc<ExistingTabHost>) -> Self {
        Self {
            host,
            preview_active: AtomicBool::new(false),
        }
    }

    fn target_info(tab: TabInfo) -> TargetInfo {
        let title = if tab.title.trim().is_empty() {
            tab.url.clone()
        } else {
            tab.title.clone()
        };
        TargetInfo {
            target_id: tab.tab_id,
            title,
            app_name: "managed-browser".into(),
            kind: "managed-tab".into(),
            pid: None,
            backend: "playwright-managed".into(),
            execution_mode: "exclusive".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: tab.document_generation.max(1),
            display_id: "managed-browser".into(),
            coordinate_space: "image-pixels".into(),
            scope_label: format!("{}/{}", tab.session, tab.run_id),
        }
    }

    fn require_owned_target(&self, run_id: &str, target_id: &str) -> Result<TabInfo, String> {
        let tab = self
            .host
            .managed_target_info(target_id)
            .ok_or_else(|| "managed browser target is dead or unavailable".to_string())?;
        if tab.run_id != run_id {
            return Err("managed browser target belongs to another run".into());
        }
        Ok(tab)
    }
}

fn node_actions(role: &str, disabled: bool) -> Vec<String> {
    if disabled {
        return Vec::new();
    }
    let mut actions = vec!["click".into(), "wait".into()];
    match role {
        "textbox" | "searchbox" => {
            actions.extend(["set_value".into(), "type_text".into(), "key".into()]);
        }
        "combobox" | "checkbox" | "radio" | "option" => actions.push("key".into()),
        _ => {}
    }
    actions
}

fn convert_observation(run_id: &str, target_id: &str, managed: ManagedObservation) -> Observation {
    let page_generation = managed.page_generation;
    let image_width = managed.image_width;
    let image_height = managed.image_height;
    Observation {
        text: managed.aria,
        version: PROTOCOL_VERSION,
        run_id: run_id.to_string(),
        target_id: target_id.to_string(),
        target_generation: 1,
        snapshot_id: managed.snapshot_id,
        captured_at: chrono::Utc::now().to_rfc3339(),
        geometry_revision: page_generation,
        coordinate_space: "image-pixels".into(),
        image: ObservationImage {
            width: image_width,
            height: image_height,
            content_id: managed.image_content_id,
            png_base64: managed.png_base64,
        },
        nodes: managed
            .nodes
            .into_iter()
            .map(|node| ObservationNode {
                actions: node_actions(&node.role, node.disabled),
                node_ref: node.element_ref,
                role: node.role,
                name: node.name,
                truncated: node.truncated,
                x: None,
                y: None,
                width: None,
                height: None,
            })
            .collect(),
        truncated: managed.truncated,
        crop_x: 0,
        crop_y: 0,
        crop_width: image_width,
        crop_height: image_height,
        scale: 1.0,
        dpi: 96.0,
        origin_x: 0,
        origin_y: 0,
        topology_revision: page_generation,
    }
}

impl ComputerUseAdapter for ManagedBrowserAdapter {
    fn backend_id(&self) -> &'static str {
        "playwright-managed"
    }

    fn capabilities(&self) -> Capabilities {
        let mut capabilities =
            Capabilities::for_surface(SurfaceKind::ManagedBrowser, self.backend_id());
        capabilities.coordinate_click = false;
        capabilities.click = ActionSupport::SEMANTIC;
        capabilities.type_text = ActionSupport::SEMANTIC;
        capabilities.drag = ActionSupport::SEMANTIC;
        capabilities.notes = vec![
            "App-owned Playwright profile with run-scoped Host grants".into(),
            "worker completion requires a fresh observation for verification".into(),
        ];
        capabilities
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(Vec::new())
    }

    fn list_targets_for_run(&self, run_id: &str) -> Result<Vec<TargetInfo>, String> {
        Ok(self
            .host
            .list_managed_for_run(run_id)
            .into_iter()
            .map(Self::target_info)
            .collect())
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.host.managed_target_info(target_id).is_some()
    }

    fn observe(&self, _target_id: &str) -> Result<Observation, String> {
        Err("managed browser observation requires a run-scoped grant".into())
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.capture_for_run(run_id, target_id, CaptureOptions::model(true))
    }

    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: CaptureOptions,
    ) -> Result<Observation, String> {
        let tab = self.require_owned_target(run_id, target_id)?;
        let request = options
            .managed_request
            .as_ref()
            .ok_or("managed observation requires admitted identity")?;
        self.host
            .for_managed_request(request)
            .map_err(|error| error.to_string())?
            .capture_managed_tab(run_id, target_id, tab.document_generation.max(1), options)
            .map(|observation| convert_observation(run_id, target_id, observation))
            .map_err(|error| error.to_string())
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        req.admit(self)?;
        self.require_owned_target(&req.run_id, &req.target_id)?;
        let locator = match &req.target {
            ActionTarget::Element { element_ref } => ManagedLocator {
                element_ref: Some(element_ref.clone()),
            },
            ActionTarget::Coord { .. } => ManagedLocator::default(),
        };
        let kind = match req.action {
            ActionKind::Click => "click",
            ActionKind::SetValue => "set_value",
            ActionKind::TypeText => "type_text",
            ActionKind::Key => "key",
            ActionKind::Scroll => "scroll",
            ActionKind::Drag => "drag",
            ActionKind::Wait => "wait",
        };
        let request = req
            .managed_request
            .as_ref()
            .ok_or("managed action requires admitted identity")?;
        let mut params = req.parameters.clone();
        if req.action == ActionKind::Drag {
            let (x, y) = crate::protocol::drag_destination(&params)?;
            params = serde_json::json!({"toX":x,"toY":y});
        }
        self.host
            .for_managed_request(request)
            .map_err(|error| error.to_string())?
            .act_managed_tab(
                &req.run_id,
                &req.target_id,
                ManagedTabAction {
                    page_generation: req.geometry_revision,
                    snapshot_id: &req.snapshot_id,
                    action_id: &req.action_id,
                    kind,
                    locator,
                    params,
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: req.action == ActionKind::Wait,
            verifiable: req.action == ActionKind::Wait,
            detail: if req.action == ActionKind::Wait {
                "observed element name matched; no input dispatched"
            } else {
                "managed browser action applied; observe again to verify"
            }
            .into(),
        })
    }

    fn abort(&self, run_id: &str, _generation: u64) -> Result<(), String> {
        self.host
            .pause_managed_run(run_id)
            .map_err(|error| error.to_string())
    }

    fn is_idle(&self, run_id: &str) -> bool {
        self.host.managed_run_idle(run_id)
    }

    fn start_periodic_preview(&self, target_id: &str) {
        if self.target_alive(target_id) {
            self.preview_active.store(true, Ordering::SeqCst);
        }
    }

    fn stop_periodic_preview(&self) {
        self.preview_active.store(false, Ordering::SeqCst);
    }

    fn periodic_preview_active(&self) -> bool {
        self.preview_active.load(Ordering::SeqCst)
    }

    fn current_geometry_revision_for(&self, target_id: &str) -> u64 {
        self.host
            .managed_target_info(target_id)
            .map(|tab| tab.document_generation.max(1))
            .unwrap_or(0)
    }

    fn claim_target(&self, _target_id: &str) -> Result<(), String> {
        Err("managed browser target requires a run-scoped claim".into())
    }

    fn claim_target_for_run(&self, run_id: &str, target_id: &str) -> Result<(), String> {
        self.require_owned_target(run_id, target_id).map(|_| ())
    }

    fn worker_attached(&self) -> bool {
        self.host.worker_attached()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::ManagedNode;

    #[test]
    fn managed_observation_keeps_page_revision_separate_from_target_generation() {
        let observation = convert_observation(
            "run-a",
            "managed:p1:page-1",
            ManagedObservation {
                page_id: "page-1".into(),
                page_generation: 7,
                snapshot_id: "snapshot-1".into(),
                url: "https://example.test/private?token=redacted".into(),
                title: "Private".into(),
                aria: "- textbox Search".into(),
                nodes: vec![
                    ManagedNode {
                        element_ref: "ref-search".into(),
                        role: "textbox".into(),
                        name: "Search".into(),
                        disabled: false,
                        truncated: false,
                    },
                    ManagedNode {
                        element_ref: "ref-disabled".into(),
                        role: "button".into(),
                        name: "Disabled".into(),
                        disabled: true,
                        truncated: true,
                    },
                ],
                truncated: true,
                text_only: true,
                image_width: 0,
                image_height: 0,
                image_content_id: String::new(),
                png_base64: None,
                image_omitted_reason: Some("not_captured".into()),
            },
        );

        assert_eq!(observation.target_generation, 1);
        assert_eq!(observation.geometry_revision, 7);
        assert_eq!(observation.topology_revision, 7);
        assert_eq!(observation.nodes[0].node_ref, "ref-search");
        assert!(observation.nodes[0].actions.contains(&"set_value".into()));
        assert!(observation.nodes[1].actions.is_empty());
        assert!(observation.nodes[1].truncated);
        assert!(observation.truncated);
        assert!(observation.image.png_base64.is_none());
        assert_eq!(
            serde_json::to_value(&observation).unwrap()["text"],
            "- textbox Search"
        );
    }
}
