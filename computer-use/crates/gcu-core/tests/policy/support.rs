//! Shared fixtures for policy tests.

pub(crate) use async_trait::async_trait;
pub(crate) use gcu_core::driver::{AppQuery, Cancel, Driver, TreeOpts, WindowRef};
pub(crate) use gcu_core::policy::{
    revalidate, ActionIntent, ApprovalBinding, ConfirmKind, GrantTable, Invalidated, PolicyCtx,
    PolicyDecision, PolicyEngine, PolicyWindow, RouteLimit, Tool,
};
pub(crate) use gcu_core::session::{InputRelease, Session};
pub(crate) use gcu_core::tree::{Pipeline, ProcessedNode, ProcessedTree};
pub(crate) use gcu_driver_fake::FakeDriver;
pub(crate) use gcu_host_api::testing::{FakeUserActivity, ScriptedApprovals};
pub(crate) use gcu_host_api::{
    ActionSummary, AppIdentity, AppInfo, ApprovalDecision, ConfirmationLevel, ForegroundPolicy,
    HandoffReason, Platform, PolicyConfig, Risk, SignatureState,
};
pub(crate) use std::sync::Arc;
pub(crate) use std::time::{Duration, Instant};

pub(crate) struct NopRelease;

#[async_trait]
impl InputRelease for NopRelease {
    async fn release_all_input(&self) {}
}

pub(crate) fn engine() -> PolicyEngine {
    PolicyEngine::new()
}

pub(crate) fn config() -> PolicyConfig {
    PolicyConfig {
        allow_foreground: ForegroundPolicy::Always,
        ..PolicyConfig::default()
    }
}

pub(crate) fn grants_for(app: &AppInfo) -> GrantTable {
    let mut grants = GrantTable::default();
    grants.record_session(app);
    grants
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval(
    app: &AppInfo,
    config: &PolicyConfig,
    grants: &GrantTable,
    activity: &FakeUserActivity,
    window: Option<&PolicyWindow>,
    tree: Option<&ProcessedTree>,
    intent: &ActionIntent,
    closed: bool,
) -> PolicyDecision {
    engine().evaluate(
        &PolicyCtx {
            session_id: "s",
            app,
            window,
            tree,
            activity,
            config,
            grants,
            policy_version: 7,
            closed,
            browser: None,
        },
        intent,
    )
}

pub(crate) fn identity(
    platform: Platform,
    key: &str,
    image: &str,
    sha256: Option<[u8; 32]>,
    signature: SignatureState,
    team_id: Option<&str>,
    publisher: Option<&str>,
) -> AppIdentity {
    AppIdentity {
        platform,
        key: key.into(),
        team_id: team_id.map(str::to_string),
        publisher: publisher.map(str::to_string),
        sha256,
        signature,
        image_path: image.into(),
    }
}

pub(crate) fn app(name: &str, identity: AppIdentity) -> AppInfo {
    AppInfo {
        name: name.into(),
        pid: Some(42),
        identity,
        icon: None,
    }
}

pub(crate) fn notes() -> AppInfo {
    app(
        "Notes",
        identity(
            Platform::Linux,
            "org.example.Notes",
            "/usr/bin/notes",
            Some([0x11; 32]),
            SignatureState::Unsigned,
            None,
            None,
        ),
    )
}

pub(crate) fn macos_notes() -> AppInfo {
    app(
        "Notes",
        identity(
            Platform::Macos,
            "com.example.notes",
            "/Applications/Notes.app/Contents/MacOS/Notes",
            Some([0x11; 32]),
            SignatureState::Valid,
            Some("TEAM123456"),
            None,
        ),
    )
}

pub(crate) fn windows_notes() -> AppInfo {
    app(
        "Notes",
        identity(
            Platform::Windows,
            "C:\\Program Files\\Notes\\Notes.exe",
            "C:\\Program Files\\Notes\\Notes.exe",
            Some([0x22; 32]),
            SignatureState::Valid,
            None,
            Some("CN=Example Publisher"),
        ),
    )
}

pub(crate) fn refused(decision: &PolicyDecision) -> Option<gcu_protocol::ErrorCode> {
    match decision {
        PolicyDecision::Refuse(code, _) => Some(*code),
        _ => None,
    }
}

pub(crate) fn reason(decision: &PolicyDecision) -> &str {
    match decision {
        PolicyDecision::Refuse(_, reason) => reason,
        _ => "",
    }
}

pub(crate) async fn scene(name: &str, title: &str) -> (PolicyWindow, ProcessedTree) {
    let driver = FakeDriver::from_scene(name).unwrap();
    let app_ref = driver
        .resolve_app(&AppQuery(format!("fake.{name}")))
        .await
        .unwrap();
    let windows = driver.list_windows(Some(&app_ref)).await.unwrap();
    let info = windows.iter().find(|window| window.title == title).unwrap();
    let raw = driver
        .snapshot_tree(
            &WindowRef {
                app: app_ref,
                id: info.id.clone(),
            },
            &TreeOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap();
    let tree = Pipeline::default().run(raw, &TreeOpts::default());
    (
        PolicyWindow {
            id: info.id.clone(),
            title: info.title.clone(),
            url: None,
        },
        tree,
    )
}

pub(crate) fn node<'a>(tree: &'a ProcessedTree, id: &str) -> &'a ProcessedNode {
    tree.nodes()
        .into_iter()
        .map(|(_, node)| node)
        .find(|node| node.node.identifier.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("missing node {id}"))
}

pub(crate) fn center(node: &ProcessedNode) -> (f64, f64) {
    let frame = node.node.frame.unwrap();
    (frame.x + frame.w / 2.0, frame.y + frame.h / 2.0)
}

pub(crate) fn click(index: u32) -> ActionIntent {
    let mut intent = ActionIntent::new(Tool::Click);
    intent.element_index = Some(index);
    intent.snapshot_id = "snap".into();
    intent.action_digest = [3; 32];
    intent.parameters = "click".into();
    intent
}

pub(crate) async fn labeled_button(name: &str) -> (PolicyWindow, ProcessedTree) {
    let source = format!(
        r#"(apps: [(id: "fake.labels", name: "Fixture labels", executable: "/fixtures/labels", pid: 7,
windows: [(id: "labels", title: "Labels", frame: (x: 0.0, y: 0.0, w: 400.0, h: 200.0), scale: 1.0,
root: Some((id: "root", role: window, states: "ENABLED", name: Some("Labels"),
frame: Some((x: 0.0, y: 0.0, w: 400.0, h: 200.0)),
children: [(id: "btn", role: button, states: "ENABLED", name: Some("{name}"),
frame: Some((x: 20.0, y: 20.0, w: 140.0, h: 32.0)), actions: [press])])))])])"#
    );
    let driver = FakeDriver::from_ron(&source).unwrap();
    let app_ref = driver
        .resolve_app(&AppQuery("fake.labels".into()))
        .await
        .unwrap();
    let info = driver.key_window(&app_ref).await.unwrap();
    let raw = driver
        .snapshot_tree(
            &WindowRef {
                app: app_ref,
                id: info.id.clone(),
            },
            &TreeOpts::default(),
            &Cancel::default(),
        )
        .await
        .unwrap();
    (
        PolicyWindow {
            id: info.id,
            title: info.title,
            url: None,
        },
        Pipeline::default().run(raw, &TreeOpts::default()),
    )
}
