//! Real store transactions in a synthetic home. A runtime observation is not a
//! new composer intent, even if its row was captured before the connection.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

type Publish = fn(&SessionMeta) -> Result<(), String>;

fn fixture(scope: &str) -> (IsolatedAppHome, Project, SessionMeta) {
    let home = IsolatedAppHome::acquire("composer-runtime-intent");
    let path = home.path.join("project");
    fs::create_dir(&path).unwrap();
    let project = add_project(path.to_string_lossy().into_owned(), true).unwrap();
    let mut settings = load_settings();
    settings.composer_prefs_scope = scope.into();
    // Avoid default migrations determining any of these assertions.
    settings.model_id = Some("grok-4.6".into());
    settings.effort = Some("low".into());
    settings.mode = "agent".into();
    settings.permission_policy = "ask".into();
    save_settings(&settings).unwrap();
    let mut row = create_session(Some(project.id.clone()), None, false).unwrap();
    row.model_id = Some("grok-4.6".into());
    row.effort = Some("low".into());
    row.mode = Some("agent".into());
    row.permission_policy = Some("ask".into());
    update_session_meta(&row).unwrap();
    (home, project, row)
}

fn stored(id: &str) -> SessionMeta {
    load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == id)
        .unwrap()
}

fn assert_preserves_field(publish: Publish, field: &str) {
    let (_home, project, old) = fixture("session");
    let values = match field {
        "model" => (Some("grok-4.5".into()), None, None, None),
        "effort" => (None, Some("medium".into()), None, None),
        "mode" => (None, None, Some("ask".into()), None),
        "policy" => (None, None, None, Some("accept_edits".into())),
        _ => unreachable!(),
    };
    save_composer_prefs(
        Some(&project.id),
        Some(&old.id),
        values.0,
        values.1,
        values.2,
        values.3,
    )
    .unwrap();
    let expected = stored(&old.id);
    publish(&old).unwrap();
    let actual = stored(&old.id);
    assert_eq!(
        actual.model_id, expected.model_id,
        "{field}: model reverted"
    );
    assert_eq!(actual.effort, expected.effort, "{field}: effort reverted");
    assert_eq!(actual.mode, expected.mode, "{field}: mode reverted");
    assert_eq!(
        actual.permission_policy, expected.permission_policy,
        "{field}: policy reverted"
    );
}

macro_rules! intent_test {
    ($name:ident, $publish:path, $field:literal) => {
        #[test]
        fn $name() {
            assert_preserves_field($publish, $field);
        }
    };
}

intent_test!(runtime_model, update_runtime_session_meta, "model");
intent_test!(connected_model, update_connected_session_meta, "model");
intent_test!(runtime_effort, update_runtime_session_meta, "effort");
intent_test!(connected_effort, update_connected_session_meta, "effort");
intent_test!(runtime_mode, update_runtime_session_meta, "mode");
intent_test!(connected_mode, update_connected_session_meta, "mode");
intent_test!(runtime_policy, update_runtime_session_meta, "policy");
intent_test!(connected_policy, update_connected_session_meta, "policy");

