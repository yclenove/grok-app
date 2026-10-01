//! Host current-observation allowlist. Stale/unauthorized/schema rejects
//! happen before the worker is called.

use std::collections::HashSet;

use super::{ManagedLocator, ManagedObservation};
use crate::error::BrokerError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostObservation {
    pub page_id: String,
    pub page_generation: u64,
    pub snapshot_id: String,
    pub refs: HashSet<String>,
    pub has_image: bool,
    pub image_width: u32,
    pub image_height: u32,
}

impl HostObservation {
    pub fn from_managed(obs: &ManagedObservation) -> Self {
        Self {
            page_id: obs.page_id.clone(),
            page_generation: obs.page_generation,
            snapshot_id: obs.snapshot_id.clone(),
            refs: obs
                .nodes
                .iter()
                .filter(|node| !node.truncated)
                .map(|node| node.element_ref.clone())
                .collect(),
            has_image: crate::browser::observation::coordinate_action_allowed(obs),
            image_width: obs.image_width,
            image_height: obs.image_height,
        }
    }
}

fn is_dummy_element_ref(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "dummy" | "page" | "none" | "null" | "undefined"
    )
}

fn kind_requires_element(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "click" | "set_value" | "fill" | "type_text" | "type" | "select" | "drag" | "wait"
    )
}

fn kind_requires_snapshot(kind: &str) -> bool {
    kind_requires_element(kind)
        || matches!(
            kind.trim().to_ascii_lowercase().as_str(),
            "key" | "scroll" | "drag"
        )
}

fn is_coordinate_click(locator: &ManagedLocator, params: &serde_json::Value) -> bool {
    locator.element_ref.is_none() && params.get("x").is_some() && params.get("y").is_some()
}

pub fn preflight_typed_action(
    kind: &str,
    current: Option<&HostObservation>,
    page_id: &str,
    page_generation: u64,
    snapshot_id: &str,
    locator: &ManagedLocator,
    params: &serde_json::Value,
) -> Result<(), BrokerError> {
    let kind = kind.trim().to_ascii_lowercase();
    let coordinate = is_coordinate_click(locator, params);
    if kind_requires_snapshot(&kind) && snapshot_id.trim().is_empty() {
        return Err(BrokerError::Schema("snapshotId required".into()));
    }
    if kind_requires_element(&kind) && !coordinate {
        match locator.element_ref.as_deref() {
            None | Some("") => {
                return Err(BrokerError::Schema("elementRef required".into()));
            }
            Some(value) if is_dummy_element_ref(value) => {
                return Err(BrokerError::Schema("dummy elementRef".into()));
            }
            Some(_) => {}
        }
    }
    if kind == "key" || kind == "scroll" {
        if let Some(value) = locator.element_ref.as_deref() {
            if is_dummy_element_ref(value) {
                return Err(BrokerError::Schema("dummy elementRef".into()));
            }
        }
    }
    if kind == "drag" {
        for key in ["toSelector", "selector", "to", "drop"] {
            if params.get(key).is_some() {
                return Err(BrokerError::Schema(
                    "selector locators are not a production worker route".into(),
                ));
            }
        }
        let refs = ["destElementRef", "dropElementRef"]
            .iter()
            .filter(|key| params.get(**key).is_some())
            .count();
        let pixels = params.get("toX").is_some() || params.get("toY").is_some();
        if refs > 1
            || (refs > 0 && pixels)
            || params.get("x1").is_some()
            || params.get("y1").is_some()
        {
            return Err(BrokerError::Schema("ambiguous drag destination".into()));
        }
        if refs > 0
            && params
                .get("destElementRef")
                .or_else(|| params.get("dropElementRef"))
                .and_then(|v| v.as_str())
                .is_none()
        {
            return Err(BrokerError::Schema(
                "destination elementRef required".into(),
            ));
        }
        if let Some(dest) = params
            .get("destElementRef")
            .or_else(|| params.get("dropElementRef"))
            .and_then(|v| v.as_str())
        {
            if is_dummy_element_ref(dest) {
                return Err(BrokerError::Schema("dummy elementRef".into()));
            }
            if let Some(obs) = current {
                if !obs.refs.contains(dest) {
                    return Err(BrokerError::IdentityMismatch("destElementRef"));
                }
            }
        } else {
            let (x, y) = crate::protocol::drag_destination(params).map_err(BrokerError::Schema)?;
            preflight_coordinate(current)?;
            let obs = current.ok_or(BrokerError::IdentityMismatch("managed snapshot"))?;
            if x >= f64::from(obs.image_width) || y >= f64::from(obs.image_height) {
                return Err(BrokerError::Schema(
                    "drag destination outside observed image".into(),
                ));
            }
        }
    }
    preflight_node_action(current, page_id, page_generation, snapshot_id, locator)
}

