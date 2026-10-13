//! RON fixture descriptions. Loading parses caller-provided data; no filesystem IO.

use gcu_core::{
    driver::{AppRef, WindowRef},
    fingerprint::refresh_tree,
    model::*,
};
use gcu_protocol::{types::AppIdentity, AppInfo, WindowInfo};
use serde::Deserialize;
use std::collections::HashSet;
use thiserror::Error;

/// Scene parse/validation errors, without echoing potentially sensitive payloads.
#[derive(Debug, Error)]
pub enum SceneError {
    /// Malformed RON document.
    #[error("invalid RON scene")]
    Parse(#[from] ron::error::SpannedError),
    /// Invalid/duplicate identities, geometry, or excessive fixture size.
    #[error("invalid scene: {0}")]
    Invalid(&'static str),
}

/// Desktop containing apps and windows; consumes no OS resources.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Fixture apps.
    pub apps: Vec<SceneApp>,
}
/// App fixture and its windows.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneApp {
    /// Platform-like stable ID.
    pub id: String,
    /// Discoverable display name.
    pub name: String,
    /// Synthetic canonical executable path.
    pub executable: String,
    /// Synthetic PID.
    pub pid: u32,
    /// App windows in key-window order.
    pub windows: Vec<SceneWindow>,
}
/// Window fixture; root=None models a canvas without accessibility.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneWindow {
    /// Stable window ID.
    pub id: String,
    /// Window title.
    pub title: String,
    /// Screen frame in physical pixels.
    pub frame: Rect,
    /// Display scale.
    pub scale: f64,
    /// Optional accessibility root.
    pub root: Option<SceneNode>,
}
/// Raw-node fixture. Repetition expands list rows without thousands of file lines.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneNode {
    /// Unique identifier within the window; {i} expands in repeated nodes.
    pub id: String,
    /// Normalized role.
    pub role: Role,
    /// Native role (terminal/console can be retained with Role::Unknown).
    #[serde(default)]
    pub native_role: String,
    /// Accessible name.
    #[serde(default)]
    pub name: Option<String>,
    /// Initial value.
    #[serde(default)]
    pub value: Option<NodeValue>,
    /// State flags using RON's human-readable bitflags representation.
    #[serde(default)]
    pub states: States,
    /// Advertised actions.
    #[serde(default)]
    pub actions: Vec<NodeAction>,
    /// Window-relative frame in physical pixels.
    #[serde(default)]
    pub frame: Option<Rect>,
    /// Link/document URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Native child order.
    #[serde(default)]
    pub children: Vec<SceneNode>,
    /// Number of copies; names and IDs replace {i} with the zero-based row.
    #[serde(default = "one")]
    pub repeat: usize,
}
fn one() -> usize {
    1
}

/// Internal fake native handle; world identity prevents cross-driver handle reuse.
#[derive(Debug, Clone)]
pub(crate) struct Handle {
    pub world: u64,
    pub window: String,
    pub id: String,
}

