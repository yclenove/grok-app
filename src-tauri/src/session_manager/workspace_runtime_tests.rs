use super::*;
use crate::session_fsm::SessionState;
use crate::session_manager::{routing_tests::bare_live_session, SessionManager};
use crate::session_migration::test_home::IsolatedAppHome;

struct Fixture {
    home: IsolatedAppHome,
    meta: store::SessionMeta,
    workspace: ws::WorkspaceRecord,
    context: RuntimeWorkspaceContext,
}

impl Fixture {
    fn new(bound: bool) -> Self {
        let home = IsolatedAppHome::acquire("runtime-workspace");
        let primary = home.path.join("primary");
        let extra = home.path.join("extra");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        let project = store::ensure_import_project("runtime-project", primary.to_str().unwrap())
            .unwrap()
            .project;
        let workspace = ws::upsert_workspace(
            None,
            "Runtime fixture".into(),
            project.id.clone(),
            vec![
                ws::WorkspaceRoot {
                    path: project.path.clone(),
                    role: ws::WorkspaceRootRole::Primary,
                    access: ws::WorkspaceRootAccess::Write,
                    path_ok: Some(true),
                },
                ws::WorkspaceRoot {
                    path: directory(extra.to_str().unwrap()).unwrap(),
                    role: ws::WorkspaceRootRole::Extra,
                    access: ws::WorkspaceRootAccess::Read,
                    path_ok: Some(true),
                },
            ],
        )
        .unwrap();
        let mut meta = store::create_session(Some(project.id), None, false).unwrap();
        if bound {
            meta = store::set_session_workspace(
                &meta.id,
                Some(workspace.id.clone()),
                Some(ws::root_snapshot(&workspace.roots)),
                Some("context_only".into()),
            )
            .unwrap();
        }
        let context =
            RuntimeWorkspaceContext::capture(&meta, &project.path, &store::load_settings(), false)
                .unwrap();
        Self {
            home,
            meta,
            workspace,
            context,
        }
    }

    fn manager(&self, background: bool) -> SessionManager {
        let manager = SessionManager::new();
        let mut live = bare_live_session(&self.meta.id, "synthetic-process");
        live.meta = self.meta.clone();
        live.project_path = Some(self.context.cwd.clone());
        live.workspace_context = Some(self.context.clone());
        if background {
            manager.background.lock().insert(self.meta.id.clone(), live);
            let mut other = bare_live_session("other-synthetic-chat", "other-process");
            other.fsm.begin_stream().unwrap();
            other.prompt_in_flight = true;
            *manager.inner.lock() = Some(other);
        } else {
            *manager.inner.lock() = Some(live);
        }
        manager
    }

    fn change_roots(&self) {
        let mut workspace = self.workspace.clone();
        workspace.roots.pop();
        ws::save_workspace_store(&ws::WorkspaceStoreFile {
            workspaces: vec![workspace],
            ..Default::default()
        })
        .unwrap();
    }

    fn assert_unsent(&self, manager: &SessionManager) {
        assert!(store::load_messages(&self.meta.id).is_empty());
        assert!(!crate::turn_lease::lease_path(&self.meta.id).exists());
        manager
            .with_session_mut(&self.meta.id, |live| {
                assert_eq!(live.fsm.state(), SessionState::Ready);
                assert!(!live.prompt_in_flight);
                assert!(!live.sent_prompt_this_visit);
                assert!(live.active_turn_id.is_none());
            })
            .unwrap();
    }
}

#[test]
fn production_turn_open_rejects_pending_composer_respawn_without_journal_or_lease() {
    for background in [false, true] {
        let fixture = Fixture::new(true);
        let manager = fixture.manager(background);
        manager
            .pending_soft_respawn
            .lock()
            .insert(fixture.meta.id.clone(), "composer_alignment".into());
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(manager.open_user_turn(
                &fixture.meta.id,
                "must not send",
                "must not send",
                None,
            ));
        assert!(matches!(result, Err(ref error) if error.starts_with("CONNECT_FAILED:")));
        fixture.assert_unsent(&manager);
        assert!(manager
            .pending_soft_respawn
            .lock()
            .contains_key(&fixture.meta.id));
        if background {
            assert!(manager.inner.lock().as_ref().unwrap().prompt_in_flight);
        }
    }
}

