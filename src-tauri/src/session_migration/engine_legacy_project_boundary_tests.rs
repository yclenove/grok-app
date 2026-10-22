//! Actual project/store boundary of a delayed historical producer.
//! Synthetic production-path fixtures, not real App import or vendor evidence.
use super::*;
use crate::session_migration::{catalog, job_control, project_link, workspace::WorkspaceMapping};
use std::cell::Cell;

fn project_snapshot() -> serde_json::Value {
    serde_json::to_value(crate::store::load_projects_snapshot().unwrap()).unwrap()
}

fn mapping_snapshot(conn: &rusqlite::Connection) -> serde_json::Value {
    serde_json::to_value(catalog::list_mappings(conn).unwrap()).unwrap()
}

fn mapping_to(project_id: String, path: String) -> WorkspaceMapping {
    WorkspaceMapping {
        source: None,
        root_id: "synthetic-user-mapping".into(),
        target_project_id: Some(project_id),
        explicit_target: Some(path),
        unassigned: false,
        user_choice_version: 2,
        primary: true,
        suggested_target: None,
        create_project: false,
        import_unassigned_anyway: false,
    }
}

fn publish_with_busy_adoption(
    conn: &rusqlite::Connection,
    job_id: &str,
    item: &CommitItem,
    pending: &PlanState,
) -> (jobs::JobItem, String) {
    let probes = Cell::new(0);
    let refused = Cell::new(false);
    let request = Uuid::new_v4().to_string();
    let projects_before = project_snapshot();
    let mappings_before = mapping_snapshot(conn);
    let published = jobs::publish_item(conn, job_id, item, &|| {
        let probe = probes.get() + 1;
        probes.set(probe);
        if probe == 2 {
            assert_eq!(
                jobs::latest_receipt(conn, job_id, &item.candidate_id)
                    .unwrap()
                    .unwrap()
                    .phase,
                "prepared"
            );
            let counts = table_counts();
            let error = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap_err();
            assert_eq!(error.code, ErrorCode::SourceBusy);
            assert_eq!(table_counts(), counts);
            assert_no_provenance_extension();
            assert_eq!(project_snapshot(), projects_before);
            assert_eq!(mapping_snapshot(conn), mappings_before);
            refused.set(true);
        }
        false
    })
    .unwrap();
    assert!(
        refused.get(),
        "adoption refused while publisher held its owner"
    );
    assert_eq!(published.phase, "committed");
    (published, request)
}

#[test]
fn winning_legacy_creation_is_preserved_by_explicit_adoption_retry() {
    let home = IsolatedAppHome::acquire("legacy-project-creation-overlap");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let target = home.path.join("explicit synthetic project");
    std::fs::create_dir(&target).unwrap();
    let (delayed, item) = queue_registration_custom(
        &previews[0],
        seed.installation.clone(),
        false,
        json!({"rawRetention":"normalized-only"}),
        |item| {
            item.create_project = true;
            item.explicit_target = Some(target.to_string_lossy().into_owned());
        },
    );
    let canonical = resident(&previews[0]).source_id;
    let pending = plan(previews, vec![]).unwrap();
    let conn = open_catalog().unwrap();
    let (published, request) = publish_with_busy_adoption(&conn, &delayed, &item, &pending);
    assert_eq!(entry_identity(&seed).0, seed.installation);
    assert_archive_unchanged(&seed);
    assert_eq!(table_counts().0, 1);
    let projects = crate::store::load_projects_snapshot().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(published.mapping_project_id.as_ref(), Some(&projects[0].id));
    let rightful_projects = project_snapshot();
    let rightful_mappings = mapping_snapshot(&conn);
    job_control::run_pending().unwrap();
    reset_runtime();
    assert_eq!(job_get(&delayed).unwrap().state, JobState::Completed);
    let accepted = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    assert_eq!(entry_identity(&seed).0, canonical);
    assert_eq!(project_snapshot(), rightful_projects);
    assert_eq!(
        mapping_snapshot(&open_catalog().unwrap()),
        rightful_mappings
    );
    job_control::run_pending().unwrap();
    reset_runtime();
    assert_eq!(
        job_get(&accepted.job_id).unwrap().state,
        JobState::Completed
    );
    assert_eq!(project_snapshot(), rightful_projects);
    assert_archive_unchanged(&seed);
}

