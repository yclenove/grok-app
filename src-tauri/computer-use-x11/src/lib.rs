//! Native X11 adapter, separated from Tauri so the exact production input and
//! capture code can be exercised against an owned X server. Not Wayland support.
#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod accessibility;
mod authority;
mod capture;
mod client;
pub mod clipboard;
#[cfg(feature = "native-probe")]
mod fixture;
mod input;
mod keyboard;
#[cfg(feature = "native-probe")]
mod semantic_fixture;
#[cfg(feature = "native-probe")]
pub use fixture::run_native_selftest;
#[cfg(feature = "native-probe")]
pub use semantic_fixture::run_semantic_selftest;

use base64::Engine;
use grok_computer_use_core::adapter::{
    ActionSupport, AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest,
    SurfaceKind, TargetInfo,
};
use grok_computer_use_core::native_action::NativeActionSlot;
use grok_computer_use_core::protocol::{Observation, ObservationImage, PROTOCOL_VERSION};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct LinuxAdapter {
    client: Mutex<Option<client::Client>>,
    input: NativeActionSlot,
    accessibility: Mutex<accessibility::Accessibility>,
    authority: Mutex<authority::Authority>,
    semantic_available: AtomicBool,
    clipboard_retention: clipboard::ClipboardRetention,
}

impl LinuxAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retain only an already-restored original. This is NOT native input
    /// completion; the caller must independently establish paste quiescence.
    pub fn retain_restored_clipboard(
        &self,
        pending: &mut Option<(clipboard::ClipboardOwner, clipboard::ClipboardLease)>,
    ) -> Result<clipboard::RetainedClipboard, String> {
        self.clipboard_retention.retain(pending)
    }

    pub fn clipboard_retention_statuses(&self) -> Vec<clipboard::RetentionStatus> {
        self.clipboard_retention.statuses()
    }

    pub fn request_clipboard_handoff(&self) {
        self.clipboard_retention.request_handoff();
    }

    fn recover_native_completion(&self) {
        // Nonblocking on the action owner: Stop/status never wait for AT-SPI.
        if let Some(mut accessibility) = self.accessibility.try_lock() {
            if let Some(completion) = accessibility.native_completion() {
                // All observations preceding an unknown effect are stale. Do
                // this BEFORE releasing the exact old owner's occupancy.
                self.authority.lock().retire_all();
                completion.confirm_native_completion();
            }
        }
    }

    fn with_client<T>(
        &self,
        f: impl FnOnce(&mut client::Client) -> Result<T, String>,
    ) -> Result<T, String> {
        require_native_x11()?;
        let mut client = self.client.lock();
        if client.is_none() {
            *client = Some(client::Client::connect()?);
        }
        f(client.as_mut().expect("client initialized"))
    }
}

