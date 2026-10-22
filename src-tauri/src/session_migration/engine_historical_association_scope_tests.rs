//! Scope corruption is only negative SQL over genuine accepted producer facts.
//! Empty/context-correct classifications are not association reader grants.
use super::*;
use rusqlite::{params, Connection};

fn meta(conn: &Connection) -> Vec<(i64, String, String)> {
    conn.prepare("SELECT rowid,key,hex(CAST(value AS BLOB)) FROM meta ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn raw_jobs(conn: &Connection) -> Vec<(i64, String, String)> {
    conn.prepare("SELECT rowid,job_id,json FROM jobs ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_scope_forgeries_predecode() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let confirmed = confirmed_plan();
    let queued = acceptance(&confirmed, "synthetic-scope-negative-producer").unwrap();
    let scope_key = format!("job_entry_scope:{}", queued.job_id);
    let witness_key = format!("job_entry_scope_accepted:{}", queued.job_id);
    let extra_key = format!("registration_association_accepted:{}:汉", queued.job_id);
    let captured = |key: &str| {
        conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
    };
    let scope = captured(&scope_key);
    let witness = captured(&witness_key);
    assert_eq!(witness.1, hash::sha256_bytes(scope.1.as_bytes()));
    let value: serde_json::Value = serde_json::from_str(&scope.1).unwrap();
    assert_eq!(value["references"].as_array().unwrap().len(), 1);
    assert_eq!(value["references"][0]["version"], 2);
    let (_, plan_json) = load_plan_json(&conn, &confirmed.plan_id).unwrap();
    let original_jobs = raw_jobs(&conn);
    let old_history = text(IMPORT, REVISION);
    let original_meta = meta(&conn);
    let old_receipts = receipts(&conn);
    let entry = serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap();
    let preview_path = jobs::find_preview_dir(&confirmed.preview_ids[0])
        .unwrap()
        .unwrap();
    let lease = std::fs::read(preview_path.join("lease.json")).unwrap();
    let archive = jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap();
    let mut violations = Vec::new();
    for (fault, expected) in [
        ("missing-scope", ErrorCode::HistoryAccessUnavailable),
        ("missing-witness", ErrorCode::HistoryAccessUnavailable),
        ("wrong-witness", ErrorCode::HistoryAccessUnavailable),
        ("blob-scope", ErrorCode::HistoryAccessUnavailable),
        ("utf8-scope", ErrorCode::HistoryAccessUnavailable),
        ("scope-version", ErrorCode::HistoryAccessUnavailable),
        ("wrong-context", ErrorCode::PlanStale),
        ("wrong-job", ErrorCode::HistoryAccessUnavailable),
        ("omitted-association", ErrorCode::HistoryAccessUnavailable),
        (
            "version1-impersonation",
            ErrorCode::HistoryAccessUnavailable,
        ),
        ("duplicate-reference", ErrorCode::HistoryAccessUnavailable),
        ("unknown-scope-field", ErrorCode::HistoryAccessUnavailable),
        ("retained-plan-bytes", ErrorCode::PlanStale),
        ("oversized-scope", ErrorCode::ResourceLimit),
        ("extra-job-witness", ErrorCode::SnapshotUnavailable),
    ] {
        match fault {
            "extra-job-witness" => {
                conn.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,'synthetic-fault')",
                    [&extra_key],
                )
                .unwrap();
            }
            "missing-scope" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&scope_key])
                    .unwrap();
            }
            "missing-witness" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&witness_key])
                    .unwrap();
            }
            "wrong-witness" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params!["0".repeat(64), witness_key],
                )
                .unwrap();
            }
            "blob-scope" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [&scope_key],
                )
                .unwrap();
            }
            "utf8-scope" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(x'ff' AS TEXT) WHERE key=?1",
                    [&scope_key],
                )
                .unwrap();
            }
            "oversized-scope" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![" ".repeat(16 * 1024 * 1024 + 1), scope_key],
                )
                .unwrap();
            }
            "retained-plan-bytes" => {
                conn.execute(
                    "UPDATE plans SET json=json || ' ' WHERE plan_id=?1",
                    [&confirmed.plan_id],
                )
                .unwrap();
            }
            _ => {
                let mut changed = value.clone();
                match fault {
                    "scope-version" => changed["version"] = json!(2),
                    "wrong-context" => changed["contextDigest"] = json!("0".repeat(64)),
                    "wrong-job" => changed["jobId"] = json!(Uuid::new_v4().to_string()),
                    "omitted-association" => changed["references"] = json!([]),
                    "version1-impersonation" => changed["references"][0]["version"] = json!(1),
                    "duplicate-reference" => changed["references"]
                        .as_array_mut()
                        .unwrap()
                        .push(value["references"][0].clone()),
                    "unknown-scope-field" => changed["trustedOrdinary"] = json!(true),
                    _ => unreachable!(),
                }
                let raw = changed.to_string();
                // A checksum-correct scope is STILL not authority. The forged
                // ordinary/legacy cases retain the actual association context.
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![raw, scope_key],
                )
                .unwrap();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![hash::sha256_bytes(raw.as_bytes()), witness_key],
                )
                .unwrap();
            }
        }
        let damaged_meta = meta(&conn);
        for entry in ["get", "page", "retry"] {
            jobs::take_job_item_deserializes();
            let outcome = match entry {
                "get" => job_get(&queued.job_id),
                "page" => job_items_page(&queued.job_id, None, Some(1)),
                "retry" => commit(&confirmed.plan_id, &confirmed.plan_hash, &queued.request_id),
                _ => unreachable!(),
            };
            let decoded = jobs::take_job_item_deserializes();
            let observed = outcome.as_ref().err().map(|error| error.code);
            println!(
                "ACCEPTED_SCOPE_PREDECODE={}",
                json!({"fault": fault, "entry": entry,
                "expected": expected, "observed": observed, "jobItemDeserializations": decoded})
            );
            if observed != Some(expected) || decoded != 0 {
                violations.push((fault, entry, observed, decoded));
            }
        }
        let after_read = meta(&conn);
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "INSERT OR REPLACE INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
            params![scope.0, scope_key, scope.1],
        )
        .unwrap();
        tx.execute(
            "INSERT OR REPLACE INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
            params![witness.0, witness_key, witness.1],
        )
        .unwrap();
        tx.execute(
            "UPDATE plans SET json=?1 WHERE plan_id=?2",
            params![plan_json, confirmed.plan_id],
        )
        .unwrap();
        tx.execute("DELETE FROM meta WHERE key=?1", [&extra_key])
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            after_read, damaged_meta,
            "no reader backfill/repair for {fault}"
        );
        assert_eq!(meta(&conn), original_meta);
        assert_eq!(raw_jobs(&conn), original_jobs);
        assert_eq!(receipts(&conn), old_receipts);
        assert_eq!(
            serde_json::to_value(get_entry(&conn, IMPORT).unwrap()).unwrap(),
            entry
        );
        assert_eq!(
            std::fs::read(preview_path.join("lease.json")).unwrap(),
            lease
        );
        assert_eq!(
            jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap(),
            archive
        );
        assert_eq!(text(IMPORT, REVISION), old_history);
        assert_eq!(job_get(&queued.job_id).unwrap().job_id, queued.job_id);
        assert_eq!(
            commit(&confirmed.plan_id, &confirmed.plan_hash, &queued.request_id)
                .unwrap()
                .job_id,
            queued.job_id
        );
    }
    assert!(
        violations.is_empty(),
        "scope is not a reader grant: {violations:?}"
    );
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_unsealed_old_job_isolation() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let original = job_get(OLD_JOB).unwrap();
    let retained: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE plan_id=?1)",
            [&original.plan_id],
            |row| row.get(0),
        )
        .unwrap();
    // Read-only inspection of the hash-pinned seed confirms its actual
    // version1 plan survives. Do not delete it to manufacture a no-plan case.
    assert!(
        retained,
        "the pinned V243 producer retains its original plan"
    );
    let (_, old_plan) = load_plan_json(&conn, &original.plan_id).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&old_plan).unwrap()["version"],
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM meta WHERE key=?1 OR key=?2",
            params![
                format!("job_entry_scope:{OLD_JOB}"),
                format!("job_entry_scope_accepted:{OLD_JOB}")
            ],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let (legacy, _, _, _) = accept_legacy(cursor_source(&home, 1));
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM meta WHERE key=?1 OR key=?2",
            params![
                format!("job_entry_scope:{OLD_JOB}"),
                format!("job_entry_scope_accepted:{OLD_JOB}")
            ],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2,
        "the genuine acceptance writer migrates both old scope facts"
    );
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let proof: (i64, String, String) = conn
        .query_row(
            "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
            [&legacy.import_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&proof.1).unwrap()["version"],
        1
    );
    let original_rows = raw_jobs(&conn);
    let old_history = text(IMPORT, REVISION);
    let before_meta = meta(&conn);
    conn.execute(
        "UPDATE registration_provenance SET json='{' WHERE import_id=?1",
        [&legacy.import_id],
    )
    .unwrap();
    let get = job_get(OLD_JOB);
    let page = job_items_page(OLD_JOB, None, Some(1));
    conn.execute(
        "UPDATE registration_provenance SET json=?1,digest=?2 WHERE rowid=?3 AND import_id=?4",
        params![proof.1, proof.2, proof.0, legacy.import_id],
    )
    .unwrap();
    assert_eq!(raw_jobs(&conn), original_rows);
    assert_eq!(
        meta(&conn),
        before_meta,
        "reading older jobs cannot create a new accepted scope"
    );
    assert_eq!(text(IMPORT, REVISION), old_history);
    assert_archive_unchanged(&legacy);
    assert_eq!(job_get(OLD_JOB).unwrap().job_id, original.job_id);
    println!(
        "UNSEALED_OLD_JOB_ISOLATION={}",
        json!({"retainedPlan": retained, "initialScopePresent": false, "acceptedScopePresent": true,
        "getError": get.as_ref().err().map(|e| e.code), "pageError": page.as_ref().err().map(|e| e.code)})
    );
    assert_eq!(
        get.unwrap().job_id,
        original.job_id,
        "old ordinary jobs cannot be hidden behind a modern-only fast path"
    );
    assert_eq!(page.unwrap().job_id, original.job_id);
}
