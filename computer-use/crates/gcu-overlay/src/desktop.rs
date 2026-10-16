//! Thread-affine native UI and independent trusted Host controls.
use crate::{
    availability::{self, Availability},
    ButtonAction, Catalog, Clock, Config, CopyKey, Overlay, Rect, Scene,
};
use std::{marker::PhantomData, rc::Rc, sync::Arc, time::Duration};

/// Native initialization/render/input failure. Diagnostic text is not UI copy.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Desktop cannot support the safety properties required for overlay input.
    #[error("overlay unavailable: {0:?}")]
    Unavailable(CopyKey),
    /// A required OS operation failed. Windows are hidden before returning.
    #[error("overlay native operation failed: {0}")]
    Native(String),
    /// Host refused a persisted-latch or approval-window transition.
    #[error("overlay Host control failed: {0}")]
    Host(String),
}
impl Error {
    pub(crate) fn native(e: impl std::fmt::Display) -> Self {
        Self::Native(e.to_string())
    }
}
/// Trusted local Host callbacks, never exported through MCP or normal Host RPC.
/// Implementations use the existing persistent UserActivity latch and platform
/// watchdog. No callback may wait for an action, approval, driver or filesystem.
pub trait Controls: Send + Sync {
    /// Latch synchronously (≤100ms), then signal input release on the dedicated
    /// control channel (≤500ms). Persistence runs off the UI thread. Idempotent.
    fn stop(&self);
    /// Explicit physical-user resume. Enqueue on the dedicated control channel;
    /// persisted Stop remains latched until that channel commits successfully.
    /// A returned error leaves Stop latched. Must return without waiting for IO.
    fn resume(&self) -> Result<(), Error>;
    /// Read the same Stop bit used by UserActivity. Never consumes it.
    fn stopped(&self) -> bool;
    /// Are independent hotkey, physical provenance and watchdog healthy?
    /// Failure disables Resume; the Host must also close dependent input routes.
    fn healthy(&self) -> bool;
    /// Open the existing pending approval window for this exact opaque request.
    /// Never interpret the click as an approval decision. Nonblocking notification.
    fn open_approval(&self, request: &str) -> Result<(), Error>;
}
pub(crate) struct UserEvent {
    pub action: ButtonAction,
    pub revision: u64,
}
/// Main/desktop-thread owner. Drop destroys every native window; provider clones
/// can outlive it but cannot create windows or synthesize control events.
/// Native calls are synchronous: pump on a dedicated UI thread (macOS main),
/// independently of the action/approval runtime. The Host must retain its
/// independent watchdog even if this UI thread fails or stalls.
pub struct DesktopOverlay {
    overlay: Overlay,
    controls: Arc<dyn Controls>,
    native: crate::native::Surface,
    last: Option<Scene>,
    debounce: Debounce,
    _thread: PhantomData<Rc<()>>,
}
impl DesktopOverlay {
    /// Create native resources on the UI thread. Reject Wayland/XWayland and
    /// missing copy before creating any window. Does not start a resident process.
    /// Initialization failure latches Stop. No cancellation while inside OS calls.
    pub fn open(
        clock: Arc<dyn Clock>,
        catalog: Arc<dyn Catalog>,
        controls: Arc<dyn Controls>,
        config: Config,
    ) -> Result<Self, Error> {
        let eligibility = Availability::for_session(availability::detect());
        if !eligibility.available {
            controls.stop();
            return Err(Error::Unavailable(
                eligibility.reason.unwrap_or(CopyKey::Unavailable),
            ));
        }
        for key in CopyKey::ALL {
            let text = catalog.text(key);
            if text.trim().is_empty() || text == key.as_str() {
                controls.stop();
                return Err(Error::Unavailable(CopyKey::Unavailable));
            }
        }
        let native = crate::native::Surface::open().inspect_err(|_| controls.stop())?;
        Ok(Self {
            overlay: Overlay::new(clock, catalog, config),
            controls,
            native,
            last: None,
            debounce: Debounce::default(),
            _thread: PhantomData,
        })
    }
    /// Obtain the nonblocking OverlayProvider. No IO, timeout or cancellation.
    pub fn provider(&self) -> Overlay {
        self.overlay.clone()
    }
    /// Pump bounded native input before rendering one frame. Call at 60fps using
    /// animation::FRAME_INTERVAL, independently of engine work. A native failure
    /// hides all windows and latches Stop before returning. This never waits for
    /// user input; OS call deadlines/watchdog belong to the embedding Host.
    pub fn tick(&mut self) -> Result<(), Error> {
        let result = self.tick_inner();
        if result.is_err() {
            self.overlay
                .capture_proven
                .store(false, std::sync::atomic::Ordering::Release);
            self.controls.stop();
            self.native.hide();
        }
        result
    }
    fn tick_inner(&mut self) -> Result<(), Error> {
        for event in self.native.events()? {
            if let Some(scene) = &self.last {
                dispatch(
                    &*self.controls,
                    &mut self.debounce,
                    self.overlay.clock.now(),
                    scene,
                    event,
                )?;
            }
        }
        if !self.controls.healthy() {
            self.controls.stop();
        }
        let target = self.overlay.target();
        #[cfg(target_os = "macos")]
        self.native.coordinate_scale(target.map(|(_, scale)| scale));
        let monitor = self.native.monitor(target.map(|(frame, _)| frame))?;
        let scene = self
            .overlay
            .scene(monitor, self.controls.stopped(), self.controls.healthy());
        self.native.render(&scene)?;
        self.overlay.capture_proven.store(
            cfg!(any(windows, target_os = "macos")),
            std::sync::atomic::Ordering::Release,
        );
        self.last = Some(scene);
        Ok(())
    }
}
#[derive(Default)]
struct Debounce {
    last: Option<(ButtonAction, Duration)>,
}
fn dispatch(
    controls: &dyn Controls,
    debounce: &mut Debounce,
    now: Duration,
    scene: &Scene,
    event: UserEvent,
) -> Result<(), Error> {
    if event.revision != scene.revision || !scene.buttons.iter().any(|b| b.action == event.action) {
        return Ok(());
    }
    if (event.action != ButtonAction::Stop || controls.stopped())
        && debounce.last.is_some_and(|(action, at)| {
            (action == event.action
                || (action == ButtonAction::Stop && event.action == ButtonAction::Resume))
                && now.saturating_sub(at) < Duration::from_millis(250)
        })
    {
        return Ok(());
    }
    debounce.last = Some((event.action, now));
    match event.action {
        ButtonAction::Stop => controls.stop(),
        ButtonAction::Resume if controls.stopped() && controls.healthy() => controls.resume()?,
        ButtonAction::Approval if !controls.stopped() => {
            if let Some(id) = &scene.approval {
                controls.open_approval(id)?;
            }
        }
        _ => (),
    }
    Ok(())
}
impl Drop for DesktopOverlay {
    fn drop(&mut self) {
        self.overlay
            .capture_proven
            .store(false, std::sync::atomic::Ordering::Release);
    }
}
/// A separate noninteractive surface for each part; control windows have exactly
/// the button bounds. Cursor/status/border never receive mouse or keyboard input.
pub(crate) struct Part {
    pub frame: Rect,
    pub text: String,
    pub action: Option<ButtonAction>,
    pub circle: bool,
    pub warning: bool,
    pub revision: u64,
}
pub(crate) fn parts(scene: &Scene) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut push = |frame, text, action, circle| {
        parts.push(Part {
            frame,
            text,
            action,
            circle,
            warning: matches!(
                scene.state,
                crate::State::Foreground | crate::State::Stopped | crate::State::Unavailable
            ),
            revision: scene.revision,
        })
    };
    if let Some(frame) = scene.border {
        for edge in [
            Rect { h: 3., ..frame },
            Rect {
                y: frame.y + frame.h - 3.,
                h: 3.,
                ..frame
            },
            Rect { w: 3., ..frame },
            Rect {
                x: frame.x + frame.w - 3.,
                w: 3.,
                ..frame
            },
        ] {
            push(edge, String::new(), None, false);
        }
    }
    if let Some(mut frame) = scene.capsule {
        // Stop and approval get independent windows, so status is input-transparent.
        let reserved = if scene.approval.is_some() && scene.buttons.len() > 1 {
            256.
        } else {
            112.
        };
        frame.w = (frame.w - reserved).max(80.);
        push(frame, scene.message.clone(), None, false);
    }
    for button in &scene.buttons {
        push(
            button.frame,
            button.label.clone(),
            Some(button.action),
            false,
        );
    }
    if let Some(cursor) = &scene.cursor {
        use gcu_host_api::CursorKind;
        let (x, y) = (cursor.at.x, cursor.at.y);
        let r = cursor.radius;
        match cursor.kind {
            CursorKind::Caret => {
                push(
                    Rect {
                        x: x - 1.,
                        y: y - 12.,
                        w: 2.,
                        h: 24.,
                    },
                    String::new(),
                    None,
                    false,
                );
                return parts;
            }
            CursorKind::Drag => {
                for (dx, dy, w, h) in [
                    (-8., -8., 16., 2.),
                    (-8., 6., 16., 2.),
                    (-8., -8., 2., 16.),
                    (6., -8., 2., 16.),
                ] {
                    push(
                        Rect {
                            x: x + dx,
                            y: y + dy,
                            w,
                            h,
                        },
                        String::new(),
                        None,
                        false,
                    );
                }
                return parts;
            }
            CursorKind::Scroll => {
                for (dx, dy, w, h) in [
                    (-1., -10., 2., 20.),
                    (-5., -10., 10., 2.),
                    (-5., 8., 10., 2.),
                ] {
                    push(
                        Rect {
                            x: x + dx,
                            y: y + dy,
                            w,
                            h,
                        },
                        String::new(),
                        None,
                        false,
                    );
                }
                return parts;
            }
            CursorKind::Pointer => (),
        }
        push(
            Rect {
                x: cursor.at.x - r,
                y: cursor.at.y - r,
                w: 2. * r,
                h: 2. * r,
            },
            String::new(),
            None,
            true,
        );
    }
    parts
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
