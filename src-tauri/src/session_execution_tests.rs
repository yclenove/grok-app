//! Actual plan/journal/media consumers against synthetic, colliding histories.
use crate::{cli_sessions, paths, plan_chrome, store};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

#[path = "session_execution_workspace_tests.rs"]
mod workspace;

pub(crate) struct Fixture {
    root: PathBuf,
    pub(crate) meta: store::SessionMeta,
    pub(crate) actual: PathBuf,
    pub(crate) decoy: PathBuf,
    _env: crate::session_migration::test_home::TestEnvGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn inject(id: &str, receipt: Value) {
    let mut rows: Value =
        serde_json::from_slice(&fs::read(paths::sessions_index_file()).unwrap()).unwrap();
    let row = rows
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == id)
        .unwrap();
    row["agentExecution"] = receipt;
    fs::write(
        paths::sessions_index_file(),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
}

fn receipt(agent: &str, source: Value) -> Value {
    json!({"schemaVersion":1,"agentSessionId":agent,"source":source,
        "providerId":"generic-a","modelId":"grok-4.6"})
}

pub(crate) fn fixture(source: &str) -> Fixture {
    let env = crate::session_migration::test_home::TestEnvGuard::acquire(&["HOME", "USERPROFILE"]);
    let root = std::env::temp_dir().join(format!("execution-consumers-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    std::env::set_var("HOME", root.join("user"));
    std::env::set_var("USERPROFILE", root.join("user"));
    std::env::set_var("GROK_APP_HOME", root.join("app"));
    paths::ensure_app_dirs().unwrap();
    let mut settings = store::load_settings();
    settings.session_data_mode = if source == "legacy_shared" {
        "independent"
    } else {
        "shared"
    }
    .into();
    store::save_settings(&settings).unwrap();
    let cwd = root.join("project");
    fs::create_dir(&cwd).unwrap();
    let project = store::add_project(cwd.to_string_lossy().into_owned(), true).unwrap();
    let mut meta =
        store::create_session(Some(project.id), Some("Synthetic execution".into()), false).unwrap();
    meta.agent_session_id = Some("agent-collision".into());
    // Desired B must never decide where the still-running A's history lives.
    meta.provider_id = Some("generic-b".into());
    store::update_session_meta(&meta).unwrap();
    let generation = uuid::Uuid::new_v4().to_string();
    let source_value = if source == "app_profile" {
        json!({"kind":source,"ownerSessionId":meta.id,"generation":generation})
    } else {
        json!({"kind":source})
    };
    let actual_home = match source {
        "legacy_shared" => root.join("user/.grok"),
        "legacy_independent" => paths::agent_home_dir(),
        "app_profile" => paths::app_data_root()
            .join("execution-profiles")
            .join(&meta.id)
            .join(&generation),
        _ => unreachable!(),
    };
    fs::create_dir_all(&actual_home).unwrap();
    if source == "app_profile" {
        fs::write(
            actual_home.join("execution-profile.json"),
            serde_json::to_vec(&json!({
                "schemaVersion":1,"ownerSessionId":meta.id,"generation":generation,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let decoy_home = paths::resolve_agent_grok_home(&settings.session_data_mode);
    let suffix = PathBuf::from("sessions")
        .join(paths::percent_encode_path_component(cwd.to_str().unwrap()))
        .join("agent-collision");
    let actual = actual_home.join(&suffix);
    let decoy = decoy_home.join(&suffix);
    for (dir, content) in [(&actual, "ACTUAL_A"), (&decoy, "WRONG_GLOBAL_B")] {
        fs::create_dir_all(dir.join("images")).unwrap();
        fs::write(dir.join("plan.md"), content).unwrap();
        fs::write(
            dir.join("chat_history.jsonl"),
            format!("{{\"type\":\"assistant\",\"content\":\"{content}\"}}\n"),
        )
        .unwrap();
        fs::write(dir.join("images/1.png"), content).unwrap();
    }
    inject(&meta.id, receipt("agent-collision", source_value));
    Fixture {
        root,
        meta,
        actual,
        decoy,
        _env: env,
    }
}

fn plan_uses_execution(source: &str) {
    let fixture = fixture(source);
    let before = fs::read(paths::sessions_index_file()).unwrap();
    let plan = plan_chrome::load_agent_plan_snapshot(&fixture.meta.id);
    assert_eq!(plan.plan_body.as_deref(), Some("ACTUAL_A"));
    assert_eq!(
        PathBuf::from(plan.plan_path.unwrap())
            .canonicalize()
            .unwrap(),
        fixture.actual.join("plan.md").canonicalize().unwrap()
    );
    assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
}

#[test]
fn independent_plan_is_not_read_from_global_shared() {
    plan_uses_execution("legacy_independent");
}
#[test]
fn shared_plan_is_not_read_from_global_independent() {
    plan_uses_execution("legacy_shared");
}
#[test]
fn profile_plan_is_not_read_from_global_home() {
    plan_uses_execution("app_profile");
}

fn journal_uses_execution(source: &str) {
    let fixture = fixture(source);
    let actual_before = fs::read(fixture.actual.join("chat_history.jsonl")).unwrap();
    let decoy_before = fs::read(fixture.decoy.join("chat_history.jsonl")).unwrap();
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        1
    );
    let rows = store::load_messages(&fixture.meta.id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, "ACTUAL_A");
    assert_eq!(
        fs::read(fixture.actual.join("chat_history.jsonl")).unwrap(),
        actual_before
    );
    assert_eq!(
        fs::read(fixture.decoy.join("chat_history.jsonl")).unwrap(),
        decoy_before
    );
}

#[test]
fn independent_journal_is_not_read_from_global_shared() {
    journal_uses_execution("legacy_independent");
}
#[test]
fn shared_journal_is_not_read_from_global_independent() {
    journal_uses_execution("legacy_shared");
}
#[test]
fn profile_journal_is_not_read_from_global_home() {
    journal_uses_execution("app_profile");
}

#[tokio::test]
async fn relative_media_uses_actual_profile_not_desired_or_global_route() {
    let fixture = fixture("app_profile");
    let media = crate::commands::session_resolve_relative_media(
        fixture.meta.id.clone(),
        vec!["images/1.png".into()],
    )
    .await
    .unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(
        PathBuf::from(&media[0].path),
        fixture.actual.join("images/1.png")
    );
}

#[test]
fn missing_execution_directory_cannot_fall_back_to_global_history() {
    let fixture = fixture("legacy_independent");
    // Move only our explicitly owned synthetic directory; preserve its bytes.
    fs::rename(
        &fixture.actual,
        fixture.actual.with_file_name("preserved-actual"),
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
fn invalid_receipt_never_recovers_index_or_reads_global_home() {
    let fixture = fixture("legacy_independent");
    for bad in [
        receipt("different-agent", json!({"kind":"legacy_independent"})),
        receipt(
            "agent-collision",
            json!({"kind":"app_profile","ownerSessionId":"../escape","generation":"../escape"}),
        ),
        receipt("agent-collision", json!({"kind":"unknown_future_source"})),
        json!({"schemaVersion":99,"agentSessionId":"agent-collision","source":{"kind":"legacy_independent"}}),
        receipt("../agent-collision", json!({"kind":"legacy_independent"})),
    ] {
        inject(&fixture.meta.id, bad);
        let before = fs::read(paths::sessions_index_file()).unwrap();
        assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
        assert_eq!(
            cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
            0
        );
        assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
        assert!(store::load_messages(&fixture.meta.id).is_empty());
    }
}

fn current(fixture: &Fixture) -> store::SessionMeta {
    store::load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == fixture.meta.id)
        .unwrap()
}

#[test]
fn stale_runtime_and_connected_writebacks_preserve_actual_source() {
    let fixture = fixture("app_profile");
    let before = current(&fixture);
    let mut stale = fixture.meta.clone();
    stale.agent_execution = serde_json::from_value::<
        Option<crate::session_execution::ExecutionReceipt>,
    >(receipt("agent-collision", json!({"kind":"legacy_shared"})))
    .unwrap();
    stale.provider_id = Some("stale-provider".into());
    stale.model_id = Some("stale-model".into());
    store::update_runtime_session_meta(&stale).unwrap();
    store::update_connected_session_meta(&stale).unwrap();
    let after = current(&fixture);
    assert_eq!(after.agent_execution, before.agent_execution);
    assert_eq!(after.provider_id, before.provider_id);
    assert_eq!(after.model_id, before.model_id);
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&after.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A")
    );
}

#[test]
fn unreceipted_connected_agent_replacement_is_not_published() {
    let fixture = fixture("app_profile");
    let mut replacement = current(&fixture);
    replacement.agent_session_id = Some("new-agent".into());
    let before = fs::read(paths::sessions_index_file()).unwrap();
    assert!(store::update_connected_session_meta(&replacement).is_err());
    assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
}

#[test]
fn invalid_future_receipt_is_not_rewritten_by_store_mutations() {
    let fixture = fixture("legacy_independent");
    for bad in [
        json!({"schemaVersion":99,"agentSessionId":"agent-collision","source":{"kind":"future"},"opaqueFutureField":{"keep":"byte-exact"}}),
        json!({"schemaVersion":1,"agentSessionId":"different-agent","source":{"kind":"legacy_shared"}}),
        json!("not-an-object"),
    ] {
        inject(&fixture.meta.id, bad);
        let before = fs::read(paths::sessions_index_file()).unwrap();
        let row = current(&fixture);
        assert!(store::rename_session(&row.id, "must not persist").is_err());
        assert!(store::update_runtime_session_meta(&row).is_err());
        assert!(store::update_connected_session_meta(&row).is_err());
        assert!(store::update_sessions_index_strict(|_| Ok(())).is_err());
        assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
    }
}

#[test]
fn explicit_clear_drops_receipt_and_stale_runtime_cannot_restore_it() {
    let fixture = fixture("app_profile");
    let stale = current(&fixture);
    assert!(store::clear_session_agent_session_id(&stale.id).unwrap());
    store::update_runtime_session_meta(&stale).unwrap();
    let after = current(&fixture);
    assert!(after.agent_session_id.is_none());
    assert!(after.agent_execution.is_none());
    assert_eq!(after.provider_id, stale.provider_id);
    assert!(!store::clear_session_agent_session_id(&stale.id).unwrap());
}

#[test]
fn project_move_clears_execution_receipt_with_agent_identity() {
    let fixture = fixture("app_profile");
    let moved = store::move_session_to_project(&fixture.meta.id, None).unwrap();
    assert!(moved.agent_session_id.is_none());
    assert!(moved.agent_execution.is_none());
    assert_eq!(moved.provider_id, fixture.meta.provider_id);
    assert!(fixture.actual.join("chat_history.jsonl").is_file());
}

#[test]
fn only_native_pending_fork_inherits_actual_source_and_partial_failure_clears_it() {
    let fixture = fixture("app_profile");
    let parent = current(&fixture);
    let ordinary = store::fork_session(&parent.id, None, None, false).unwrap();
    assert!(ordinary.agent_session_id.is_none());
    assert!(ordinary.agent_execution.is_none());
    let full = store::fork_session(&parent.id, None, None, true).unwrap();
    assert_eq!(full.agent_execution, parent.agent_execution);
    let after_full_failure = store::clear_session_fork_after_connect_failure(&full.id).unwrap();
    assert_eq!(after_full_failure.agent_execution, parent.agent_execution);
    let mut partial = store::fork_session(&parent.id, None, None, true).unwrap();
    partial.fork_rewind_prompt_index = Some(0);
    store::update_session_meta(&partial).unwrap();
    let failed = store::clear_session_fork_after_connect_failure(&partial.id).unwrap();
    assert!(failed.agent_session_id.is_none());
    assert!(failed.agent_execution.is_none());
    assert_eq!(current(&fixture).agent_execution, parent.agent_execution);
}

#[tokio::test]
async fn server_owned_and_unavailable_receipts_cannot_read_local_project_media() {
    let fixture = fixture("app_profile");
    let project_media = fixture.root.join("project/images/1.png");
    fs::create_dir_all(project_media.parent().unwrap()).unwrap();
    fs::write(&project_media, "WRONG_LOCAL_PROJECT").unwrap();
    for source in ["remote", "unavailable"] {
        inject(
            &fixture.meta.id,
            receipt("agent-collision", json!({"kind":source})),
        );
        let before = fs::read(paths::sessions_index_file()).unwrap();
        let media = crate::commands::session_resolve_relative_media(
            fixture.meta.id.clone(),
            vec!["images/1.png".into()],
        )
        .await
        .unwrap();
        assert!(media.is_empty(), "{source} read a local project's media");
        assert_eq!(
            cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
            0
        );
        assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
        assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
    }
}

#[tokio::test]
async fn orphaned_receipt_cannot_fall_through_to_local_project_media() {
    let fixture = fixture("app_profile");
    let project_media = fixture.root.join("project/images/1.png");
    fs::create_dir_all(project_media.parent().unwrap()).unwrap();
    fs::write(&project_media, "WRONG_LOCAL_PROJECT").unwrap();
    let index = paths::sessions_index_file();
    let mut rows: Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    let row = rows
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == fixture.meta.id)
        .unwrap();
    row["agentSessionId"] = Value::Null;
    fs::write(&index, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    let before = fs::read(&index).unwrap();
    assert!(crate::commands::session_resolve_relative_media(
        fixture.meta.id.clone(),
        vec!["images/1.png".into()],
    )
    .await
    .unwrap()
    .is_empty());
    assert_eq!(fs::read(index).unwrap(), before);
}

#[tokio::test]
async fn missing_or_unowned_profile_markers_never_fall_back() {
    let fixture = fixture("app_profile");
    let home = fixture
        .actual
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let marker = home.join("execution-profile.json");
    let valid = fs::read(&marker).unwrap();
    fs::rename(&marker, home.join("preserved-marker.json")).unwrap();
    for bad in [
        None,
        Some(b"{}".as_slice()),
        Some(b"not-json".as_slice()),
        Some(valid.as_slice()),
    ] {
        if let Some(bytes) = bad {
            let mut bytes = bytes.to_vec();
            if bytes == valid {
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["ownerSessionId"] = json!(uuid::Uuid::new_v4().to_string());
                bytes = serde_json::to_vec(&value).unwrap();
            }
            fs::write(&marker, bytes).unwrap();
        }
        let before = fs::read(paths::sessions_index_file()).unwrap();
        assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
        assert_eq!(
            cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
            0
        );
        assert!(crate::commands::session_resolve_relative_media(
            fixture.meta.id.clone(),
            vec!["images/1.png".into()],
        )
        .await
        .unwrap()
        .is_empty());
        assert_eq!(fs::read(paths::sessions_index_file()).unwrap(), before);
    }
    fs::write(&marker, vec![b' '; 16 * 1024 + 1]).unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
}

#[test]
fn interrupt_recovery_reads_actual_profile_permission_trail() {
    let fixture = fixture("app_profile");
    fs::write(
        fixture.actual.join("events.jsonl"),
        "{\"type\":\"permission_requested\",\"tool_name\":\"actual_a_tool\"}\n",
    )
    .unwrap();
    assert!(crate::turn_interrupt::heal_interrupted_turn(&fixture.meta.id).is_some());
    let lease = crate::turn_lease::read_lease(&fixture.meta.id).unwrap();
    assert_eq!(lease.pending_tool.unwrap().tool_name, "actual_a_tool");
}

#[test]
fn interrupt_recovery_does_not_use_foreign_global_permission_trail() {
    let fixture = fixture("app_profile");
    fs::write(
        fixture.decoy.join("events.jsonl"),
        "{\"type\":\"permission_requested\",\"tool_name\":\"wrong_global_tool\"}\n",
    )
    .unwrap();
    assert!(crate::turn_interrupt::heal_interrupted_turn(&fixture.meta.id).is_none());
    assert!(store::load_messages(&fixture.meta.id).is_empty());
    assert!(crate::turn_lease::read_lease(&fixture.meta.id).is_none());
}

#[tokio::test]
async fn known_execution_source_does_not_recover_corrupt_global_settings() {
    let fixture = fixture("app_profile");
    let settings = paths::settings_file();
    let corrupt = b"{synthetic-corrupt-global-settings";
    fs::write(&settings, corrupt).unwrap();
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&fixture.meta.id)
            .plan_body
            .as_deref(),
        Some("ACTUAL_A")
    );
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        1
    );
    let media = crate::commands::session_resolve_relative_media(
        fixture.meta.id.clone(),
        vec!["images/1.png".into()],
    )
    .await
    .unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(fs::read(settings).unwrap(), corrupt);
}

#[test]
fn receipt_absent_legacy_rows_keep_existing_global_mode_semantics() {
    let fixture = fixture("legacy_independent");
    inject(&fixture.meta.id, Value::Null);
    assert_eq!(
        plan_chrome::load_agent_plan_snapshot(&fixture.meta.id)
            .plan_body
            .as_deref(),
        Some("WRONG_GLOBAL_B")
    );
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        1
    );
    assert_eq!(
        store::load_messages(&fixture.meta.id)[0].content,
        "WRONG_GLOBAL_B"
    );
}

#[cfg(unix)]
#[test]
fn profile_home_cannot_redirect_to_another_generation_inside_app_storage() {
    let fixture = fixture("app_profile");
    let home = fixture
        .actual
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let preserved = home.with_file_name("preserved-profile");
    fs::rename(home, &preserved).unwrap();
    std::os::unix::fs::symlink(&preserved, home).unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        0
    );
}

#[cfg(unix)]
#[test]
fn sessions_root_cannot_redirect_outside_execution_home() {
    let fixture = fixture("legacy_independent");
    let sessions = fixture.actual.parent().unwrap().parent().unwrap();
    let preserved = fixture.root.join("preserved-sessions");
    fs::rename(sessions, &preserved).unwrap();
    std::os::unix::fs::symlink(&preserved, sessions).unwrap();
    assert!(!plan_chrome::load_agent_plan_snapshot(&fixture.meta.id).found);
    assert_eq!(
        cli_sessions::try_reconcile_linked_session(&fixture.meta.id),
        0
    );
}