#[test]
fn winning_legacy_match_keeps_user_project_metadata_through_adoption_retry() {
    let home = IsolatedAppHome::acquire("legacy-project-mapping-overlap");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let old_path = home.path.join("old synthetic target");
    let kept_path = home.path.join("kept synthetic target");
    std::fs::create_dir(&old_path).unwrap();
    std::fs::create_dir(&kept_path).unwrap();
    let old = crate::store::add_project(old_path.to_string_lossy().into_owned(), false).unwrap();
    let kept = crate::store::add_project(kept_path.to_string_lossy().into_owned(), false).unwrap();
    crate::store::rename_project(&kept.id, "synthetic user name").unwrap();
    crate::store::set_project_pinned(&kept.id, true).unwrap();
    let conn = open_catalog().unwrap();
    project_link::apply_late_mapping(
        &conn,
        &seed.import_id,
        &mapping_to(kept.id.clone(), kept_path.to_string_lossy().into_owned()),
    )
    .unwrap();
    let (delayed, item) = queue_registration_custom(
        &previews[0],
        seed.installation.clone(),
        false,
        json!({"rawRetention":"normalized-only"}),
        |item| item.mapping_project_id = Some(old.id.clone()),
    );
    let canonical = resident(&previews[0]).source_id;
    let pending = plan(previews, vec![]).unwrap();
    let projects_before = project_snapshot();
    let (_, request) = publish_with_busy_adoption(&conn, &delayed, &item, &pending);
    assert_eq!(entry_identity(&seed).0, seed.installation);
    assert_archive_unchanged(&seed);
    assert_eq!(table_counts().0, 1);
    assert_eq!(project_snapshot(), projects_before);
    let rightful_mappings = mapping_snapshot(&conn);
    assert_eq!(
        project_link::mapping_for_import(&conn, &seed.import_id)
            .unwrap()
            .unwrap()
            .target_project_id,
        Some(old.id)
    );
    job_control::run_pending().unwrap();
    reset_runtime();
    assert_eq!(job_get(&delayed).unwrap().state, JobState::Completed);
    commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    assert_eq!(entry_identity(&seed).0, canonical);
    assert_eq!(project_snapshot(), projects_before);
    assert_eq!(
        mapping_snapshot(&open_catalog().unwrap()),
        rightful_mappings
    );
}

#[test]
fn adoption_winning_first_refuses_old_project_effects_before_publication() {
    for create in [true, false] {
        let home = IsolatedAppHome::acquire("legacy-adoption-before-project");
        let previews = cursor_source(&home, 1);
        let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
        let old_path = home.path.join("old synthetic target");
        let kept_path = home.path.join("kept synthetic target");
        std::fs::create_dir(&old_path).unwrap();
        std::fs::create_dir(&kept_path).unwrap();
        let old = (!create).then(|| {
            crate::store::add_project(old_path.to_string_lossy().into_owned(), false).unwrap()
        });
        let kept =
            crate::store::add_project(kept_path.to_string_lossy().into_owned(), false).unwrap();
        crate::store::rename_project(&kept.id, "synthetic saved user name").unwrap();
        crate::store::set_project_pinned(&kept.id, true).unwrap();
        let conn = open_catalog().unwrap();
        project_link::apply_late_mapping(
            &conn,
            &seed.import_id,
            &mapping_to(kept.id, kept_path.to_string_lossy().into_owned()),
        )
        .unwrap();
        let (delayed, item) = queue_registration_custom(
            &previews[0],
            seed.installation.clone(),
            false,
            json!({"rawRetention":"normalized-only"}),
            |item| {
                item.create_project = create;
                item.explicit_target = create.then(|| old_path.to_string_lossy().into_owned());
                item.mapping_project_id = old.as_ref().map(|project| project.id.clone());
            },
        );
        let canonical = resident(&previews[0]).source_id;
        let pending = plan(previews, vec![]).unwrap();
        commit(
            &pending.plan_id,
            &pending.plan_hash,
            &Uuid::new_v4().to_string(),
        )
        .unwrap();
        let projects_before = project_snapshot();
        let mappings_before = mapping_snapshot(&conn);
        let counts = table_counts();
        let error = jobs::publish_item(&conn, &delayed, &item, &|| false).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale);
        assert_eq!(entry_identity(&seed).0, canonical);
        assert!(jobs::latest_receipt(&conn, &delayed, &item.candidate_id)
            .unwrap()
            .is_none());
        assert_eq!(table_counts(), counts);
        assert_eq!(
            project_snapshot(),
            projects_before,
            "no obsolete project creation"
        );
        assert_eq!(
            mapping_snapshot(&conn),
            mappings_before,
            "no obsolete mapping overwrite"
        );
        assert_archive_unchanged(&seed);
        reset_runtime();
        assert_eq!(project_snapshot(), projects_before);
        assert_eq!(mapping_snapshot(&open_catalog().unwrap()), mappings_before);
    }
}