pub fn preflight_node_action(
    current: Option<&HostObservation>,
    page_id: &str,
    page_generation: u64,
    snapshot_id: &str,
    locator: &ManagedLocator,
) -> Result<(), BrokerError> {
    let Some(obs) = current else {
        return Err(BrokerError::IdentityMismatch("managed snapshot"));
    };
    if obs.page_id != page_id {
        return Err(BrokerError::IdentityMismatch("managed pageId"));
    }
    if obs.page_generation != page_generation {
        return Err(BrokerError::IdentityMismatch("managed page generation"));
    }
    if snapshot_id.is_empty() || snapshot_id != obs.snapshot_id {
        return Err(BrokerError::IdentityMismatch("managed snapshot"));
    }
    if let Some(element_ref) = locator.element_ref.as_deref() {
        if element_ref.is_empty() {
            return Err(BrokerError::Schema("elementRef required".into()));
        }
        if !obs.refs.contains(element_ref) {
            return Err(BrokerError::IdentityMismatch("elementRef"));
        }
    }
    Ok(())
}

pub fn preflight_coordinate(current: Option<&HostObservation>) -> Result<(), BrokerError> {
    let Some(obs) = current else {
        return Err(BrokerError::IdentityMismatch("managed snapshot"));
    };
    if !obs.has_image {
        return Err(BrokerError::Schema(
            "coordinate action requires a visual observation".into(),
        ));
    }
    Ok(())
}

