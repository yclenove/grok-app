//! Coordinated negative mutations of real mixed acceptance, not positive SQL fixtures.
use super::*;
use rusqlite::{params, Connection};

type StoredRow = (i64, String, String, String);

fn rows(conn: &Connection, job_id: &str) -> Vec<StoredRow> {
    let mut statement = conn.prepare("SELECT rowid,candidate_id,phase,result_json FROM job_items WHERE job_id=?1 ORDER BY rowid").unwrap();
    statement
        .query_map([job_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn genuine_mixed_plan(home: &IsolatedAppHome, ordinary_count: usize) -> (PlanState, JobRecord) {
    let association = confirmed_plan();
    // Uses ordinary discovery, registration and immutable previews. No current
    // adapter is relabelled as historical and no positive SQL authority is made.
    let ordinary_previews = cursor_source(home, ordinary_count + 1);
    let ordinary = plan(ordinary_previews[..ordinary_count].to_vec(), vec![]).unwrap();
    let mut selections = association.selection_set.clone();
    selections
        .entries
        .extend(ordinary.selection_set.entries.clone());
    let mut preview_ids = association.preview_ids.clone();
    preview_ids.extend_from_slice(&ordinary_previews[..ordinary_count]);
    let mixed = AssociationPlanRequest {
        version: 1,
        preview_ids,
        mappings: vec![],
        selection_set: selections,
        identity_associations: association.identity_associations.clone(),
    }
    .build()
    .unwrap();
    let unrelated = plan(vec![ordinary_previews[ordinary_count].clone()], vec![]).unwrap();
    let extra = commit(
        &unrelated.plan_id,
        &unrelated.plan_hash,
        "synthetic-genuine-out-of-plan-producer",
    )
    .unwrap();
    assert_eq!(extra.items.len(), 1);
    (mixed, extra)
}

fn membership_probe(fault: &str) {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let ordinary_count = if fault == "duplicate" { 2 } else { 1 };
    let (confirmed, extra) = genuine_mixed_plan(&home, ordinary_count);
    let request = format!("synthetic-coherent-members-{fault}");
    let queued = acceptance(&confirmed, &request).unwrap();
    assert_eq!(
        queued.items.len(),
        ordinary_count + 1,
        "genuine mixed acceptance is a required premise"
    );
    let conn = open_catalog().unwrap();
    let originals = rows(&conn, &queued.job_id);
    assert_eq!(originals.len(), ordinary_count + 1);
    let ordinary = originals
        .iter()
        .filter(|row| {
            let value: serde_json::Value = serde_json::from_str(&row.3).unwrap();
            value.get("identityAssociation").is_none()
        })
        .min_by_key(|row| &row.1)
        .unwrap();
    let extra_rows = rows(&conn, &extra.job_id);
    assert_eq!(extra_rows.len(), 1);
    assert_ne!(extra_rows[0].1, ordinary.1);
    let original_json: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let display_rows = conn
        .prepare("SELECT rowid,item_index,candidate_id,json FROM job_item_states WHERE job_id=?1 ORDER BY item_index")
        .unwrap()
        .query_map([&queued.job_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (_, original_plan) = load_plan_json(&conn, &confirmed.plan_id).unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let preview_path = jobs::find_preview_dir(&confirmed.identity_associations[0].preview_id)
        .unwrap()
        .unwrap();
    let lease = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    assert!(jobs::load_published_auth_with(&conn, IMPORT, Some(REVISION), None).is_ok());
    assert_eq!(
        acceptance(&confirmed, &request).unwrap().job_id,
        queued.job_id
    );
    assert!(!job_control::preview_retention::completed_owners(
        &conn,
        &[(preview_path.clone(), lease.clone())]
    )
    .unwrap()
    .remove(0)
    .unwrap());

    let mut changed = queued.clone();
    let tx = conn.unchecked_transaction().unwrap();
    if !matches!(fault, "extra" | "duplicate") {
        changed.items.retain(|item| item.candidate_id != ordinary.1);
        assert_eq!(
            tx.execute(
                "DELETE FROM job_items WHERE job_id=?1 AND candidate_id=?2",
                params![queued.job_id, ordinary.1]
            )
            .unwrap(),
            1
        );
    }
    if !matches!(fault, "remove" | "duplicate") {
        changed.items.push(extra.items[0].clone());
        tx.execute(
            "INSERT INTO job_items(job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4)",
            params![
                queued.job_id,
                extra_rows[0].1,
                extra_rows[0].2,
                extra_rows[0].3
            ],
        )
        .unwrap();
    }
    if fault == "duplicate" {
        let second = originals
            .iter()
            .find(|row| {
                let value: serde_json::Value = serde_json::from_str(&row.3).unwrap();
                value.get("identityAssociation").is_none() && row.1 != ordinary.1
            })
            .unwrap();
        let first: serde_json::Value = serde_json::from_str(&ordinary.3).unwrap();
        let mut copied: serde_json::Value = serde_json::from_str(&second.3).unwrap();
        // Keep the second input's authentic candidate/source-member binding.
        // Only duplicate preview metadata. The first candidate sorts earlier,
        // so this reaches unique coverage before snapshot/source-pin refusal.
        assert!(ordinary.1 < second.1);
        for key in [
            "selectionSet",
            "selectionHash",
            "previewPath",
            "previewDigest",
            "previewFileHashes",
        ] {
            copied[key] = first[key].clone();
        }
        tx.execute(
            "UPDATE job_items SET result_json=?1 WHERE job_id=?2 AND candidate_id=?3",
            params![copied.to_string(), queued.job_id, second.1],
        )
        .unwrap();
    }
    changed.item_count = changed.items.len() as u32;
    jobs::save_job(&tx, &changed).unwrap();
    tx.commit().unwrap();
    // Both projections are coherent. The extra/substitute body is well-formed
    // authentic ordinary input from a different plan, not a malformed shortcut.
    let raw: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let projected_count = jobs::item_store::aggregate(&conn, &queued.job_id)
        .unwrap()
        .0;
    assert_eq!(
        projected_count as u64,
        actual["itemCount"].as_u64().unwrap()
    );
    assert_eq!(rows(&conn, &queued.job_id).len(), projected_count as usize);
    jobs::take_job_item_deserializes();
    let job_read = jobs::get_job(&conn, &queued.job_id);
    let job_read_decoded = jobs::take_job_item_deserializes();
    jobs::take_job_item_deserializes();
    let reader = jobs::load_published_auth_with(&conn, IMPORT, Some(REVISION), None);
    jobs::take_job_item_deserializes();
    let retry =
        job_control::enqueue_confirmed_associations(requested_job(&confirmed, &request), vec![]);
    let retry_decoded = jobs::take_job_item_deserializes();
    let owner = job_control::preview_retention::completed_owners(
        &conn,
        &[(preview_path.clone(), lease.clone())],
    )
    .unwrap()
    .remove(0);
    let owner_decoded = jobs::take_job_item_deserializes();

    // Restore the exact captured rows/rowids/body before any red assertion. The
    // ordinary producer remains genuinely queued; no fake positive is installed.
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        params![original_json, queued.job_id],
    )
    .unwrap();
    tx.execute(
        "DELETE FROM job_item_states WHERE job_id=?1",
        [&queued.job_id],
    )
    .unwrap();
    for row in &display_rows {
        tx.execute("INSERT INTO job_item_states(rowid,job_id,item_index,candidate_id,json) VALUES(?1,?2,?3,?4,?5)",
            params![row.0, queued.job_id, row.1, row.2, row.3]).unwrap();
    }
    tx.execute("DELETE FROM job_items WHERE job_id=?1", [&queued.job_id])
        .unwrap();
    for row in &originals {
        tx.execute("INSERT INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4,?5)",
            params![row.0, queued.job_id, row.1, row.2, row.3]).unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(rows(&conn, &queued.job_id), originals);
    assert_eq!(rows(&conn, &extra.job_id), extra_rows);
    assert_eq!(
        load_plan_json(&conn, &confirmed.plan_id).unwrap().1,
        original_plan
    );
    assert_eq!(receipts(&conn), old_receipts);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(
        acceptance(&confirmed, &request).unwrap().job_id,
        queued.job_id
    );
    assert!(!job_control::preview_retention::completed_owners(
        &conn,
        &[(preview_path.clone(), lease.clone())]
    )
    .unwrap()
    .remove(0)
    .unwrap());
    let restored = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(lease).unwrap()
    );
    println!(
        "ASSOCIATION_COHERENT_MEMBERS={}",
        json!({
            "fault": fault, "readerAccepted": reader.is_ok(), "retryAccepted": retry.is_ok(),
            "ownerAccepted": owner.is_ok(), "readerError": reader.as_ref().err().map(|e| &e.code),
            "retryError": retry.as_ref().err().map(|e| &e.code), "ownerError": owner.as_ref().err().map(|e| &e.code),
            "retryJobItemsDecoded": retry_decoded, "ownerJobItemsDecoded": owner_decoded,
            "jobReadError": job_read.as_ref().err().map(|error| error.code),
            "jobReadItemsDecoded": job_read_decoded,
            "exactRowsRestored": true, "genuineMixedAcceptance": true,
        })
    );
    assert_eq!(job_read.unwrap_err().code, ErrorCode::SnapshotUnavailable);
    assert_eq!(job_read_decoded, 0);
    let reader_error = reader.expect_err("coherent out-of-plan set must not authorize old history");
    if fault == "duplicate" {
        assert_eq!(
            reader_error.message,
            "accepted input selection or identity differs from confirmed plan membership",
            "must reach the actual unique-preview refusal, not source-member shape"
        );
    }
    assert_eq!(
        reader_error.code,
        ErrorCode::SnapshotUnavailable,
        "reader {fault}"
    );
    assert_eq!(
        retry.unwrap_err().code,
        ErrorCode::SnapshotUnavailable,
        "retry {fault}"
    );
    assert_eq!(
        owner.unwrap_err().code,
        ErrorCode::SnapshotUnavailable,
        "owner {fault}"
    );
    assert_eq!(retry_decoded, 0);
    assert_eq!(owner_decoded, 0);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_coherent_extra_member() {
    membership_probe("extra");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_coherent_removed_member() {
    membership_probe("remove");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_coherent_substituted_member() {
    membership_probe("substitute");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_coherent_duplicate_preview() {
    membership_probe("duplicate");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_legacy_mixed_ordinary_large_input() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let (confirmed, _) = genuine_mixed_plan(&home, 1);
    let confirmed = historical_legacy_plan(&confirmed);
    let request = "synthetic-mixed-ordinary-input-capacity";
    let queued = acceptance(&confirmed, request).unwrap();
    let conn = open_catalog().unwrap();
    let originals = rows(&conn, &queued.job_id);
    let ordinary = originals
        .iter()
        .find(|row| {
            let value: serde_json::Value = serde_json::from_str(&row.3).unwrap();
            value.get("identityAssociation").is_none()
        })
        .unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let path = jobs::find_preview_dir(&confirmed.identity_associations[0].preview_id)
        .unwrap()
        .unwrap();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&ordinary.3).unwrap();
    // Historical capacity-only display metadata on accepted ordinary input. The
    // association proof/input/confirmation, snapshot and member pins are intact.
    value["adapter"]["graph"]["title"] = json!("x".repeat(16 * 1024 * 1024));
    let large = value.to_string();
    assert!(large.len() > 16 * 1024 * 1024);
    conn.execute(
        "UPDATE job_items SET result_json=?1 WHERE job_id=?2 AND candidate_id=?3",
        params![large, queued.job_id, ordinary.1],
    )
    .unwrap();
    let reader = jobs::load_published_auth_with(&conn, IMPORT, Some(REVISION), None);
    let retry =
        job_control::enqueue_confirmed_associations(requested_job(&confirmed, request), vec![]);
    let owner = job_control::preview_retention::completed_owners(&conn, &[(path, lease)])
        .unwrap()
        .remove(0);
    conn.execute(
        "UPDATE job_items SET result_json=?1 WHERE job_id=?2 AND candidate_id=?3",
        params![ordinary.3, queued.job_id, ordinary.1],
    )
    .unwrap();
    assert_eq!(rows(&conn, &queued.job_id), originals);
    assert_eq!(receipts(&conn), old_receipts);
    assert_eq!(text(IMPORT, REVISION), old_text);
    reader.expect("mixed-plan membership must preserve ordinary v2 body capacity");
    assert_eq!(
        retry
            .expect("ordinary large member must not block genuine retry")
            .job_id,
        queued.job_id
    );
    assert!(!owner.expect("ordinary large member must not become an unknown retention owner"));
    println!("ASSOCIATION_LEGACY_MIXED_ORDINARY_CAPACITY=true");
}