#[test]
fn adoption_refuses_a_real_os_publication_owner_atomically_and_retries() {
    let home = IsolatedAppHome::acquire("legacy-adoption-os-owner");
    let previews = cursor_source(&home, 1);
    let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
    let fixed = resident(&previews[0]);
    let pending = plan(previews, vec![]).unwrap();
    let lease_before = std::fs::read(fixed.prepared.path.join("lease.json")).unwrap();
    let counts = table_counts();
    let request = Uuid::new_v4().to_string();
    let owner = job_control::ImportLock::acquire("publish.lock").unwrap();
    let error = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap_err();
    assert_eq!(error.code, ErrorCode::SourceBusy);
    assert_eq!(entry_identity(&seed).0, seed.installation);
    assert_eq!(table_counts(), counts);
    assert_no_provenance_extension();
    assert_eq!(
        std::fs::read(fixed.prepared.path.join("lease.json")).unwrap(),
        lease_before
    );
    assert_archive_unchanged(&seed);
    drop(owner);
    let accepted = commit(&pending.plan_id, &pending.plan_hash, &request).unwrap();
    assert_eq!(entry_identity(&seed).0, fixed.source_id);
    assert_eq!(
        commit(&pending.plan_id, &pending.plan_hash, &request)
            .unwrap()
            .job_id,
        accepted.job_id
    );
    assert_archive_unchanged(&seed);
}

#[test]
fn non_adopted_legacy_project_creation_and_match_still_publish() {
    for create in [true, false] {
        let home = IsolatedAppHome::acquire("legacy-project-publication-control");
        let previews = cursor_source(&home, 1);
        let seed = seed_registration(&previews[0], legacy_registration(&previews[0]));
        let target = home.path.join("supported synthetic project");
        std::fs::create_dir(&target).unwrap();
        let project = (!create).then(|| {
            crate::store::add_project(target.to_string_lossy().into_owned(), false).unwrap()
        });
        let (delayed, _) = queue_registration_custom(
            &previews[0],
            seed.installation.clone(),
            false,
            json!({"rawRetention":"normalized-only"}),
            |item| {
                item.create_project = create;
                item.explicit_target = create.then(|| target.to_string_lossy().into_owned());
                item.mapping_project_id = project.as_ref().map(|project| project.id.clone());
            },
        );
        job_control::run_pending().unwrap();
        reset_runtime();
        let completed = job_get(&delayed).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        let projects = crate::store::load_projects_snapshot().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(
            completed.items[0].mapping_project_id.as_ref(),
            Some(&projects[0].id)
        );
        assert_eq!(
            completed.items[0].import_id.as_deref(),
            Some(seed.import_id.as_str())
        );
        assert_eq!(entry_identity(&seed).0, seed.installation);
        assert_archive_unchanged(&seed);
    }
}