impl Scene {
    /// Parse and validate a scene, synchronously, with no side effects.
    pub fn from_ron(source: &str) -> Result<Self, SceneError> {
        let scene: Self = ron::from_str(source)?;
        let mut apps = HashSet::new();
        let mut windows = HashSet::new();
        for app in &scene.apps {
            if app.id.is_empty() || !apps.insert(&app.id) {
                return Err(SceneError::Invalid("duplicate or empty app ID"));
            }
            for win in &app.windows {
                if win.id.is_empty() || !windows.insert(&win.id) {
                    return Err(SceneError::Invalid("duplicate or empty window ID"));
                }
                if !valid_rect(win.frame)
                    || win.frame.w < 1.0
                    || win.frame.h < 1.0
                    || win.frame.w > 4096.0
                    || win.frame.h > 4096.0
                    || !win.scale.is_finite()
                    || win.scale <= 0.0
                    || win.frame.w.fract() != 0.0
                    || win.frame.h.fract() != 0.0
                {
                    return Err(SceneError::Invalid("invalid physical window geometry"));
                }
                if let Some(root) = &win.root {
                    let mut ids = HashSet::new();
                    let mut count = 0;
                    validate_node(root, 0, &mut count, &mut ids)?;
                    if root.repeat != 1 {
                        return Err(SceneError::Invalid("root repetition"));
                    }
                }
            }
        }
        Ok(scene)
    }
}
pub(crate) fn valid_rect(frame: Rect) -> bool {
    [frame.x, frame.y, frame.w, frame.h]
        .iter()
        .all(|v| v.is_finite())
        && frame.w >= 0.0
        && frame.h >= 0.0
}
fn validate_node(
    node: &SceneNode,
    depth: usize,
    count: &mut usize,
    ids: &mut HashSet<String>,
) -> Result<(), SceneError> {
    if depth > 64 || node.repeat == 0 || node.repeat > 5000 {
        return Err(SceneError::Invalid("node depth or repetition limit"));
    }
    for i in 0..node.repeat {
        *count += 1;
        if *count > 5000 {
            return Err(SceneError::Invalid("node limit"));
        }
        let id = node.id.replace("{i}", &i.to_string());
        if id.is_empty() || !ids.insert(id) {
            return Err(SceneError::Invalid("duplicate or empty node ID"));
        }
        if node.frame.is_some_and(|r| !valid_rect(r)) {
            return Err(SceneError::Invalid("invalid node frame"));
        }
        if let Some(value) = &node.value {
            let valid = match value {
                NodeValue::Number(value) => value.is_finite(),
                NodeValue::Range { value, min, max } => {
                    value.is_finite()
                        && min.is_finite()
                        && max.is_finite()
                        && min <= value
                        && value <= max
                }
                _ => true,
            };
            if !valid {
                return Err(SceneError::Invalid("invalid numeric node value"));
            }
        }
        for child in &node.children {
            validate_node(child, depth + 1, count, ids)?;
        }
    }
    Ok(())
}

pub(crate) fn app_ref(app: &SceneApp) -> AppRef {
    AppRef {
        info: AppInfo {
            id: app.id.clone(),
            name: app.name.clone(),
            pid: Some(app.pid),
            identity: Some(AppIdentity {
                id: app.id.clone(),
                executable: Some(app.executable.clone()),
                team_id: None,
                publisher: None,
                code_hash: None,
            }),
        },
    }
}
pub(crate) fn window_ref(app: &AppRef, win: &SceneWindow) -> WindowRef {
    WindowRef {
        app: app.clone(),
        id: win.id.clone(),
    }
}
pub(crate) fn window_info(win: &SceneWindow) -> WindowInfo {
    WindowInfo {
        id: win.id.clone(),
        title: win.title.clone(),
        frame: win.frame,
        scale: win.scale,
        on_current_desktop: None,
    }
}
pub(crate) fn raw_node(node: &SceneNode, world: u64, window: &str, i: usize) -> RawNode {
    let id = node.id.replace("{i}", &i.to_string());
    let mut raw = RawNode {
        role: node.role,
        native_role: if node.native_role.is_empty() {
            format!("fake::{:?}", node.role).into()
        } else {
            node.native_role.as_str().into()
        },
        subrole: None,
        name: node.name.as_ref().map(|s| s.replace("{i}", &i.to_string())),
        value: node.value.clone(),
        description: None,
        placeholder: None,
        help: None,
        identifier: Some(id.clone()),
        states: node.states,
        actions: node.actions.iter().cloned().collect(),
        frame: node.frame,
        text_extras: None,
        url: node.url.clone(),
        children: node
            .children
            .iter()
            .flat_map(|child| (0..child.repeat).map(|j| raw_node(child, world, window, j)))
            .collect(),
        handle: NativeHandle::new(Handle {
            world,
            window: window.into(),
            id,
        }),
        fingerprint: Fingerprint::default(),
    };
    refresh_tree(&mut raw, Fingerprint::default(), 0);
    raw
}

/// Built-in offline fixture sources. No file access is needed by tests or engines.
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "form" => Some(include_str!("../scenes/form.ron")),
        "list" => Some(include_str!("../scenes/list.ron")),
        "modal" => Some(include_str!("../scenes/modal.ron")),
        "chat" => Some(include_str!("../scenes/chat.ron")),
        "webarea" => Some(include_str!("../scenes/webarea.ron")),
        "canvas" => Some(include_str!("../scenes/canvas.ron")),
        "terminal" => Some(include_str!("../scenes/terminal.ron")),
        "login" => Some(include_str!("../scenes/login.ron")),
        "ide" => Some(include_str!("../scenes/ide.ron")),
        "web_terminal" => Some(include_str!("../scenes/web_terminal.ron")),
        "submit_unknown" => Some(include_str!("../scenes/submit_unknown.ron")),
        _ => None,
    }
}