fn assert_memory_scope(scope: &str) {
    let (_home, project, old) = fixture(scope);
    let expected = save_composer_prefs(
        Some(&project.id),
        Some(&old.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        Some("ask".into()),
        Some("accept_edits".into()),
    )
    .unwrap();
    for publish in [update_runtime_session_meta, update_connected_session_meta] {
        publish(&old).unwrap();
        let actual = resolve_composer_prefs(Some(&project.id), Some(&old.id));
        assert_eq!(actual.model_id, expected.model_id, "{scope}: model");
        assert_eq!(actual.effort, expected.effort, "{scope}: effort");
        assert_eq!(actual.mode, expected.mode, "{scope}: mode");
        assert_eq!(
            actual.permission_policy, expected.permission_policy,
            "{scope}: policy"
        );
    }
}

#[test]
fn latest_global_intent_survives_late_callbacks() {
    assert_memory_scope("global");
}

#[test]
fn latest_project_intent_survives_late_callbacks() {
    assert_memory_scope("project");
}

#[test]
fn latest_session_intent_survives_late_callbacks() {
    assert_memory_scope("session");
}

#[test]
fn passive_runtime_does_not_turn_inherited_choices_into_explicit_overrides() {
    let (_home, project, old) = fixture("session");
    let inherited = create_session(Some(project.id), None, false).unwrap();
    let mut observation = inherited.clone();
    observation.model_id = old.model_id;
    observation.effort = old.effort;
    observation.mode = old.mode;
    observation.permission_policy = old.permission_policy;
    update_runtime_session_meta(&observation).unwrap();
    let actual = stored(&inherited.id);
    assert!(actual.model_id.is_none());
    assert!(actual.effort.is_none());
    assert!(actual.mode.is_none());
    assert!(actual.permission_policy.is_none());
}

#[test]
fn valid_connection_still_publishes_identity_and_runtime_progress() {
    let (_home, _, mut row) = fixture("session");
    row.agent_session_id = Some("synthetic-new-agent".into());
    row.title = "Synthetic runtime title".into();
    row.updated_at += chrono::Duration::seconds(1);
    update_connected_session_meta(&row).unwrap();
    let actual = stored(&row.id);
    assert_eq!(actual.agent_session_id, row.agent_session_id);
    assert_eq!(actual.title, row.title);
    assert_eq!(actual.updated_at, row.updated_at);
    row.title = "Later runtime title".into();
    row.agent_session_id = Some("stale-other-agent".into());
    update_runtime_session_meta(&row).unwrap();
    let actual = stored(&row.id);
    assert_eq!(actual.title, row.title);
    assert_eq!(
        actual.agent_session_id.as_deref(),
        Some("synthetic-new-agent")
    );
}

#[test]
fn directory_change_and_new_intent_both_survive_old_connection() {
    let (_home, project, mut old) = fixture("session");
    old.agent_session_id = Some("stale-bound-agent".into());
    save_composer_prefs(
        Some(&project.id),
        Some(&old.id),
        Some("grok-4.5".into()),
        None,
        None,
        None,
    )
    .unwrap();
    set_session_project(&old.id, None).unwrap();
    let expected = stored(&old.id);
    update_connected_session_meta(&old).unwrap();
    let actual = stored(&old.id);
    assert_eq!(actual.project_id, expected.project_id);
    assert_eq!(actual.agent_session_id, expected.agent_session_id);
    assert_eq!(actual.model_id, expected.model_id);
}

#[test]
fn deleted_target_cannot_be_recreated_by_either_callback() {
    let (_home, _, old) = fixture("session");
    delete_session(&old.id).unwrap();
    for publish in [update_runtime_session_meta, update_connected_session_meta] {
        assert!(publish(&old).unwrap_err().contains("session not found"));
        assert!(load_sessions_snapshot()
            .unwrap()
            .iter()
            .all(|row| row.id != old.id));
        assert!(!crate::paths::session_dir(&old.id).exists());
    }
}

#[test]
fn first_connection_remembers_effort_without_freezing_other_defaults() {
    let (_home, project, _) = fixture("global");
    let row = create_session(Some(project.id), None, false).unwrap();
    remember_initial_session_effort(&row.id, "low").unwrap();
    let mut settings = load_settings();
    settings.effort = Some("medium".into());
    save_settings(&settings).unwrap();
    let actual = stored(&row.id);
    assert_eq!(actual.effort.as_deref(), Some("low"));
    assert!(actual.model_id.is_none());
    assert!(actual.mode.is_none());
    assert!(actual.permission_policy.is_none());
    assert_eq!(resolve_composer_prefs(None, Some(&row.id)).effort, "low");
}

#[test]
fn initial_effort_seed_cannot_overwrite_a_saved_choice() {
    let (_home, _, row) = fixture("session");
    save_composer_prefs(None, Some(&row.id), None, Some("medium".into()), None, None).unwrap();
    remember_initial_session_effort(&row.id, "low").unwrap();
    assert_eq!(stored(&row.id).effort.as_deref(), Some("medium"));
}

#[test]
fn active_mode_and_permission_intents_do_not_replay_other_fields() {
    let (_home, _, row) = fixture("session");
    save_composer_prefs(
        None,
        Some(&row.id),
        Some("grok-4.5".into()),
        Some("medium".into()),
        None,
        None,
    )
    .unwrap();
    update_session_mode_intent(&row.id, "plan".into()).unwrap();
    save_composer_prefs(
        None,
        Some(&row.id),
        None,
        None,
        None,
        Some("accept_edits".into()),
    )
    .unwrap();
    update_runtime_session_meta(&row).unwrap();
    let actual = stored(&row.id);
    assert_eq!(actual.model_id.as_deref(), Some("grok-4.5"));
    assert_eq!(actual.effort.as_deref(), Some("medium"));
    assert_eq!(actual.mode.as_deref(), Some("plan"));
    assert_eq!(actual.permission_policy.as_deref(), Some("accept_edits"));
}

#[test]
fn effort_seed_and_active_intent_cannot_revive_a_deleted_chat() {
    let (_home, _, row) = fixture("session");
    delete_session(&row.id).unwrap();
    assert!(remember_initial_session_effort(&row.id, "low")
        .unwrap_err()
        .contains("session not found"));
    assert!(update_session_mode_intent(&row.id, "plan".into())
        .unwrap_err()
        .contains("session not found"));
    assert!(load_sessions_snapshot()
        .unwrap()
        .iter()
        .all(|other| other.id != row.id));
    assert!(!crate::paths::session_dir(&row.id).exists());
}
