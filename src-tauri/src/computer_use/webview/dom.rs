//! Fixed native-world DOM commands. Page ids/selectors never become authority.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::adapter::DispatchRequest;
use super::super::protocol::{ActionKind, ActionTarget, ObservationNode};

pub(super) const KERNEL: &str = include_str!("dom.js");

fn script(command: Value) -> String {
    // Only JSON data varies. The model cannot supply executable source.
    format!("({KERNEL})\n({command})")
}

pub(super) fn observe(owner: &str, snapshot: &str, for_model: bool) -> String {
    script(json!({"version": 1, "kind": "observe", "owner": owner,
        "snapshot": snapshot, "forModel": for_model}))
}

pub(super) fn action_name(action: ActionKind) -> Result<&'static str, String> {
    match action {
        ActionKind::Click => Ok("click"),
        ActionKind::SetValue => Ok("set_value"),
        ActionKind::TypeText => Ok("type_text"),
        ActionKind::Scroll => Ok("scroll"),
        ActionKind::Wait => Ok("wait"),
        _ => Err("unsupported typed WebView action".into()),
    }
}

pub(super) fn wait_check(
    owner: &str,
    req: &DispatchRequest,
    name_equals: &str,
) -> Result<String, String> {
    let ActionTarget::Element { element_ref } = &req.target else {
        return Err("typed WebView wait requires an elementRef".into());
    };
    Ok(script(
        json!({"version": 1, "kind": "wait_check", "owner": owner,
        "snapshot": req.snapshot_id, "elementRef": element_ref,
        "nameEquals": name_equals}),
    ))
}

pub(super) fn action(
    owner: &str,
    req: &DispatchRequest,
    preflight: bool,
) -> Result<String, String> {
    let ActionTarget::Element { element_ref } = &req.target else {
        return Err("typed WebView action requires an elementRef".into());
    };
    let mut command = json!({"version": 1, "kind": if preflight {"preflight"} else {"act"},
        "owner": owner, "snapshot": req.snapshot_id, "elementRef": element_ref,
        "action": action_name(req.action)?});
    match req.action {
        ActionKind::SetValue | ActionKind::TypeText => {
            command["text"] = req
                .parameters
                .get("text")
                .or_else(|| req.parameters.get("value"))
                .cloned()
                .ok_or("typed WebView fill requires text")?;
        }
        ActionKind::Scroll => {
            command["dy"] = req
                .parameters
                .get("delta")
                .or_else(|| req.parameters.get("dy"))
                .cloned()
                .unwrap_or(json!(80));
        }
        _ => {}
    }
    Ok(script(command))
}

pub(super) fn download_prepare(owner: &str, snapshot: &str, element_ref: &str) -> String {
    script(
        json!({"version": 1, "kind": "download_prepare", "owner": owner,
        "snapshot": snapshot, "elementRef": element_ref}),
    )
}

pub(super) fn download_dispatch(owner: &str, snapshot: &str, element_ref: &str) -> String {
    script(
        json!({"version": 1, "kind": "download_dispatch", "owner": owner,
        "snapshot": snapshot, "elementRef": element_ref}),
    )
}

pub(super) fn upload_prepare(owner: &str, snapshot: &str, element_ref: &str) -> String {
    script(
        json!({"version": 1, "kind": "upload_prepare", "owner": owner,
        "snapshot": snapshot, "elementRef": element_ref}),
    )
}

pub(super) fn upload_dispatch(
    owner: &str,
    snapshot: &str,
    element_ref: &str,
    file: &grok_computer_use_core::adapter::HostSelectedUploadFile,
) -> String {
    use base64::Engine;
    script(
        json!({"version": 1, "kind": "upload_dispatch", "owner": owner,
        "snapshot": snapshot, "elementRef": element_ref, "filename": file.filename(),
        "mime": file.mime(), "bytes": file.bytes().len(),
        "data": base64::engine::general_purpose::STANDARD.encode(file.bytes())}),
    )
}