pub fn cas_identity_unchanged(
    current: Option<&HostObservation>,
    dispatched: &HostObservation,
) -> Result<(), BrokerError> {
    match current {
        Some(now)
            if now.page_id == dispatched.page_id
                && now.page_generation == dispatched.page_generation
                && now.snapshot_id == dispatched.snapshot_id =>
        {
            Ok(())
        }
        _ => Err(BrokerError::Adapter(
            "late worker result after identity change".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::ManagedNode;
    use crate::error::BrokerError;

    fn obs() -> HostObservation {
        HostObservation::from_managed(&ManagedObservation {
            page_id: "page-1".into(),
            page_generation: 3,
            snapshot_id: "snap-1".into(),
            url: "https://example.test/".into(),
            title: String::new(),
            aria: String::new(),
            nodes: vec![
                ManagedNode {
                    element_ref: "ref-ok".into(),
                    role: "button".into(),
                    name: "Go".into(),
                    disabled: false,
                    truncated: false,
                },
                ManagedNode {
                    element_ref: "ref-cut".into(),
                    role: "button".into(),
                    name: "Cut".into(),
                    disabled: false,
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
        })
    }

    fn locator(element_ref: &str) -> ManagedLocator {
        ManagedLocator {
            element_ref: Some(element_ref.into()),
        }
    }

    #[test]
    fn stale_unauthorized_and_truncated_refs_fail_before_worker() {
        let current = obs();
        assert!(matches!(
            preflight_node_action(None, "page-1", 3, "snap-1", &locator("ref-ok")),
            Err(BrokerError::IdentityMismatch("managed snapshot"))
        ));
        assert!(matches!(
            preflight_node_action(
                Some(&current),
                "page-1",
                3,
                "snap-stale",
                &locator("ref-ok")
            ),
            Err(BrokerError::IdentityMismatch("managed snapshot"))
        ));
        assert!(matches!(
            preflight_node_action(Some(&current), "page-1", 9, "snap-1", &locator("ref-ok")),
            Err(BrokerError::IdentityMismatch("managed page generation"))
        ));
        assert!(matches!(
            preflight_node_action(Some(&current), "page-1", 3, "snap-1", &locator("ref-cut")),
            Err(BrokerError::IdentityMismatch("elementRef"))
        ));
        assert!(matches!(
            preflight_node_action(
                Some(&current),
                "page-1",
                3,
                "snap-1",
                &locator("ref-missing")
            ),
            Err(BrokerError::IdentityMismatch("elementRef"))
        ));
        preflight_node_action(Some(&current), "page-1", 3, "snap-1", &locator("ref-ok")).unwrap();
        assert!(matches!(
            preflight_coordinate(Some(&current)),
            Err(BrokerError::Schema(_))
        ));
    }

    #[test]
    fn late_result_cannot_replace_a_newer_observation() {
        let dispatched = obs();
        let mut newer = dispatched.clone();
        newer.snapshot_id = "snap-2".into();
        newer.page_generation = 4;
        assert!(matches!(
            cas_identity_unchanged(Some(&newer), &dispatched),
            Err(BrokerError::Adapter(_))
        ));
        cas_identity_unchanged(Some(&dispatched), &dispatched).unwrap();
        assert!(matches!(
            cas_identity_unchanged(None, &dispatched),
            Err(BrokerError::Adapter(_))
        ));
    }

    #[test]
    fn drag_destination_has_one_authority_and_pixels_require_image_bounds() {
        let mut current = obs();
        let preflight = |obs: &HostObservation, params: serde_json::Value| {
            preflight_typed_action(
                "drag",
                Some(obs),
                "page-1",
                3,
                "snap-1",
                &locator("ref-ok"),
                &params,
            )
        };
        preflight(&current, serde_json::json!({"destElementRef": "ref-ok"})).unwrap();
        preflight(&current, serde_json::json!({"dropElementRef": "ref-ok"})).unwrap();
        assert!(preflight(&current, serde_json::json!({"toX": 12, "toY": 14})).is_err());
        current.has_image = true;
        current.image_width = 400;
        current.image_height = 300;
        preflight(&current, serde_json::json!({"toX": 12, "toY": 14})).unwrap();
        for params in [
            serde_json::json!({"destElementRef": "ref-ok", "toX": 12, "toY": 14}),
            serde_json::json!({"destElementRef": "ref-ok", "dropElementRef": "ref-ok"}),
            serde_json::json!({"destElementRef": 12}),
            serde_json::json!({"toX": null, "toY": 14}),
            serde_json::json!({"toX": "12", "toY": 14}),
            serde_json::json!({"toX": 400, "toY": 14}),
            serde_json::json!({"toX": 12, "toY": 300}),
            serde_json::json!({"toX": 12, "toY": 14, "x1": 50, "y1": 60}),
            serde_json::json!({"toX": 12, "toY": 14, "selector": {}}),
        ] {
            assert!(preflight(&current, params.clone()).is_err(), "{params}");
        }
    }

    #[test]
    fn recording_worker_is_not_called_for_stale_or_unknown_refs() {
        use crate::browser::{
            ExistingTabHost, ManagedLocator, ManagedTabAction, RecordingBrowserWorker,
        };
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!("cu-allow-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        let host = ExistingTabHost::new();
        host.set_profile_root(root.join("profiles"));
        host.set_worker(worker.clone());
        let tab = host
            .open_managed_profile("sess", "run", "profile")
            .expect("open");
        host.observe_managed_tab("run", &tab.tab_id, tab.document_generation.max(1))
            .expect("observe");
        let stale = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: tab.document_generation.max(1),
                snapshot_id: "stale-snap",
                action_id: "act-stale",
                kind: "click",
                locator: ManagedLocator {
                    element_ref: Some("ref-ok".into()),
                },
                params: serde_json::json!({}),
            },
        );
        assert!(
            matches!(
                stale,
                Err(BrokerError::IdentityMismatch("managed snapshot"))
            ),
            "{stale:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 0);
        let unknown = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: tab.document_generation.max(1),
                snapshot_id: "rec-snap",
                action_id: "act-miss",
                kind: "click",
                locator: ManagedLocator {
                    element_ref: Some("missing-ref".into()),
                },
                params: serde_json::json!({}),
            },
        );
        assert!(
            matches!(unknown, Err(BrokerError::IdentityMismatch("elementRef"))),
            "{unknown:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn click_set_value_type_text_select_require_element_ref_before_worker() {
        use crate::browser::{
            ExistingTabHost, ManagedLocator, ManagedTabAction, RecordingBrowserWorker,
        };
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!("cu-typed-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        let host = ExistingTabHost::new();
        host.set_profile_root(root.join("profiles"));
        host.set_worker(worker.clone());
        let tab = host
            .open_managed_profile("sess", "run", "profile")
            .expect("open");
        host.observe_managed_tab("run", &tab.tab_id, tab.document_generation.max(1))
            .expect("observe");
        let generation = tab.document_generation.max(1);
        for kind in ["click", "set_value", "type_text", "select"] {
            let out = host.act_managed_tab(
                "run",
                &tab.tab_id,
                ManagedTabAction {
                    page_generation: generation,
                    snapshot_id: "rec-snap",
                    action_id: &format!("missing-ref-{kind}"),
                    kind,
                    locator: ManagedLocator::default(),
                    params: serde_json::json!({"text": "x", "key": "enter"}),
                },
            );
            assert!(
                matches!(out, Err(BrokerError::Schema(ref msg)) if msg.contains("elementRef")),
                "{kind}: {out:?}"
            );
        }
        assert_eq!(*worker.act_calls.lock(), 0);

        let dummy = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: generation,
                snapshot_id: "rec-snap",
                action_id: "key-dummy",
                kind: "key",
                locator: ManagedLocator {
                    element_ref: Some("dummy".into()),
                },
                params: serde_json::json!({"key": "enter"}),
            },
        );
        assert!(
            matches!(dummy, Err(BrokerError::Schema(ref msg)) if msg.to_ascii_lowercase().contains("dummy")),
            "{dummy:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 0);

        let missing_snap = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: generation,
                snapshot_id: "",
                action_id: "click-no-snap",
                kind: "click",
                locator: ManagedLocator {
                    element_ref: Some("rec-ref".into()),
                },
                params: serde_json::json!({}),
            },
        );
        assert!(
            match &missing_snap {
                Err(BrokerError::Schema(msg)) if msg.contains("snapshot") => true,
                Err(BrokerError::IdentityMismatch("managed snapshot")) => true,
                _ => false,
            },
            "{missing_snap:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 0);

        let drag_sel = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: generation,
                snapshot_id: "rec-snap",
                action_id: "drag-selector",
                kind: "drag",
                locator: ManagedLocator {
                    element_ref: Some("rec-ref".into()),
                },
                params: serde_json::json!({"toSelector": "#drop"}),
            },
        );
        assert!(
            matches!(drag_sel, Err(BrokerError::Schema(ref msg)) if msg.contains("selector")),
            "{drag_sel:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn download_without_snapshot_or_ref_is_zero_dispatch() {
        use crate::browser::{ExistingTabHost, RecordingBrowserWorker};
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!("cu-dl-id-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        let host = ExistingTabHost::new();
        host.set_profile_root(root.join("profiles"));
        host.set_staging_root(root.clone());
        host.set_worker(worker.clone());
        let tab = host
            .open_managed_profile("sess", "run", "profile")
            .expect("open");
        for (snap, element) in [("", None), ("rec-snap", None), ("", Some("rec-ref"))] {
            let out = host.stage_download_with_action(
                "run",
                &tab.tab_id,
                tab.document_generation.max(1),
                "dl-missing",
                snap,
                element,
                "report.bin",
            );
            assert!(out.is_err(), "{snap:?} {element:?} {out:?}");
        }
        assert_eq!(*worker.downloads.lock(), 0);
        let ok = host.stage_download_with_action(
            "run",
            &tab.tab_id,
            tab.document_generation.max(1),
            "dl-ok",
            "rec-snap",
            Some("rec-ref"),
            "report.bin",
        );
        assert!(ok.is_ok(), "{ok:?}");
        assert_eq!(*worker.downloads.lock(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worker_not_started_replays_rejected_not_unknown() {
        use crate::browser::{
            ExistingTabHost, ManagedTabAction, RecordingBrowserWorker, WorkerError,
        };
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!("cu-rej-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(RecordingBrowserWorker::new(root.clone()));
        let host = ExistingTabHost::new();
        host.set_profile_root(root.join("profiles"));
        host.set_worker(worker.clone());
        let tab = host
            .open_managed_profile("sess", "run", "profile")
            .expect("open");
        host.observe_managed_tab("run", &tab.tab_id, tab.document_generation.max(1))
            .expect("observe");
        *worker.act_error.lock() = Some(WorkerError::not_started(
            400,
            "stale_snapshot",
            "snapshot changed",
        ));
        let first = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: tab.document_generation.max(1),
                snapshot_id: "rec-snap",
                action_id: "click-1",
                kind: "click",
                locator: locator("rec-ref"),
                params: serde_json::json!({}),
            },
        );
        assert!(first.is_err(), "{first:?}");
        assert_eq!(*worker.act_calls.lock(), 1);
        let replay = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: tab.document_generation.max(1),
                snapshot_id: "rec-snap",
                action_id: "click-1",
                kind: "click",
                locator: locator("rec-ref"),
                params: serde_json::json!({}),
            },
        );
        assert!(
            matches!(replay, Err(BrokerError::Schema(ref msg)) if msg.contains("rejected")),
            "{replay:?}"
        );
        assert_eq!(*worker.act_calls.lock(), 1);
        let second = host.act_managed_tab(
            "run",
            &tab.tab_id,
            ManagedTabAction {
                page_generation: tab.document_generation.max(1),
                snapshot_id: "rec-snap",
                action_id: "click-2",
                kind: "click",
                locator: locator("rec-ref"),
                params: serde_json::json!({}),
            },
        );
        assert!(second.is_ok(), "{second:?}");
        assert_eq!(*worker.act_calls.lock(), 2);
        let _ = std::fs::remove_dir_all(root);
    }
}
