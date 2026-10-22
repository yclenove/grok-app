//! Historical persisted v1/v2 producer fixture, never a signed grant or override.
//! Its own plan ID/hash and real commit entry preserve the pre-signed contract.
use super::*;

pub(in crate::session_migration) fn enqueue(plan: &PlanState, request: &str) -> JobRecord {
    let legacy = historical_plan(plan);
    assert!(legacy.identity_associations.is_empty());
    commit(&legacy.plan_id, &legacy.plan_hash, request).unwrap()
}

pub(super) fn historical_plan(plan: &PlanState) -> PlanState {
    let conn = open_catalog().unwrap();
    let (_, json) = load_plan_json(&conn, &plan.plan_id).unwrap();
    let mut record: serde_json::Value = serde_json::from_str(&json).unwrap();
    let id = Uuid::new_v4().to_string();
    record["planId"] = id.clone().into();
    record["version"] = 1.into();
    record.as_object_mut().unwrap().remove("publicationOutcome");
    let payload = serde_json::json!({
        "version":record["version"], "previewIds":record["previewIds"],
        "mappings":record["mappings"], "snapshots":record["snapshots"],
        "selectionSet":record["selectionSet"], "selectionHash":record["selectionHash"],
        "options":record["options"],
    });
    let payload = crate::session_migration::identity_association::bind_payload(
        payload,
        &plan.identity_associations,
        &id,
        record["created"].as_str().unwrap(),
        record["expiry"].as_str().unwrap(),
    );
    record["version"] = payload["version"].clone();
    let hash = crate::session_migration::hash::options_hash(&payload);
    record["planHash"] = hash.clone().into();
    crate::session_migration::catalog::persist_plan_json(
        &conn,
        &id,
        &hash,
        &record.to_string(),
        record["created"].as_str().unwrap(),
        record["expiry"].as_str().unwrap(),
    )
    .unwrap();
    let loaded = engine_prepare_plan::load_durable_plan(&id, &hash).unwrap();
    assert_eq!(
        loaded.publication_protocol,
        crate::session_migration::publication_protocol::PublicationProtocol::Legacy
    );
    loaded
}
