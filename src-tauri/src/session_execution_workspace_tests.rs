//! Workspace identity at the real history/plan/media/interrupt consumers.
use super::{current, fixture, Fixture};
use crate::{cli_sessions, paths, plan_chrome, session_execution, store};
use std::{fs, path::PathBuf};

fn home(fixture: &Fixture) -> PathBuf {
    fixture.actual.ancestors().nth(3).unwrap().to_path_buf()
}

fn cwd(fixture: &Fixture) -> String {
    fixture.root.join("project").to_str().unwrap().into()
}

fn relocate(fixture: &Fixture, component: &str) -> PathBuf {
    let workspace = home(fixture).join("sessions").join(component);
    fs::create_dir_all(&workspace).unwrap();
    let session = workspace.join("agent-collision");
    fs::rename(&fixture.actual, &session).unwrap();
    session
}

fn other_workspace(fixture: &Fixture) -> PathBuf {
    relocate(
        fixture,
        &paths::percent_encode_path_component(
            fixture.root.join("foreign-project").to_str().unwrap(),
        ),
    )
}

fn worktree_session(fixture: &Fixture) -> PathBuf {
    let worktree = fixture.root.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    let requested = worktree.to_str().unwrap();
    let actual = relocate(fixture, &paths::percent_encode_path_component(requested));
    store::set_session_worktree(
        &fixture.meta.id,
        Some(requested.into()),
        Some("synthetic-branch".into()),
    )
    .unwrap();
    // Same agent under the main project must not override the linked worktree.
    fs::create_dir_all(fixture.actual.join("images")).unwrap();
    fs::write(fixture.actual.join("plan.md"), "WRONG_MAIN_PROJECT").unwrap();
    fs::write(
        fixture.actual.join("chat_history.jsonl"),
        "{\"type\":\"assistant\",\"content\":\"WRONG_MAIN_PROJECT\"}\n",
    )
    .unwrap();
    fs::write(fixture.actual.join("images/1.png"), "WRONG_MAIN_PROJECT").unwrap();
    actual
}

#[tokio::test]
async fn linked_worktree_routes_plan_journal_and_media_without_using_project_root() {
    for source in ["legacy_shared", "legacy_independent", "app_profile"] {
        let fixture = fixture(source);
        let actual = worktree_session(&fixture);
        let before = fs::read(fixture.actual.join("chat_history.jsonl")).unwrap();
        assert_eq!(
            plan_chrome::load_agent_plan_snapshot(&fixture.meta.id)
                .plan_body
                .as_deref(),
            Some("ACTUAL_A"),
        );
        assert_eq!(
            cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
            1
        );
        assert_eq!(
            store::load_messages(&fixture.meta.id)[0].content,
            "ACTUAL_A"
        );
        let media = crate::commands::session_resolve_relative_media(
            fixture.meta.id.clone(),
            vec!["images/1.png".into()],
        )
        .await
        .unwrap();
        assert_eq!(media.len(), 1);
        assert_eq!(PathBuf::from(&media[0].path), actual.join("images/1.png"));
        assert_eq!(
            fs::read(fixture.actual.join("chat_history.jsonl")).unwrap(),
            before
        );
    }
}

#[test]
fn linked_worktree_permission_and_pending_fork_keep_the_worktree_source() {
    let fixture = fixture("app_profile");
    let actual = worktree_session(&fixture);
    fs::write(
        actual.join("events.jsonl"),
        "{\"type\":\"permission_requested\",\"tool_name\":\"actual_worktree_tool\"}\n",
    )
    .unwrap();
    fs::write(
        fixture.actual.join("events.jsonl"),
        "{\"type\":\"permission_requested\",\"tool_name\":\"wrong_project_tool\"}\n",
    )
    .unwrap();
    assert!(crate::turn_interrupt::heal_interrupted_turn(&fixture.meta.id).is_some());
    assert_eq!(
        crate::turn_lease::read_lease(&fixture.meta.id)
            .unwrap()
            .pending_tool
            .unwrap()
            .tool_name,
        "actual_worktree_tool",
    );
    let parent = current(&fixture);
    let child = store::fork_session(&parent.id, None, None, true).unwrap();
    assert_eq!(child.worktree_path, parent.worktree_path);
    assert_eq!(child.agent_execution, parent.agent_execution);
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&child.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A"),
    );
}

#[test]
fn missing_worktree_history_does_not_fall_back_to_the_project() {
    let fixture = fixture("app_profile");
    store::set_session_worktree(
        &fixture.meta.id,
        Some(
            fixture
                .root
                .join("missing-worktree")
                .to_str()
                .unwrap()
                .into(),
        ),
        None,
    )
    .unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        0
    );
    assert!(store::load_messages(&fixture.meta.id).is_empty());
}

