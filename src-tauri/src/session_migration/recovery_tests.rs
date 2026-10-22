use super::*;
use crate::session_migration::{job_control, jobs, test_home::IsolatedAppHome};

fn fixture_plan(home: &IsolatedAppHome, name: &str) -> PlanState {
    let source = home.path.join(name);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("chat.json"),
        br#"[{"role":"user","content":"synthetic question"},{"role":"assistant","content":"synthetic answer"}]"#,
    )
    .unwrap();
    let scan = scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = &scan.candidates[0];
    let (id, _) = preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = propose_mappings(std::slice::from_ref(&id), None).unwrap();
    plan(vec![id], mappings).unwrap()
}

fn after_original_expiry(plan: &PlanState) -> crate::session_migration::clock::Fixed {
    let conn = open_catalog().unwrap();
    let (_, original) = load_plan_json(&conn, &plan.plan_id).unwrap();
    let value: serde_json::Value = serde_json::from_str(&original).unwrap();
    let expiry = chrono::DateTime::parse_from_rfc3339(value["expiry"].as_str().unwrap()).unwrap();
    let clock = crate::session_migration::clock::Fixed::at(
        expiry.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1),
    );
    reset_runtime();
    let error = commit(&plan.plan_id, &plan.plan_hash, "fresh-expired-submission").unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert!(error.message.contains("expired"));
    assert_eq!(load_plan_json(&conn, &plan.plan_id).unwrap().1, original);
    clock
}