#[test]
fn injected_turn_origin_marks_only_its_own_persisted_row_live_and_background() {
    for background in [false, true] {
        let fixture = Fixture::new(true);
        let manager = fixture.manager(background);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for origin in ["external", "pr_monitor"] {
            let opened = runtime
                .block_on(manager.open_user_turn_with_origin(
                    &fixture.meta.id,
                    "same text",
                    "same text",
                    None,
                    Some(origin),
                ))
                .unwrap();
            let rows = store::load_messages(&fixture.meta.id);
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].marker.as_deref(),
                Some(crate::injected_prompts::MARKER)
            );
            assert_eq!(rows[0].id, opened.user_row.id);
            assert!(runtime
                .block_on(
                    manager.rollback_opened_turn_before_dispatch(&opened, "synthetic refusal")
                )
                .starts_with("TURN_NOT_DISPATCHED_ROLLED_BACK:"));
            fixture.assert_unsent(&manager);
        }
        let user = runtime
            .block_on(manager.open_user_turn(&fixture.meta.id, "same text", "same text", None))
            .unwrap();
        assert!(user.user_row.marker.is_none());
        assert!(store::load_messages(&fixture.meta.id)[0].marker.is_none());
        if background {
            assert!(manager.inner.lock().as_ref().unwrap().prompt_in_flight);
        }
    }
}

#[test]
fn cancelled_injected_turn_open_cannot_mark_intervening_user_message() {
    let fixture = Fixture::new(true);
    let manager = fixture.manager(false);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let lock = manager.post_turn_journal_lock(&fixture.meta.id);
            let held = lock.lock().await;
            let mut cancelled = Box::pin(manager.open_user_turn_with_origin(
                &fixture.meta.id,
                "same text",
                "same text",
                None,
                Some("pr_monitor"),
            ));
            tokio::select! {
                biased;
                _ = &mut cancelled => panic!("journal lock must suspend turn open"),
                _ = async {} => {}
            }
            drop(cancelled);
            fixture.assert_unsent(&manager);
            drop(held);
            let user = manager
                .open_user_turn(&fixture.meta.id, "same text", "same text", None)
                .await
                .unwrap();
            assert!(user.user_row.marker.is_none());
            assert_eq!(store::load_messages(&fixture.meta.id).len(), 1);
        });
}

#[test]
fn respawn_requested_during_turn_open_validation_is_rechecked_before_journal() {
    let fixture = Fixture::new(true);
    let manager = fixture.manager(false);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let journal_lock = manager.post_turn_journal_lock(&fixture.meta.id);
            let held = journal_lock.lock().await;
            let mut open =
                Box::pin(manager.open_user_turn(&fixture.meta.id, "unsent", "unsent", None));
            // Poll the actual turn opener until it suspends at the held journal
            // lock, then introduce the intent without a sleep or scheduling guess.
            tokio::select! {
                biased;
                _ = &mut open => panic!("journal lock must keep turn unopened"),
                _ = async {} => {}
            }
            manager
                .pending_soft_respawn
                .lock()
                .insert(fixture.meta.id.clone(), "effort".into());
            drop(held);
            assert!(matches!(open.await, Err(ref error) if error.starts_with("CONNECT_FAILED:")));
            fixture.assert_unsent(&manager);
        });
}

#[test]
fn foreign_pending_respawn_does_not_block_this_turn() {
    let fixture = Fixture::new(true);
    let manager = fixture.manager(false);
    manager
        .pending_soft_respawn
        .lock()
        .insert("foreign-synthetic-chat".into(), "effort".into());
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(manager.open_user_turn(&fixture.meta.id, "allowed", "allowed", None))
        .unwrap();
    assert_eq!(store::load_messages(&fixture.meta.id).len(), 1);
    assert!(manager
        .pending_soft_respawn
        .lock()
        .contains_key("foreign-synthetic-chat"));
}