#[test]
fn specified_workspace_never_reads_another_workspaces_plan_or_journal() {
    for source in ["legacy_shared", "legacy_independent", "app_profile"] {
        let fixture = fixture(source);
        let foreign = other_workspace(&fixture);
        let history = fs::read(foreign.join("chat_history.jsonl")).unwrap();
        let index = fs::read(paths::sessions_index_file()).unwrap();
        assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
        assert_eq!(
            cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
            0
        );
        assert!(store::load_messages(&fixture.meta.id).is_empty());
        assert_eq!(
            fs::read(foreign.join("chat_history.jsonl")).unwrap(),
            history
        );
        assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), index);
    }
}

#[tokio::test]
async fn specified_workspace_never_reads_another_workspaces_media() {
    for source in ["legacy_shared", "legacy_independent", "app_profile"] {
        let fixture = fixture(source);
        let foreign = other_workspace(&fixture);
        let media = fs::read(foreign.join("images/1.png")).unwrap();
        assert!(crate::commands::session_resolve_relative_media(
            fixture.meta.id.clone(),
            vec!["images/1.png".into()],
        )
        .await
        .unwrap()
        .is_empty());
        assert_eq!(fs::read(foreign.join("images/1.png")).unwrap(), media);
    }
}

#[test]
fn specified_workspace_never_recovers_another_workspaces_permission_trail() {
    let fixture = fixture("app_profile");
    let foreign = other_workspace(&fixture);
    fs::write(
        foreign.join("events.jsonl"),
        "{\"type\":\"permission_requested\",\"tool_name\":\"foreign_workspace_tool\"}\n",
    )
    .unwrap();
    assert!(crate::turn_interrupt::heal_interrupted_turn(&fixture.meta.id).is_none());
    assert!(crate::turn_lease::read_lease(&fixture.meta.id).is_none());
    assert!(store::load_messages(&fixture.meta.id).is_empty());
}

#[tokio::test]
async fn long_chinese_workspace_metadata_routes_real_plan_journal_and_media() {
    let fixture = fixture("app_profile");
    let long_project = fixture.root.join("中文工作区".repeat(12));
    fs::create_dir_all(&long_project).unwrap();
    let requested = long_project.to_str().unwrap();
    assert!(paths::percent_encode_path_component(requested).len() > 255);
    let project = store::add_project(requested.into(), true).unwrap();
    let mut meta = current(&fixture);
    meta.project_id = Some(project.id);
    store::update_session_meta(&meta).unwrap();
    // Build's long-path shape: a short slug/hash name plus authoritative .cwd.
    // No production hash implementation is duplicated by this reader test.
    let actual = relocate(&fixture, "workspace-0123456789abcdef");
    fs::write(actual.parent().unwrap().join(".cwd"), requested).unwrap();
    let plan = plan_chrome::load_agent_plan_snapshot(&meta.id);
    assert_eq!(plan.plan_body.as_deref(), Some("ACTUAL_A"));
    assert_eq!(cli_sessions::try_reconcile_linked_session(&meta.id), 1);
    assert_eq!(store::load_messages(&meta.id)[0].content, "ACTUAL_A");
    let media =
        crate::commands::session_resolve_relative_media(meta.id, vec!["images/1.png".into()])
            .await
            .unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(PathBuf::from(&media[0].path), actual.join("images/1.png"));
    assert!(fixture.decoy.join("chat_history.jsonl").is_file());
}

#[test]
fn unmatched_or_invalid_long_workspace_metadata_never_selects_history() {
    for bytes in [
        b"/a/different/workspace".to_vec(),
        vec![0xff, 0xfe],
        vec![b' '; 128 * 1024 + 1],
    ] {
        let fixture = fixture("app_profile");
        let actual = relocate(&fixture, "workspace-0123456789abcdef");
        fs::write(actual.parent().unwrap().join(".cwd"), bytes).unwrap();
        assert!(session_execution::find_in_home(
            &home(&fixture),
            "agent-collision",
            Some(&cwd(&fixture)),
        )
        .is_none());
        assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
    }
}

#[test]
fn missing_or_non_file_long_workspace_metadata_never_selects_history() {
    let fixture = fixture("app_profile");
    let actual = relocate(&fixture, "workspace-0123456789abcdef");
    let marker = actual.parent().unwrap().join(".cwd");
    assert!(session_execution::find_in_home(
        &home(&fixture),
        "agent-collision",
        Some(&cwd(&fixture)),
    )
    .is_none());
    fs::create_dir(&marker).unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
}

#[test]
fn short_encoded_workspace_identity_takes_precedence_over_foreign_metadata() {
    let fixture = fixture("app_profile");
    let requested = cwd(&fixture);
    let encoded = paths::percent_encode_path_component(&requested)
        .replace("%2F", "%2f")
        .replace("%5C", "%5c")
        .replace("%3A", "%3a");
    assert_ne!(encoded, paths::percent_encode_path_component(&requested));
    let actual = relocate(&fixture, &encoded);
    fs::write(actual.parent().unwrap().join(".cwd"), "/foreign").unwrap();
    assert_eq!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&requested))
            .unwrap()
            .canonicalize()
            .unwrap(),
        actual.canonicalize().unwrap(),
    );
    assert!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some("/foreign"),)
            .is_none()
    );
}

