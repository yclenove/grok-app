//! A selector-free status may read original consent metadata, not a target.
use super::*;
use crate::task_consent::BearerClass;
use serde_json::Value;

impl ComputerUseBroker {
    pub(crate) fn ungranted_access_status(
        &self,
        token: &str,
        args: &Value,
    ) -> Result<Option<Value>, BrokerError> {
        let options = args
            .as_object()
            .ok_or_else(|| BrokerError::Schema("status input must be an object".into()))?;
        if options
            .keys()
            .any(|key| !matches!(key.as_str(), "runId" | "run_id" | "targetId" | "target_id"))
        {
            return Err(BrokerError::Schema("invalid status input".into()));
        }
        let claim = crate::tools_claim::execution_claim(args)?;
        if claim.run_id.is_some() || claim.target_id.is_some() {
            // Never replace an explicit execution selector with request metadata.
            return Ok(None);
        }
        let inner = self.inner.lock();
        let BearerClass::Live(ctx) = inner.consent.classify(token) else {
            return Err(BrokerError::IdentityMismatch("transport"));
        };
        inner.consent.ungranted_access_status(&ctx)
    }
}
