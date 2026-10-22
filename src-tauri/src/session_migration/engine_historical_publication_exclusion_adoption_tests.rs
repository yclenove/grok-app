//! A real archived producer receives a ticket before genuine later adoption.
//! No positive proof/ticket/fence/accepted row is inserted by this test.
use super::super::isolation_tests::catalog_rows;
use super::*;
use rusqlite::params;

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_publication_exclusion_later_adoption() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let history = text(IMPORT, REVISION);
    let original_receipts = all_receipts(&conn);
    let original_job = jobs::get_job(&conn, OLD_JOB).unwrap().unwrap();
    let (_, encoded) = load_plan_json(&conn, &original_job.plan_id).unwrap();
    let retained: PlanState = serde_json::from_str(&encoded).unwrap();
    let requested = requested_job(&retained, "synthetic-genuine-old-exclusion-before-adoption");
    let item = restore_old_item(&home, &requested);
    let old = job_control::enqueue_registered(requested, vec![item.clone()]).unwrap();
    let ticket_key = format!("job_publication_exclusion:{}", old.job_id);
    let ticket: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key=?1",
            [&ticket_key],
            |row| row.get(0),
        )
        .unwrap();
    let ticket: serde_json::Value = serde_json::from_str(&ticket).unwrap();
    assert_eq!(ticket["items"].as_array().unwrap().len(), 1);
    let key = format!(
        "publication_tuple_fence:{}",
        ticket["items"][0]["tupleDigest"].as_str().unwrap()
    );
    let before: String = conn
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(ticket["items"][0]["fence"], before);
    let confirmed = confirmed_plan();
    let adopted = acceptance(
        &confirmed,
        "synthetic-genuine-later-adoption-revokes-exclusion",
    )
    .unwrap();
    let after: String = conn
        .query_row("SELECT value FROM meta WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .unwrap();
    assert_ne!(
        before, after,
        "genuine proof2 acceptance rotates original old tuple"
    );
    for fault in [
        "none",
        "missing-owner-selectors",
        "substituted-owner-selectors",
        "malformed-proof",
    ] {
        let tx = conn.unchecked_transaction().unwrap();
        match fault {
            "none" => {}
            "missing-owner-selectors" => {
                tx.execute(
                    "DELETE FROM registration_association_old_owners WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
                tx.execute(
                    "DELETE FROM meta WHERE key LIKE 'registration_association_old_owner:%'",
                    [],
                )
                .unwrap();
            }
            "substituted-owner-selectors" => {
                tx.execute("UPDATE registration_association_old_owners SET tuple_digest=?1 WHERE import_id=?2", params!["0".repeat(64), IMPORT]).unwrap();
                tx.execute(
                    "UPDATE meta SET key=?1 WHERE key LIKE 'registration_association_old_owner:%'",
                    [format!(
                        "registration_association_old_owner:{}",
                        "0".repeat(64)
                    )],
                )
                .unwrap();
            }
            "malformed-proof" => {
                tx.execute(
                    "UPDATE registration_provenance SET json='{' WHERE import_id=?1",
                    [IMPORT],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let damaged = catalog_rows(&tx);
        let result = jobs::publish_item(&tx, &old.job_id, &item, &|| false);
        let observed = catalog_rows(&tx);
        tx.rollback().unwrap();
        assert!(
            result.is_err(),
            "stale accepted producer cannot revive: {fault}"
        );
        assert_eq!(observed, damaged, "no receipt/catalog mutation: {fault}");
        println!("PUBLICATION_EXCLUSION_LATER_ADOPTION={fault}:genuine-revocation:no-revival");
    }
    assert_eq!(job_get(&adopted.job_id).unwrap().state, JobState::Queued);
    assert_eq!(text(IMPORT, REVISION), history);
    assert_eq!(all_receipts(&conn), original_receipts);
}
