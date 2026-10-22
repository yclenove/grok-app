use super::*;
use crate::session_migration::{catalog, engine, prepare};

fn finish_plan(
    batch_id: &str,
    prepared: prepare::ValidatedPreparePlan,
) -> Result<engine::PlanState, crate::session_migration::errors::ImportError> {
    assert_eq!(prepared.origin.batch_id, batch_id);
    engine::plan_from_validated_prepare(prepared, Vec::new())
}

#[test]
fn validated_plan_cannot_outlive_a_prepare_retry() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE items SET state='failed',preview_id=NULL,selection_json=NULL WHERE seq=1",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE meta SET state='completed-with-issues',state_version=state_version+1 WHERE id=1",
        [],
    )
    .unwrap();
    drop(conn);
    let prepared = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    assert_eq!(prepared.preview_ids.len(), 1);
    prepare::retry(&batch.batch_id).unwrap();
    let done = wait_terminal(&batch.batch_id);
    assert_eq!(done.prepared, 2);
    assert!(finish_plan(&batch.batch_id, prepared).is_err());
    assert!(catalog::latest_plan_row(&catalog::open_catalog().unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn failed_plan_finalization_cannot_restore_a_plan_on_restart() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let prepared = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_planned BEFORE UPDATE OF state ON meta
         WHEN NEW.state='planned' BEGIN SELECT RAISE(ABORT,'injected finalization failure'); END;",
    )
    .unwrap();
    drop(conn);
    assert!(finish_plan(&batch.batch_id, prepared).is_err());
    let (plan_id, plan_hash, _, _) = catalog::latest_plan_row(&catalog::open_catalog().unwrap())
        .unwrap()
        .expect("inert catalog record retained for fault recovery");
    assert!(engine::commit(&plan_id, &plan_hash, "unfinalized-plan").is_err());
    prepare::reset_runtime();
    engine::reset_runtime();
    let recovery = engine::recovery().unwrap();
    assert!(
        recovery.plan.is_none(),
        "a failed command must not restore an unpublished plan"
    );
    assert_eq!(recovery.prepare.unwrap().batch_id, batch.batch_id);
}

#[test]
fn competing_plan_requests_cannot_replace_the_successful_recovery_owner() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let first = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let second = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let winner = finish_plan(&batch.batch_id, first).unwrap();
    assert!(finish_plan(&batch.batch_id, second).is_err());
    prepare::reset_runtime();
    engine::reset_runtime();
    assert_eq!(
        engine::recovery().unwrap().plan.unwrap().plan_id,
        winner.plan_id
    );
}

#[test]
fn finalized_plan_receipt_survives_restart_and_guards_commit() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let prepared = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let plan = finish_plan(&batch.batch_id, prepared).unwrap();
    assert_eq!(
        prepare::status_raw(&batch.batch_id).unwrap().state,
        PrepareBatchState::Planned
    );
    prepare::reset_runtime();
    engine::reset_runtime();
    assert_eq!(
        engine::recovery().unwrap().plan.unwrap().plan_id,
        plan.plan_id
    );
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute("UPDATE plan_receipt SET plan_hash='tampered'", [])
        .unwrap();
    drop(conn);
    assert!(engine::recovery().unwrap().plan.is_none());
    assert!(engine::commit(&plan.plan_id, &plan.plan_hash, "tampered-receipt").is_err());
}

#[test]
fn failed_finalization_can_be_retried_without_repreparing_the_batch() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_planned BEFORE UPDATE OF state ON meta
         WHEN NEW.state='planned' BEGIN SELECT RAISE(ABORT,'injected finalization failure'); END;",
    )
    .unwrap();
    drop(conn);
    assert!(finish_plan(
        &batch.batch_id,
        prepare::validated_plan_inputs(&batch.batch_id).unwrap()
    )
    .is_err());
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute_batch("DROP TRIGGER fail_planned").unwrap();
    drop(conn);
    let plan = finish_plan(
        &batch.batch_id,
        prepare::validated_plan_inputs(&batch.batch_id).unwrap(),
    )
    .unwrap();
    let recovery = engine::recovery().unwrap();
    assert_eq!(recovery.plan.unwrap().plan_id, plan.plan_id);
    assert!(recovery.failed.is_none());
    let job = engine::commit(&plan.plan_id, &plan.plan_hash, "recovered-plan").unwrap();
    assert_eq!(
        crate::session_migration::job_control::run_pending().unwrap(),
        1
    );
    assert_eq!(
        engine::job_get(&job.job_id).unwrap().state,
        crate::session_migration::jobs::JobState::Completed
    );
}

#[test]
fn simultaneous_plan_requests_have_one_durable_winner() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let start = Arc::new(Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let prepared = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
            let batch_id = batch.batch_id.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                finish_plan(&batch_id, prepared)
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    let winners = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().ok())
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    let conn = catalog::open_catalog().unwrap();
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        engine::recovery().unwrap().plan.unwrap().plan_id,
        winners[0].plan_id
    );
}

#[test]
fn catalog_failure_does_not_consume_the_prepared_batch() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let before = wait_terminal(&batch.batch_id);
    let conn = catalog::open_catalog().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_plan BEFORE INSERT ON plans BEGIN
         SELECT RAISE(ABORT,'injected catalog write failure'); END;",
    )
    .unwrap();
    drop(conn);
    assert!(finish_plan(
        &batch.batch_id,
        prepare::validated_plan_inputs(&batch.batch_id).unwrap()
    )
    .is_err());
    let after = prepare::status_raw(&batch.batch_id).unwrap();
    assert_eq!(after.state, before.state);
    assert_eq!(after.state_version, before.state_version);
    let recovery = engine::recovery().unwrap();
    assert!(recovery.plan.is_none());
    assert_eq!(recovery.prepare.unwrap().batch_id, batch.batch_id);
}

#[test]
fn plan_receipt_retention_outlives_the_new_plan_lease() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    wait_terminal(&batch.batch_id);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    conn.execute(
        "UPDATE meta SET retention_expires_unix=?1 WHERE id=1",
        [chrono::Utc::now().timestamp() + 1],
    )
    .unwrap();
    drop(conn);
    let plan = finish_plan(
        &batch.batch_id,
        prepare::validated_plan_inputs(&batch.batch_id).unwrap(),
    )
    .unwrap();
    let (_, json) =
        catalog::load_plan_json(&catalog::open_catalog().unwrap(), &plan.plan_id).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    let plan_expiry = chrono::DateTime::parse_from_rfc3339(json["expiry"].as_str().unwrap())
        .unwrap()
        .timestamp();
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    let receipt_expiry: i64 = conn
        .query_row(
            "SELECT retention_expires_unix FROM meta WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(receipt_expiry > plan_expiry);
}
