//! Thin Tauri adapter. Layout, state and Stop logic live in the protocol crate.
mod ffi;
pub use gcu_host_protocol::overlay::command_allowed;
use gcu_host_protocol::{
    messages::{OverlayShow, OverlayStatus, Request, SessionId},
    overlay::{
        safety::{FileLatch, Safety},
        Desk, Monitor, Phase,
    },
};
use gcu_protocol::Rect;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const DRAW: &str = "cu-overlay";
pub const CONTROLS: &str = "cu-overlay-controls";

#[derive(Clone)]
pub struct Overlay {
    desk: Arc<Mutex<Desk>>,
    pub safety: Safety,
    frame: Arc<Mutex<Frame>>,
    failed: Arc<AtomicBool>,
}
#[derive(Clone, Serialize)]
pub struct Cursor {
    x: f64,
    y: f64,
    kind: gcu_host_protocol::messages::CursorKind,
}
#[derive(Clone, Serialize)]
pub struct Frame {
    phase: Phase,
    message: String,
    connected: bool,
    cursor: Option<Cursor>,
    revision: u64,
    response_ms: u32,
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
impl Overlay {
    pub fn new(app: &AppHandle) -> Self {
        let path = crate::paths::app_data_root()
            .join("computer-use")
            .join("stop-latch");
        let safety = Safety::new(Arc::new(FileLatch(path)));
        let overlay = Self {
            desk: Arc::new(Mutex::new(Desk::default())),
            safety,
            failed: Arc::new(AtomicBool::new(false)),
            frame: Arc::new(Mutex::new(Frame {
                phase: Phase::Hidden,
                message: String::new(),
                connected: false,
                cursor: None,
                revision: 0,
                response_ms: gcu_host_protocol::overlay::RESPONSE_MS,
            })),
        };
        let owner = overlay.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                if owner.frame().phase == Phase::Hidden
                    && lock(&owner.desk).snapshot().phase == Phase::Hidden
                    && !owner.safety.stopped()
                {
                    continue;
                }
                let ui_owner = owner.clone();
                let ui = app.clone();
                let (done, wait) = tokio::sync::oneshot::channel();
                if app
                    .run_on_main_thread(move || {
                        if !ui_owner.failed.load(Ordering::Acquire) {
                            if let Err(error) = ui_owner.render(&ui) {
                                tracing::error!(%error, "overlay surface failed");
                                ui_owner.fail();
                            }
                        }
                        if ui_owner.failed.load(Ordering::Acquire) {
                            hide(&ui);
                        }
                        let _ = done.send(());
                    })
                    .is_err()
                {
                    break;
                }
                if !matches!(
                    tokio::time::timeout(std::time::Duration::from_millis(500), wait).await,
                    Ok(Ok(()))
                ) {
                    // A stalled GTK/AppKit/window-server call cannot stall the
                    // independent control task or leave input running.
                    owner.fail();
                    break;
                }
            }
        });
        overlay
    }
    fn fail(&self) {
        if self.failed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.safety.latch();
        let safety = self.safety.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = safety.persist().await {
                tracing::error!(%error, "overlay Stop persistence failed");
            }
        });
    }
    pub fn notification(&self, request: &Request) -> Result<(), gcu_host_protocol::Error> {
        let mut desk = lock(&self.desk);
        match request.method.as_str() {
            "host.overlay.show" => desk.show_at(
                serde_json::from_value::<OverlayShow>(request.params.clone())
                    .map_err(|_| gcu_host_protocol::Error::Invalid)?,
                super::now_ms(),
            ),
            "host.overlay.status" => desk.status(
                serde_json::from_value::<OverlayStatus>(request.params.clone())
                    .map_err(|_| gcu_host_protocol::Error::Invalid)?,
            ),
            "host.overlay.hide" => {
                let session: SessionId = serde_json::from_value(request.params.clone())
                    .map_err(|_| gcu_host_protocol::Error::Invalid)?;
                desk.hide(&session.session_id);
                Ok(())
            }
            _ => Err(gcu_host_protocol::Error::Invalid),
        }
    }
    pub fn disconnected(&self) {
        lock(&self.desk).disconnect();
    }
    pub fn frame(&self) -> Frame {
        lock(&self.frame).clone()
    }
    fn render(&self, app: &AppHandle) -> Result<(), String> {
        let snapshot = {
            let mut desk = lock(&self.desk);
            desk.set_stopped(self.safety.stopped());
            desk.snapshot_at(super::now_ms())
        };
        if snapshot.phase == Phase::Hidden {
            hide(app);
            lock(&self.frame).phase = Phase::Hidden;
            return Ok(());
        }
        let monitors = app
            .available_monitors()
            .map_err(error)?
            .into_iter()
            .map(|m| Monitor {
                frame: Rect {
                    x: m.position().x.into(),
                    y: m.position().y.into(),
                    w: m.size().width.into(),
                    h: m.size().height.into(),
                },
                scale: m.scale_factor(),
            })
            .collect::<Vec<_>>();
        let Some(p) = ffi::placement(&monitors, snapshot.target.as_ref()) else {
            hide(app);
            return Ok(());
        };
        let mut frame = lock(&self.frame);
        frame.phase = snapshot.phase;
        frame.message = snapshot.message;
        frame.connected = self.safety.connected();
        frame.cursor = p
            .cursor
            .filter(|_| snapshot.cursor_visible)
            .zip(snapshot.target.as_ref())
            .map(|(at, target)| Cursor {
                x: at.x,
                y: at.y,
                kind: target.kind,
            });
        frame.revision = frame.revision.wrapping_add(1);
        drop(frame);
        let draw = ensure(app, DRAW, true)?;
        let controls = ensure(app, CONTROLS, false)?;
        ffi::position(&draw, p.monitor.frame, p.monitor.scale)?;
        ffi::position(&controls, p.capsule, p.monitor.scale)?;
        ffi::present(&draw, &controls, &p)
    }
}
fn ensure(app: &AppHandle, label: &str, cursor: bool) -> Result<WebviewWindow, String> {
    if let Some(win) = app.get_webview_window(label) {
        return Ok(win);
    }
    let settings = crate::store::load_settings();
    let locale = crate::tray_i18n::Locale::parse(&settings.locale);
    let theme = if settings.theme == "light"
        || (settings.theme != "dark" && !crate::os_theme::os_prefers_dark())
    {
        "light"
    } else {
        "dark"
    };
    let script = format!("window.__GROK_BOOT_LOCALE__={:?};document.addEventListener('DOMContentLoaded',()=>{{document.documentElement.dataset.theme={theme:?};}});", locale.as_tag());
    let dev_origin = app.config().build.dev_url.clone();
    let win = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("computer-use-overlay.html#/{label}").into()),
    )
    .title("Grok")
    .visible(false)
    .focused(false)
    .focusable(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .decorations(false)
    .shadow(false)
    .resizable(false)
    .accept_first_mouse(true)
    .initialization_script(script)
    .on_navigation(move |url| {
        url.path() == "/computer-use-overlay.html"
            && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                || (matches!(url.scheme(), "http" | "https")
                    && url.host_str() == Some("tauri.localhost")
                    && url.port().is_none())
                || (cfg!(debug_assertions)
                    && dev_origin
                        .as_ref()
                        .is_some_and(|dev| url.origin() == dev.origin())))
    })
    .build()
    .map_err(error)?;
    if let Err(e) =
        gcu_host_protocol::overlay::prepare_surface(&mut ffi::WindowSurface(&win), cursor)
    {
        let _ = win.destroy();
        return Err(e);
    }
    Ok(win)
}
fn hide(app: &AppHandle) {
    for label in [DRAW, CONTROLS] {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.hide();
        }
    }
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Trusted document snapshot. Caller checks also apply when app commands have no ACL.
#[tauri::command]
pub fn cu_overlay_snapshot(
    window: WebviewWindow,
    host: tauri::State<'_, super::ComputerUseHost>,
) -> Result<Frame, String> {
    if !matches!(window.label(), DRAW | CONTROLS) {
        return Err("untrusted".into());
    }
    Ok(host.overlay().frame())
}

#[cfg(test)]
mod tests {
    #[test]
    fn capability_does_not_include_browser_media_events_or_approval() {
        let cap = include_str!("../../capabilities/cu-overlay.json");
        for forbidden in [
            "remote",
            "media",
            "approval",
            "core:default",
            "event",
            "set-focus",
        ] {
            assert!(!cap.contains(forbidden));
        }
    }
}