fn capture_observation(
    adapter: &LinuxAdapter,
    run_id: &str,
    target_id: &str,
    options: &grok_computer_use_core::adapter::CaptureOptions,
) -> Result<Observation, String> {
    adapter.recover_native_completion();
    let snapshot_id = format!("x11-snap-{}", uuid::Uuid::new_v4());
    if options.for_model {
        if run_id.trim().is_empty() {
            return Err("X11 model capture requires a run ID".into());
        }
        adapter
            .authority
            .lock()
            .begin(run_id, target_id, &snapshot_id);
    }
    options.cancellation.check()?;
    let (geometry, png) = adapter.with_client(|c| c.capture(target_id))?;
    options.cancellation.check()?;
    let semantic = if options.for_model {
        adapter
            .with_client(|c| c.semantic_binding(target_id))
            .and_then(|(current, pid, title)| {
                if current != geometry {
                    return Err("native geometry changed during observation".into());
                }
                adapter.accessibility.lock().observe(
                    run_id,
                    target_id,
                    &snapshot_id,
                    geometry,
                    pid,
                    &title,
                )
            })
    } else {
        Ok((Vec::new(), false))
    };
    if options.for_model && semantic.is_ok() {
        // A successful AT-SPI observation proves bus availability even when no
        // capability query preceded it. Active input/Wait may now hold the lock.
        adapter.semantic_available.store(true, Ordering::Release);
    }
    // Many X11 clients have no AT-SPI provider. Pixel observation remains
    // available; no semantic reference or input authority is invented.
    let (nodes, truncated) = semantic.unwrap_or_else(|_| (Vec::new(), true));
    if adapter.current_geometry_revision_for(target_id) != geometry.revision() {
        return Err("native geometry changed during accessibility observation".into());
    }
    options.cancellation.check()?;
    if options.for_model {
        adapter.authority.lock().publish(
            run_id,
            target_id,
            &snapshot_id,
            geometry.revision(),
            options.screenshot,
        )?;
    }
    let width = u32::from(geometry.width);
    let height = u32::from(geometry.height);
    Ok(Observation {
        version: PROTOCOL_VERSION,
        run_id: run_id.into(),
        target_id: target_id.to_owned(),
        target_generation: 0,
        snapshot_id,
        captured_at: chrono::Utc::now().to_rfc3339(),
        geometry_revision: geometry.revision(),
        coordinate_space: "image-pixels".into(),
        image: ObservationImage {
            width,
            height,
            content_id: format!("png-{}", uuid::Uuid::new_v4()),
            png_base64: options
                .screenshot
                .then(|| base64::engine::general_purpose::STANDARD.encode(png)),
        },
        text: String::new(),
        nodes,
        truncated,
        crop_x: 0,
        crop_y: 0,
        crop_width: width,
        crop_height: height,
        scale: 1.0,
        dpi: 96.0,
        origin_x: i32::from(geometry.x),
        origin_y: i32::from(geometry.y),
        topology_revision: geometry.revision(),
    })
}

fn require_native_x11() -> Result<(), String> {
    let wayland = std::env::var("XDG_SESSION_TYPE")
        .is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    if wayland {
        return Err("native Wayland needs its portal adapter; XWayland is not a substitute".into());
    }
    let display = std::env::var("DISPLAY").unwrap_or_default();
    // Local input only. Do not send an authorization to a remote DISPLAY host.
    if !display.starts_with(':') && !display.starts_with("unix:") {
        return Err("a local native X11 DISPLAY is required".into());
    }
    Ok(())
}

