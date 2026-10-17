//! Strict private extension DTOs; not a change to Driver/core/Host protocols.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Browser-owned top-frame scope announced only by the extension background.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    /// Browser tab ID; not a native window ID.
    pub tab_id: u32,
    /// P8-01 injects only the top frame (zero); children stay in that document tree.
    pub frame_id: u32,
    /// Chrome injection result's document ID, never a page-supplied URL or selector.
    pub document_id: String,
    /// Random per-click consent identity, discarded on navigation/revoke/disconnect.
    pub share_id: String,
}
impl Scope {
    pub(super) fn valid(&self) -> bool {
        self.frame_id == 0
            && self.tab_id <= i32::MAX as u32
            && identifier(&self.document_id)
            && identifier(&self.share_id)
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Incoming {
    Ready {
        #[serde(rename = "connectionId")]
        connection_id: String,
        protocol: u32,
    },
    Shared {
        #[serde(rename = "connectionId")]
        connection_id: String,
        scope: Scope,
    },
    Revoked {
        #[serde(rename = "connectionId")]
        connection_id: String,
        #[serde(rename = "shareId")]
        share_id: String,
    },
    Response {
        #[serde(rename = "connectionId")]
        connection_id: String,
        #[serde(rename = "requestId")]
        request_id: String,
        result: Value,
    },
}
impl Incoming {
    pub(super) fn connection(&self) -> &str {
        match self {
            Self::Ready { connection_id, .. }
            | Self::Shared { connection_id, .. }
            | Self::Revoked { connection_id, .. }
            | Self::Response { connection_id, .. } => connection_id,
        }
    }
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn keys(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn id(value: &Value) -> bool {
    value.as_str().is_some_and(identifier)
}
pub(super) fn valid_operation(value: &Value) -> bool {
    match value["type"].as_str() {
        Some("observe") => keys(value, &["type"]),
        Some("click") => {
            keys(value, &["type", "snapshotId", "nodeId"])
                && id(&value["snapshotId"])
                && id(&value["nodeId"])
        }
        Some("set_value" | "select") => {
            keys(value, &["type", "snapshotId", "nodeId", "value"])
                && id(&value["snapshotId"])
                && id(&value["nodeId"])
                && value["value"]
                    .as_str()
                    .is_some_and(|v| v.encode_utf16().count() <= 8192 && !v.contains('\0'))
        }
        Some("scroll") => {
            keys(value, &["type", "snapshotId", "nodeId", "dx", "dy"])
                && id(&value["snapshotId"])
                && id(&value["nodeId"])
                && ["dx", "dy"].iter().all(|k| {
                    value[*k]
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && n.abs() <= 2400.0)
                })
        }
        _ => false,
    }
}
pub(super) fn valid_result(value: &Value) -> bool {
    match value["kind"].as_str() {
        Some("action") => {
            keys(value, &["kind", "outcome"])
                && matches!(value["outcome"].as_str(), Some("done" | "unknown"))
        }
        Some("error") => {
            keys(value, &["kind", "code"])
                && matches!(
                    value["code"].as_str(),
                    Some(
                        "app_denied"
                            | "approval_denied"
                            | "capability_unavailable"
                            | "stale_element"
                            | "handoff_required"
                            | "busy"
                            | "not_settable"
                            | "element_not_actionable"
                            | "driver_error"
                    )
                )
        }
        // The DOM tree remains untrusted data. Only P8-03 can convert it into RawTree.
        Some("snapshot") => {
            keys(value, &["kind", "snapshot"])
                && keys(
                    &value["snapshot"],
                    &[
                        "id",
                        "root",
                        "frameSpace",
                        "geometry",
                        "incomplete",
                        "handoff",
                    ],
                )
                && id(&value["snapshot"]["id"])
                && value["snapshot"]["root"].is_object()
                && value["snapshot"]["frameSpace"] == "tab_viewport_physical"
                && value["snapshot"]["handoff"].is_boolean()
        }
        _ => false,
    }
}
