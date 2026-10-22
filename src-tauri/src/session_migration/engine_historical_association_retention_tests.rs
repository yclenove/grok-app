//! Real accepted retention/retry preloads, not a global SQLite allocation claim.
use super::*;
use rusqlite::params;

fn preload_probe(retry: bool) {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let plan = confirmed_plan();
    let request = if retry {
        "synthetic-association-retry-preload"
    } else {
        "synthetic-association-retention-preload"
    };
    let queued = acceptance(&plan, request).unwrap();
    let conn = open_catalog().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    let raw: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let preview_path = jobs::find_preview_dir(&plan.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    let mut fault = queued.clone();
    fault.items[0].title = Some("x".repeat(16 * 1024 * 1024));
    // Negative display-body mutation of an actually accepted job. No positive
    // proof/input/identity/member row is manufactured by this probe.
    jobs::save_job(&conn, &fault).unwrap();
    jobs::take_job_item_deserializes();
    let result = if retry {
        job_control::enqueue_confirmed_associations(requested_job(&plan, request), vec![])
            .map(|_| false)
    } else {
        job_control::preview_retention::completed_owners(
            &conn,
            &[(preview_path.clone(), lease.clone())],
        )
        .unwrap()
        .remove(0)
    };
    let decoded = jobs::take_job_item_deserializes();
    // Restore only this owned fault before asserting; preserve readable history
    // and prove the valid retry/owner path even when the original probe is red.
    conn.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        params![raw, queued.job_id],
    )
    .unwrap();
    jobs::save_job(&conn, &queued).unwrap();
    assert_eq!(receipts(&conn), old_receipts);
    assert_eq!(text(IMPORT, REVISION), old_text);
    assert_eq!(
        job_control::enqueue_confirmed_associations(requested_job(&plan, request), vec![])
            .unwrap()
            .job_id,
        queued.job_id
    );
    assert!(!job_control::preview_retention::completed_owners(
        &conn,
        &[(preview_path.clone(), lease.clone())]
    )
    .unwrap()
    .remove(0)
    .unwrap());
    let after = jobs::read_preview_lease(&preview_path).unwrap().unwrap();
    assert_eq!(after.retained_by_job, lease.retained_by_job);
    assert_eq!(after.expires_unix, lease.expires_unix);
    assert_eq!(after.file_hashes, lease.file_hashes);
    let error = result.unwrap_err();
    println!(
        "ASSOCIATION_PRELOAD={}",
        json!({
            "retry": retry, "error": error.code, "jobItemDeserializations": decoded,
            "restoredRetryAndOwner": true, "oldHistoryReadable": true,
        })
    );
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    assert_eq!(
        decoded, 0,
        "refuse the association header before owning JobRecord.items"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_retry_header_preload() {
    preload_probe(true);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_retention_header_preload() {
    preload_probe(false);
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_ordinary_large_job_capacity() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let mut original = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    assert_eq!(original.items.len(), 1);
    let raw: String = conn
        .query_row("SELECT json FROM jobs WHERE job_id=?1", [OLD_JOB], |row| {
            row.get(0)
        })
        .unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    // Capacity control on the genuine old ordinary job, through its existing
    // display/state owner. No identity/proof/input authority is invented.
    original.items[0].title = Some("x".repeat(16 * 1024 * 1024));
    jobs::save_job(&conn, &original).unwrap();
    let stored = job_control::enqueue_registered(original.clone(), vec![]);
    conn.execute(
        "UPDATE jobs SET json=?1 WHERE job_id=?2",
        params![raw, OLD_JOB],
    )
    .unwrap();
    assert_eq!(receipts(&conn), old_receipts);
    assert_eq!(text(IMPORT, REVISION), old_text);
    let stored = stored.expect("association bounds must not cap ordinary job display bodies");
    assert_eq!(stored.job_id, OLD_JOB);
    assert_eq!(stored.items[0].title, original.items[0].title);
    reset_runtime();
    assert_eq!(text(IMPORT, REVISION), old_text);
    println!("ORDINARY_LARGE_JOB_RETENTION_CAPACITY=true");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_corrupt_retention_owners() {
    let _home = seed_home();
    let _runtime = RuntimeReset;
    let plan = confirmed_plan();
    let request = "synthetic-real-retention-owner-corruption";
    let queued = acceptance(&plan, request).unwrap();
    let candidate = &queued.items[0].candidate_id;
    let conn = open_catalog().unwrap();
    let (input_row, phase, input): (i64, String, String) = conn
        .query_row(
            "SELECT rowid,phase,result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2",
            params![queued.job_id, candidate],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let (proof_row, proof, digest): (i64, String, String) = conn
        .query_row(
            "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
            [IMPORT],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let key = format!(
        "registration_association_accepted:{}:{}",
        queued.job_id,
        crate::session_migration::hash::sha256_bytes(candidate.as_bytes())
    );
    let witness: String = conn
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    let extra_key = format!("registration_association_accepted:{}:汉", queued.job_id);
    let path = jobs::find_preview_dir(&plan.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = jobs::read_preview_lease(&path).unwrap().unwrap();
    let old_text = text(IMPORT, REVISION);
    let old_receipts = receipts(&conn);
    for (fault, expected) in [
        ("input-null", ErrorCode::SnapshotUnavailable),
        ("input-type", ErrorCode::SnapshotUnavailable),
        ("missing-input", ErrorCode::SnapshotUnavailable),
        ("downgraded-input", ErrorCode::SnapshotUnavailable),
        ("oversized-input", ErrorCode::ResourceLimit),
        ("missing-proof", ErrorCode::SnapshotUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("extra-witness", ErrorCode::SnapshotUnavailable),
    ] {
        match fault {
            "input-null" | "input-type" => {
                let sql = if fault == "input-null" {
                    "UPDATE job_items SET result_json=NULL WHERE rowid=?1"
                } else {
                    "UPDATE job_items SET result_json=zeroblob(8) WHERE rowid=?1"
                };
                conn.execute(sql, [input_row]).unwrap();
            }
            "missing-input" => {
                conn.execute("DELETE FROM job_items WHERE rowid=?1", [input_row])
                    .unwrap();
            }
            "downgraded-input" | "oversized-input" => {
                let changed = if fault == "oversized-input" {
                    format!("{input}{}", " ".repeat(16 * 1024 * 1024))
                } else {
                    let mut value: serde_json::Value = serde_json::from_str(&input).unwrap();
                    value["version"] = json!(2);
                    value.as_object_mut().unwrap().remove("identityAssociation");
                    value.to_string()
                };
                conn.execute(
                    "UPDATE job_items SET result_json=?1 WHERE rowid=?2",
                    params![changed, input_row],
                )
                .unwrap();
            }
            "missing-proof" => {
                conn.execute(
                    "DELETE FROM registration_provenance WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            "missing-witness" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&key])
                    .unwrap();
            }
            "extra-witness" => {
                conn.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,'synthetic-fault')",
                    [&extra_key],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        jobs::take_job_item_deserializes();
        let retry =
            job_control::enqueue_confirmed_associations(requested_job(&plan, request), vec![]);
        let retry_decoded = jobs::take_job_item_deserializes();
        let owner = job_control::preview_retention::completed_owners(
            &conn,
            &[(path.clone(), lease.clone())],
        )
        .unwrap()
        .remove(0);
        let owner_decoded = jobs::take_job_item_deserializes();
        // Restore exact captured rows, only to undo this committed negative
        // mutation. These are not manufactured positive authority fixtures.
        conn.execute("INSERT INTO job_items(rowid,job_id,candidate_id,phase,result_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(job_id,candidate_id) DO UPDATE SET result_json=excluded.result_json",
            params![input_row, queued.job_id, candidate, phase, input]).unwrap();
        conn.execute("INSERT INTO registration_provenance(rowid,import_id,json,digest) VALUES(?1,?2,?3,?4) ON CONFLICT(import_id) DO UPDATE SET json=excluded.json,digest=excluded.digest",
            params![proof_row, IMPORT, proof, digest]).unwrap();
        conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,witness]).unwrap();
        conn.execute("DELETE FROM meta WHERE key=?1", [&extra_key])
            .unwrap();
        assert_eq!(receipts(&conn), old_receipts);
        assert_eq!(text(IMPORT, REVISION), old_text);
        assert_eq!(acceptance(&plan, request).unwrap().job_id, queued.job_id);
        assert_eq!(retry.unwrap_err().code, expected, "retry {fault}");
        assert_eq!(owner.unwrap_err().code, expected, "owner {fault}");
        assert_eq!(
            (retry_decoded, owner_decoded),
            (0, 0),
            "{fault} must refuse before full job decoding"
        );
        assert!(path.is_dir());
        assert_eq!(
            jobs::read_preview_lease(&path)
                .unwrap()
                .unwrap()
                .retained_by_job,
            lease.retained_by_job
        );
        println!("ASSOCIATION_CORRUPT_OWNER={fault}");
    }
}
