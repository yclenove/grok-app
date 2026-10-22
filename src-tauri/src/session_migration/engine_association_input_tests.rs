//! Ordinary durable inputs must never swallow association behavior.
use super::*;
use crate::session_migration::job_control;

// Preserve the real historical v2 loader/worker contract independently of the
// new signed producer. These low-level legacy jobs never claim signed grants.
fn legacy_queued(home: &IsolatedAppHome) -> JobRecord {
    let previews = cursor_source(home, 1);
    let preview = &previews[0];
    let (job, _) = queue_registration(preview, resident(preview).source_id, false, json!({}));
    job_get(&job).unwrap()
}

fn durable_json(conn: &rusqlite::Connection, queued: &JobRecord) -> String {
    conn.query_row(
        "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, queued.items[0].candidate_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn replace_input(conn: &rusqlite::Connection, queued: &JobRecord, json: &str) {
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, queued.items[0].candidate_id, json],
    )
    .unwrap();
}

#[test]
fn ordinary_input_rejects_association_behavior_even_hidden_in_options() {
    let home = IsolatedAppHome::acquire("association-input-legacy-injection");
    let preview = cursor_source(&home, 1);
    let pending = plan(preview, vec![]).unwrap();
    let queued = commit(
        &pending.plan_id,
        &pending.plan_hash,
        "legacy-input-injection",
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let candidate = &queued.items[0].candidate_id;
    let original: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            rusqlite::params![queued.job_id, candidate],
            |row| row.get(0),
        )
        .unwrap();
    for key in ["identityAssociation", "identityAssociations"] {
        for location in ["record", "options"] {
            for extra in [json!(null), json!([]), json!({"authorized":true})] {
                let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
                if location == "options" {
                    value["options"][key] = extra;
                } else {
                    value[key] = extra;
                }
                conn.execute(
                    "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
                    rusqlite::params![queued.job_id, candidate, value.to_string()],
                )
                .unwrap();
                assert!(
                    job_control::load_input(&conn, &queued.job_id, candidate).is_err(),
                    "legacy v2 must reject {location}.{key}, including null"
                );
            }
        }
    }
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, candidate, original],
    )
    .unwrap();
    assert!(job_control::load_input(&conn, &queued.job_id, candidate).is_ok());
    assert!(list_entries(&conn).unwrap().is_empty());
    reset_runtime();
}

#[test]
fn hidden_association_is_refused_before_enqueue_or_direct_publish_side_effects() {
    let home = IsolatedAppHome::acquire("association-input-producer-refusal");
    let preview = cursor_source(&home, 1);
    let pending = plan(preview, vec![]).unwrap();
    let queued = commit(&pending.plan_id, &pending.plan_hash, "producer-refusal").unwrap();
    let conn = open_catalog().unwrap();
    let input =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    let prepared = input.prepared.as_ref().unwrap();
    let lease = std::fs::read(prepared.path.join("lease.json")).unwrap();
    let hashes = jobs::prepared_hashes(&prepared.path).unwrap();
    let before = durable_json(&conn, &queued);
    for key in ["identityAssociation", "identityAssociations"] {
        for extra in [json!(null), json!([]), json!({"authorized":true})] {
            let mut changed = input.clone();
            changed.options[key] = extra;
            let mut new_job = queued.clone();
            new_job.job_id = Uuid::new_v4().to_string();
            new_job.request_id = Uuid::new_v4().to_string();
            new_job.items.clear();
            for registered in [false, true] {
                let error = if registered {
                    job_control::enqueue_registered(new_job.clone(), vec![changed.clone()])
                } else {
                    job_control::enqueue(new_job.clone(), vec![changed.clone()])
                }
                .unwrap_err();
                assert_eq!(error.code, ErrorCode::PlanStale);
                assert!(jobs::get_job(&conn, &new_job.job_id).unwrap().is_none());
            }
            let error =
                jobs::publish_item(&conn, &new_job.job_id, &changed, &|| false).unwrap_err();
            assert_eq!(error.code, ErrorCode::PlanStale);
            assert!(
                jobs::latest_receipt(&conn, &new_job.job_id, &input.candidate_id)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                std::fs::read(prepared.path.join("lease.json")).unwrap(),
                lease
            );
            assert_eq!(jobs::prepared_hashes(&prepared.path).unwrap(), hashes);
            assert!(list_entries(&conn).unwrap().is_empty());
            assert_eq!(durable_json(&conn, &queued), before);
        }
    }
    // Ordinary serialization remains v2 with no additional behavior field.
    let value: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(jobs::DURABLE_SCHEMA_VERSION, 2);
    assert!(value.get("identityAssociation").is_none());
    assert!(value.get("identityAssociations").is_none());
    reset_runtime();
}