#[test]
fn production_turn_open_accepts_unchanged_project_and_workspace_live_and_background() {
    for bound in [false, true] {
        for background in [false, true] {
            let fixture = Fixture::new(bound);
            let manager = fixture.manager(background);
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let opened = runtime
                .block_on(manager.open_user_turn(
                    &fixture.meta.id,
                    "synthetic prompt",
                    "synthetic prompt",
                    None,
                ))
                .unwrap();
            assert_eq!(opened.agent_prompt, "synthetic prompt");
            assert_eq!(store::load_messages(&fixture.meta.id).len(), 1);
            assert!(crate::turn_lease::lease_path(&fixture.meta.id).exists());
            if background {
                assert!(manager.inner.lock().as_ref().unwrap().prompt_in_flight);
            }
        }
    }
}

#[test]
fn continuation_capability_rejection_rolls_back_the_opened_turn() {
    use crate::session_migration::continuation::{
        apply_continuation_first_send, write_draft_text, write_sidecar, ContinuationSidecar,
        DispatchState,
    };

    let fixture = Fixture::new(false);
    let manager = fixture.manager(false);
    manager
        .with_session_mut(&fixture.meta.id, |session| {
            session.needs_history_bootstrap = true;
            session.sent_prompt_this_visit = true;
        })
        .unwrap();
    let before = session(&fixture.meta.id).unwrap();
    write_sidecar(
        &fixture.meta.id,
        &ContinuationSidecar {
            import_id: "synthetic-import".into(),
            revision_id: "synthetic-revision".into(),
            branch_leaf: "synthetic-leaf".into(),
            request_id: "synthetic-request".into(),
            handoff_hash: "synthetic-handoff".into(),
            runtime_capability: "local-full".into(),
            grant_token: "synthetic-grant".into(),
            ..Default::default()
        },
    )
    .unwrap();
    write_draft_text(&fixture.meta.id, "synthetic selected history").unwrap();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let opened = manager
            .open_user_turn(&fixture.meta.id, "continue", "continue", None)
            .await
            .unwrap();
        assert_eq!(store::load_messages(&fixture.meta.id).len(), 1);
        assert!(session(&fixture.meta.id).unwrap().updated_at > before.updated_at);
        let cause =
            apply_continuation_first_send(&fixture.meta.id, &opened.agent_prompt, opened.policy)
                .unwrap_err()
                .into_command_err();
        let error = manager
            .rollback_opened_turn_before_dispatch(&opened, &cause)
            .await;
        assert!(error.starts_with("TURN_NOT_DISPATCHED_ROLLED_BACK: HISTORY_ACCESS_UNAVAILABLE:"));
    });

    assert!(store::load_messages(&fixture.meta.id).is_empty());
    assert_eq!(
        session(&fixture.meta.id).unwrap().updated_at,
        before.updated_at
    );
    assert!(!crate::turn_lease::lease_path(&fixture.meta.id).exists());
    manager
        .with_session_mut(&fixture.meta.id, |session| {
            assert_eq!(session.fsm.state(), SessionState::Ready);
            assert!(!session.prompt_in_flight);
            assert!(session.sent_prompt_this_visit);
            assert!(session.needs_history_bootstrap);
            assert!(session.active_turn_id.is_none());
            assert!(session.streaming_message_id.is_none());
        })
        .unwrap();
    assert_eq!(
        crate::session_migration::continuation::read_sidecar_strict(&fixture.meta.id)
            .unwrap()
            .unwrap()
            .state,
        DispatchState::Prepared
    );
}

#[test]
fn opened_turn_rollback_fails_closed_after_a_concurrent_journal_append() {
    let fixture = Fixture::new(false);
    let manager = fixture.manager(false);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let opened = manager
            .open_user_turn(&fixture.meta.id, "continue", "continue", None)
            .await
            .unwrap();
        let mut later = opened.user_row.clone();
        later.id = "concurrent-tail".into();
        later.content = "must survive".into();
        later.created_at = chrono::Utc::now();
        store::append_message(&fixture.meta.id, later.clone()).unwrap();

        let error = manager
            .rollback_opened_turn_before_dispatch(&opened, "synthetic refusal")
            .await;

        assert_eq!(
            error,
            "DISPATCH_UNKNOWN: local pre-dispatch rollback could not be verified"
        );
        assert_eq!(
            store::load_messages(&fixture.meta.id),
            vec![opened.user_row.clone(), later]
        );
    });
}

