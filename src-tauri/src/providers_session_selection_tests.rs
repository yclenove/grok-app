//! The public writer must select real TOML tables, never text inside values.
use crate::session_migration::test_home::IsolatedAppHome;
use crate::{paths, store};
use std::fs;

const REAL: &str = "[model.real]\nmodel = \"target\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\n";
const FAKE: &str = "[model.fake]\nmodel = \"target\"\nbase_url = \"https://synthetic.invalid/v1\"\napi_key = \"synthetic-not-a-credential\"\n";

fn fixture(config: &str) -> (IsolatedAppHome, store::Project, store::SessionMeta) {
    let home = IsolatedAppHome::acquire_with_user_home("provider-selection-toml");
    let directory = home.path.join("project");
    fs::create_dir(&directory).unwrap();
    let project = store::add_project(directory.to_string_lossy().into_owned(), true).unwrap();
    let mut settings = store::load_settings();
    settings.locale = "zh".into();
    store::save_settings(&settings).unwrap();
    let session =
        store::create_session(Some(project.id.clone()), Some("中文会话".into()), false).unwrap();
    fs::write(paths::agent_config_toml(), config).unwrap();
    (home, project, session)
}

fn files() -> Vec<Vec<u8>> {
    [
        paths::settings_file(),
        paths::projects_file(),
        paths::sessions_index_file(),
        paths::agent_config_toml(),
    ]
    .iter()
    .map(|path| fs::read(path).unwrap())
    .collect()
}

fn select(project: &str, session: &str, provider: &str) -> Result<store::ComposerPrefs, String> {
    store::save_composer_prefs_with_route(
        Some(project),
        Some(session),
        Some("target".into()),
        Some("medium".into()),
        None,
        None,
        Some(provider.into()),
    )
}

fn rejects_fake(config: &str, provider: &str) {
    let (_home, project, session) = fixture(config);
    let before = files();
    let result = select(&project.id, &session.id, provider);
    assert!(
        result.is_err(),
        "non-table or ambiguous route must be rejected"
    );
    let error = result.err().unwrap();
    assert!(!error.contains("synthetic-not-a-credential"));
    assert!(!error.contains("synthetic.invalid"));
    assert_eq!(
        files(),
        before,
        "rejection must not mutate any source or preference file"
    );
}

fn literal_multiline_is_not_a_provider() {
    rejects_fake(&format!("note = '''\n{FAKE}'''\n{REAL}"), "fake");
}

fn basic_multiline_is_not_a_provider() {
    rejects_fake(&format!("note = \"\"\"\n{FAKE}\"\"\"\n{REAL}"), "fake");
}

fn array_multiline_is_not_a_provider() {
    rejects_fake(&format!("notes = [\n'''\n{FAKE}''',\n]\n{REAL}"), "fake");
}

#[test]
fn reject_literal_multiline_route_at_public_writer() {
    literal_multiline_is_not_a_provider();
}

#[test]
fn reject_basic_multiline_route_at_public_writer() {
    basic_multiline_is_not_a_provider();
}

#[test]
fn reject_array_multiline_route_at_public_writer() {
    array_multiline_is_not_a_provider();
}

#[test]
fn incompatible_auth_policy_is_rejected_before_the_public_preference_write() {
    for policy in [
        "[auth]\ndisable_api_key_auth = true\n",
        "[grok_com_config]\ndisable_api_key_auth = true\n",
        "[auth]\nforce_login_team_uuid = ['synthetic-private-team']\n",
        "[auth]\npreferred_method = 'future-method'\n",
    ] {
        rejects_fake(&format!("{policy}{REAL}"), "real");
    }
}

#[test]
fn nested_table_is_not_a_dotted_provider_id() {
    rejects_fake(
        &format!(
            "{}\n{REAL}",
            FAKE.replace("[model.fake]", "[model.fake.extra_headers]")
        ),
        "fake.extra_headers",
    );
}

#[test]
fn duplicate_route_fields_fail_closed_without_mutation() {
    for duplicate in [
        "model = \"target\"",
        "api_key = \"synthetic-not-a-credential\"",
        "base_url = \"https://synthetic.invalid/v1\"",
    ] {
        rejects_fake(&format!("{REAL}{duplicate}\n"), "real");
    }
}

#[test]
fn actual_table_after_multiline_value_still_selects() {
    let (_home, project, session) = fixture(&format!("note = '''\n{FAKE}'''\n{REAL}"));
    let before = files();
    let selected = select(&project.id, &session.id, "real").unwrap();
    assert_eq!(selected.model_id, "target");
    let after = files();
    assert_eq!(after[..2], before[..2]);
    assert_eq!(after[3], before[3]);
    let stored = store::load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == session.id)
        .unwrap();
    assert_eq!(stored.provider_id.as_deref(), Some("real"));
    assert_eq!(stored.model_id.as_deref(), Some("target"));
}

