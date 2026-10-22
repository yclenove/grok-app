#[test]
fn r03_ten_items_cancel_during_fifth_publish_keeps_four_committed() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let items: Vec<_> = (0..10)
        .map(|i| {
            if i == 4 {
                input_named(&format!("c{i}"), &"x".repeat(2 * 1024 * 1024))
            } else {
                input_named(&format!("c{i}"), &format!("item-{i}"))
            }
        })
        .collect();
    let queued = enqueue(job(), items).unwrap();
    for i in 0..4 {
        let durable =
            super::job_control::load_input(&conn, &queued.job_id, &format!("c{i}")).unwrap();
        let published = publish_item(&conn, &queued.job_id, &durable, &|| false).unwrap();
        assert_eq!(published.phase, "committed", "c{i}");
    }
    assert_eq!(list_entries(&conn).unwrap().len(), 4);

    let fifth = super::job_control::load_input(&conn, &queued.job_id, "c4").unwrap();
    let count = std::cell::Cell::new(0);
    let interrupted = publish_item(&conn, &queued.job_id, &fifth, &|| {
        count.set(count.get() + 1);
        count.get() > 10
    })
    .unwrap();
    assert_eq!(interrupted.phase, "cancelled");
    assert_eq!(list_entries(&conn).unwrap().len(), 4);

    cancel(&queued.job_id).unwrap();
    run_pending().unwrap();
    let stopped = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert_eq!(
        stopped
            .items
            .iter()
            .filter(|i| i.phase == "committed")
            .count(),
        4
    );
    assert!(
        stopped.items[4..].iter().all(|i| i.phase == "cancelled"),
        "item 5 and later must not start: {:?}",
        stopped.items
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 4);

    recover_incomplete(&conn).unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 4);
    let first_rev = stopped.items[0].revision_id.clone();

    retry(&queued.job_id).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.items[0].revision_id, first_rev);
    assert_eq!(list_entries(&conn).unwrap().len(), 10);
}

#[test]
fn a_tombstoned_item_is_a_finished_item_in_job_progress() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input_named("c-gone", "gone")]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    delete_imported(&conn, done.items[0].import_id.as_deref().unwrap()).unwrap();
    // A later ordinary import of the undone session is refused, not pending.
    let again = enqueue(job(), vec![input_named("c-gone", "gone")]).unwrap();
    run_pending().unwrap();
    let page = crate::session_migration::engine::job_get(&again.job_id).unwrap();
    assert_eq!(page.items[0].phase, "tombstoned");
    let progress = page.progress.unwrap();
    assert_eq!((progress.done, progress.retryable), (1, 0));
}

#[test]
fn r05_delete_tombstone_blocks_until_explicit_reimport() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let proj_dir = _home.path.join("user-project");
    std::fs::create_dir_all(&proj_dir).unwrap();
    std::fs::write(proj_dir.join("keep-me.txt"), "owned").unwrap();
    let project = crate::store::add_project(proj_dir.to_string_lossy().into(), true).unwrap();
    crate::store::set_project_pinned(&project.id, true).unwrap();

    let mut item = input_named("c-del", "keep");
    item.mapping_project_id = Some(project.id.clone());
    let queued = enqueue(job(), vec![item]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    let import_id = done.items[0].import_id.clone().unwrap();
    let rev = done.items[0].revision_id.clone().unwrap();
    let dest = revision_dir(&import_id, &rev).unwrap();
    assert!(dest.exists());

    delete_imported(&conn, &import_id).unwrap();
    assert!(is_tombstoned(&conn, "installation", "generic-file", "c-del").unwrap());
    assert!(get_entry(&conn, &import_id).unwrap().unwrap().hidden);
    assert!(dest.exists(), "delete must keep revision snapshots");
    assert!(
        proj_dir.join("keep-me.txt").is_file(),
        "delete must not remove project directories"
    );
    assert!(crate::store::load_projects()
        .iter()
        .any(|p| p.id == project.id && p.pinned && p.trusted));

    recover_incomplete(&conn).unwrap();
    assert!(get_entry(&conn, &import_id).unwrap().unwrap().hidden);

    let blocked =
        publish_item(&conn, "job-scan", &input_named("c-del", "keep"), &|| false).unwrap();
    assert_eq!(blocked.outcome.as_deref(), Some("tombstoned"));
    assert!(get_entry(&conn, &import_id).unwrap().unwrap().hidden);

    let mut reimport = input_named("c-del", "keep");
    reimport.explicit_reimport = true;
    let queued_re = enqueue(job(), vec![reimport]).unwrap();
    cancel(&queued_re.job_id).unwrap();
    run_pending().unwrap();
    assert!(
        is_tombstoned(&conn, "installation", "generic-file", "c-del").unwrap(),
        "cancel before publish must not clear the tombstone"
    );

    retry(&queued_re.job_id).unwrap();
    run_pending().unwrap();
    let restored = get_job(&conn, &queued_re.job_id).unwrap().unwrap();
    assert_eq!(restored.items[0].phase, "committed");
    assert_eq!(
        restored.items[0].import_id.as_deref(),
        Some(import_id.as_str())
    );
    assert!(!get_entry(&conn, &import_id).unwrap().unwrap().hidden);
    assert!(!is_tombstoned(&conn, "installation", "generic-file", "c-del").unwrap());
    assert!(dest.exists());
    assert!(proj_dir.join("keep-me.txt").is_file());
}