pub(super) fn acknowledge(value: &Value) -> Result<(), String> {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    // Errors are fixed Host descriptions, never arbitrary page-controlled data.
    Err(match value.get("reason").and_then(Value::as_str) {
        Some("unsupported_cross_origin") => "unsupported: cross-origin iframe",
        Some("unsupported_download" | "unsupported_download_limit") => {
            "unsupported: complex download"
        }
        Some("unsupported_permission") => "unsupported: browser permission UI",
        Some("stale_snapshot" | "stale_element") => "stale WebView snapshot or elementRef",
        Some("document_unavailable") => "WebView document unavailable",
        _ => "typed WebView command did not acknowledge dispatch",
    }
    .into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadPreparedReply {
    ok: bool,
    url: String,
}

pub(super) fn prepared_download_url(value: Value) -> Result<String, String> {
    acknowledge(&value)?;
    let reply: DownloadPreparedReply = serde_json::from_value(value)
        .map_err(|_| "invalid typed WebView download preparation".to_string())?;
    if !reply.ok || reply.url.len() > 4096 {
        return Err("invalid typed WebView download preparation".into());
    }
    let url = tauri::Url::parse(&reply.url)
        .map_err(|_| "invalid typed WebView download preparation".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("invalid typed WebView download preparation".into());
    }
    Ok(reply.url)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedActionReply {
    ok: bool,
    verified: bool,
}

pub(super) fn action_verified(value: Value) -> Result<bool, String> {
    acknowledge(&value)?;
    let reply: VerifiedActionReply = serde_json::from_value(value)
        .map_err(|_| "invalid typed WebView verified action reply".to_string())?;
    if !reply.ok || !reply.verified {
        return Err("typed WebView action postcondition was not verified".into());
    }
    Ok(true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitReply {
    ok: bool,
    matched: bool,
}

pub(super) fn wait_matched(value: Value) -> Result<bool, String> {
    acknowledge(&value)?;
    let reply: WaitReply = serde_json::from_value(value)
        .map_err(|_| "invalid typed WebView wait reply".to_string())?;
    if !reply.ok {
        return Err("invalid typed WebView wait reply".into());
    }
    Ok(reply.matched)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    element_ref: String,
    role: String,
    name: String,
    actions: Vec<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    ok: bool,
    snapshot: String,
    nodes: Vec<Row>,
    text: String,
    width: u32,
    height: u32,
    truncated: bool,
}

pub(super) struct Snapshot {
    pub id: String,
    pub nodes: Vec<ObservationNode>,
    pub text: String,
    pub width: u32,
    pub height: u32,
    pub truncated: bool,
}

impl Snapshot {
    pub fn parse(value: Value, expected: &str) -> Result<Self, String> {
        acknowledge(&value)?;
        let invalid = || "invalid typed WebView snapshot".to_string();
        let reply: Reply = serde_json::from_value(value).map_err(|_| invalid())?;
        if !reply.ok
            || reply.snapshot != expected
            || reply.nodes.len() > 128
            || reply.text.encode_utf16().count() > 32000
            || !(1..=100_000).contains(&reply.width)
            || !(1..=100_000).contains(&reply.height)
        {
            return Err(invalid());
        }
        let mut nodes = Vec::with_capacity(reply.nodes.len());
        for (index, row) in reply.nodes.into_iter().enumerate() {
            if row.element_ref != format!("{expected}:{}", index + 1)
                || row.role.encode_utf16().count() > 64
                || row.name.encode_utf16().count() > 256
                || row.actions.is_empty()
                || row.actions.len() > 6
                || row.actions.iter().any(|action| {
                    !matches!(
                        action.as_str(),
                        "click"
                            | "set_value"
                            | "type_text"
                            | "scroll"
                            | "wait"
                            | "download"
                            | "upload"
                    )
                })
                || row
                    .actions
                    .iter()
                    .enumerate()
                    .any(|(i, a)| row.actions[..i].contains(a))
                || [row.x, row.y, row.width, row.height]
                    .iter()
                    .any(|n| !n.is_finite() || n.abs() > 10_000_000.0)
                || row.width <= 0.0
                || row.height <= 0.0
            {
                return Err(invalid());
            }
            nodes.push(ObservationNode {
                node_ref: row.element_ref,
                role: row.role,
                name: row.name,
                actions: row.actions,
                truncated: false,
                x: Some(row.x),
                y: Some(row.y),
                width: Some(row.width),
                height: Some(row.height),
            });
        }
        Ok(Self {
            id: reply.snapshot,
            nodes,
            text: reply.text,
            width: reply.width,
            height: reply.height,
            truncated: reply.truncated,
        })
    }

    pub fn published(&self) -> Published {
        Published {
            id: self.id.clone(),
            nodes: self
                .nodes
                .iter()
                .map(|node| (node.node_ref.clone(), node.actions.clone()))
                .collect(),
        }
    }
}

pub(super) struct Published {
    pub id: String,
    pub nodes: HashMap<String, Vec<String>>,
}

#[derive(Default)]
pub(super) struct ObservationState {
    revision: u64,
    pub snapshot: Option<Published>,
}

impl ObservationState {
    pub fn begin(&mut self) -> Result<u64, String> {
        self.snapshot = None;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("WebView observation revision exhausted")?;
        Ok(self.revision)
    }

    pub fn publish(&mut self, revision: u64, snapshot: Published) -> Result<(), String> {
        if self.revision != revision {
            return Err("WebView observation superseded before publication".into());
        }
        self.snapshot = Some(snapshot);
        Ok(())
    }

    pub fn validate(&self, req: &DispatchRequest) -> Result<(), String> {
        let ActionTarget::Element { element_ref } = &req.target else {
            return Err("typed WebView action requires an elementRef".into());
        };
        let action = action_name(req.action)?;
        if !self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.id == req.snapshot_id
                && snapshot
                    .nodes
                    .get(element_ref)
                    .is_some_and(|actions| actions.iter().any(|a| a == action))
        }) {
            return Err("stale WebView snapshot or elementRef".into());
        }
        Ok(())
    }

    pub fn validate_download(&self, snapshot_id: &str, element_ref: &str) -> Result<(), String> {
        self.validate_file(snapshot_id, element_ref, "download")
    }

    pub fn validate_upload(&self, snapshot_id: &str, element_ref: &str) -> Result<(), String> {
        self.validate_file(snapshot_id, element_ref, "upload")
    }

    fn validate_file(
        &self,
        snapshot_id: &str,
        element_ref: &str,
        action: &str,
    ) -> Result<(), String> {
        if !self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.id == snapshot_id
                && snapshot
                    .nodes
                    .get(element_ref)
                    .is_some_and(|actions| actions.iter().any(|candidate| candidate == action))
        }) {
            return Err("stale WebView snapshot or elementRef".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply() -> Value {
        json!({"ok":true,"snapshot":"s","nodes":[{"elementRef":"s:1","role":"button",
            "name":"Safe label","actions":["click"],"x":1,"y":2,"width":20,"height":10}],
            "text":"Visible text","width":800,"height":600,"truncated":false})
    }

    #[test]
    fn validates_exact_references_dimensions_and_bounded_native_reply() {
        assert!(Snapshot::parse(reply(), "s").is_ok());
        for (field, value) in [
            ("snapshot", json!("other")),
            ("width", json!(0)),
            ("text", json!("a".repeat(32001))),
        ] {
            let mut data = reply();
            data[field] = value;
            assert!(Snapshot::parse(data, "s").is_err());
        }
        for (field, value) in [
            ("elementRef", json!("#target")),
            ("actions", json!(["eval"])),
            ("actions", json!(["click", "click"])),
            ("width", json!(-1)),
            ("name", json!("x".repeat(257))),
        ] {
            let mut data = reply();
            data["nodes"][0][field] = value;
            assert!(Snapshot::parse(data, "s").is_err());
        }
    }

    #[test]
    fn superseded_observation_cannot_restore_an_old_published_snapshot() {
        let mut state = ObservationState::default();
        let old = state.begin().unwrap();
        let new = state.begin().unwrap();
        assert!(state
            .publish(old, Snapshot::parse(reply(), "s").unwrap().published())
            .is_err());
        assert!(state.snapshot.is_none());
        state
            .publish(new, Snapshot::parse(reply(), "s").unwrap().published())
            .unwrap();
        assert_eq!(state.snapshot.as_ref().unwrap().id, "s");
    }

    #[test]
    fn wait_reply_is_strict_and_never_echoes_page_fields() {
        assert!(wait_matched(json!({"ok":true,"matched":true})).unwrap());
        assert!(!wait_matched(json!({"ok":true,"matched":false})).unwrap());
        for reply in [
            json!({"ok":true}),
            json!({"ok":true,"matched":"yes"}),
            json!({"ok":true,"matched":true,"private":"secret"}),
        ] {
            let error = wait_matched(reply).unwrap_err();
            assert_eq!(error, "invalid typed WebView wait reply");
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn verified_action_reply_is_strict_and_never_echoes_page_fields() {
        assert!(action_verified(json!({"ok":true,"verified":true})).unwrap());
        for reply in [
            json!({"ok":true}),
            json!({"ok":true,"verified":false}),
            json!({"ok":true,"verified":true,"value":"secret"}),
        ] {
            let error = action_verified(reply).unwrap_err();
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn download_preparation_is_strict_and_keeps_only_safe_http_urls() {
        assert_eq!(
            prepared_download_url(json!({"ok":true,"url":"https://example.test/file.bin"}))
                .unwrap(),
            "https://example.test/file.bin"
        );
        for reply in [
            json!({"ok":true,"url":"blob:https://example.test/id"}),
            json!({"ok":true,"url":"https://user:pass@example.test/file"}),
            json!({"ok":true,"url":"https://example.test/file","private":"secret"}),
        ] {
            let error = prepared_download_url(reply).unwrap_err();
            assert!(!error.contains("secret"));
        }
    }
}
