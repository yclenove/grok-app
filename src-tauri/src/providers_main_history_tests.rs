use super::*;
use crate::{
    extensions::ExtensionsPrefs,
    paths,
    providers::{MainProfileRequest, PreparedMainProfile},
    session_execution::ExecutionReceipt,
    session_migration::test_home::IsolatedAppHome,
    store::{self, AppSettings},
};
use std::{path::Path, sync::atomic::AtomicBool};

struct Fixture {
    request: MainProfileRequest,
    source: PathBuf,
    _home: IsolatedAppHome,
}

fn fixture(shared: bool, long: bool) -> Fixture {
    let home = IsolatedAppHome::acquire_with_user_home("selected-main-history");
    fs::write(
        paths::agent_config_toml(),
        "[models]\ndefault = 'other'\n[model.selected]\nmodel = 'target'\nbase_url = 'https://synthetic.invalid/v1'\napi_key = 'synthetic-key'\n",
    )
    .unwrap();
    let mut session = store::create_session(None, Some("合成会话".into()), false).unwrap();
    session.agent_session_id = Some("selected-agent".into());
    session.provider_id = Some("unrelated-desired-provider".into());
    let source = if shared {
        ExecutionSource::LegacyShared
    } else {
        ExecutionSource::LegacyIndependent
    };
    session.agent_execution = Some(ExecutionReceipt {
        schema_version: 1,
        agent_session_id: "selected-agent".into(),
        source: source.clone(),
        provider_id: Some("observed-old-provider".into()),
        model_id: Some("observed-old-model".into()),
        process_id: None,
    });
    // History matching is lexical and must not probe this nonexistent cwd.
    // Encoding the long isolated home here exceeds Windows' component limit
    // and would fail fixture creation instead of testing short-path migration.
    let cwd = if cfg!(windows) {
        "Z:/合成/项目"
    } else {
        "/synthetic/合成/项目"
    }
    .to_owned();
    let name = if long {
        "physical-slug-hash-do-not-recompute".into()
    } else {
        paths::percent_encode_path_component(&cwd)
    };
    let workspace = source.home().unwrap().unwrap().join("sessions").join(&name);
    let selected = workspace.join("selected-agent");
    fs::create_dir_all(selected.join("empty")).unwrap();
    for (name, text) in [
        ("chat_history.jsonl", "{\"content\":\"你好，继续工作\"}\n"),
        ("events.jsonl", "{\"event\":\"synthetic\"}\n"),
        ("plan.md", "中文计划"),
        ("plan_mode.json", "{\"mode\":\"plan\"}"),
        ("images/1.png", "synthetic-image"),
        ("videos/1.mp4", "synthetic-video"),
        ("cache/session-note", "retain-session-content"),
        ("config.toml", "retain-unknown-session-metadata"),
    ] {
        let path = selected.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::write(workspace.join(".cwd"), format!("{cwd}\r\n")).unwrap();
    fs::create_dir_all(workspace.join("foreign-agent")).unwrap();
    fs::write(
        workspace.join("foreign-agent/plan.md"),
        "DO_NOT_COPY_SIBLING",
    )
    .unwrap();
    let other = if shared {
        paths::agent_home_dir()
    } else {
        crate::process_util::user_home().join(".grok")
    }
    .join("sessions")
    .join(&name)
    .join("selected-agent");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("plan.md"), "DO_NOT_COPY_GLOBAL").unwrap();
    Fixture {
        request: MainProfileRequest {
            owner_session_id: session.id.clone(),
            provider_id: "selected".into(),
            model_id: "target".into(),
            history: Some(MainHistoryRequest {
                session,
                cwd,
                // Deliberately wrong fallback. Receipt must win.
                legacy_source: if shared {
                    ExecutionSource::LegacyIndependent
                } else {
                    ExecutionSource::LegacyShared
                },
            }),
        },
        source: selected,
        _home: home,
    }
}

fn prepare(request: &MainProfileRequest) -> Result<PreparedMainProfile, String> {
    PreparedMainProfile::prepare(
        request,
        &AppSettings::default(),
        "ask",
        false,
        &ExtensionsPrefs::default(),
        None,
        &AtomicBool::new(false),
    )
}

