//! Admission checks. These run before a wrapped driver touches content.
//! None of them deliver input or capture pixels.

use super::model::{FixtureScope, InstanceEvidence, WindowBinding};
use crate::driver::{AppRef, DriverAction, DriverError, WindowRef};
use crate::model::{NativeHandle, RawNode, RawTree};
use gcu_protocol::AppInfo;

pub(super) struct SealedHandle {
    pub run_id: String,
    pub generation: u64,
    pub window: WindowBinding,
    pub inner: NativeHandle,
}

pub(super) struct SealEntry {
    generation: u64,
    run_id: String,
    window: WindowBinding,
    inner: NativeHandle,
    sealed: NativeHandle,
}

pub(super) struct PostCheck {
    window: Option<(WindowRef, WindowBinding)>,
    handle: Option<(NativeHandle, WindowBinding)>,
}

pub(super) fn admit_app(
    scope: &FixtureScope,
    evidence: &impl InstanceEvidence,
    app: &AppRef,
) -> Result<super::model::ProcessInstance, DriverError> {
    let instance = evidence.app_instance(app).ok_or(DriverError::ScopeDenied)?;
    if scope.admits_instance(&instance) {
        Ok(instance)
    } else {
        Err(DriverError::ScopeDenied)
    }
}

pub(super) fn admit_window(
    scope: &FixtureScope,
    evidence: &impl InstanceEvidence,
    window: &WindowRef,
) -> Result<WindowBinding, DriverError> {
    let binding = evidence
        .window_binding(window)
        .ok_or(DriverError::ScopeDenied)?;
    if scope.admits_window(&binding) {
        Ok(binding)
    } else {
        Err(DriverError::ScopeDenied)
    }
}

pub(super) fn scrub(err: DriverError) -> DriverError {
    match err {
        DriverError::Native { maybe_executed, .. } => DriverError::Native {
            message: "native driver failure".into(),
            maybe_executed,
        },
        DriverError::PermissionMissing { permission } if !permission_ok(&permission) => {
            DriverError::ScopeDenied
        }
        other => other,
    }
}

fn permission_ok(permission: &str) -> bool {
    !permission.is_empty()
        && permission.len() <= 64
        && !permission.contains('/')
        && !permission.chars().any(char::is_whitespace)
}

pub(super) fn matches_query(app: &AppInfo, query: &str) -> bool {
    let executable = app
        .identity
        .as_ref()
        .and_then(|id| id.executable.as_deref());
    app.id == query
        || app.name == query
        || executable == Some(query)
        || executable
            .and_then(|exe| std::path::Path::new(exe).file_name())
            .and_then(|name| name.to_str())
            == Some(query)
}

pub(super) fn filter_windows<E: InstanceEvidence>(
    scope: &FixtureScope,
    evidence: &E,
    app: &AppRef,
    windows: Vec<gcu_protocol::WindowInfo>,
) -> Vec<gcu_protocol::WindowInfo> {
    windows
        .into_iter()
        .filter(|info| {
            let window = WindowRef {
                app: app.clone(),
                id: info.id.clone(),
            };
            admit_window(scope, evidence, &window).is_ok()
        })
        .collect()
}

pub(super) fn prepare_cache(cache: &mut Vec<SealEntry>, scope: &FixtureScope) {
    cache.retain(|entry| entry.generation == scope.generation() && entry.run_id == scope.run_id());
}

pub(super) fn seal_tree<E: InstanceEvidence>(
    tree: &mut RawTree,
    scope: &FixtureScope,
    expected: &WindowBinding,
    evidence: &E,
    cache: &mut Vec<SealEntry>,
) -> Result<(), DriverError> {
    if let Some(root) = &mut tree.root {
        seal_node(root, scope, expected, evidence, cache)?;
    }
    Ok(())
}

fn seal_node<E: InstanceEvidence>(
    node: &mut RawNode,
    scope: &FixtureScope,
    expected: &WindowBinding,
    evidence: &E,
    cache: &mut Vec<SealEntry>,
) -> Result<(), DriverError> {
    node.handle = seal_handle(&node.handle, scope, expected, evidence, cache)?;
    for child in &mut node.children {
        seal_node(child, scope, expected, evidence, cache)?;
    }
    Ok(())
}

pub(super) fn seal_handle<E: InstanceEvidence>(
    raw: &NativeHandle,
    scope: &FixtureScope,
    expected: &WindowBinding,
    evidence: &E,
    cache: &mut Vec<SealEntry>,
) -> Result<NativeHandle, DriverError> {
    let live = evidence
        .element_binding(raw)
        .ok_or(DriverError::ScopeDenied)?;
    if live.window() != expected || !scope.admits_window(live.window()) {
        return Err(DriverError::ScopeDenied);
    }
    if let Some(found) = cache.iter().find(|entry| {
        entry.generation == scope.generation()
            && entry.run_id == scope.run_id()
            && &entry.window == expected
            && entry.inner.ptr_eq(raw)
    }) {
        return Ok(found.sealed.clone());
    }
    let sealed = NativeHandle::new(SealedHandle {
        run_id: scope.run_id().to_owned(),
        generation: scope.generation(),
        window: expected.clone(),
        inner: raw.clone(),
    });
    cache.push(SealEntry {
        generation: scope.generation(),
        run_id: scope.run_id().to_owned(),
        window: expected.clone(),
        inner: raw.clone(),
        sealed: sealed.clone(),
    });
    Ok(sealed)
}

