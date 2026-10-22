//! Plan-bound result protocol selection, not a caller option or Undo grant.
//! V1/v2 canonical payloads are unchanged. New plans bind signed outcome v1;
//! decoding a protocol never grants acceptance or publication ownership.
use serde::Deserialize;
use serde_json::{json, Value};

use super::errors::{ErrorCode, ImportError};

#[path = "publication_job_id.rs"]
mod job_id;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::session_migration) enum PublicationProtocol {
    #[default]
    Legacy,
    SignedOutcomeV1,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    version: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u64,
    #[serde(default)]
    publication_outcome: Option<Descriptor>,
}

fn stale(message: &str) -> ImportError {
    ImportError::new(ErrorCode::PlanStale, message)
}

impl PublicationProtocol {
    pub(in crate::session_migration) fn decode(
        json: &str,
        value: &Value,
    ) -> Result<Self, ImportError> {
        let protocol = Self::decode_value(value)?;
        // Keep the established legacy parser/capacity path; do not add a second
        // full text scan to each ordinary plan read. The caller parsed Value
        // from these exact bytes and still checks the original canonical hash.
        if protocol == Self::Legacy {
            return Ok(protocol);
        }
        // New headers additionally parse the original text (not only Value),
        // rejecting duplicate version/descriptor fields before map collapse.
        let header: Header = serde_json::from_str(json)
            .map_err(|_| stale("durable publication protocol header is invalid"))?;
        let object = value
            .as_object()
            .ok_or_else(|| stale("durable plan must be an object"))?;
        if object.get("version").and_then(Value::as_u64) != Some(header.version) {
            return Err(stale("durable publication protocol version differs"));
        }
        if protocol != Self::from_header(header, object.contains_key("publicationOutcome"))? {
            return Err(stale("durable publication protocol descriptor differs"));
        }
        Ok(protocol)
    }

    /// Shape-only check for already parsed internal records. Copies only the
    /// small descriptor, never serializes or clones a complete ordinary plan.
    pub(in crate::session_migration) fn decode_value(value: &Value) -> Result<Self, ImportError> {
        let object = value
            .as_object()
            .ok_or_else(|| stale("durable plan must be an object"))?;
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| stale("durable plan missing version"))?;
        let publication_outcome = object
            .get("publicationOutcome")
            .map(|descriptor| {
                serde_json::from_value(descriptor.clone())
                    .map_err(|_| stale("durable publication protocol descriptor is invalid"))
            })
            .transpose()?;
        Self::from_header(
            Header {
                version,
                publication_outcome,
            },
            object.contains_key("publicationOutcome"),
        )
    }

    fn from_header(header: Header, has_descriptor: bool) -> Result<Self, ImportError> {
        match header.version {
            1 | 2 if !has_descriptor => Ok(Self::Legacy),
            1 | 2 => Err(stale(
                "legacy plan cannot declare a publication outcome protocol",
            )),
            3 => {
                let descriptor = header
                    .publication_outcome
                    .ok_or_else(|| stale("outcome plan requires its publication protocol"))?;
                match descriptor.version {
                    super::publication_outcome::VERSION => Ok(Self::SignedOutcomeV1),
                    version => Err(ImportError::new(
                        if version > super::publication_outcome::VERSION {
                            ErrorCode::ImportSchemaTooNew
                        } else {
                            ErrorCode::SchemaUnsupported
                        },
                        "unsupported publication outcome protocol",
                    )),
                }
            }
            _ => Err(stale("durable plan version mismatch")),
        }
    }

    /// Called AFTER the established ordinary/association canonical binding.
    /// New versions commit to the protocol AND plan identity/lifetime, even
    /// without identity associations. Legacy does not add a wrapper or default.
    pub(in crate::session_migration) fn bind_payload(
        self,
        mut payload: Value,
        plan_id: &str,
        created: &str,
        expiry: &str,
    ) -> Value {
        if self == Self::SignedOutcomeV1 {
            payload["version"] = json!(3);
            payload["publicationOutcome"] = json!({"version": 1});
            payload["planId"] = json!(plan_id);
            payload["created"] = json!(created);
            payload["expiry"] = json!(expiry);
        }
        payload
    }
}

#[cfg(test)]
#[path = "publication_protocol_tests.rs"]
mod tests;