fn capture(fixture: &Fixture) -> Result<Snapshot, String> {
    Snapshot::capture(
        fixture.request.history.as_ref().unwrap(),
        &fixture.request.owner_session_id,
    )
}

fn preserves_selected_history(shared: bool, long: bool) {
    let fixture = fixture(shared, long);
    let snapshot = capture(&fixture).unwrap();
    let before = profile_io::Tree::capture_history(&fixture.source).unwrap();
    let profile = prepare(&fixture.request).unwrap();
    let destination = profile.home.join(&snapshot.relative);
    assert!(before.same_contents(&profile_io::Tree::capture_history(&destination).unwrap()));
    before.unchanged().unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("plan.md")).unwrap(),
        "中文计划"
    );
    assert_eq!(
        fs::read(destination.parent().unwrap().join(".cwd")).unwrap(),
        fs::read(fixture.source.parent().unwrap().join(".cwd")).unwrap()
    );
    assert!(destination.join("empty").is_dir());
    assert!(!destination.parent().unwrap().join("foreign-agent").exists());
    assert_eq!(
        fs::read_dir(profile.home.join("sessions")).unwrap().count(),
        1
    );
    assert!(profile.home.join("execution-profile.json").is_file());
    assert_eq!(profile.model_binding.resolve("target"), "selected");
}

#[test]
fn shared_receipt_selects_only_its_agent_before_custom_profile_publication() {
    preserves_selected_history(true, false);
}

#[test]
fn independent_receipt_preserves_long_physical_workspace_name_and_cwd_bytes() {
    preserves_selected_history(false, true);
}

#[test]
fn linked_worktree_precedes_the_connect_project_hint() {
    let mut fixture = fixture(false, true);
    let history = fixture.request.history.as_mut().unwrap();
    history.session.worktree_path = Some(history.cwd.clone());
    history.cwd = fixture
        ._home
        .path
        .join("wrong-project-root")
        .to_str()
        .unwrap()
        .into();
    let snapshot = capture(&fixture).unwrap();
    assert_eq!(snapshot.workspace.path, fixture.source.parent().unwrap());
    assert!(prepare(&fixture.request).is_ok());
}

#[test]
fn legacy_rows_require_explicit_source_and_never_infer_from_global_mode() {
    let mut fixture = fixture(true, true);
    let history = fixture.request.history.as_mut().unwrap();
    history.session.agent_execution = None;
    history.legacy_source = ExecutionSource::LegacyShared;
    assert!(prepare(&fixture.request).is_ok());
    for source in [ExecutionSource::Remote, ExecutionSource::Unavailable] {
        fixture.request.history.as_mut().unwrap().legacy_source = source;
        assert!(capture(&fixture).is_err());
    }
}

#[test]
fn parent_profile_receipt_is_preserved_for_a_pending_child_fork() {
    let mut fixture = fixture(false, true);
    let parent = MainProfileRequest {
        owner_session_id: uuid::Uuid::new_v4().to_string(),
        provider_id: "selected".into(),
        model_id: "target".into(),
        history: None,
    };
    let parent = prepare(&parent).unwrap();
    let physical = fixture.source.parent().unwrap().file_name().unwrap();
    let destination = parent
        .home
        .join("sessions")
        .join(physical)
        .join("selected-agent");
    fs::create_dir_all(&destination).unwrap();
    profile_io::Tree::capture_history(&fixture.source)
        .unwrap()
        .copy_into(&destination)
        .unwrap();
    fs::write(
        destination.parent().unwrap().join(".cwd"),
        fs::read(fixture.source.parent().unwrap().join(".cwd")).unwrap(),
    )
    .unwrap();
    let history = fixture.request.history.as_mut().unwrap();
    history.session.fork_agent_session = true;
    history.session.agent_execution.as_mut().unwrap().source =
        parent.configuration.as_ref().unwrap().execution_source();
    fixture.source = destination;
    let snapshot = capture(&fixture).unwrap();
    let child = prepare(&fixture.request).unwrap();
    assert_eq!(
        fs::read_to_string(child.home.join(&snapshot.relative).join("plan.md")).unwrap(),
        "中文计划"
    );
    snapshot.unchanged().unwrap();
}

