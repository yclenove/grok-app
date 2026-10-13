//! Host-only exact worker/context/page witnesses, without document content.
use serde_json::Value;

use super::WorkerError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedScopeIdentity {
    pub owner: String,
    pub profile: String,
    pub worker_instance_id: String,
    pub context_instance_id: String,
    pub page_id: String,
}

impl ManagedScopeIdentity {
    pub(crate) fn target_id(&self) -> Option<String> {
        (segment(&self.owner, 128)
            && segment(&self.profile, 64)
            && segment(&self.page_id, 128)
            && instance(&self.worker_instance_id)
            && instance(&self.context_instance_id))
        .then(|| format!("managed:{}:{}", self.profile, self.page_id))
    }
}

fn segment(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
}

fn instance(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| id.get_version_num() == 4 && id.hyphenated().to_string() == value)
}

/// Reject missing, echoed, malformed or content-bearing replies. This is not
/// a fallback to profile names, URL/title, page generation or process liveness.
pub fn parse_managed_scope_identity(value: &Value) -> Result<ManagedScopeIdentity, WorkerError> {
    let object = value
        .as_object()
        .ok_or_else(|| WorkerError::invalid_response(200))?;
    const FIELDS: [&str; 6] = [
        "ok",
        "owner",
        "profile",
        "workerInstanceId",
        "contextInstanceId",
        "pageId",
    ];
    if object.len() != FIELDS.len()
        || object.keys().any(|key| !FIELDS.contains(&key.as_str()))
        || object.get("ok") != Some(&Value::Bool(true))
    {
        return Err(WorkerError::invalid_response(200));
    }
    let field = |key| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| WorkerError::invalid_response(200))
    };
    let identity = ManagedScopeIdentity {
        owner: field("owner")?,
        profile: field("profile")?,
        worker_instance_id: field("workerInstanceId")?,
        context_instance_id: field("contextInstanceId")?,
        page_id: field("pageId")?,
    };
    if identity.target_id().is_none() {
        return Err(WorkerError::invalid_response(200));
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply() -> Value {
        json!({"ok":true,"owner":"run-a","profile":"task-a","pageId":"page-a",
            "workerInstanceId":uuid::Uuid::new_v4().to_string(),
            "contextInstanceId":uuid::Uuid::new_v4().to_string()})
    }

    #[test]
    fn exact_metadata_parses_without_document_authority() {
        let identity = parse_managed_scope_identity(&reply()).unwrap();
        assert_eq!(
            identity.target_id().as_deref(),
            Some("managed:task-a:page-a")
        );
        let native = crate::adapter::NativeScopeIdentity::ManagedPage { identity };
        assert_eq!(native.target_id().as_deref(), Some("managed:task-a:page-a"));
        assert_eq!(native.decision_class(), "host-native-scope");
    }

    #[test]
    fn malformed_or_content_bearing_scope_replies_fail_closed() {
        for key in [
            "ok",
            "owner",
            "profile",
            "pageId",
            "workerInstanceId",
            "contextInstanceId",
        ] {
            for value in [Value::Null, json!(0), json!(""), json!("../outside")] {
                let mut candidate = reply();
                candidate[key] = value;
                assert!(parse_managed_scope_identity(&candidate).is_err(), "{key}");
            }
            let mut candidate = reply();
            candidate.as_object_mut().unwrap().remove(key);
            assert!(
                parse_managed_scope_identity(&candidate).is_err(),
                "missing {key}"
            );
        }
        for key in [
            "url",
            "title",
            "aria",
            "nodes",
            "pageGeneration",
            "cookie",
            "unknown",
        ] {
            let mut candidate = reply();
            candidate[key] = json!("private");
            assert!(parse_managed_scope_identity(&candidate).is_err(), "{key}");
        }
        for (key, max) in [("owner", 128), ("profile", 64), ("pageId", 128)] {
            let mut candidate = reply();
            candidate[key] = json!("x".repeat(max + 1));
            assert!(parse_managed_scope_identity(&candidate).is_err());
        }
        for value in [
            uuid::Uuid::nil().to_string(),
            uuid::Uuid::new_v4().simple().to_string(),
        ] {
            let mut candidate = reply();
            candidate["contextInstanceId"] = json!(value);
            assert!(parse_managed_scope_identity(&candidate).is_err());
        }
    }

    #[test]
    fn every_instance_component_participates_in_scope_equality() {
        let identity = parse_managed_scope_identity(&reply()).unwrap();
        for field in [
            "owner",
            "profile",
            "pageId",
            "workerInstanceId",
            "contextInstanceId",
        ] {
            let mut candidate = reply();
            candidate["owner"] = json!(identity.owner);
            candidate["profile"] = json!(identity.profile);
            candidate["pageId"] = json!(identity.page_id);
            candidate["workerInstanceId"] = json!(identity.worker_instance_id);
            candidate["contextInstanceId"] = json!(identity.context_instance_id);
            candidate[field] = if field.ends_with("InstanceId") {
                json!(uuid::Uuid::new_v4().to_string())
            } else {
                json!("replacement")
            };
            assert_ne!(parse_managed_scope_identity(&candidate).unwrap(), identity);
        }
    }
}