#[test]
fn worker_refuses_hidden_association_instead_of_publishing_an_independent_entry() {
    let home = IsolatedAppHome::acquire("association-input-worker-refusal");
    let queued = legacy_queued(&home);
    let conn = open_catalog().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&durable_json(&conn, &queued)).unwrap();
    value["options"]["identityAssociation"] = json!(null);
    replace_input(&conn, &queued, &value.to_string());
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let failed = jobs::get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert_eq!(failed.items[0].phase, "failed");
    assert!(failed.items[0].error.is_some());
    assert!(list_entries(&conn).unwrap().is_empty());
    assert!(
        jobs::latest_receipt(&conn, &queued.job_id, &queued.items[0].candidate_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn ordinary_input_codec_preserves_exact_v2_bytes_and_refuses_version_downgrades() {
    let home = IsolatedAppHome::acquire("association-input-codec-v2");
    let preview = cursor_source(&home, 1);
    let pending = plan(preview, vec![]).unwrap();
    let queued = commit(
        &pending.plan_id,
        &pending.plan_hash,
        "ordinary-codec-control",
    )
    .unwrap();
    let conn = open_catalog().unwrap();
    let original = durable_json(&conn, &queued);
    let item = job_control::decode_input(&original, &queued).unwrap();
    assert!(item.identity_association.is_none());
    assert_eq!(job_control::encode_input(&item).unwrap(), original);
    for version in [0, 1, 3, 4] {
        let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
        value["version"] = json!(version);
        assert!(job_control::decode_input(&value.to_string(), &queued).is_err());
    }
    for extra in [json!(null), json!({}), json!([])] {
        let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
        value["identityAssociation"] = extra;
        assert!(job_control::decode_input(&value.to_string(), &queued).is_err());
    }
    let duplicate = format!("{{\"version\":2,{}", &original[1..]);
    assert!(job_control::decode_input(&duplicate, &queued).is_err());
    reset_runtime();
}

#[test]
fn oversized_association_is_refused_before_materializing_sql_text() {
    let home = IsolatedAppHome::acquire("association-input-before-fetch");
    let queued = legacy_queued(&home);
    let conn = open_catalog().unwrap();
    let candidate = &queued.items[0].candidate_id;
    let original = durable_json(&conn, &queued);
    let item = job_control::load_input(&conn, &queued.job_id, candidate).unwrap();
    let prepared = item.prepared.as_ref().unwrap();
    let hashes = jobs::prepared_hashes(&prepared.path).unwrap();
    let lease = std::fs::read(prepared.path.join("lease.json")).unwrap();
    // This is deliberately unaccepted/corrupt input, not a positive association.
    // SQLite BLOB cannot be fetched as String: ResourceLimit must be decided
    // before that full-body conversion, not merely after parsing an owned copy.
    let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
    value["version"] = json!(3);
    value["identityAssociation"] = json!(null);
    value["options"]["syntheticPadding"] = json!("汉".repeat(6 * 1024 * 1024));
    let oversized = value.to_string().into_bytes();
    assert!(oversized.len() > 16 * 1024 * 1024);
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, candidate, oversized],
    )
    .unwrap();
    assert_eq!(
        job_control::load_input(&conn, &queued.job_id, candidate)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit,
        "v3 budget must be checked before attempting the full SQL String fetch"
    );
    reset_runtime();
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let failed = job_get(&queued.job_id).unwrap();
    assert_eq!(failed.state, JobState::CompletedWithIssues);
    assert!(failed.items[0]
        .error
        .as_ref()
        .unwrap()
        .contains(ErrorCode::ResourceLimit.as_str()));
    assert!(list_entries(&conn).unwrap().is_empty());
    assert!(jobs::latest_receipt(&conn, &queued.job_id, candidate)
        .unwrap()
        .is_none());
    assert_eq!(jobs::prepared_hashes(&prepared.path).unwrap(), hashes);
    assert_eq!(
        std::fs::read(prepared.path.join("lease.json")).unwrap(),
        lease
    );
    reset_runtime();
}