impl ComputerUseAdapter for LinuxAdapter {
    fn backend_id(&self) -> &'static str {
        "linux"
    }

    fn capabilities(&self) -> Capabilities {
        let available = self.with_client(|_| Ok(()));
        let native = available.is_ok();
        // A read-only Wait owns the AT-SPI snapshot lock, not the capability
        // query/Stop path. Report the last bus availability while it is busy.
        let semantic = native
            && match self.accessibility.try_lock() {
                Some(mut accessibility) => {
                    let available = accessibility.available();
                    self.semantic_available.store(available, Ordering::Release);
                    available
                }
                None => self.semantic_available.load(Ordering::Acquire),
            };
        let mut caps = Capabilities::for_surface(SurfaceKind::Desktop, "linux");
        caps.observe_screenshot = native;
        caps.observe_ax = semantic;
        caps.semantic_click = semantic;
        caps.coordinate_click = native;
        caps.unicode_text = semantic;
        caps.chinese_ime = false;
        caps.native_wayland = false;
        caps.native_x11 = native;
        let coordinate = if native {
            ActionSupport::COORDINATE
        } else {
            ActionSupport::NONE
        };
        caps.click = ActionSupport {
            semantic,
            coordinate: native,
        };
        caps.key = coordinate;
        caps.scroll = coordinate;
        caps.drag = coordinate;
        let text = if semantic {
            ActionSupport::SEMANTIC
        } else {
            ActionSupport::NONE
        };
        caps.set_value = text;
        caps.type_text = text;
        caps.wait = text;
        caps.notes = vec![
            "Native X11: exact window lifetime, foreground input, XTEST and visual-mask capture".into(),
            "AT-SPI observed controls: primary action and Unicode edit with native readback; no coordinate/text fallback".into(),
            "Read-only Wait retains the exact AT-SPI object and observed ancestry; it never re-observes or types".into(),
            "IME composition and native Wayland remain separate unfinished work".into(),
        ];
        if let Err(error) = available {
            caps.notes.push(error);
        }
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.with_client(client::Client::list_targets)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        self.with_client(|c| c.geometry(target_id).map(|_| ()))
            .is_ok()
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        // Legacy unscoped observations are display-only, never model authority.
        capture_observation(
            self,
            "",
            target_id,
            &grok_computer_use_core::adapter::CaptureOptions {
                for_model: false,
                screenshot: true,
                managed_request: None,
                cancellation: Default::default(),
            },
        )
    }

    fn observe_for_run(&self, run_id: &str, target_id: &str) -> Result<Observation, String> {
        self.capture_for_run(
            run_id,
            target_id,
            grok_computer_use_core::adapter::CaptureOptions::model(true),
        )
    }

    fn capture_for_run(
        &self,
        run_id: &str,
        target_id: &str,
        options: grok_computer_use_core::adapter::CaptureOptions,
    ) -> Result<Observation, String> {
        capture_observation(self, run_id, target_id, &options)
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.recover_native_completion();
        req.admit(self)?;
        let mut owner = self
            .input
            .begin(&req.run_id, req.generation, &req.cancellation)?;
        self.authority.lock().admit(req)?;
        if matches!(
            req.target,
            grok_computer_use_core::protocol::ActionTarget::Element { .. }
        ) {
            let mut accessibility = self
                .accessibility
                .try_lock()
                .ok_or("AT-SPI capture is busy")?;
            let result = accessibility.act(req, || {
                self.authority.lock().check(req)?;
                self.with_client(|c| {
                    c.fenced(|c| {
                        if c.geometry(&req.target_id)?.revision() != req.geometry_revision {
                            return Err("native geometry changed before semantic input".into());
                        }
                        if req.action != grok_computer_use_core::protocol::ActionKind::Wait
                            && !c.foreground(&req.target_id)?
                        {
                            return Err("native focus changed before semantic input".into());
                        }
                        if req.action != grok_computer_use_core::protocol::ActionKind::Wait {
                            c.require_released_input()?;
                        }
                        req.cancellation.check()
                    })
                })
            });
            let x11_uncertain = self
                .client
                .lock()
                .as_ref()
                .is_some_and(|c| c.input_uncertain);
            if accessibility.calls.uncertain || x11_uncertain {
                owner.retain_until_native_recovery();
                // A toolkit reply proves nothing about a broken X11 cleanup.
                if !x11_uncertain {
                    accessibility.calls.defer(&mut owner);
                }
            }
            return result;
        }
        let result = self.with_client(|c| c.act(req, || self.authority.lock().check(req)));
        // A broken X connection after dispatch is not proof of completed input.
        // Preserve occupancy, rather than allow a replacement to overlap it.
        if self
            .client
            .lock()
            .as_ref()
            .is_some_and(|c| c.input_uncertain)
        {
            owner.retain_until_native_recovery();
        }
        result.map(|applied| AdapterActResult {
            applied,
            outcome: None,
            postcondition_ok: false,
            verifiable: false,
            detail: if applied {
                "native X11 input processed; application postcondition not verified"
            } else {
                "native X11 zero scroll: no input dispatched"
            }
            .into(),
        })
    }

    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String> {
        self.input.cancel_before(run_id, generation);
        self.authority.lock().retire_before(run_id, generation);
        Ok(())
    }
    fn wait_uses_retained_reference(&self) -> bool {
        true
    }
    fn release_target_for_run(&self, run_id: &str, target_id: &str) {
        self.authority.lock().retire(run_id, target_id);
    }
    fn is_idle(&self, _run_id: &str) -> bool {
        self.recover_native_completion();
        self.input.is_idle()
    }
    fn foreground_input_available(&self, target: &str) -> bool {
        self.with_client(|c| c.foreground(target)).unwrap_or(false)
    }
    fn input_available(&self) -> bool {
        require_native_x11().is_ok()
    }
    fn current_geometry_revision_for(&self, target: &str) -> u64 {
        self.with_client(|c| c.geometry(target).map(|g| g.revision()))
            .unwrap_or(0)
    }
    fn start_periodic_preview(&self, _target: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}
