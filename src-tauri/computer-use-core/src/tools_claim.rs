//! A malformed selector is not an absent selector. Shared by permission and
//! execution so a bad claim cannot silently borrow the only available grant.
use crate::{error::BrokerError, task_consent::ExecutionTargetClaim};
use serde_json::Value;

fn selector<'a>(args: &'a Value, camel: &str, snake: &str) -> Result<Option<&'a str>, BrokerError> {
    if args.get(camel).is_some() && args.get(snake).is_some() {
        return Err(BrokerError::Schema("ambiguous target selector".into()));
    }
    args.get(camel)
        .or_else(|| args.get(snake))
        .map(|value| {
            value
                .as_str()
                .filter(|id| !id.trim().is_empty() && id.len() <= 256)
                .ok_or_else(|| BrokerError::Schema("invalid target selector".into()))
        })
        .transpose()
}

pub(crate) fn execution_claim(args: &Value) -> Result<ExecutionTargetClaim<'_>, BrokerError> {
    if !args.is_object() {
        return Err(BrokerError::Schema(
            "tool arguments must be an object".into(),
        ));
    }
    Ok(ExecutionTargetClaim {
        run_id: selector(args, "runId", "run_id")?,
        target_id: selector(args, "targetId", "target_id")?,
    })
}