#[test]
fn r06_undo_mixed_refresh_renamed_project_and_continuation() {
    use super::continuation::{save_continuation_ref, ContinuationSidecar, DispatchState};
    let _home = Home::new();
    let conn = open_catalog().unwrap();

    let mut v1 = input("v1");
    v1.candidate_id = "base".into();
    v1.adapter.graph.native_session_id = "base".into();
    v1 = super::jobs::prepare_test_commit_item(v1);
    let first = enqueue(job(), vec![v1]).unwrap();
    run_pending().unwrap();
    let first = get_job(&conn, &first.job_id).unwrap().unwrap();
    let base_id = first.items[0].import_id.clone().unwrap();
    let v1_rev = first.items[0].revision_id.clone().unwrap();

    let orig = _home.path.join("orig-project");
    std::fs::create_dir_all(&orig).unwrap();
    std::fs::write(orig.join("keep-me.txt"), "owned").unwrap();
    let project = crate::store::add_project(orig.to_string_lossy().into(), true).unwrap();
    crate::store::set_project_pinned(&project.id, true).unwrap();
    let renamed = _home.path.join("renamed-by-user");
    std::fs::create_dir_all(&renamed).unwrap();
    std::fs::write(renamed.join("keep-me.txt"), "owned").unwrap();
    crate::store::relocate_project(&project.id, renamed.to_string_lossy().into()).unwrap();

    let mut refresh = input("v2");
    refresh.candidate_id = "base-v2".into();
    refresh.adapter.graph.native_session_id = "base".into();
    refresh = super::jobs::prepare_test_commit_item(refresh);
    let mut mapped = input_named("created", "new-session");
    mapped.mapping_project_id = Some(project.id.clone());
    let continued = input_named("continued", "live");
    let batch = enqueue(job(), vec![refresh, mapped, continued]).unwrap();
    run_pending().unwrap();
    let batch_done = get_job(&conn, &batch.job_id).unwrap().unwrap();
    assert!(
        batch_done.items.iter().all(|i| i.phase == "committed"),
        "{:?}",
        batch_done.items
    );

    let continued_id = batch_done.items[2].import_id.clone().unwrap();
    let continued_rev = batch_done.items[2].revision_id.clone().unwrap();
    save_continuation_ref(
        &conn,
        "sess-live",
        &ContinuationSidecar {
            import_id: continued_id.clone(),
            revision_id: continued_rev.clone(),
            branch_leaf: "e1".into(),
            allowed_selection_hash: "h".into(),
            allowed_event_ids: vec!["e1".into()],
            allowed_selection: None,
            handoff_hash: "hh".into(),
            request_id: "req-live".into(),
            dispatch_id: None,
            prompt_hash: None,
            agent_session_id: None,
            state: DispatchState::Prepared,
            grant_token: "g".into(),
            ..Default::default()
        },
    )
    .unwrap();

    let undone = undo_job(&conn, &batch.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert_eq!(undone.items[1].phase, "undone");
    assert_eq!(undone.items[2].phase, "conflict");
    assert!(
        undone.items[2]
            .error
            .as_ref()
            .is_some_and(|e| e.contains("MAPPING_CONFLICT")),
        "{:?}",
        undone.items[2].error
    );

    let base = get_entry(&conn, &base_id).unwrap().unwrap();
    assert!(!base.hidden);
    assert_eq!(base.current_revision.as_deref(), Some(v1_rev.as_str()));
    let v2_rev = batch_done.items[0].revision_id.as_ref().unwrap();
    assert!(revision_dir(&base_id, &v1_rev).unwrap().exists());
    assert!(revision_dir(&base_id, v2_rev).unwrap().exists());

    let created_id = batch_done.items[1].import_id.clone().unwrap();
    assert!(get_entry(&conn, &created_id).unwrap().unwrap().hidden);
    assert!(revision_dir(
        &created_id,
        batch_done.items[1].revision_id.as_ref().unwrap()
    )
    .unwrap()
    .exists());

    let live = get_entry(&conn, &continued_id).unwrap().unwrap();
    assert!(!live.hidden);
    let err = delete_imported(&conn, &continued_id).unwrap_err();
    assert_eq!(err.code, ErrorCode::MappingConflict);

    assert!(renamed.join("keep-me.txt").is_file());
    let kept = crate::store::load_projects()
        .into_iter()
        .find(|p| p.id == project.id)
        .unwrap();
    assert!(kept.pinned && kept.trusted);
    assert_eq!(kept.path, renamed.to_string_lossy());

    retry(&batch.job_id).unwrap();
    run_pending().unwrap();
    let after = get_job(&conn, &batch.job_id).unwrap().unwrap();
    assert_eq!(after.items[0].phase, "undone");
    assert_eq!(after.items[1].phase, "undone");
    assert_eq!(after.items[2].phase, "conflict");
}

#[test]
fn created_project_survives_crash_and_does_not_duplicate() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let dir = _home.path.join("created-proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.txt"), "x").unwrap();
    let mut item = input("create-crash");
    item.create_project = true;
    item.explicit_target = Some(dir.to_string_lossy().into());
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_linked BEFORE INSERT ON receipts WHEN NEW.phase='linked' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    assert!(publish_item(&conn, &queued.job_id, &item, &|| false).is_err());
    let after_crash = crate::store::load_projects();
    assert_eq!(after_crash.len(), 1, "{after_crash:?}");
    let created_id = after_crash[0].id.clone();
    assert!(!after_crash[0].pinned && !after_crash[0].trusted);
    conn.execute_batch("DROP TRIGGER crash_linked").unwrap();
    let done = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap();
    assert_eq!(done.phase, "committed");
    assert_eq!(
        done.mapping_project_id.as_deref(),
        Some(created_id.as_str())
    );
    assert_eq!(crate::store::load_projects().len(), 1);
    assert!(dir.join("keep.txt").is_file());
}

