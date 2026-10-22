//! Progress-dispatch primitives with normal legacy jobs. These do not bypass
//! the signed rollout fence or fabricate an accepted signed publication grant.
use super::*;
use crate::session_migration::{
    catalog, job_control, jobs,
    publication_crypto::JobCrypto,
    test_home::{job_read::queued, IsolatedAppHome},
};

#[test]
fn recorded_progress_finish_keeps_committed_ack_unchanged_after_restart() {
    let _home = IsolatedAppHome::acquire("recorded-progress-replay");
    let conn = catalog::open_catalog().unwrap();
    let (job, _item) = queued("recorded-candidate");
    assert_eq!(job_control::run_pending().unwrap(), 1);
    let done = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(done.state, jobs::JobState::Completed);
    let result = done.items[0].clone();
    assert_eq!(result.phase, "committed");
    drop(conn);

    let conn = catalog::open_catalog().unwrap();
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    // Any repeated progress setter is a test failure, even if it would write
    // identical JSON. The worker must also preserve the already advanced job
    // header version and the original durable input bytes.
    conn.execute_batch(
        "CREATE TRIGGER reject_replay_job_update BEFORE UPDATE ON jobs
         BEGIN SELECT RAISE(ABORT,'repeated job ACK'); END;
         CREATE TRIGGER reject_replay_item_update BEFORE UPDATE ON job_items
         BEGIN SELECT RAISE(ABORT,'repeated item ACK'); END;",
    )
    .unwrap();
    let saved: (u32, String, String, String) = conn
        .query_row(
            "SELECT j.state_version,j.json,i.phase,i.result_json FROM jobs j
             JOIN job_items i ON i.job_id=j.job_id WHERE j.job_id=?1",
            [&job.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    let changes = conn.total_changes();
    for _ in 0..10 {
        ItemProgress {
            item: result.clone(),
            recorded_version: Some(done.state_version),
        }
        .finish(&conn, &job.job_id, 0, &crypto)
        .unwrap();
    }
    assert_eq!(conn.total_changes(), changes);
    let after: (u32, String, String, String) = conn
        .query_row(
            "SELECT j.state_version,j.json,i.phase,i.result_json FROM jobs j
             JOIN job_items i ON i.job_id=j.job_id WHERE j.job_id=?1",
            [&job.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(after, saved);
}

#[test]
fn pending_progress_still_updates_phase_and_version_without_rewriting_input() {
    let _home = IsolatedAppHome::acquire("pending-progress-legacy");
    let conn = catalog::open_catalog().unwrap();
    let (job, _item) = queued("pending-candidate");
    let crypto = JobCrypto::prepare(&conn, &job.job_id).unwrap();
    let original: String = conn
        .query_row(
            "SELECT result_json FROM job_items WHERE job_id=?1",
            [&job.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut running = job.items[0].clone();
    running.phase = "running".into();
    let changes = conn.total_changes();
    ItemProgress::pending(running.clone())
        .finish(&conn, &job.job_id, 0, &crypto)
        .unwrap();
    assert!(conn.total_changes() > changes);
    let after = jobs::get_job(&conn, &job.job_id).unwrap().unwrap();
    assert_eq!(after.state_version, job.state_version + 1);
    assert_eq!(after.state, job.state);
    assert_eq!(
        serde_json::to_value(&after.items[0]).unwrap(),
        serde_json::to_value(&running).unwrap()
    );
    let (phase, input): (String, String) = conn
        .query_row(
            "SELECT phase,result_json FROM job_items WHERE job_id=?1",
            [&job.job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(phase, "running");
    assert_eq!(input, original);
}