#[test]
fn mismatched_receipts_owner_remote_missing_agent_and_cwd_fail_before_publication() {
    let fixture = fixture(false, true);
    for change in [
        |history: &mut MainHistoryRequest| history.session.id = uuid::Uuid::new_v4().to_string(),
        |history: &mut MainHistoryRequest| {
            history
                .session
                .agent_execution
                .as_mut()
                .unwrap()
                .schema_version = 0
        },
        |history: &mut MainHistoryRequest| {
            history.session.agent_session_id = Some("different-agent".into())
        },
        |history: &mut MainHistoryRequest| {
            history.session.agent_execution.as_mut().unwrap().source = ExecutionSource::Remote
        },
        |history: &mut MainHistoryRequest| {
            history.session.agent_execution.as_mut().unwrap().source = ExecutionSource::Unavailable
        },
        |history: &mut MainHistoryRequest| history.session.agent_session_id = None,
        |history: &mut MainHistoryRequest| history.cwd.clear(),
        |history: &mut MainHistoryRequest| history.cwd.push_str("-wrong-workspace"),
    ] {
        let mut request = fixture.request.clone();
        change(request.history.as_mut().unwrap());
        assert!(prepare(&request).is_err());
        assert!(!paths::app_data_root().join("execution-profiles").exists());
    }
}

#[test]
fn source_content_and_same_bytes_file_replacement_invalidate_snapshot() {
    let fixture = fixture(false, true);
    let snapshot = capture(&fixture).unwrap();
    let path = fixture.source.join("plan.md");
    fs::write(&path, "改变").unwrap();
    assert!(snapshot.unchanged().is_err());
    let snapshot = capture(&fixture).unwrap();
    fs::rename(&path, fixture._home.path.join("held-original-plan")).unwrap();
    fs::write(&path, "改变").unwrap();
    assert!(snapshot.unchanged().is_err());
}

#[test]
fn cwd_and_workspace_same_path_replacement_invalidate_snapshot() {
    let fixture = fixture(false, true);
    let snapshot = capture(&fixture).unwrap();
    let path = fixture.source.parent().unwrap().join(".cwd");
    let bytes = fs::read(&path).unwrap();
    fs::rename(&path, fixture._home.path.join("held-cwd")).unwrap();
    fs::write(&path, bytes).unwrap();
    assert!(snapshot.unchanged().is_err());
    let snapshot = capture(&fixture).unwrap();
    let workspace = fixture.source.parent().unwrap();
    fs::rename(workspace, workspace.with_extension("held")).unwrap();
    fs::create_dir_all(&fixture.source).unwrap();
    assert!(snapshot.unchanged().is_err());
}

#[test]
fn copied_history_and_cwd_are_rechecked_and_partial_destination_is_never_reused() {
    let fixture = fixture(false, true);
    let snapshot = capture(&fixture).unwrap();
    let pending = PendingHome::create(&fixture.request.owner_session_id).unwrap();
    let copied = snapshot.copy_into(&pending).unwrap();
    assert!(snapshot.copy_into(&pending).is_err());
    fs::write(
        pending.home().join(&snapshot.relative).join("plan.md"),
        "destination-changed",
    )
    .unwrap();
    assert!(copied.unchanged().is_err());
    assert!(!pending.home().join("execution-profile.json").exists());
    let pending = PendingHome::create(&fixture.request.owner_session_id).unwrap();
    let copied = snapshot.copy_into(&pending).unwrap();
    fs::write(
        pending
            .home()
            .join(&snapshot.relative)
            .parent()
            .unwrap()
            .join(".cwd"),
        "changed-cwd",
    )
    .unwrap();
    assert!(copied.unchanged().is_err());
}

fn link(directory: bool, target: &Path, path: &Path) {
    #[cfg(windows)]
    if directory {
        std::os::windows::fs::symlink_dir(target, path).unwrap();
    } else {
        std::os::windows::fs::symlink_file(target, path).unwrap();
    }
    #[cfg(unix)]
    {
        let _ = directory;
        std::os::unix::fs::symlink(target, path).unwrap();
    }
}

