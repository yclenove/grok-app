//! Strict model intent. Only the original task's Host can select file contents.
use crate::{
    broker::{
        ComputerUseBroker, HostUploadCancel, WebViewUploadCompletion, WebViewUploadIntent,
        WebViewUploadReady,
    },
    error::BrokerError,
    protocol::OutcomeKind,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn parse(args: &Value) -> Result<WebViewUploadIntent, BrokerError> {
    let mut object = args
        .as_object()
        .cloned()
        .ok_or_else(|| BrokerError::Schema("upload input must be an object".into()))?;
    if object
        .remove("version")
        .and_then(|version| version.as_u64())
        != Some(1)
    {
        return Err(BrokerError::Schema(
            "upload requires protocol version 1".into(),
        ));
    }
    let intent: WebViewUploadIntent =
        serde_json::from_value(Value::Object(object)).map_err(|_| {
            BrokerError::Schema(
                "invalid upload identity; paths and contents are not accepted".into(),
            )
        })?;
    for id in [
        &intent.run_id,
        &intent.target_id,
        &intent.action_id,
        &intent.snapshot_id,
        &intent.element_ref,
    ] {
        if id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(BrokerError::Schema(
                "invalid upload identity length or characters".into(),
            ));
        }
    }
    if !intent.target_id.starts_with("wv|") {
        return Err(BrokerError::Schema(
            "Host-selected upload is only available for App WebView".into(),
        ));
    }
    if intent.target_generation == 0
        || intent.target_generation > crate::protocol::JS_MAX_SAFE_INTEGER
        || intent.geometry_revision > crate::protocol::JS_MAX_SAFE_INTEGER
    {
        return Err(BrokerError::Schema(
            "invalid upload generation or geometryRevision".into(),
        ));
    }
    Ok(intent)
}

pub(crate) fn prepare(
    broker: &Arc<ComputerUseBroker>,
    token: &str,
    args: &Value,
    register: impl FnOnce(HostUploadCancel) -> bool,
) -> Result<WebViewUploadReady, BrokerError> {
    let intent = parse(args)?;
    let permit = broker.begin_model_call_claiming(
        token,
        "computer_upload",
        Some(&intent.run_id),
        Some(&intent.target_id),
    )?;
    #[cfg(test)]
    if let Some(hook) = broker.take_execution_barrier() {
        hook();
    }
    broker.request_webview_upload_for_caller(intent, &permit, register)
}

pub(crate) fn response(completion: WebViewUploadCompletion) -> Value {
    let verified = completion.outcome.kind == OutcomeKind::Verified;
    let value = json!({"outcome":completion.outcome,"bytes":completion.bytes,"assignmentVerified":verified,"submissionVerified":false});
    let mut response = crate::ipc::success(value);
    if !verified {
        response["isError"] = json!(true);
        response["code"] = json!(if completion.outcome.kind == OutcomeKind::Rejected {
            "schema"
        } else {
            "adapter"
        });
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intent() -> Value {
        json!({"version":1,"runId":"run","targetId":"wv|original","targetGeneration":1,
            "actionId":"upload","snapshotId":"snapshot","geometryRevision":0,"elementRef":"file"})
    }
    #[test]
    fn upload_identity_is_explicit_versioned_bounded_and_has_no_file_access_fields() {
        assert!(parse(&intent()).is_ok());
        for key in [
            "version",
            "runId",
            "targetId",
            "targetGeneration",
            "actionId",
            "snapshotId",
            "geometryRevision",
            "elementRef",
        ] {
            let mut missing = intent();
            missing.as_object_mut().unwrap().remove(key);
            assert!(parse(&missing).is_err(), "{key}");
            let mut null = intent();
            null[key] = Value::Null;
            assert!(parse(&null).is_err(), "{key}");
        }
        for key in [
            "path",
            "filename",
            "mime",
            "bytes",
            "url",
            "script",
            "selector",
            "tabId",
            "pageGeneration",
            "alwaysApprove",
        ] {
            let mut forged = intent();
            forged[key] = json!("model supplied");
            assert!(parse(&forged).is_err(), "{key}");
        }
        for (key, value) in [
            ("version", json!(2)),
            ("targetId", json!("window")),
            ("runId", json!(" ")),
            ("snapshotId", json!("x".repeat(257))),
            ("elementRef", json!("file\nother")),
            ("targetGeneration", json!(0)),
            ("targetGeneration", json!(9007199254740992u64)),
            ("geometryRevision", json!(-1)),
            ("geometryRevision", json!(9007199254740992u64)),
        ] {
            let mut invalid = intent();
            invalid[key] = value;
            assert!(parse(&invalid).is_err(), "{key}");
        }
    }
}