#[test]
fn matching_long_workspace_metadata_cannot_choose_between_two_histories() {
    let fixture = fixture("app_profile");
    let requested = cwd(&fixture);
    let first = relocate(&fixture, "workspace-0123456789abcdef");
    fs::write(first.parent().unwrap().join(".cwd"), &requested).unwrap();
    let second = home(&fixture).join("sessions/workspace-fedcba9876543210");
    fs::create_dir_all(second.join("agent-collision")).unwrap();
    fs::write(second.join(".cwd"), &requested).unwrap();
    assert!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&requested),)
            .is_none()
    );
}

#[test]
fn unknown_workspace_accepts_only_a_unique_contained_agent() {
    let fixture = fixture("app_profile");
    for hint in [None, Some("")] {
        assert_eq!(
            session_execution::find_in_home(&home(&fixture), "agent-collision", hint),
            Some(fixture.actual.clone()),
        );
    }
    let other = home(&fixture).join("sessions/another-workspace/agent-collision");
    fs::create_dir_all(&other).unwrap();
    for hint in [None, Some("")] {
        assert!(
            session_execution::find_in_home(&home(&fixture), "agent-collision", hint).is_none()
        );
    }
    assert_eq!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&cwd(&fixture)),),
        Some(fixture.actual.clone()),
    );
}

#[test]
fn inherited_pending_fork_receipt_still_resolves_the_parents_exact_workspace() {
    let fixture = fixture("app_profile");
    let parent = current(&fixture);
    let child = store::fork_session(&parent.id, None, None, true).unwrap();
    assert_ne!(child.id, parent.id);
    assert_eq!(child.agent_execution, parent.agent_execution);
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&child.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A"),
    );
}

#[test]
fn unavailable_workspace_path_does_not_require_canonicalizing_the_project() {
    let fixture = fixture("app_profile");
    let requested = fixture
        .root
        .join("offline-project")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(!PathBuf::from(&requested).exists());
    let actual = relocate(&fixture, &paths::percent_encode_path_component(&requested));
    assert_eq!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&requested)),
        Some(actual),
    );
}

#[test]
fn long_workspace_metadata_accepts_a_legacy_line_ending() {
    let fixture = fixture("app_profile");
    let actual = relocate(&fixture, "workspace-0123456789abcdef");
    fs::write(
        actual.parent().unwrap().join(".cwd"),
        format!("{}\r\n", cwd(&fixture)),
    )
    .unwrap();
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&fixture.meta.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A"),
    );
}

#[test]
fn invalid_percent_encoded_utf8_cannot_alias_a_real_workspace() {
    let fixture = fixture("app_profile");
    let requested = fixture.root.join("\u{fffd}").to_str().unwrap().to_owned();
    let invalid = paths::percent_encode_path_component(&requested).replace("%EF%BF%BD", "%FF");
    relocate(&fixture, &invalid);
    assert!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&requested),)
            .is_none()
    );
}

#[cfg(windows)]
#[test]
fn windows_workspace_case_and_separator_variants_match() {
    let fixture = fixture("app_profile");
    let requested = cwd(&fixture);
    let variant = requested.to_ascii_uppercase().replace('\\', "/");
    let actual = relocate(&fixture, &paths::percent_encode_path_component(&variant));
    assert_eq!(
        session_execution::find_in_home(&home(&fixture), "agent-collision", Some(&requested)),
        Some(actual),
    );
}

#[cfg(windows)]
#[test]
fn windows_namespace_case_does_not_make_contained_history_unavailable() {
    let fixture = fixture("app_profile");
    let sessions = home(&fixture).join("sessions");
    fs::rename(&sessions, home(&fixture).join("Sessions")).unwrap();
    fs::rename(
        fixture.actual.clone(),
        fixture.actual.with_file_name("AGENT-COLLISION"),
    )
    .unwrap();
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&fixture.meta.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A"),
    );
}

#[cfg(unix)]
#[test]
fn unix_workspace_case_is_not_collapsed() {
    let fixture = fixture("app_profile");
    // On a case-insensitive volume (default APFS) "Project" and "project" are
    // one folder, so the premise of this case-sensitive check does not hold.
    std::fs::create_dir(fixture.root.join("CaseProbe")).unwrap();
    if fixture.root.join("caseprobe").exists() {
        eprintln!("skipped: case-insensitive file system");
        return;
    }
    let variant = fixture.root.join("Project").to_str().unwrap().to_owned();
    relocate(&fixture, &paths::percent_encode_path_component(&variant));
    assert!(session_execution::find_in_home(
        &home(&fixture),
        "agent-collision",
        Some(&cwd(&fixture)),
    )
    .is_none());
}

#[cfg(unix)]
#[test]
fn long_workspace_metadata_cannot_redirect_outside_its_directory() {
    let fixture = fixture("app_profile");
    let actual = relocate(&fixture, "workspace-0123456789abcdef");
    let foreign = fixture.root.join("foreign-cwd-metadata");
    fs::write(&foreign, cwd(&fixture)).unwrap();
    std::os::unix::fs::symlink(&foreign, actual.parent().unwrap().join(".cwd")).unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
}
