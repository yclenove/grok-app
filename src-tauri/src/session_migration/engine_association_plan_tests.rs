//! Version boundaries must reject new behavior, not silently import independently.
use super::*;

fn v1_injection(field: &str) {
    let home = IsolatedAppHome::acquire("association-v1-injection");
    let preview = cursor_source(&home, 1);
    let pending = plan(preview, vec![]).unwrap();
    let conn = open_catalog().unwrap();
    let (hash, json) = load_plan_json(&conn, &pending.plan_id).unwrap();
    for extra in [json!(null), json!([]), json!([{"authorized":true}])] {
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        if field == "options" {
            value["options"]["identityAssociations"] = extra;
            // This is deliberately hash-valid. A behavior field cannot hide in
            // the extensible options object merely by recomputing its hash.
            let changed_hash =
                crate::session_migration::hash::options_hash(&canonical_plan_payload(
                    &pending.preview_ids,
                    &pending.mappings,
                    value["snapshots"].as_array().unwrap(),
                    &pending.selection_set,
                    &pending.selection_hash,
                    &value["options"],
                ));
            value["planHash"] = json!(changed_hash);
        } else {
            value["identityAssociations"] = extra;
        }
        let changed_hash = value["planHash"].as_str().unwrap();
        conn.execute(
            "UPDATE plans SET plan_hash=?1,json=?2 WHERE plan_id=?3",
            rusqlite::params![changed_hash, value.to_string(), pending.plan_id],
        )
        .unwrap();
        let result = engine_prepare_plan::load_durable_plan(&pending.plan_id, changed_hash);
        assert_eq!(
            result.unwrap_err().code,
            ErrorCode::PlanStale,
            "v1 must not ignore {field} identity association behavior"
        );
    }
    conn.execute(
        "UPDATE plans SET plan_hash=?1,json=?2 WHERE plan_id=?3",
        rusqlite::params![hash, json, pending.plan_id],
    )
    .unwrap();
    assert!(engine_prepare_plan::load_durable_plan(&pending.plan_id, &pending.plan_hash).is_ok());
    reset_runtime();
}

#[test]
fn ordinary_v1_plan_rejects_identity_association_field_including_null_and_empty() {
    v1_injection("record");
}

#[test]
fn ordinary_v1_plan_cannot_hide_identity_association_in_options() {
    v1_injection("options");
}

#[test]
fn ordinary_v1_plan_refuses_singular_association_behavior_in_options() {
    let home = IsolatedAppHome::acquire("association-v1-singular-option");
    let preview = cursor_source(&home, 1);
    let pending = plan(preview, vec![]).unwrap();
    let conn = open_catalog().unwrap();
    let (_, original) = load_plan_json(&conn, &pending.plan_id).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
    value["options"]["identityAssociation"] = json!(null);
    let changed_hash = crate::session_migration::hash::options_hash(&canonical_plan_payload(
        &pending.preview_ids,
        &pending.mappings,
        value["snapshots"].as_array().unwrap(),
        &pending.selection_set,
        &pending.selection_hash,
        &value["options"],
    ));
    value["planHash"] = json!(changed_hash);
    conn.execute(
        "UPDATE plans SET plan_hash=?1,json=?2 WHERE plan_id=?3",
        rusqlite::params![changed_hash, value.to_string(), pending.plan_id],
    )
    .unwrap();
    assert_eq!(
        engine_prepare_plan::load_durable_plan(&pending.plan_id, &changed_hash)
            .unwrap_err()
            .code,
        ErrorCode::PlanStale
    );
    reset_runtime();
}