#[test]
fn production_turn_open_rejects_directory_changes_without_journal_or_lease() {
    for background in [false, true] {
        for change in [
            "roots",
            "missing",
            "project",
            "detach",
            "workspace-deleted",
            "session-deleted",
            "corrupt-index",
        ] {
            let fixture = Fixture::new(true);
            let manager = fixture.manager(background);
            match change {
                "roots" => fixture.change_roots(),
                "missing" => std::fs::remove_dir(fixture.home.path.join("extra")).unwrap(),
                "project" => {
                    let path = fixture.home.path.join("moved");
                    std::fs::create_dir(&path).unwrap();
                    store::relocate_project(
                        fixture.meta.project_id.as_deref().unwrap(),
                        path.to_string_lossy().into_owned(),
                    )
                    .unwrap();
                }
                "detach" => {
                    store::set_session_workspace(&fixture.meta.id, None, None, None).unwrap();
                }
                "workspace-deleted" => {
                    ws::save_workspace_store(&ws::WorkspaceStoreFile::default()).unwrap()
                }
                "session-deleted" => store::delete_session(&fixture.meta.id).unwrap(),
                "corrupt-index" => {
                    std::fs::write(crate::paths::sessions_index_file(), b"{broken").unwrap()
                }
                _ => unreachable!(),
            }
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let error = runtime
                .block_on(manager.open_user_turn(
                    &fixture.meta.id,
                    "must not send",
                    "must not send",
                    None,
                ))
                .err()
                .unwrap();
            assert!(error.starts_with("WORKSPACE_CHANGED:"), "{change}: {error}");
            fixture.assert_unsent(&manager);
            if background {
                assert_eq!(
                    manager.inner.lock().as_ref().unwrap().app_session_id,
                    "other-synthetic-chat"
                );
            }
        }
    }
}

#[test]
fn stopped_or_replaced_owner_during_journal_wait_cannot_open_a_turn() {
    for stop in [false, true] {
        let fixture = Fixture::new(true);
        let manager = fixture.manager(false);
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let lock = manager.post_turn_journal_lock(&fixture.meta.id);
            let held = lock.lock().await;
            let open = manager.open_user_turn(&fixture.meta.id, "unsent", "unsent", None);
            tokio::pin!(open);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), &mut open)
                    .await
                    .is_err()
            );
            manager
                .with_session_mut(&fixture.meta.id, |s| {
                    if stop {
                        s.turn_open_epoch += 1;
                    } else {
                        s.process_id = "replacement-process".into();
                    }
                })
                .unwrap();
            drop(held);
            let error = open.await.err().unwrap();
            assert!(error.starts_with(if stop {
                "SEND_CANCELLED:"
            } else {
                "WORKSPACE_CHANGED:"
            }));
        });
        fixture.assert_unsent(&manager);
    }
}

#[test]
fn review_revalidates_and_explicit_confirmation_allows_fresh_connection() {
    let fixture = Fixture::new(true);
    fixture.change_roots();
    let review = preview_workspace_runtime(&fixture.meta.id).unwrap();
    assert_eq!(review.roots.len(), 1);
    assert!(fixture.context.validate(&fixture.meta.id).is_err());
    assert!(confirm_review(&fixture.meta.id, "wrong hash").is_err());
    confirm_review(&fixture.meta.id, &review.request_hash).unwrap();
    let updated = session(&fixture.meta.id).unwrap();
    assert_ne!(
        updated.workspace_root_snapshot,
        fixture.meta.workspace_root_snapshot
    );
    assert!(fixture.context.validate(&fixture.meta.id).is_err());
    let new_context =
        RuntimeWorkspaceContext::capture(&updated, &review.cwd, &store::load_settings(), false)
            .unwrap();
    new_context.validate(&fixture.meta.id).unwrap();
    let manager = fixture.manager(false);
    manager.with_session_mut(&fixture.meta.id, |live| {
        live.meta = updated;
        live.workspace_context = Some(new_context);
    });
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(manager.open_user_turn(&fixture.meta.id, "reviewed", "reviewed", None))
        .unwrap();
}

