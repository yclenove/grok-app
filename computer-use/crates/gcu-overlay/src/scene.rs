//! Portable layout in physical screen pixels, with separate input-only controls.
use crate::{controller::clean, lock, CopyKey, Overlay, Rect};
use gcu_host_api::{CursorKind, ScreenPoint};
use std::time::Duration;

/// Capsule state, with safety states taking precedence over engine notifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No active session.
    Hidden,
    /// Background work.
    Running,
    /// Foreground delivery with takeover guidance.
    Foreground,
    /// A pending Host approval can be opened.
    Approval,
    /// Persistent user Stop latch.
    Stopped,
    /// Safety input monitoring failed; Resume is disabled.
    Unavailable,
}
/// The only actions native physical pointer events can request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    /// Latch Stop and signal the independent input-release path.
    Stop,
    /// Ask the trusted Host to explicitly clear its persisted Stop latch.
    Resume,
    /// Open a pending approval; never grant it.
    Approval,
}
/// One custom-drawn nonactivating button. Never exposed as an automation action.
#[derive(Clone, Debug)]
pub struct Button {
    /// Physical screen rectangle, the only pointer input region.
    pub frame: Rect,
    /// Localized text supplied by the Host.
    pub label: String,
    /// Trusted control operation.
    pub action: ButtonAction,
}
/// Software-only pointer; never warps the OS cursor.
#[derive(Clone, Debug)]
pub struct Cursor {
    /// Physical screen coordinate.
    pub at: ScreenPoint,
    /// Pointer glyph kind.
    pub kind: CursorKind,
    /// Click pulse radius in physical pixels.
    pub radius: f64,
}
/// Immutable scene. Native windows use opaque capsule/buttons and clear cursor backing.
#[derive(Clone, Debug)]
pub struct Scene {
    /// Visual state.
    pub state: State,
    /// Opaque capsule rectangle (noninteractive body).
    pub capsule: Option<Rect>,
    /// Localized status text.
    pub message: String,
    /// Physical pointer controls.
    pub buttons: Vec<Button>,
    /// Software pointer, if recently active.
    pub cursor: Option<Cursor>,
    /// Foreground target border, never interactive.
    pub border: Option<Rect>,
    /// Exact pending approval identity; never engine/model-selected.
    pub approval: Option<String>,
    pub(crate) revision: u64,
}
pub(crate) fn build(o: &Overlay, monitor: Rect, stopped: bool, healthy: bool) -> Scene {
    let now = o.clock.now();
    let model = lock(&o.model);
    let active = model.sessions.values().max_by_key(|s| s.revision);
    let state = if !healthy {
        State::Unavailable
    } else if stopped {
        State::Stopped
    } else if active.is_some_and(|s| s.approval.is_some()) {
        State::Approval
    } else if active.is_some_and(|s| s.foreground) {
        State::Foreground
    } else if active.is_some() {
        State::Running
    } else {
        State::Hidden
    };
    let mut scene = Scene {
        state,
        capsule: None,
        message: String::new(),
        buttons: Vec::new(),
        cursor: None,
        border: None,
        approval: active.and_then(|s| s.approval.clone()),
        revision: active.map_or(0, |s| s.revision),
    };
    if !o.config.visible
        || state == State::Hidden
        || ![monitor.x, monitor.y, monitor.w, monitor.h]
            .iter()
            .all(|v| v.is_finite())
        || monitor.w < 240.
        || monitor.h < 80.
    {
        return scene;
    }
    let w = monitor.w.min(680.);
    let frame = Rect {
        x: monitor.x + (monitor.w - w) / 2.,
        y: monitor.y + 12.,
        w,
        h: 48.,
    };
    scene.capsule = Some(frame);
    let key = match state {
        State::Foreground => CopyKey::Foreground,
        State::Approval => CopyKey::Waiting,
        State::Stopped => CopyKey::Stopped,
        State::Unavailable => CopyKey::Unavailable,
        _ => CopyKey::Running,
    };
    scene.message = clean(&o.catalog.text(key));
    if matches!(state, State::Running | State::Foreground) {
        if let Some(s) = active.filter(|s| !s.message.is_empty()) {
            scene.message = clean(&format!("{} · {}", scene.message, s.message));
        }
    }
    let action = if stopped {
        ButtonAction::Resume
    } else {
        ButtonAction::Stop
    };
    if !stopped || healthy {
        scene.buttons.push(Button {
            frame: Rect {
                x: frame.x + frame.w - 104.,
                y: frame.y + 4.,
                w: 100.,
                h: 40.,
            },
            label: clean(&o.catalog.text(if stopped {
                CopyKey::Resume
            } else {
                CopyKey::Stop
            })),
            action,
        });
    }
    if state == State::Approval && frame.w >= 480. {
        scene.buttons.push(Button {
            frame: Rect {
                x: frame.x + frame.w - 248.,
                y: frame.y + 4.,
                w: 140.,
                h: 40.,
            },
            label: clean(&o.catalog.text(CopyKey::Approval)),
            action: ButtonAction::Approval,
        });
    }
    if !stopped && healthy {
        if let Some(s) = active {
            if s.foreground {
                scene.border = s.window;
            }
            if let Some((spring, kind, at)) = &s.cursor {
                let age = now.saturating_sub(*at);
                if age < Duration::from_secs(30) {
                    let pulse = ((age.as_secs_f64() - o.config.response) / 0.35).clamp(0., 1.);
                    scene.cursor = Some(Cursor {
                        at: spring.sample(now),
                        kind: *kind,
                        radius: 5. + 10. * (std::f64::consts::PI * pulse).sin(),
                    });
                }
            }
        }
    }
    scene
}