#[test]
fn selected_history_and_cwd_redirects_fail_but_unselected_siblings_are_not_read() {
    let fixture = fixture(false, true);
    let external = fixture._home.path.join("external");
    fs::create_dir(&external).unwrap();
    link(
        true,
        &external,
        &fixture.source.parent().unwrap().join("unselected-redirect"),
    );
    assert!(capture(&fixture).is_ok());
    link(true, &external, &fixture.source.join("selected-redirect"));
    assert!(capture(&fixture).is_err());
}

#[test]
fn redirected_or_oversized_cwd_is_rejected_before_generation_creation() {
    let fixture = fixture(false, false);
    let cwd = fixture.source.parent().unwrap().join(".cwd");
    fs::write(&cwd, vec![b'a'; 128 * 1024 + 1]).unwrap();
    assert!(prepare(&fixture.request).is_err());
    fs::rename(&cwd, fixture._home.path.join("held-large-cwd")).unwrap();
    let external = fixture._home.path.join("external-cwd");
    fs::write(&external, &fixture.request.history.as_ref().unwrap().cwd).unwrap();
    link(false, &external, &cwd);
    assert!(prepare(&fixture.request).is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
}

#[test]
fn history_debug_never_discloses_session_metadata_or_workspace() {
    let fixture = fixture(false, true);
    let text = format!("{:?}", fixture.request);
    assert!(!text.contains(&fixture.request.history.as_ref().unwrap().cwd));
    assert!(!text.contains("合成会话"));
    assert!(!text.contains("selected-agent"));
}

#[test]
fn short_encoded_workspace_does_not_require_cwd_but_ambiguous_long_layout_is_refused() {
    let fixture = fixture(false, false);
    let marker = fixture.source.parent().unwrap().join(".cwd");
    fs::rename(marker, fixture._home.path.join("held-optional-cwd")).unwrap();
    let snapshot = capture(&fixture).unwrap();
    assert!(snapshot.cwd.is_none());
    let profile = prepare(&fixture.request).unwrap();
    assert!(!profile
        .home
        .join(&snapshot.relative)
        .parent()
        .unwrap()
        .join(".cwd")
        .exists());
    // Change the physical workspace into a slug, then create another matching
    // slug in the same observed home. The agent id is no longer unique.
    let old = fixture.source.parent().unwrap();
    let first = old.parent().unwrap().join("first-physical-slug");
    fs::rename(old, &first).unwrap();
    let cwd = &fixture.request.history.as_ref().unwrap().cwd;
    fs::write(first.join(".cwd"), cwd).unwrap();
    let second = first.parent().unwrap().join("second-physical-slug");
    fs::create_dir_all(second.join("selected-agent")).unwrap();
    fs::write(second.join(".cwd"), cwd).unwrap();
    assert!(capture(&fixture).is_err());
}

#[test]
fn oversized_selected_media_is_rejected_without_truncation_or_publication() {
    let fixture = fixture(false, true);
    let large = fs::File::create(fixture.source.join("videos/oversized.mp4")).unwrap();
    large.set_len(64 * 1024 * 1024 + 1).unwrap();
    assert!(prepare(&fixture.request).is_err());
    assert!(!paths::app_data_root().join("execution-profiles").exists());
    assert_eq!(large.metadata().unwrap().len(), 64 * 1024 * 1024 + 1);
}

#[test]
fn redirected_destination_is_refused_before_writing_any_history_outside_generation() {
    let fixture = fixture(false, true);
    let snapshot = capture(&fixture).unwrap();
    let pending = PendingHome::create(&fixture.request.owner_session_id).unwrap();
    let outside = fixture._home.path.join("outside-destination");
    fs::create_dir(&outside).unwrap();
    link(true, &outside, &pending.home().join("sessions"));
    assert!(snapshot.copy_into(&pending).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    assert!(!pending.home().join("execution-profile.json").exists());
}

#[cfg(unix)]
#[test]
fn selected_session_executable_bits_survive_the_copy() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = fixture(false, true);
    fs::set_permissions(
        fixture.source.join("plan.md"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let snapshot = capture(&fixture).unwrap();
    let profile = prepare(&fixture.request).unwrap();
    assert_eq!(
        fs::metadata(profile.home.join(snapshot.relative).join("plan.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}
