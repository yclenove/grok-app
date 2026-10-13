//! Consent-only readback for the original request; never creates a permit.
use super::types::{request_body, RequestState, TransportContext};
use super::ConsentState;
use crate::error::BrokerError;
use serde_json::{json, Value};

impl ConsentState {
    pub(crate) fn ungranted_access_status(
        &self,
        ctx: &TransportContext,
    ) -> Result<Option<Value>, BrokerError> {
        self.require_model_owner(ctx)?;
        let Some(request) = self.latest_request(ctx) else {
            return Ok(None);
        };
        if request.state == RequestState::Approved {
            // Executable status still needs the original committed scope.
            return Ok(None);
        }
        let mut value = request_body(request);
        value["statusKind"] = json!("access-request");
        let expired = request.state.is_pending() && self.clock.now() >= request.deadline;
        if expired {
            // Readback must not sweep/retire resources or prolong a deadline.
            value["state"] = json!("expired");
            value["instruction"] = json!(
                "Explain the expired decision. Do not repeat this request or call execution tools."
            );
        }
        value["nextAction"] = json!(if request.state.is_pending() && !expired {
            "wait-for-host"
        } else {
            "explain-decision"
        });
        Ok(Some(value))
    }
}