#[test]
fn review_stale_configuration_or_missing_root_does_not_mutate_session() {
    let fixture = Fixture::new(true);
    let review = preview_workspace_runtime(&fixture.meta.id).unwrap();
    let before = std::fs::read(crate::paths::sessions_index_file()).unwrap();
    fixture.change_roots();
    assert!(confirm_review(&fixture.meta.id, &review.request_hash).is_err());
    assert_eq!(
        before,
        std::fs::read(crate::paths::sessions_index_file()).unwrap()
    );
    std::fs::remove_dir(fixture.home.path.join("primary")).unwrap();
    assert!(preview_workspace_runtime(&fixture.meta.id).is_err());
    assert_eq!(
        before,
        std::fs::read(crate::paths::sessions_index_file()).unwrap()
    );
}

#[test]
fn runtime_metadata_preserves_new_binding_and_does_not_resurrect_deleted_session() {
    let fixture = Fixture::new(true);
    store::set_session_workspace(&fixture.meta.id, None, None, None).unwrap();
    store::update_runtime_session_meta(&fixture.meta).unwrap();
    assert!(session(&fixture.meta.id).unwrap().workspace_id.is_none());
    store::delete_session(&fixture.meta.id).unwrap();
    assert!(store::update_runtime_session_meta(&fixture.meta).is_err());
    assert!(session(&fixture.meta.id).is_err());
}

#[test]
fn ordinary_unbound_session_uses_general_directory_and_detects_later_binding() {
    let fixture = Fixture::new(false);
    let meta = store::create_session(None, None, false).unwrap();
    let context = RuntimeWorkspaceContext::capture(
        &meta,
        crate::paths::general_workspace_dir().to_str().unwrap(),
        &store::load_settings(),
        false,
    )
    .unwrap();
    context.validate(&meta.id).unwrap();
    store::set_session_project(&meta.id, fixture.meta.project_id.clone()).unwrap();
    assert!(context.validate(&meta.id).is_err());
}

