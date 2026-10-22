//! Plans remain usable from durable storage without a process-lifetime RAM copy.

use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn synthetic_plan(home: &IsolatedAppHome, name: &str) -> PlanState {
    let source = home.path.join(name);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("chat.json"),
        format!(r#"[{{"role":"user","content":"{name}"}}]"#),
    )
    .unwrap();
    let scan = scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = scan.candidates.first().unwrap();
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();
    plan(vec![preview_id], mappings).unwrap()
}

#[test]
fn earlier_plan_remains_committable_after_a_newer_plan_and_runtime_reset() {
    let home = IsolatedAppHome::acquire("durable-plan-no-ram-copy");
    reset_runtime();
    let first = synthetic_plan(&home, "first");
    let second = synthetic_plan(&home, "second");
    assert_ne!(first.plan_id, second.plan_id);
    reset_runtime();
    let latest = recovery().unwrap();
    assert_eq!(latest.plan.unwrap().plan_id, second.plan_id);
    assert!(latest.failed.is_none());

    let first_job = commit(&first.plan_id, &first.plan_hash, "first-plan").unwrap();
    let second_job = commit(&second.plan_id, &second.plan_hash, "second-plan").unwrap();
    assert_ne!(first_job.job_id, second_job.job_id);
    assert_eq!(
        crate::session_migration::job_control::run_pending().unwrap(),
        2
    );
    for (job, content) in [(first_job, "first"), (second_job, "second")] {
        let completed = job_get(&job.job_id).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.item_count, 1);
        let item = &completed.items[0];
        let import_id = item.import_id.as_deref().unwrap();
        let (events, next, total) = events_page(import_id, None, None, Some(10), None).unwrap();
        assert_eq!(total, 1);
        assert_eq!(events.len(), 1);
        assert!(next.is_none());
        assert!(serde_json::to_string(&events[0]).unwrap().contains(content));
        assert_eq!(
            std::fs::read_to_string(home.path.join(content).join("chat.json")).unwrap(),
            format!(r#"[{{"role":"user","content":"{content}"}}]"#)
        );
    }
    reset_runtime();
}

#[test]
fn repeated_recovery_returns_only_the_durable_reference_without_changing_plan() {
    let home = IsolatedAppHome::acquire("durable-plan-recovery-reference");
    reset_runtime();
    let plan = synthetic_plan(&home, "recover");
    let before = load_plan_json(&open_catalog().unwrap(), &plan.plan_id).unwrap();
    for _ in 0..8 {
        reset_runtime();
        let restored = recovery().unwrap();
        let reference = serde_json::to_value(restored.plan.unwrap()).unwrap();
        assert_eq!(
            reference,
            serde_json::json!({
                "planId": plan.plan_id,
                "planHash": plan.plan_hash,
                "previewCount": 1,
                "associationTargets": []
            })
        );
        assert!(restored.job.is_none());
        assert!(restored.failed.is_none());
        assert_eq!(
            load_plan_json(&open_catalog().unwrap(), &plan.plan_id).unwrap(),
            before
        );
    }
    let job = commit(&plan.plan_id, &plan.plan_hash, "recovered-plan").unwrap();
    let again = commit(&plan.plan_id, &plan.plan_hash, "recovered-plan").unwrap();
    assert_eq!(job.job_id, again.job_id);
    assert_eq!(
        crate::session_migration::job_control::run_pending().unwrap(),
        1
    );
    assert_eq!(job_get(&job.job_id).unwrap().state, JobState::Completed);
    reset_runtime();
}
