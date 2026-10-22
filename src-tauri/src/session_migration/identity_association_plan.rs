//! Confirmation and durable-plan boundaries. These are intents, not grants.
use super::*;
use crate::session_migration::{engine, selection::SelectionSet, workspace::WorkspaceMapping};
use serde_json::json;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssociationConfirmation {
    pub version: u32,
    pub import_id: String,
    pub expected_revision: String,
    pub preview_id: String,
    pub review_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssociationPlanRequest {
    pub version: u32,
    pub preview_ids: Vec<String>,
    pub mappings: Vec<WorkspaceMapping>,
    pub selection_set: SelectionSet,
    pub identity_associations: Vec<AssociationConfirmation>,
}

fn version(version: u32) -> Result<(), ImportError> {
    if version == VERSION {
        return Ok(());
    }
    Err(ImportError::new(
        if version > VERSION {
            ErrorCode::ImportSchemaTooNew
        } else {
            ErrorCode::SchemaUnsupported
        },
        "unsupported identity association confirmation version",
    ))
}

impl AssociationConfirmation {
    pub(in crate::session_migration) fn validate(&self) -> Result<(), ImportError> {
        version(self.version)?;
        let import = super::super::ids::parse_import_uuid(&self.import_id)?;
        if self.import_id != format!("imp:{import}")
            || !uuid::Uuid::parse_str(&self.expected_revision)
                .is_ok_and(|id| id.to_string() == self.expected_revision)
            || !uuid::Uuid::parse_str(&self.preview_id)
                .is_ok_and(|id| id.to_string() == self.preview_id)
            || self.review_digest.len() != 64
            || !self
                .review_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(stale(
                "association confirmation has noncanonical IDs or digest",
            ));
        }
        Ok(())
    }
}

impl AssociationPlanRequest {
    pub fn build(self) -> Result<engine::PlanState, ImportError> {
        version(self.version)?;
        if self.identity_associations.is_empty() {
            return Err(stale("an association plan requires explicit confirmations"));
        }
        engine::plan_with_identity_associations(
            self.preview_ids,
            self.mappings,
            self.selection_set,
            self.identity_associations,
        )
    }
}

/// Options are not an authority channel, including explicit null behavior keys.
pub(in crate::session_migration) fn reject_behavior_options(
    options: &Value,
) -> Result<(), ImportError> {
    if [
        "identityAssociation",
        "identityAssociations",
        "publicationOutcome",
        "publicationOutcomeVersion",
    ]
    .iter()
    .any(|field| options.get(field).is_some())
    {
        return Err(stale("publication behavior cannot be hidden in options"));
    }
    Ok(())
}

pub(in crate::session_migration) fn validate_bindings(
    selections: &SelectionSet,
    confirmations: &[AssociationConfirmation],
) -> Result<(), ImportError> {
    selections.validate_shape()?;
    if confirmations.len() > super::super::budget::PAGE_SIZE_MAX {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "too many association confirmations",
        ));
    }
    let mut targets = HashSet::new();
    let mut previews = HashSet::new();
    for confirmation in confirmations {
        confirmation.validate()?;
        if !targets.insert(&confirmation.import_id)
            || !previews.insert(&confirmation.preview_id)
            || !selections
                .entries
                .iter()
                .any(|entry| entry.preview_id == confirmation.preview_id)
        {
            return Err(stale(
                "association targets and covered previews must be unique",
            ));
        }
    }
    Ok(())
}

/// Reauthenticate the exact raw plan selection before any lease renewal/write.
pub(in crate::session_migration) fn review_confirmations(
    selections: &SelectionSet,
    confirmations: &[AssociationConfirmation],
) -> Result<Vec<AssociationConfirmation>, ImportError> {
    validate_bindings(selections, confirmations)?;
    for confirmation in confirmations {
        let preview = selections
            .entries
            .iter()
            .find(|entry| entry.preview_id == confirmation.preview_id)
            .ok_or_else(|| stale("association preview is not covered by this plan"))?;
        let review = inspect(&AssociationRequest {
            version: confirmation.version,
            import_id: confirmation.import_id.clone(),
            expected_revision: confirmation.expected_revision.clone(),
            preview: preview.clone(),
        })?;
        if review.review_digest != confirmation.review_digest {
            return Err(stale(
                "association review changed; inspect the fixed pair again",
            ));
        }
    }
    let mut result = confirmations.to_vec();
    result.sort_by(|a, b| a.preview_id.cmp(&b.preview_id));
    Ok(result)
}