#[test]
fn quoted_keys_comments_and_nested_headers_keep_the_real_route() {
    let config = "[ \"model\" . 'real.channel' ] # route\n\"model\" = \"target\" # request\n'base_url' = 'https://synthetic.invalid/v1' # endpoint\napi_key = \"synthetic-not-a-credential\" # key\n[model.'real.channel'.extra_headers]\n'X-Note' = '''\n[model.fake]\n'''\n";
    let (_home, project, session) = fixture(config);
    let before = files();
    select(&project.id, &session.id, "real.channel").unwrap();
    let after = files();
    assert_eq!(after[..2], before[..2]);
    assert_eq!(after[3], before[3]);
}

#[test]
fn missing_inherited_model_provider_is_rejected_at_the_public_writer() {
    rejects_fake(
        &format!("{REAL}model_provider = 'deleted.gateway'\n"),
        "real",
    );
}

#[test]
fn missing_named_auth_helpers_are_rejected_at_the_public_writer() {
    rejects_fake(&format!("{REAL}auth_provider = 'deleted.helper'\n"), "real");
    rejects_fake(
        &format!("{REAL}model_provider = 'gateway'\n[model_providers.gateway]\nauth_provider = 'deleted.helper'\n"),
        "real",
    );
}

#[test]
fn ambiguous_reference_types_are_rejected_without_preference_writes() {
    for field in ["model_provider", "auth_provider"] {
        for value in ["42", "['gateway']", "{name = 'gateway'}", "' '"] {
            rejects_fake(&format!("{REAL}{field} = {value}\n"), "real");
        }
    }
}

#[test]
fn explicit_provider_and_auth_references_keep_the_selected_owner() {
    let config = format!("{REAL}model_provider = 'gateway.with.dot'\nauth_provider = 'model-helper'\n[model_providers.'gateway.with.dot']\nauth_provider = 'provider-helper'\n[model_providers.'gateway.with.dot'.extra_headers]\n'X-Tenant' = 'synthetic-header'\n[auth_provider.model-helper]\ncommand = ['synthetic-helper', '--model']\n[auth_provider.provider-helper]\ncommand = 'synthetic-never-executed'\n");
    let (_home, project, session) = fixture(&config);
    let before = files();
    select(&project.id, &session.id, "real").unwrap();
    let after = files();
    assert_eq!(after[..2], before[..2]);
    assert_eq!(after[3], before[3]);
    let stored = store::load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == session.id)
        .unwrap();
    assert_eq!(stored.provider_id.as_deref(), Some("real"));
    assert_eq!(stored.model_id.as_deref(), Some("target"));
}

#[test]
fn combined_dotted_and_inline_nesting_is_bounded_before_the_public_write() {
    let path = std::iter::repeat_n("part", 16)
        .collect::<Vec<_>>()
        .join(".");
    let mut value = "'synthetic-value'".to_string();
    for _ in 0..5 {
        value = format!("{{{path} = {value}}}");
    }
    // Each individual key path/value is below the lexical parser limit, but
    // their combined structural depth must be rejected before the store write.
    rejects_fake(&format!("unknown = {value}\n{REAL}"), "real");
}

crate::session_migration::test_home::fixture_home_variants_contract_tests!(
    literal_multiline_is_not_a_provider,
    basic_multiline_is_not_a_provider,
    array_multiline_is_not_a_provider,
);

#[test]
fn official_selection_validates_runtime_structure_before_the_public_write() {
    rejects_fake("auth = 'synthetic-not-a-credential'\n", "official");
}

#[test]
fn official_selection_validates_its_separate_auth_source_before_the_public_write() {
    let (_home, project, session) = fixture(REAL);
    let official_home = crate::process_util::user_home().join(".grok");
    fs::create_dir_all(&official_home).unwrap();
    let path = official_home.join("config.toml");
    let source = b"auth = 'synthetic-not-a-credential'\n";
    fs::write(&path, source).unwrap();
    let before = files();
    let result = select(&project.id, &session.id, "official");
    assert!(
        result.is_err(),
        "official authentication structure must be validated"
    );
    let error = result.err().unwrap();
    assert!(!error.contains("synthetic-not-a-credential"));
    assert_eq!(files(), before);
    assert_eq!(fs::read(path).unwrap(), source);
}

#[test]
fn official_choice_preserves_both_sources_and_does_not_activate_global_route() {
    let (_home, project, session) = fixture(&format!(
        "[models]\ndefault = 'real'\n{REAL}[auth]\npreferred_method = 'api_key'\n"
    ));
    let official_home = crate::process_util::user_home().join(".grok");
    fs::create_dir(&official_home).unwrap();
    let path = official_home.join("config.toml");
    let policy = b"[auth]\npreferred_method = 'oidc'\ndisable_api_key_auth = true\n";
    fs::write(&path, policy).unwrap();
    let before = files();
    let result = select(&project.id, &session.id, "official").unwrap();
    assert_eq!(result.model_id, "target");
    let after = files();
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], before[1]);
    assert_eq!(after[3], before[3]);
    assert_eq!(fs::read(path).unwrap(), policy);
    let stored = store::load_sessions_snapshot()
        .unwrap()
        .into_iter()
        .find(|row| row.id == session.id)
        .unwrap();
    assert_eq!(stored.provider_id.as_deref(), Some("official"));
    assert_eq!(stored.agent_session_id, session.agent_session_id);
}