fn mark_cancelled(job_id: &str) {
    // Model an acknowledged cancellation without running unrelated queued jobs.
    job_control::change_job(&open_catalog().unwrap(), job_id, |job| {
        job.state = JobState::Cancelled;
        for item in &mut job.items {
            item.phase = "cancelled".into();
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn completed_import_does_not_restore_its_plan_or_report_it_expired() {
    let home = IsolatedAppHome::acquire("recovery-completed");
    let planned = fixture_plan(&home, "source");
    let job = commit(&planned.plan_id, &planned.plan_hash, "completed-request").unwrap();
    job_control::run_pending().unwrap();
    assert_eq!(job_get(&job.job_id).unwrap().state, JobState::Completed);
    reset_runtime();

    let restored = recovery().unwrap();
    assert!(
        restored.plan.is_none(),
        "completed job must not reopen a commit plan"
    );
    assert!(restored.job.is_none());
    let _clock = after_original_expiry(&planned);
    let restored = recovery().unwrap();
    assert!(restored.plan.is_none());
    assert!(!restored.expired, "consumed plans are not pending recovery");
    assert!(restored.failed.is_none());
    assert_eq!(list_imported_page(None, None).unwrap().0.len(), 1);
}

#[test]
fn consumed_latest_plan_does_not_resurrect_an_older_abandoned_plan() {
    let home = IsolatedAppHome::acquire("recovery-abandoned");
    let _old = fixture_plan(&home, "abandoned");
    let latest = fixture_plan(&home, "latest");
    commit(&latest.plan_id, &latest.plan_hash, "latest-request").unwrap();
    job_control::run_pending().unwrap();
    reset_runtime();
    let restored = recovery().unwrap();
    assert!(restored.plan.is_none());
    assert!(restored.job.is_none());
}

#[test]
fn queued_job_recovers_original_identity_not_a_second_plan() {
    let home = IsolatedAppHome::acquire("recovery-queued");
    let planned = fixture_plan(&home, "source");
    let job = commit(&planned.plan_id, &planned.plan_hash, "queued-request").unwrap();
    let _clock = after_original_expiry(&planned);
    reset_runtime();
    let restored = recovery().unwrap();
    assert_eq!(restored.job.unwrap().job_id, job.job_id);
    assert!(restored.plan.is_none());
    assert!(!restored.expired);
    assert!(restored.failed.is_none());
}

#[test]
fn interrupted_jobs_restore_and_retry_with_the_same_identity_after_plan_expiry() {
    for state in [
        JobState::Cancelled,
        JobState::Failed,
        JobState::CompletedWithIssues,
    ] {
        let home = IsolatedAppHome::acquire("recovery-interrupted");
        let planned = fixture_plan(&home, "source");
        let queued = commit(&planned.plan_id, &planned.plan_hash, "interrupted-request").unwrap();
        job_control::change_job(&open_catalog().unwrap(), &queued.job_id, |job| {
            job.state = state;
            job.items[0].phase = if state == JobState::Cancelled {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            job.items[0].error = Some("synthetic interruption".into());
            Ok(())
        })
        .unwrap();
        let _clock = after_original_expiry(&planned);
        reset_runtime();
        let restored = recovery().unwrap();
        let job = restored
            .job
            .expect("interrupted original job is recoverable");
        assert_eq!(job.job_id, queued.job_id);
        assert_eq!(job.request_id, queued.request_id);
        assert_eq!(job.state, state);
        assert!(restored.plan.is_none());
        assert!(!restored.expired);
        job_control::retry(&job.job_id).unwrap();
        job_control::run_pending().unwrap();
        assert_eq!(job_get(&job.job_id).unwrap().state, JobState::Completed);
        assert_eq!(list_imported_page(None, None).unwrap().0.len(), 1);
        assert!(recovery().unwrap().job.is_none());
    }
}

#[test]
fn newer_unsubmitted_plan_is_still_recovered_after_a_completed_job() {
    let home = IsolatedAppHome::acquire("recovery-new-plan");
    let old = fixture_plan(&home, "completed");
    commit(&old.plan_id, &old.plan_hash, "old-request").unwrap();
    job_control::run_pending().unwrap();
    let latest = fixture_plan(&home, "pending");
    reset_runtime();
    let restored = recovery().unwrap();
    assert_eq!(restored.plan.unwrap().plan_id, latest.plan_id);
    assert!(restored.job.is_none());
    let _clock = after_original_expiry(&latest);
    let expired = recovery().unwrap();
    assert!(expired.plan.is_none());
    assert!(expired.expired);
}

#[test]
fn same_hash_on_a_different_plan_does_not_consume_the_current_plan() {
    let home = IsolatedAppHome::acquire("recovery-plan-identity");
    let planned = fixture_plan(&home, "source");
    jobs::save_job(
        &open_catalog().unwrap(),
        &JobRecord {
            job_id: Uuid::new_v4().to_string(),
            request_id: "other-plan".into(),
            plan_id: Uuid::new_v4().to_string(),
            plan_hash: planned.plan_hash.clone(),
            state: JobState::Completed,
            state_version: 1,
            items: vec![],
            created_at: chrono::Utc::now().to_rfc3339(),
            item_count: 0,
            next_item_cursor: None,
            progress: None,
        },
    )
    .unwrap();
    reset_runtime();
    assert_eq!(recovery().unwrap().plan.unwrap().plan_id, planned.plan_id);
}

#[test]
fn active_job_takes_priority_over_a_newer_interrupted_job() {
    let home = IsolatedAppHome::acquire("recovery-active-priority");
    let first = fixture_plan(&home, "first");
    let active = commit(&first.plan_id, &first.plan_hash, "active-request").unwrap();
    let second = fixture_plan(&home, "second");
    let cancelled = commit(&second.plan_id, &second.plan_hash, "cancelled-request").unwrap();
    mark_cancelled(&cancelled.job_id);
    assert_eq!(
        job_get(&cancelled.job_id).unwrap().state,
        JobState::Cancelled
    );
    let _clock = after_original_expiry(&second);
    reset_runtime();
    let restored = recovery().unwrap();
    assert_eq!(restored.job.unwrap().job_id, active.job_id);
    assert!(restored.plan.is_none());
    assert!(!restored.expired);
}

#[test]
fn inconsistent_consuming_job_fails_closed_without_reopening_a_plan() {
    for update in [
        "UPDATE jobs SET plan_hash='wrong-hash'",
        "UPDATE jobs SET json=json_set(json, '$.planHash', 'wrong-hash')",
        "UPDATE jobs SET json=json_set(json, '$.jobId', 'wrong-id')",
        "UPDATE jobs SET json=json_set(json, '$.state', 'failed')",
        "UPDATE jobs SET json='corrupt-json'",
    ] {
        let home = IsolatedAppHome::acquire("recovery-corrupt-job");
        let planned = fixture_plan(&home, "source");
        commit(&planned.plan_id, &planned.plan_hash, "corrupt-request").unwrap();
        job_control::run_pending().unwrap();
        let conn = open_catalog().unwrap();
        conn.execute(update, []).unwrap();
        reset_runtime();
        assert_eq!(recovery().unwrap_err().code, ErrorCode::SnapshotUnavailable);
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn same_timestamp_plans_and_jobs_recover_the_most_recent_insert() {
    let home = IsolatedAppHome::acquire("recovery-tie");
    let _clock = crate::session_migration::clock::Fixed::at(chrono::Utc::now());
    let _old = fixture_plan(&home, "old");
    let latest = fixture_plan(&home, "latest");
    let conn = open_catalog().unwrap();
    let plan_times: i64 = conn
        .query_row("SELECT COUNT(DISTINCT created_at) FROM plans", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        plan_times, 1,
        "plans genuinely share a timestamp at acceptance"
    );
    assert_eq!(recovery().unwrap().plan.unwrap().plan_id, latest.plan_id);
    let first = commit(&latest.plan_id, &latest.plan_hash, "first-request").unwrap();
    let second = commit(&latest.plan_id, &latest.plan_hash, "second-request").unwrap();
    mark_cancelled(&first.job_id);
    mark_cancelled(&second.job_id);
    assert_eq!(first.created_at, second.created_at);
    reset_runtime();
    let restored = recovery().unwrap();
    assert_eq!(restored.job.unwrap().job_id, second.job_id);
    assert!(restored.plan.is_none());
}

#[test]
fn accepted_ordinary_job_recovers_and_retries_without_its_draft_plan() {
    for fault in ["missing", "malformed"] {
        let home = IsolatedAppHome::acquire("recovery-independent-input");
        let planned = fixture_plan(&home, "source");
        let queued = commit(&planned.plan_id, &planned.plan_hash, "independent-request").unwrap();
        let conn = open_catalog().unwrap();
        match fault {
            "missing" => conn.execute("DELETE FROM plans WHERE plan_id=?1", [&planned.plan_id]),
            "malformed" => conn.execute(
                "UPDATE plans SET json='{' WHERE plan_id=?1",
                [&planned.plan_id],
            ),
            _ => unreachable!(),
        }
        .unwrap();
        reset_runtime();
        let restored = recovery().unwrap();
        assert_eq!(
            restored.job.as_ref().unwrap().job_id,
            queued.job_id,
            "{fault}"
        );
        assert!(restored.plan.is_none());
        assert!(restored.failed.is_none());
        mark_cancelled(&queued.job_id);
        job_control::retry(&queued.job_id).unwrap();
        job_control::run_pending().unwrap();
        let completed = job_get(&queued.job_id).unwrap();
        assert_eq!(
            completed.state,
            JobState::Completed,
            "{fault}: {completed:?}"
        );
        assert_eq!(
            completed.items[0].import_id.as_ref().unwrap(),
            &list_imported_page(None, None).unwrap().0[0].import_id
        );
    }
}

#[test]
fn accepted_original_snapshot_corruption_fails_before_job_item_decode() {
    let home = IsolatedAppHome::acquire("recovery-snapshot-corruption");
    let planned = fixture_plan(&home, "source");
    let queued = commit(&planned.plan_id, &planned.plan_hash, "snapshot-request").unwrap();
    // Explicitly warm the ordinary original codec before every fault family.
    // A cached pure parse must never hide missing/changed current SQL facts.
    assert_eq!(job_get(&queued.job_id).unwrap().job_id, queued.job_id);
    let conn = open_catalog().unwrap();
    let snapshot_key = format!("job_entry_plan_snapshot:{}", queued.job_id);
    let scope_key = format!("job_entry_scope:{}", queued.job_id);
    let witness_key = format!("job_entry_scope_accepted:{}", queued.job_id);
    let capture = |key: &str| {
        conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
    };
    let snapshot = capture(&snapshot_key);
    let scope = capture(&scope_key);
    let witness = capture(&witness_key);
    for (fault, expected) in [
        ("missing", ErrorCode::HistoryAccessUnavailable),
        ("blob", ErrorCode::HistoryAccessUnavailable),
        ("bytes", ErrorCode::HistoryAccessUnavailable),
        ("partial-unsealed", ErrorCode::HistoryAccessUnavailable),
        ("hash-correct-modified-intent", ErrorCode::PlanStale),
    ] {
        match fault {
            "missing" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&snapshot_key])
                    .unwrap();
            }
            "blob" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [&snapshot_key],
                )
                .unwrap();
            }
            "bytes" => {
                conn.execute(
                    "UPDATE meta SET value=value || ' ' WHERE key=?1",
                    [&snapshot_key],
                )
                .unwrap();
            }
            "partial-unsealed" => {
                conn.execute(
                    "DELETE FROM meta WHERE key=?1 OR key=?2",
                    rusqlite::params![scope_key, witness_key],
                )
                .unwrap();
            }
            "hash-correct-modified-intent" => {
                let mut changed: serde_json::Value = serde_json::from_str(&snapshot.1).unwrap();
                changed["options"] = serde_json::json!({"includeSecrets": true});
                let raw = changed.to_string();
                let mut changed_scope: serde_json::Value = serde_json::from_str(&scope.1).unwrap();
                changed_scope["planJsonDigest"] =
                    serde_json::json!(crate::session_migration::hash::sha256_bytes(raw.as_bytes()));
                let raw_scope = changed_scope.to_string();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    rusqlite::params![raw, snapshot_key],
                )
                .unwrap();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    rusqlite::params![raw_scope, scope_key],
                )
                .unwrap();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    rusqlite::params![
                        crate::session_migration::hash::sha256_bytes(raw_scope.as_bytes()),
                        witness_key
                    ],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        for reader in ["get", "page"] {
            jobs::take_job_item_deserializes();
            let result = if reader == "get" {
                job_get(&queued.job_id)
            } else {
                job_items_page(&queued.job_id, None, Some(1))
            };
            let error = result.unwrap_err();
            assert_eq!(error.code, expected, "{fault}/{reader}");
            assert!(
                error.is_job_local(),
                "{fault}/{reader}: specific intent corruption is job-local"
            );
            assert_eq!(jobs::take_job_item_deserializes(), 0, "{fault}/{reader}");
        }
        for (key, original) in [
            (&snapshot_key, &snapshot),
            (&scope_key, &scope),
            (&witness_key, &witness),
        ] {
            conn.execute(
                "INSERT OR REPLACE INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                rusqlite::params![original.0, key, original.1],
            )
            .unwrap();
        }
        assert_eq!(job_get(&queued.job_id).unwrap().job_id, queued.job_id);
    }
}

#[test]
fn plan_job_lookup_does_not_deserialize_items() {
    let home = IsolatedAppHome::acquire("recovery-bounded-header");
    let planned = fixture_plan(&home, "source");
    let job = commit(&planned.plan_id, &planned.plan_hash, "bounded-request").unwrap();
    jobs::take_job_item_deserializes();
    let found = crate::session_migration::catalog::latest_plan_job_id(
        &open_catalog().unwrap(),
        &planned.plan_id,
        &planned.plan_hash,
    )
    .unwrap();
    assert_eq!(found.as_deref(), Some(job.job_id.as_str()));
    assert_eq!(jobs::take_job_item_deserializes(), 0);
}