/// Strict independent plan version dispatch, including presence versus null.
pub(in crate::session_migration) fn parse_record(
    value: &Value,
    record_version: u64,
    selections: &SelectionSet,
) -> Result<Vec<AssociationConfirmation>, ImportError> {
    let object = value
        .as_object()
        .ok_or_else(|| stale("durable plan must be an object"))?;
    if object.get("version").and_then(Value::as_u64) != Some(record_version) {
        return Err(stale(
            "durable plan record version differs from its dispatch",
        ));
    }
    const FIELDS: &[&str] = &[
        "version",
        "planId",
        "planHash",
        "previewIds",
        "mappings",
        "snapshots",
        "selectionSet",
        "selectionHash",
        "options",
        "created",
        "expiry",
        "identityAssociations",
    ];
    if let Some(options) = value.get("options") {
        reject_behavior_options(options)?;
    }
    if object.keys().any(|key| {
        !FIELDS.contains(&key.as_str()) && !(record_version == 3 && key == "publicationOutcome")
    }) {
        return Err(stale(
            "durable plan contains undeclared or misplaced behavior fields",
        ));
    }
    let confirmations = match record_version {
        1 if !object.contains_key("identityAssociations") => Vec::new(),
        2 | 3 => {
            if record_version == 3 {
                // The same protocol check applies to direct internal codec
                // calls, not just engine's raw-text/duplicate-field check.
                super::super::publication_protocol::PublicationProtocol::decode_value(value)?;
            }
            let plan_id = value
                .get("planId")
                .and_then(Value::as_str)
                .ok_or_else(|| stale("association plan is missing its identity"))?;
            if !uuid::Uuid::parse_str(plan_id).is_ok_and(|id| id.to_string() == plan_id) {
                return Err(stale("association plan identity is not canonical"));
            }
            let lifetime = |field: &str| {
                value
                    .get(field)
                    .and_then(Value::as_str)
                    .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                    .ok_or_else(|| stale("association plan lifetime is invalid"))
            };
            let created = lifetime("created")?;
            let expiry = lifetime("expiry")?;
            if expiry <= created
                || expiry - created
                    > chrono::Duration::seconds(super::super::budget::PREVIEW_TTL_SECS as i64)
            {
                return Err(stale("association plan lifetime exceeds preview policy"));
            }
            if record_version == 3 && !object.contains_key("identityAssociations") {
                Vec::new()
            } else {
                let raw = object
                    .get("identityAssociations")
                    .ok_or_else(|| stale("association plan is missing confirmations"))?;
                let confirmations: Vec<AssociationConfirmation> =
                    serde_json::from_value(raw.clone())
                        .map_err(|_| stale("association plan confirmations are invalid"))?;
                if confirmations.is_empty() {
                    return Err(stale("association plan confirmations cannot be empty"));
                }
                confirmations
            }
        }
        _ => return Err(stale("durable plan version or behavior mismatch")),
    };
    validate_bindings(selections, &confirmations)?;
    Ok(confirmations)
}

/// Ordinary v1 bytes/hash stay unchanged. Associated v2 also binds its lifetime.
pub(in crate::session_migration) fn bind_payload(
    mut payload: Value,
    confirmations: &[AssociationConfirmation],
    plan_id: &str,
    created: &str,
    expiry: &str,
) -> Value {
    if !confirmations.is_empty() {
        payload["version"] = json!(2);
        payload["identityAssociations"] = json!(confirmations);
        payload["planId"] = json!(plan_id);
        payload["created"] = json!(created);
        payload["expiry"] = json!(expiry);
    }
    payload
}
