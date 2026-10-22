//! Mutations occur AFTER the authentic detached compatibility snapshot closes.
//! They cannot be accepted using stale migration facts or leave partial scopes.
use super::super::isolation_tests::catalog_rows;
use super::*;
use std::{cell::RefCell, rc::Rc};

const OLD_PLAN: &str = "4343c2b2-6e82-4167-bbab-ad6cfbc2a826";

fn detached_race(
    conn: &Connection,
    plan: &PlanState,
    fault: &'static str,
    expected: ErrorCode,
    mutate: impl FnOnce(&Connection) + 'static,
    restore: impl FnOnce(&Connection),
) {
    let snapshot = UnacceptedSnapshot::capture(conn, plan);
    let before = catalog_rows(conn);
    let damaged = Rc::new(RefCell::new(None));
    let captured = damaged.clone();
    job_control::provenance_scope::test_after_capture(move || {
        let second = open_catalog().unwrap();
        mutate(&second);
        *captured.borrow_mut() = Some(catalog_rows(&second));
    });
    let requested = requested_job(plan, &format!("synthetic-compatibility-race-{fault}"));
    let items = engine_commit::commit_items_for_plan(plan, &requested, false).unwrap();
    let result = job_control::enqueue_confirmed_associations(requested.clone(), items);
    let after = catalog_rows(conn);
    // Restore exactly the negative mutation before any assertion/read/retry.
    restore(conn);
    let error = result.unwrap_err();
    assert_eq!(error.code, expected, "{fault}: {error:?}");
    assert_eq!(
        after,
        damaged
            .borrow()
            .as_ref()
            .expect("real detached capture reached")
            .clone(),
        "{fault}: no partial acceptance or compatibility scope writes"
    );
    assert_eq!(catalog_rows(conn), before, "original catalog bytes/rowids");
    snapshot.refused(conn, &requested);
    snapshot.restored(conn);
    snapshot.retry(conn, plan, requested);
    println!("COMPATIBILITY_DETACHED_RACE={fault}");
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_compatibility_detached_context_races() {
    for fault in [
        "old-plan-bytes",
        "old-plan-hash",
        "old-job-context",
        "scope-appeared",
    ] {
        let _home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        let plan = confirmed_plan();
        let old: (String, String) = conn
            .query_row(
                "SELECT request_id,json FROM jobs WHERE job_id=?1",
                [OLD_JOB],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let (old_hash, old_plan) = load_plan_json(&conn, OLD_PLAN).unwrap();
        let scope_key = format!("job_entry_scope:{OLD_JOB}");
        let changed_job = old.1.clone();
        let changed_plan = old_plan.clone();
        let changed_key = scope_key.clone();
        detached_race(
            &conn,
            &plan,
            fault,
            match fault {
                "old-job-context" => ErrorCode::HistoryAccessUnavailable,
                "scope-appeared" => ErrorCode::SnapshotUnavailable,
                _ => ErrorCode::PlanStale,
            },
            move |second| match fault {
                "old-plan-bytes" => {
                    second
                        .execute(
                            "UPDATE plans SET json=?1 WHERE plan_id=?2",
                            params![format!("{changed_plan} "), OLD_PLAN],
                        )
                        .unwrap();
                }
                "old-plan-hash" => {
                    second
                        .execute(
                            "UPDATE plans SET plan_hash=?1 WHERE plan_id=?2",
                            params!["0".repeat(64), OLD_PLAN],
                        )
                        .unwrap();
                }
                "old-job-context" => {
                    let mut value: serde_json::Value = serde_json::from_str(&changed_job).unwrap();
                    value["requestId"] = json!("synthetic-changed-old-context");
                    second.execute("UPDATE jobs SET request_id='synthetic-changed-old-context',json=?1 WHERE job_id=?2",
                            params![value.to_string(), OLD_JOB]).unwrap();
                }
                "scope-appeared" => {
                    second
                        .execute(
                            "INSERT INTO meta(key,value) VALUES(?1,'synthetic-invalid-scope')",
                            [&changed_key],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            },
            |original| {
                original
                    .execute(
                        "UPDATE jobs SET request_id=?1,json=?2 WHERE job_id=?3",
                        params![old.0, old.1, OLD_JOB],
                    )
                    .unwrap();
                original
                    .execute(
                        "UPDATE plans SET plan_hash=?1,json=?2 WHERE plan_id=?3",
                        params![old_hash, old_plan, OLD_PLAN],
                    )
                    .unwrap();
                original
                    .execute("DELETE FROM meta WHERE key=?1", [&scope_key])
                    .unwrap();
            },
        );
    }
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_compatibility_detached_proof_closure_races() {
    for malformed in [false, true] {
        let home = seed_home();
        let _runtime = RuntimeReset;
        let conn = open_catalog().unwrap();
        // Proof1 is produced by authentic archived-history/registered enqueue,
        // not a positive SQL fixture. Remove only the old job's new routing
        // facts to require compatibility capture beside that existing proof.
        let (legacy, _, _, _) = accept_legacy(cursor_source(&home, 1));
        let keys = [
            format!("job_entry_scope:{OLD_JOB}"),
            format!("job_entry_scope_accepted:{OLD_JOB}"),
            format!("job_entry_intent:{OLD_JOB}"),
            format!("job_entry_plan_snapshot:{OLD_JOB}"),
        ];
        conn.execute(
            "DELETE FROM meta WHERE key=?1 OR key=?2 OR key=?3 OR key=?4",
            params![keys[0], keys[1], keys[2], keys[3]],
        )
        .unwrap();
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM meta WHERE key=?1 OR key=?2 OR key=?3 OR key=?4",
                params![keys[0], keys[1], keys[2], keys[3]],
                |row| row.get(0)
            )
            .unwrap(),
            0,
            "simulate genuinely older routing, not partial new-format corruption"
        );
        let plan = confirmed_plan();
        let proof: (i64, String, String) = conn
            .query_row(
                "SELECT rowid,json,digest FROM registration_provenance WHERE import_id=?1",
                [&legacy.import_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before = catalog_rows(&conn);
        let damaged = Rc::new(RefCell::new(None));
        let captured = damaged.clone();
        let import_id = legacy.import_id.clone();
        job_control::provenance_scope::test_after_capture(move || {
            let second = open_catalog().unwrap();
            if malformed {
                second
                    .execute(
                        "UPDATE registration_provenance SET json='{' WHERE import_id=?1",
                        [&import_id],
                    )
                    .unwrap();
            } else {
                second
                    .execute(
                        "DELETE FROM registration_provenance WHERE import_id=?1",
                        [&import_id],
                    )
                    .unwrap();
            }
            *captured.borrow_mut() = Some(catalog_rows(&second));
        });
        let old_history = text(IMPORT, REVISION);
        let result = acceptance(&plan, "synthetic-compatibility-proof-closure-race");
        let after = catalog_rows(&conn);
        conn.execute("INSERT OR REPLACE INTO registration_provenance(rowid,import_id,json,digest) VALUES(?1,?2,?3,?4)",
            params![proof.0, legacy.import_id, proof.1, proof.2]).unwrap();
        let error = result.unwrap_err();
        assert_eq!(
            error.code,
            if malformed {
                ErrorCode::SchemaUnsupported
            } else {
                ErrorCode::PlanStale
            }
        );
        assert_eq!(
            after,
            damaged
                .borrow()
                .as_ref()
                .expect("real detached closure reached")
                .clone()
        );
        assert_eq!(
            catalog_rows(&conn),
            before,
            "no stale scope or new acceptance remains"
        );
        assert_eq!(text(IMPORT, REVISION), old_history);
        assert_archive_unchanged(&legacy);
        let queued = acceptance(&plan, "synthetic-compatibility-proof-closure-race").unwrap();
        assert_eq!(queued.state, JobState::Queued);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM meta WHERE key=?1 OR key=?2",
                params![keys[0], keys[1]],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        println!("COMPATIBILITY_DETACHED_PROOF_RACE=malformed:{malformed}");
    }
}