#[test]
fn recover_keeps_user_rename_of_created_project() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let dir = _home.path.join("rename-proj");
    std::fs::create_dir_all(&dir).unwrap();
    let mut item = input("create-rename");
    item.create_project = true;
    item.explicit_target = Some(dir.to_string_lossy().into());
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER crash_linked BEFORE INSERT ON receipts WHEN NEW.phase='linked' BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
    )
    .unwrap();
    assert!(publish_item(&conn, &queued.job_id, &item, &|| false).is_err());
    conn.execute_batch("DROP TRIGGER crash_linked").unwrap();
    let id = crate::store::load_projects()[0].id.clone();
    crate::store::rename_project(&id, "user-name").unwrap();
    crate::store::set_project_pinned(&id, true).unwrap();
    let done = publish_item(&conn, &queued.job_id, &item, &|| false).unwrap();
    assert_eq!(done.phase, "committed");
    let kept = crate::store::load_projects()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(kept.name, "user-name");
    assert!(kept.pinned);
    assert!(!kept.trusted);
    assert_eq!(crate::store::load_projects().len(), 1);
}

#[test]
fn unassigned_import_can_be_mapped_later_without_new_entry() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let queued = enqueue(job(), vec![input("later-map")]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    let import_id = done.items[0].import_id.clone().unwrap();
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    let dir = _home.path.join("late-proj");
    std::fs::create_dir_all(&dir).unwrap();
    let project = crate::store::add_project(dir.to_string_lossy().into(), true).unwrap();
    let mapped = crate::session_migration::project_link::apply_late_mapping(
        &conn,
        &import_id,
        &crate::session_migration::workspace::WorkspaceMapping {
            source: None,
            root_id: "tmp".into(),
            target_project_id: Some(project.id.clone()),
            explicit_target: Some(dir.to_string_lossy().into()),
            unassigned: false,
            user_choice_version: 2,
            primary: true,
            suggested_target: None,
            create_project: false,
            import_unassigned_anyway: false,
        },
    )
    .unwrap();
    assert_eq!(
        mapped.target_project_id.as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(list_entries(&conn).unwrap().len(), 1);
    let stored = crate::session_migration::project_link::mapping_for_import(&conn, &import_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.target_project_id.as_deref(),
        Some(project.id.as_str())
    );
    let other = crate::store::add_project(
        {
            let p = _home.path.join("other-proj");
            std::fs::create_dir_all(&p).unwrap();
            p.to_string_lossy().into()
        },
        false,
    )
    .unwrap();
    let err = crate::session_migration::project_link::apply_late_mapping(
        &conn,
        &import_id,
        &crate::session_migration::workspace::WorkspaceMapping {
            source: None,
            root_id: "tmp".into(),
            target_project_id: Some(other.id),
            explicit_target: None,
            unassigned: false,
            user_choice_version: 3,
            primary: true,
            suggested_target: None,
            create_project: false,
            import_unassigned_anyway: false,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::MappingConflict);
}

#[test]
fn undo_keeps_project_reused_by_a_saved_source_workspace_and_reports_the_conflict() {
    let home = Home::new();
    let conn = open_catalog().unwrap();
    let target = home.path.join("workspace-project");
    std::fs::create_dir_all(&target).unwrap();
    let mut item = input("workspace-undo-reference");
    item.create_project = true;
    item.explicit_target = Some(target.to_string_lossy().into_owned());
    let queued = enqueue(job(), vec![item]).unwrap();
    run_pending().unwrap();
    let done = get_job(&conn, &queued.job_id).unwrap().unwrap();
    let project_id = done.items[0].mapping_project_id.clone().unwrap();
    let mut workspace = crate::session_migration::workspace::source_workspace_from_folder(target.to_str().unwrap());
    workspace.mapping_choices = vec![crate::session_migration::workspace::WorkspaceRootChoice {
        root_id: workspace.roots[0].root_id.clone(), target_project_id: Some(project_id.clone()),
        explicit_target: None, primary: true,
    }];
    crate::session_migration::catalog::upsert_workspace(&conn, &workspace).unwrap();
    let undone = undo_job(&conn, &queued.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    // A legacy mutable result cannot prove creation, independently of refs.
    assert!(undone.items[0].error.as_deref().unwrap().contains("creation provenance unavailable"));
    assert!(crate::store::load_projects_snapshot().unwrap().iter().any(|project| project.id == project_id));
    assert!(target.is_dir());
}

#[test]
fn legacy_undo_keeps_unproven_project_record_and_directory_but_undoes_content() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let dir = _home.path.join("undo-proj");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.txt"), "owned").unwrap();
    let mut item = input("undo-create");
    item.create_project = true;
    item.explicit_target = Some(dir.to_string_lossy().into());
    let queued = enqueue(job(), vec![item]).unwrap();
    run_pending().unwrap();
    assert_eq!(
        get_job(&conn, &queued.job_id).unwrap().unwrap().items[0].phase,
        "committed"
    );
    assert_eq!(crate::store::load_projects().len(), 1);
    let undone = undo_job(&conn, &queued.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert_eq!(crate::store::load_projects().len(), 1);
    assert!(undone.items[0].error.as_deref().unwrap().contains("creation provenance unavailable"));
    let import = undone.items[0].import_id.as_deref().unwrap();
    assert!(get_entry(&conn, import).unwrap().unwrap().hidden);
    assert!(dir.join("keep.txt").is_file());
}

#[test]
fn undo_and_explicit_reimport_keep_workspace_link_revoked() {
    use super::{sources, types::Provider, workspace, workspace_association as association, workspace_identity};
    let home = Home::new();
    let conn = open_catalog().unwrap();
    let source = sources::register(&conn, Provider::GenericFile, &home.path, None, "synthetic").unwrap();
    let mut item = input("history with workspace");
    item.installation_id = source.source_id.clone();
    item.root_path = source.root_path;
    let mut item = super::jobs::prepare_test_commit_item(item);
    let queued = enqueue(job(), vec![item.clone()]).unwrap();
    run_pending().unwrap();
    assert_eq!(get_job(&conn, &queued.job_id).unwrap().unwrap().items[0].phase, "committed");
    let entry = list_entries(&conn).unwrap().remove(0);
    let mut ws = workspace::source_workspace_from_folder("/synthetic/offline");
    workspace_identity::scope(&conn, &mut ws, &source.source_id).unwrap();
    super::catalog::upsert_workspace(&conn, &ws).unwrap();
    let choice = association::eligible(&entry.import_id, None, None).unwrap().items.remove(0);
    let input = association::UpdateRequest { import_id: entry.import_id.clone(), expected_version: 0,
        request_id: uuid::Uuid::new_v4().to_string(), choice: Some(choice) };
    association::update(input.clone()).unwrap();
    assert_eq!(undo_job(&conn, &queued.job_id).unwrap().items[0].phase, "undone");
    assert_eq!(association::get(&entry.import_id).unwrap_err().code, ErrorCode::HistoryAccessUnavailable);
    item.explicit_reimport = true;
    let reimport = enqueue(job(), vec![super::jobs::prepare_test_commit_item(item)]).unwrap();
    run_pending().unwrap();
    assert_eq!(get_job(&conn, &reimport.job_id).unwrap().unwrap().items[0].phase, "committed");
    let result = association::get(&entry.import_id).unwrap();
    assert_eq!(result.status, association::AssociationStatus::Unassigned);
    assert_eq!(result.version, 2);
    assert_eq!(association::update(input).unwrap_err().code, ErrorCode::PlanStale);
}

#[test]
fn undo_keeps_user_renamed_created_project() {
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let dir = _home.path.join("keep-renamed");
    std::fs::create_dir_all(&dir).unwrap();
    let mut item = input("undo-renamed");
    item.create_project = true;
    item.explicit_target = Some(dir.to_string_lossy().into());
    let queued = enqueue(job(), vec![item]).unwrap();
    run_pending().unwrap();
    let id = crate::store::load_projects()[0].id.clone();
    crate::store::rename_project(&id, "kept-by-user").unwrap();
    let undone = undo_job(&conn, &queued.job_id).unwrap();
    assert_eq!(undone.items[0].phase, "undone");
    assert!(undone.items[0]
        .error
        .as_ref()
        .is_some_and(|e| e.contains("MAPPING_CONFLICT")));
    let kept = crate::store::load_projects()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(kept.name, "kept-by-user");
    assert!(dir.is_dir());
}

#[test]
fn undo_mixed_batch_keeps_rename_continuation_and_reports_kept() {
    use super::continuation::{
        list_refs_for_import, save_continuation_ref, ContinuationSidecar, DispatchState,
    };
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let first = enqueue(job(), vec![input_named("owned", "owned-v1")]).unwrap();
    run_pending().unwrap();
    let owned_id = get_job(&conn, &first.job_id).unwrap().unwrap().items[0]
        .import_id
        .clone()
        .unwrap();
    let owned_v1 = get_job(&conn, &first.job_id).unwrap().unwrap().items[0]
        .revision_id
        .clone()
        .unwrap();
    let unrelated = enqueue(job(), vec![input_named("other", "other-keep")]).unwrap();
    run_pending().unwrap();
    let other_id = get_job(&conn, &unrelated.job_id).unwrap().unwrap().items[0]
        .import_id
        .clone()
        .unwrap();
    let dir = _home.path.join("keep-renamed-mixed");
    std::fs::create_dir_all(&dir).unwrap();
    let mut refresh = input_named("owned", "owned-v2");
    refresh.candidate_id = "owned-refresh".into();
    refresh = super::jobs::prepare_test_commit_item(refresh);
    let mut child = input_named("child", "child-body");
    child.create_project = true;
    child.explicit_target = Some(dir.to_string_lossy().into());
    let mixed = enqueue(job(), vec![refresh, child]).unwrap();
    run_pending().unwrap();
    let mixed_done = get_job(&conn, &mixed.job_id).unwrap().unwrap();
    let child_id = mixed_done
        .items
        .iter()
        .find(|i| i.candidate_id == "child")
        .unwrap()
        .import_id
        .clone()
        .unwrap();
    let child_rev = mixed_done
        .items
        .iter()
        .find(|i| i.candidate_id == "child")
        .unwrap()
        .revision_id
        .clone()
        .unwrap();
    save_continuation_ref(
        &conn,
        "sess-child",
        &ContinuationSidecar {
            import_id: child_id.clone(),
            revision_id: child_rev,
            branch_leaf: "e1".into(),
            allowed_selection_hash: "h".into(),
            allowed_event_ids: vec!["e1".into()],
            allowed_selection: None,
            handoff_hash: "hh".into(),
            request_id: "req-child".into(),
            dispatch_id: None,
            prompt_hash: None,
            agent_session_id: None,
            state: DispatchState::Prepared,
            grant_token: "g".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let project_id = crate::store::load_projects()[0].id.clone();
    crate::store::rename_project(&project_id, "kept-by-user").unwrap();
    let undone = undo_job(&conn, &mixed.job_id).unwrap();
    let refresh_item = undone
        .items
        .iter()
        .find(|i| i.candidate_id == "owned-refresh")
        .unwrap();
    assert_eq!(refresh_item.phase, "undone");
    let child_item = undone
        .items
        .iter()
        .find(|i| i.candidate_id == "child")
        .unwrap();
    assert_eq!(child_item.phase, "conflict");
    assert!(
        child_item
            .error
            .as_ref()
            .is_some_and(|e| e.contains("MAPPING_CONFLICT") || e.contains("continuation")),
        "undo must report the kept continuation: {:?}",
        child_item.error
    );
    let owned = get_entry(&conn, &owned_id).unwrap().unwrap();
    assert_eq!(owned.current_revision.as_deref(), Some(owned_v1.as_str()));
    assert!(!owned.hidden);
    let child_entry = get_entry(&conn, &child_id).unwrap().unwrap();
    assert!(!child_entry.hidden);
    assert!(!list_refs_for_import(&conn, &child_id).unwrap().is_empty());
    let other = get_entry(&conn, &other_id).unwrap().unwrap();
    assert!(!other.hidden);
    let kept = crate::store::load_projects()
        .into_iter()
        .find(|p| p.id == project_id)
        .unwrap();
    assert_eq!(kept.name, "kept-by-user");
}

#[test]
fn unwritable_staging_does_not_publish_and_leaves_receipt() {
    use super::catalog::staging_dir;
    use super::hash::sha256_bytes;
    let _home = Home::new();
    let conn = open_catalog().unwrap();
    let job_id = "job-r07-staging";
    let item = input_named("r07", "must-not-publish");
    let blocker = staging_dir().join(sha256_bytes(job_id.as_bytes()));
    std::fs::create_dir_all(staging_dir()).unwrap();
    std::fs::write(&blocker, b"not-a-directory").unwrap();
    let err = publish_item(&conn, job_id, &item, &|| false).unwrap_err();
    assert_eq!(err.code, ErrorCode::SnapshotUnavailable);
    assert!(
        list_entries(&conn).unwrap().is_empty(),
        "failed staging must not publish a catalog entry"
    );
    let rec = latest_receipt(&conn, job_id, &item.candidate_id)
        .unwrap()
        .expect("recoverable receipt must remain");
    assert_eq!(rec.phase, "prepared");
    assert!(rec.import_id.is_some());
}

#[test]
fn import_commit_does_not_increment_native_unread_or_usage() {
    let home = Home::new();
    let old_grok = std::env::var_os("GROK_HOME");
    std::env::set_var("GROK_HOME", home.path.join("grok-home"));
    let _restore = scopeguard_restore_grok(old_grok);

    let native_sess = crate::paths::agent_home_dir()
        .join("sessions")
        .join("proj")
        .join("native-1");
    std::fs::create_dir_all(&native_sess).unwrap();
    std::fs::write(
        native_sess.join("signals.json"),
        serde_json::json!({
            "turnCount": 1,
            "contextTokensUsed": 77,
            "primaryModelId": "test-model"
        })
        .to_string(),
    )
    .unwrap();

    let imports = imports_dir();
    let roots = crate::account::session_roots();
    assert!(
        roots.iter().all(|root| !root.starts_with(&imports)),
        "usage walker must not include the import catalog: {roots:?}"
    );
    let app_root = crate::paths::app_data_root();
    let grok_home = std::env::var("GROK_HOME").unwrap();
    let isolated: Vec<_> = roots
        .into_iter()
        .filter(|root| root.starts_with(&app_root) || root.starts_with(&grok_home))
        .collect();
    assert!(
        !isolated.is_empty(),
        "test must isolate at least one usage root under GROK_APP_HOME"
    );

    let before_index = crate::store::load_sessions_index();
    let (before_hm, _) = crate::account::local_usage_from_roots(7, 40, &isolated);
    let before_tokens: u64 = before_hm.iter().map(|d| d.tokens).sum();
    let before_requests: u64 = before_hm.iter().map(|d| d.requests).sum();
    assert_eq!(before_tokens, 77);
    assert_eq!(before_requests, 1);

    let conn = open_catalog().unwrap();
    let job_id = "job-u07-usage";
    let mut last = None;
    for i in 0..24 {
        let mut item = input_named(&format!("old-{i}"), &format!("old session {i}"));
        item.adapter.inventory.source_tokens = 1_000_000;
        let published = publish_item(&conn, job_id, &item, &|| false).unwrap();
        assert_eq!(published.phase, "committed");
        last = Some(published);
    }
    let last = last.expect("published at least one import");
    let import_id = last.import_id.as_deref().unwrap();
    let revision_id = last.revision_id.as_deref().unwrap();
    let stored = revision_inventory(&conn, import_id, revision_id).unwrap();
    assert_eq!(stored.source_tokens, 1_000_000);
    assert!(stored.imported.messages >= 1);

    let after_index = crate::store::load_sessions_index();
    assert_eq!(
        after_index.len(),
        before_index.len(),
        "import must not create native session rows (unread/tray keys)"
    );
    assert!(
        after_index.iter().all(|s| !s.id.starts_with("imp:")),
        "import ids must not enter sessions_index"
    );

    let (after_hm, after_logs) = crate::account::local_usage_from_roots(7, 40, &isolated);
    let after_tokens: u64 = after_hm.iter().map(|d| d.tokens).sum();
    let after_requests: u64 = after_hm.iter().map(|d| d.requests).sum();
    assert_eq!(after_tokens, before_tokens);
    assert_eq!(after_requests, before_requests);
    assert!(
        after_logs
            .iter()
            .all(|e| e.id != *import_id && e.usage_tokens != 1_000_000),
        "source token stats must stay off the native call log"
    );

    let dest = revision_dir(import_id, revision_id).unwrap();
    std::fs::write(
        dest.join("signals.json"),
        serde_json::json!({
            "turnCount": 9,
            "contextTokensUsed": 999_999,
            "primaryModelId": "imported-source"
        })
        .to_string(),
    )
    .unwrap();
    let (poisoned_hm, _) = crate::account::local_usage_from_roots(7, 40, &isolated);
    assert_eq!(
        poisoned_hm.iter().map(|d| d.tokens).sum::<u64>(),
        before_tokens,
        "poisoning the import archive must not change Grok usage"
    );
    let (walked_imports, _) =
        crate::account::local_usage_from_roots(7, 40, std::slice::from_ref(&imports));
    assert!(
        walked_imports.iter().map(|d| d.tokens).sum::<u64>() >= 999_999,
        "control: walking imports would count source tokens if they leaked into session_roots"
    );
}

fn scopeguard_restore_grok(old: Option<std::ffi::OsString>) -> impl Drop {
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("GROK_HOME", value);
            } else {
                std::env::remove_var("GROK_HOME");
            }
        }
    }
    Restore(old)
}
