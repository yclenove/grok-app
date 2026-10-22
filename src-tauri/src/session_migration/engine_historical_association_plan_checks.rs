//! Positive plan proof uses only the actual pinned historical archive replay.
//! This is not accepted-job/input/identity/publication or vendor evidence.
use super::*;
use crate::session_migration::identity_association::{
    self, AssociationConfirmation, AssociationPlanRequest, AssociationRequest, AssociationReview,
};

pub(super) fn exercise(
    original: &AssociationRequest,
    review: &AssociationReview,
) -> (usize, usize, usize) {
    let old_text = text(IMPORT, REVISION);
    let mut raw = original.preview.clone();
    raw.selection.event_ids.reverse();
    let mut request = original.clone();
    request.preview = raw.clone();
    let raw_review = identity_association::inspect(&request).unwrap();
    assert_ne!(
        raw_review.review_digest, review.review_digest,
        "raw ordering is part of explicit review, not set membership alone"
    );
    let confirmation = AssociationConfirmation {
        version: 1,
        import_id: IMPORT.into(),
        expected_revision: REVISION.into(),
        preview_id: raw.preview_id.clone(),
        review_digest: raw_review.review_digest.clone(),
    };
    let plan_request = AssociationPlanRequest {
        version: 1,
        preview_ids: vec![raw.preview_id.clone()],
        mappings: vec![],
        selection_set: SelectionSet::single(raw.clone()),
        identity_associations: vec![confirmation.clone()],
    };
    let reference = tauri::async_runtime::block_on(
        crate::commands::session_import_identity_association_plan(plan_request.clone()),
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let (hash, stored) = load_plan_json(&conn, &reference.plan_id).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(value["version"], 3);
    assert_eq!(value["publicationOutcome"], json!({"version": 1}));
    assert_eq!(value["identityAssociations"], json!([confirmation.clone()]));
    assert_eq!(value["selectionSet"], json!(plan_request.selection_set));
    reset_runtime();
    let restored =
        engine_prepare_plan::load_durable_plan(&reference.plan_id, &reference.plan_hash).unwrap();
    assert_eq!(
        restored.publication_protocol,
        crate::session_migration::publication_protocol::PublicationProtocol::SignedOutcomeV1
    );
    assert_eq!(restored.identity_associations, vec![confirmation.clone()]);
    assert_eq!(restored.selection_set, plan_request.selection_set);
    assert_eq!(recovery().unwrap().plan.unwrap().plan_id, reference.plan_id);
    let mut checks = 3;

    for field in [
        "digest",
        "target",
        "revision",
        "foreign-preview",
        "duplicate",
        "selection",
        "empty",
    ] {
        let mut changed = plan_request.clone();
        match field {
            "digest" => changed.identity_associations[0].review_digest = "0".repeat(64),
            "target" => {
                changed.identity_associations[0].import_id = format!("imp:{}", Uuid::new_v4())
            }
            "revision" => {
                changed.identity_associations[0].expected_revision = Uuid::new_v4().to_string()
            }
            "foreign-preview" => {
                changed.identity_associations[0].preview_id = Uuid::new_v4().to_string()
            }
            "duplicate" => changed.identity_associations.push(confirmation.clone()),
            "selection" => changed.selection_set.entries[0]
                .selection
                .event_ids
                .reverse(),
            "empty" => changed.identity_associations.clear(),
            _ => unreachable!(),
        }
        let result = association_checks::checked_unchanged(&request, || changed.build());
        assert!(
            result.is_err(),
            "{field} must refuse without plan/lease mutation"
        );
        checks += 1;
    }

    // Still-valid metadata/alias drift changes the dry-run digest, not identity.
    let profile: String = conn
        .query_row(
            "SELECT profile FROM sources WHERE installation_id=?1",
            [INSTALLATION],
            |row| row.get(0),
        )
        .unwrap();
    for field in ["profile", "aliases"] {
        let mut changed: serde_json::Value = serde_json::from_str(&profile).unwrap();
        if field == "profile" {
            changed["profile"] = json!("synthetic-confirmation-drift");
        } else {
            changed["aliases"]
                .as_array_mut()
                .unwrap()
                .push(json!("synthetic/new-alias"));
        }
        conn.execute(
            "UPDATE sources SET profile=?1 WHERE installation_id=?2",
            rusqlite::params![changed.to_string(), INSTALLATION],
        )
        .unwrap();
        assert_eq!(
            association_checks::checked_unchanged(&request, || plan_request.clone().build())
                .unwrap_err()
                .code,
            ErrorCode::PlanStale
        );
        conn.execute(
            "UPDATE sources SET profile=?1 WHERE installation_id=?2",
            rusqlite::params![profile, INSTALLATION],
        )
        .unwrap();
        checks += 1;
    }

    for field in [
        "digest",
        "selection",
        "expiry",
        "created",
        "planId",
        "version",
        "missing-outcome",
        "future-outcome",
        "null",
        "empty",
        "unknown",
    ] {
        let mut changed = value.clone();
        match field {
            "digest" => changed["identityAssociations"][0]["reviewDigest"] = json!("0".repeat(64)),
            "selection" => changed["selectionSet"]["entries"][0]["selection"]["eventIds"]
                .as_array_mut()
                .unwrap()
                .reverse(),
            "expiry" => {
                changed["expiry"] =
                    json!((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339())
            }
            "created" => changed["created"] = json!(chrono::Utc::now().to_rfc3339()),
            "planId" => changed["planId"] = json!(Uuid::new_v4().to_string()),
            "version" => changed["version"] = json!(1),
            "missing-outcome" => {
                changed
                    .as_object_mut()
                    .unwrap()
                    .remove("publicationOutcome");
            }
            "future-outcome" => changed["publicationOutcome"]["version"] = json!(99),
            "null" => changed["identityAssociations"] = json!(null),
            "empty" => changed["identityAssociations"] = json!([]),
            "unknown" => changed["identityAssociations"][0]["authorized"] = json!(true),
            _ => unreachable!(),
        }
        conn.execute(
            "UPDATE plans SET json=?1 WHERE plan_id=?2",
            rusqlite::params![changed.to_string(), reference.plan_id],
        )
        .unwrap();
        assert!(
            engine_prepare_plan::load_durable_plan(&reference.plan_id, &hash).is_err(),
            "{field}"
        );
        conn.execute(
            "UPDATE plans SET json=?1 WHERE plan_id=?2",
            rusqlite::params![stored, reference.plan_id],
        )
        .unwrap();
        checks += 1;
    }
    assert_eq!(
        association_checks::checked_unchanged(&request, || commit_with_reimport(
            &reference.plan_id,
            &reference.plan_hash,
            "association-cannot-reimport",
            true,
        ))
        .unwrap_err()
        .code,
        ErrorCode::PlanStale
    );
    assert_eq!(text(IMPORT, REVISION), old_text);
    let (input_checks, proof_checks) = association_input_checks::exercise(&restored, &confirmation);
    (checks + 1, input_checks, proof_checks)
}