#[test]
fn explicit_detach_sentinel_is_unbound_for_runtime_capture_and_review() {
    let fixture = Fixture::new(false);
    let detached = store::set_session_workspace(
        &fixture.meta.id,
        Some(ws::UNBOUND_WORKSPACE_ID.to_string()),
        None,
        None,
    )
    .unwrap();
    let context = RuntimeWorkspaceContext::capture(
        &detached,
        &fixture.context.cwd,
        &store::load_settings(),
        false,
    )
    .unwrap();
    assert!(context.workspace.is_none());
    assert!(context.roots.is_empty());
    context.validate(&detached.id).unwrap();

    let review = preview_workspace_runtime(&detached.id).unwrap();
    assert!(review.roots.is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_retarget_refuses_next_turn() {
    let fixture = Fixture::new(false);
    let link = fixture.home.path.join("cwd-link");
    std::os::unix::fs::symlink(fixture.home.path.join("primary"), &link).unwrap();
    let context = RuntimeWorkspaceContext::capture(
        &fixture.meta,
        link.to_str().unwrap(),
        &store::load_settings(),
        false,
    )
    .unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(fixture.home.path.join("extra"), &link).unwrap();
    assert!(context.validate(&fixture.meta.id).is_err());
}

#[test]
fn changed_context_does_not_prevent_connect_after_explicit_disconnect() {
    let fixture = Fixture::new(true);
    for background in [false, true] {
        let manager = fixture.manager(background);
        assert!(manager
            .reusable_workspace_context(&fixture.meta.id)
            .is_some());
        manager.with_session_mut(&fixture.meta.id, |s| {
            s.prompt_in_flight = true;
            s.fsm.begin_stream().unwrap();
        });
        assert!(manager
            .reusable_workspace_context(&fixture.meta.id)
            .is_none());
        manager.with_session_mut(&fixture.meta.id, |s| {
            s.prompt_in_flight = false;
            s.fsm.soft_disconnect();
        });
        assert!(manager
            .reusable_workspace_context(&fixture.meta.id)
            .is_none());
    }
}

#[test]
fn late_runtime_callback_cannot_restore_cleared_agent_identity() {
    let fixture = Fixture::new(true);
    let mut runtime_meta = fixture.meta.clone();
    runtime_meta.agent_session_id = Some("synthetic-old-agent".into());
    store::update_connected_session_meta(&runtime_meta).unwrap();
    let review = preview_workspace_runtime(&fixture.meta.id).unwrap();
    confirm_review(&fixture.meta.id, &review.request_hash).unwrap();
    store::update_runtime_session_meta(&runtime_meta).unwrap();
    assert!(session(&fixture.meta.id)
        .unwrap()
        .agent_session_id
        .is_none());
}

#[test]
fn workspace_confirmation_clears_execution_source_and_invalidates_stale_review() {
    let fixture = Fixture::new(true);
    let mut row = fixture.meta.clone();
    row.agent_session_id = Some("synthetic-agent".into());
    row.agent_execution = Some(crate::session_execution::ExecutionReceipt {
        schema_version: 1,
        agent_session_id: "synthetic-agent".into(),
        source: crate::session_execution::ExecutionSource::LegacyIndependent,
        provider_id: Some("synthetic-provider".into()),
        model_id: Some("synthetic-model".into()),
        process_id: Some("process-a".into()),
    });
    store::update_session_meta(&row).unwrap();
    let stale = preview_workspace_runtime(&row.id).unwrap();
    row.agent_execution.as_mut().unwrap().process_id = Some("process-b".into());
    store::update_session_meta(&row).unwrap();
    assert!(confirm_review(&row.id, &stale.request_hash).is_err());
    let fresh = preview_workspace_runtime(&row.id).unwrap();
    confirm_review(&row.id, &fresh.request_hash).unwrap();
    store::update_runtime_session_meta(&row).unwrap();
    let after = session(&row.id).unwrap();
    assert!(after.agent_session_id.is_none());
    assert!(after.agent_execution.is_none());
}

#[test]
fn review_and_runtime_write_leave_a_corrupt_index_untouched() {
    let fixture = Fixture::new(true);
    let review = preview_workspace_runtime(&fixture.meta.id).unwrap();
    let path = crate::paths::sessions_index_file();
    std::fs::write(&path, b"{synthetic-corruption").unwrap();
    assert!(confirm_review(&fixture.meta.id, &review.request_hash).is_err());
    assert!(store::update_runtime_session_meta(&fixture.meta).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"{synthetic-corruption");
}

#[test]
fn interjection_remains_in_existing_turn_after_workspace_metadata_change() {
    let fixture = Fixture::new(true);
    let manager = fixture.manager(true);
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(manager.open_user_turn(&fixture.meta.id, "running", "running", None))
        .unwrap();
    fixture.change_roots();
    manager
        .with_session_mut(&fixture.meta.id, |s| {
            assert!(SessionManager::pick_interjection_target(s).is_ok());
            assert_eq!(s.workspace_context, Some(fixture.context.clone()));
            assert!(s.prompt_in_flight);
        })
        .unwrap();
}

#[test]
fn remote_project_review_never_probes_remote_path_on_local_host() {
    let _home = IsolatedAppHome::acquire("runtime-remote-metadata");
    let project = store::add_ssh_project(
        "synthetic-host",
        "/synthetic/not-on-this-host".into(),
        false,
    )
    .unwrap();
    let meta = store::create_session(Some(project.id), None, false).unwrap();
    let context =
        RuntimeWorkspaceContext::capture(&meta, &project.path, &store::load_settings(), true)
            .unwrap();
    context.validate(&meta.id).unwrap();
    let review = preview_workspace_runtime(&meta.id).unwrap();
    assert_eq!(review.ssh_alias.as_deref(), Some("synthetic-host"));
    confirm_review(&meta.id, &review.request_hash).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_symlink_retarget_refuses_next_turn() {
    let fixture = Fixture::new(false);
    let link = fixture.home.path.join("cwd-link");
    std::os::windows::fs::symlink_dir(fixture.home.path.join("primary"), &link).unwrap();
    let context = RuntimeWorkspaceContext::capture(
        &fixture.meta,
        link.to_str().unwrap(),
        &store::load_settings(),
        false,
    )
    .unwrap();
    std::fs::remove_dir(&link).unwrap();
    std::os::windows::fs::symlink_dir(fixture.home.path.join("extra"), &link).unwrap();
    assert!(context.validate(&fixture.meta.id).is_err());
}