fn unseal(
    handle: &NativeHandle,
    scope: &FixtureScope,
    evidence: &impl InstanceEvidence,
) -> Result<(NativeHandle, WindowBinding), DriverError> {
    let sealed = handle
        .downcast_ref::<SealedHandle>()
        .ok_or(DriverError::ScopeDenied)?;
    if sealed.generation != scope.generation() || sealed.run_id != scope.run_id() {
        return Err(DriverError::ScopeDenied);
    }
    if !scope.admits_window(&sealed.window) {
        return Err(DriverError::ScopeDenied);
    }
    let live = evidence
        .element_binding(&sealed.inner)
        .ok_or(DriverError::ScopeDenied)?;
    if live.window() != &sealed.window {
        return Err(DriverError::ScopeDenied);
    }
    Ok((sealed.inner.clone(), sealed.window.clone()))
}

pub(super) fn rewrite(
    action: &DriverAction,
    scope: &FixtureScope,
    evidence: &impl InstanceEvidence,
) -> Result<(DriverAction, PostCheck), DriverError> {
    match action {
        DriverAction::Press { el } => {
            let (inner, window) = unseal(el, scope, evidence)?;
            Ok((
                DriverAction::Press { el: inner.clone() },
                PostCheck {
                    window: None,
                    handle: Some((inner, window)),
                },
            ))
        }
        DriverAction::SecondaryAction { el, action } => {
            let (inner, window) = unseal(el, scope, evidence)?;
            Ok((
                DriverAction::SecondaryAction {
                    el: inner.clone(),
                    action: action.clone(),
                },
                PostCheck {
                    window: None,
                    handle: Some((inner, window)),
                },
            ))
        }
        DriverAction::SetValue { el, value } => {
            let (inner, window) = unseal(el, scope, evidence)?;
            Ok((
                DriverAction::SetValue {
                    el: inner.clone(),
                    value: value.clone(),
                },
                PostCheck {
                    window: None,
                    handle: Some((inner, window)),
                },
            ))
        }
        DriverAction::SelectText { el, range } => {
            let (inner, window) = unseal(el, scope, evidence)?;
            Ok((
                DriverAction::SelectText {
                    el: inner.clone(),
                    range: *range,
                },
                PostCheck {
                    window: None,
                    handle: Some((inner, window)),
                },
            ))
        }
        DriverAction::ScrollElement { el, dir, pages } => {
            let (inner, window) = unseal(el, scope, evidence)?;
            Ok((
                DriverAction::ScrollElement {
                    el: inner.clone(),
                    dir: *dir,
                    pages: *pages,
                },
                PostCheck {
                    window: None,
                    handle: Some((inner, window)),
                },
            ))
        }
        DriverAction::PointerClick { win, .. }
        | DriverAction::PointerScroll { win, .. }
        | DriverAction::PointerDrag { win, .. }
        | DriverAction::PressKeys { win, .. }
        | DriverAction::Activate { win } => {
            let binding = admit_window(scope, evidence, win)?;
            Ok((
                action.clone(),
                PostCheck {
                    window: Some((win.clone(), binding)),
                    handle: None,
                },
            ))
        }
        DriverAction::TypeText {
            win,
            focus,
            text,
            route,
        } => {
            let binding = admit_window(scope, evidence, win)?;
            let (focus, handle) = match focus {
                Some(focus) => {
                    let (inner, window) = unseal(focus, scope, evidence)?;
                    (Some(inner.clone()), Some((inner, window)))
                }
                None => (None, None),
            };
            Ok((
                DriverAction::TypeText {
                    win: win.clone(),
                    focus,
                    text: text.clone(),
                    route: *route,
                },
                PostCheck {
                    window: Some((win.clone(), binding)),
                    handle,
                },
            ))
        }
    }
}

pub(super) fn identity_changed(evidence: &impl InstanceEvidence, post: &PostCheck) -> bool {
    if let Some((window, binding)) = &post.window {
        if evidence.window_binding(window).as_ref() != Some(binding) {
            return true;
        }
    }
    if let Some((handle, binding)) = &post.handle {
        if evidence
            .element_binding(handle)
            .as_ref()
            .map(super::model::ElementBinding::window)
            != Some(binding)
        {
            return true;
        }
    }
    false
}
