//! Normal legacy enqueue fixture, not a signed producer or registered grant.
use crate::session_migration::{
    adapters::generic,
    job_control,
    jobs::{self, CommitItem, JobRecord, JobState},
    mapping_scope,
};

pub(crate) fn queued(candidate: &str) -> (JobRecord, CommitItem) {
    let adapter = generic::normalize_bytes(
        br#"[{"role":"user","content":"synthetic read fixture"}]"#,
        &format!("{candidate}.json"),
    )
    .unwrap();
    let item = jobs::prepare_test_commit_item(CommitItem {
        candidate_id: candidate.into(),
        installation_id: "installation".into(),
        provider: "generic-file".into(),
        root_path: "synthetic-offline".into(),
        adapter,
        prepared: None,
        options: mapping_scope::options(None),
        mapping_project_id: None,
        explicit_reimport: false,
        create_project: false,
        import_unassigned_anyway: false,
        explicit_target: None,
        selection_set: Default::default(),
        identity_association: None,
    });
    let requested = JobRecord {
        job_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        plan_id: "synthetic-plan".into(),
        plan_hash: "synthetic-plan-hash".into(),
        state: JobState::Queued,
        state_version: 1,
        items: vec![],
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        next_item_cursor: None,
        progress: None,
    };
    (
        job_control::enqueue(requested, vec![item.clone()]).unwrap(),
        item,
    )
}