#[test]
fn oversized_ordinary_v2_keeps_its_existing_loader_capacity() {
    let home = IsolatedAppHome::acquire("ordinary-input-original-capacity");
    let queued = legacy_queued(&home);
    let conn = open_catalog().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&durable_json(&conn, &queued)).unwrap();
    value["options"]["syntheticPadding"] = json!("x".repeat(16 * 1024 * 1024));
    let large = value.to_string();
    assert!(large.len() > 16 * 1024 * 1024);
    replace_input(&conn, &queued, &large);
    let item =
        job_control::load_input(&conn, &queued.job_id, &queued.items[0].candidate_id).unwrap();
    assert!(item.identity_association.is_none());
    assert_eq!(item.options, value["options"]);
    // Value::to_string sorts object keys, while the codec preserves its original
    // v2 struct field order. Require complete semantic equality, then exact
    // codec-byte stability; do not print multi-MiB bodies on assertion failure.
    let encoded = job_control::encode_input(&item).unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap() == value,
        "ordinary v2 must preserve the entire large input"
    );
    let reopened = job_control::decode_input(&encoded, &queued).unwrap();
    assert!(
        job_control::encode_input(&reopened).unwrap() == encoded,
        "ordinary v2 codec bytes must remain stable"
    );
    assert!(list_entries(&conn).unwrap().is_empty());
    reset_runtime();
}

#[test]
fn registration_input_has_a_metadata_bound_without_shrinking_worker_v2_capacity() {
    use crate::session_migration::hash;
    let home = IsolatedAppHome::acquire("registration-input-before-fetch");
    let queued = legacy_queued(&home);
    let conn = open_catalog().unwrap();
    let candidate = &queued.items[0].candidate_id;
    let original = durable_json(&conn, &queued);
    let ordinary = job_control::registration_input(&conn, &queued.job_id, candidate).unwrap();
    assert_eq!(ordinary.digest, hash::sha256_bytes(original.as_bytes()));
    let mut item = job_control::load_input(&conn, &queued.job_id, candidate).unwrap();
    let prepared = item.prepared.as_ref().unwrap();
    let lease = std::fs::read(prepared.path.join("lease.json")).unwrap();
    let hashes = jobs::prepared_hashes(&prepared.path).unwrap();

    // The original queued record is a normal producer control. Modified rows
    // below test point-read bounds only, not accepted proof/publication authority.
    // Use canonical codec bytes, not whitespace that fails the exact input SHA.
    item.options["syntheticPadding"] = json!("");
    let base = job_control::encode_input(&item).unwrap();
    let bound = 16 * 1024 * 1024;
    item.options["syntheticPadding"] = json!("x".repeat(bound - base.len()));
    let boundary = job_control::encode_input(&item).unwrap();
    assert_eq!(boundary.len(), bound);
    replace_input(&conn, &queued, &boundary);
    let read = job_control::registration_input(&conn, &queued.job_id, candidate).unwrap();
    assert_eq!(read.digest, hash::sha256_bytes(boundary.as_bytes()));
    assert!(conn.is_autocommit());

    item.options["syntheticPadding"] = json!("x".repeat(bound - base.len() + 1));
    let oversized = job_control::encode_input(&item).unwrap();
    assert_eq!(oversized.len(), bound + 1);
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, candidate, oversized.as_bytes()],
    )
    .unwrap();
    assert_eq!(
        job_control::registration_input(&conn, &queued.job_id, candidate)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit,
        "registration budget must refuse before a full BLOB-to-String fetch"
    );
    assert!(conn.is_autocommit());
    replace_input(&conn, &queued, &oversized);
    assert_eq!(
        job_control::registration_input(&conn, &queued.job_id, candidate)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
    let loaded = job_control::load_input(&conn, &queued.job_id, candidate).unwrap();
    assert!(job_control::encode_input(&loaded).unwrap() == oversized);
    conn.execute(
        "UPDATE job_items SET result_json=?3 WHERE job_id=?1 AND candidate_id=?2",
        rusqlite::params![queued.job_id, candidate, original.as_bytes()],
    )
    .unwrap();
    assert!(job_control::registration_input(&conn, &queued.job_id, candidate).is_err());
    replace_input(&conn, &queued, &original);
    assert_eq!(
        job_control::registration_input(&conn, &queued.job_id, candidate).unwrap(),
        ordinary
    );
    assert_eq!(
        std::fs::read(prepared.path.join("lease.json")).unwrap(),
        lease
    );
    assert_eq!(jobs::prepared_hashes(&prepared.path).unwrap(), hashes);
    assert!(list_entries(&conn).unwrap().is_empty());
    assert!(jobs::latest_receipt(&conn, &queued.job_id, candidate)
        .unwrap()
        .is_none());
    reset_runtime();
}
