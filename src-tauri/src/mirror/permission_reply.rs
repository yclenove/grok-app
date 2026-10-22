use super::RpcError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyIdentity {
    #[serde(default, alias = "request_generation")]
    request_generation: Option<uuid::Uuid>,
}

pub(super) fn generation(params: &Value) -> Result<Option<uuid::Uuid>, RpcError> {
    serde_json::from_value::<ReplyIdentity>(params.clone())
        .map(|reply| reply.request_generation)
        .map_err(|_| RpcError::bad_params("invalid permission requestGeneration"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mirror_permission_identity_accepts_both_protocol_spellings() {
        let id = uuid::Uuid::new_v4();
        for key in ["requestGeneration", "request_generation"] {
            assert_eq!(generation(&json!({key: id, "rpcId": 0})).unwrap(), Some(id));
        }
    }

    #[test]
    fn missing_mirror_identity_is_left_for_the_pending_gate_compatibility_check() {
        assert_eq!(generation(&json!({"rpcId": 81})).unwrap(), None);
        assert_eq!(
            generation(&json!({"requestGeneration": null})).unwrap(),
            None
        );
    }

    #[test]
    fn invalid_or_ambiguous_mirror_identity_is_not_silently_dropped() {
        for params in [
            json!({"requestGeneration": 4}),
            json!({"requestGeneration": "bad"}),
            json!({"requestGeneration": uuid::Uuid::new_v4(), "request_generation": uuid::Uuid::new_v4()}),
        ] {
            assert_eq!(generation(&params).unwrap_err().code, "BAD_PARAMS");
        }
    }
}
