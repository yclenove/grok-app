//! Deterministic test adapter. Never interacts with a real desktop.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use super::adapter::{
    ActionScope, AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use super::protocol::{Observation, ObservationImage, ObservationNode, PROTOCOL_VERSION};

#[derive(Debug)]
struct Inner {
    executions: Vec<String>,
    list_calls: u32,
    fail_listing: bool,
    desktop_fallback: u32,
    observe_calls: u32,
    preview_ticks: u32,
    preview_active: bool,
    target_alive: bool,
    in_flight: u32,
    abort_called: u32,
    abort_blocked: bool,
    release_called: u32,
    last_generation: u64,
    delay: Duration,
    hang: bool,
    /// abort() returns but does not clear in_flight until `finish_in_flight`.
    abort_does_not_quiesce: bool,
    verify_ok: bool,
    snapshot_seq: u64,
    geometry: u64,
    cancelled: bool,
    blank_image: bool,
    include_png: bool,
    fixture_id: String,
    host_id: String,
    session_locked: bool,
    focus_on_fixture: bool,
    user_input: bool,
    origin: (i32, i32),
    dpi_scale: f64,
    last_virtual: Option<(i32, i32)>,
    iconic: bool,
    last_click_button: String,
    last_click_count: u32,
    last_key: String,
    last_dispatch_scope: Option<ActionScope>,
    last_dispatch_target_id: String,
    last_dispatch_target: Option<crate::protocol::ActionTarget>,
    fail_semantic: bool,
    semantic_fallback: u32,
    verifiable: bool,
    node_count: usize,
    huge_png: bool,
    include_decoy: bool,
    include_overlay: bool,
    include_statusbar: bool,
    truncate_ref: Option<String>,
    stale_observe_revision: bool,
}

pub struct FakeAdapter {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl FakeAdapter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                executions: Vec::new(),
                list_calls: 0,
                fail_listing: false,
                desktop_fallback: 0,
                observe_calls: 0,
                preview_ticks: 0,
                preview_active: false,
                target_alive: true,
                in_flight: 0,
                abort_called: 0,
                abort_blocked: false,
                release_called: 0,
                last_generation: 0,
                delay: Duration::from_millis(0),
                hang: false,
                abort_does_not_quiesce: false,
                verify_ok: true,
                snapshot_seq: 0,
                geometry: 1,
                cancelled: false,
                blank_image: false,
                include_png: false,
                fixture_id: "fake:window:1".into(),
                host_id: "fake:host:auth".into(),
                session_locked: false,
                focus_on_fixture: true,
                user_input: false,
                origin: (0, 0),
                dpi_scale: 1.0,
                last_virtual: None,
                iconic: false,
                last_click_button: "left".into(),
                last_click_count: 1,
                last_key: String::new(),
                last_dispatch_scope: None,
                last_dispatch_target_id: String::new(),
                last_dispatch_target: None,
                fail_semantic: false,
                semantic_fallback: 0,
                verifiable: true,
                node_count: 1,
                huge_png: false,
                include_decoy: false,
                include_overlay: false,
                include_statusbar: false,
                truncate_ref: None,
                stale_observe_revision: false,
            }),
            cv: Condvar::new(),
        }
    }

    pub fn fixture_id(&self) -> String {
        self.inner.lock().fixture_id.clone()
    }

    pub fn set_listing_error(&self, fail: bool) {
        self.inner.lock().fail_listing = fail;
    }

    pub fn list_calls(&self) -> u32 {
        self.inner.lock().list_calls
    }

    pub fn host_id(&self) -> String {
        self.inner.lock().host_id.clone()
    }

    pub fn rotate_identity(&self) {
        let mut g = self.inner.lock();
        g.fixture_id = format!("fake:window:{}", g.snapshot_seq.saturating_add(1000));
    }

    pub fn set_session_locked(&self, locked: bool) {
        self.inner.lock().session_locked = locked;
    }

    pub fn set_focus_drifted(&self, drifted: bool) {
        self.inner.lock().focus_on_fixture = !drifted;
    }

    pub fn set_user_input_active(&self, on: bool) {
        self.inner.lock().user_input = on;
    }

    pub fn bump_geometry(&self) {
        let mut g = self.inner.lock();
        g.geometry = g.geometry.saturating_add(1);
    }

    pub fn set_origin(&self, x: i32, y: i32) {
        let mut g = self.inner.lock();
        g.origin = (x, y);
        g.geometry = g.geometry.saturating_add(1);
    }

    pub fn set_dpi_scale(&self, scale: f64) {
        let mut g = self.inner.lock();
        g.dpi_scale = scale;
        g.geometry = g.geometry.saturating_add(1);
    }

    pub fn virtual_from_image(&self, x: f64, y: f64) -> (i32, i32) {
        let g = self.inner.lock();
        (
            g.origin.0 + (x * g.dpi_scale).round() as i32,
            g.origin.1 + (y * g.dpi_scale).round() as i32,
        )
    }

    pub fn last_virtual(&self) -> Option<(i32, i32)> {
        self.inner.lock().last_virtual
    }

    pub fn set_iconic(&self, iconic: bool) {
        self.inner.lock().iconic = iconic;
    }

    pub fn set_alive(&self, alive: bool) {
        self.inner.lock().target_alive = alive;
    }

    pub fn set_delay(&self, d: Duration) {
        self.inner.lock().delay = d;
    }

    pub fn set_hang(&self, hang: bool) {
        self.inner.lock().hang = hang;
    }

    pub fn set_abort_does_not_quiesce(&self, v: bool) {
        self.inner.lock().abort_does_not_quiesce = v;
    }

    pub fn set_verify_ok(&self, ok: bool) {
        self.inner.lock().verify_ok = ok;
    }

    pub fn set_blank_image(&self, blank: bool) {
        self.inner.lock().blank_image = blank;
    }

    pub fn set_include_png(&self, on: bool) {
        self.inner.lock().include_png = on;
    }

    pub fn set_node_count(&self, n: usize) {
        self.inner.lock().node_count = n.max(1);
    }

    pub fn set_huge_png(&self, on: bool) {
        self.inner.lock().huge_png = on;
    }

    pub fn set_include_decoy(&self, on: bool) {
        self.inner.lock().include_decoy = on;
    }

    pub fn decoy_id(&self) -> String {
        "fake:decoy:other".into()
    }

    pub fn set_include_overlay(&self, on: bool) {
        self.inner.lock().include_overlay = on;
    }

    pub fn overlay_id(&self) -> String {
        "fake:overlay:nv".into()
    }

    pub fn set_include_statusbar(&self, on: bool) {
        self.inner.lock().include_statusbar = on;
    }

    pub fn statusbar_id(&self) -> String {
        "fake:bar:status".into()
    }

    pub fn set_truncate_ref(&self, node_ref: impl Into<String>) {
        self.inner.lock().truncate_ref = Some(node_ref.into());
    }

    pub fn set_stale_observe_revision(&self, on: bool) {
        self.inner.lock().stale_observe_revision = on;
    }

    pub fn executions(&self) -> Vec<String> {
        self.inner.lock().executions.clone()
    }

    /// Observe the adapter's actual dispatch boundary, not the caller's timeout.
    /// The broker may return Unknown before its worker receives a CPU timeslice.
    pub fn wait_for_execution(&self, action_id: &str, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut guard = self.inner.lock();
        while !guard.executions.iter().any(|id| id == action_id) {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            self.cv.wait_for(&mut guard, deadline - now);
        }
        true
    }

    pub fn last_click_button(&self) -> String {
        self.inner.lock().last_click_button.clone()
    }

    pub fn last_click_count(&self) -> u32 {
        self.inner.lock().last_click_count
    }

    pub fn last_key(&self) -> String {
        self.inner.lock().last_key.clone()
    }

    pub fn set_fail_semantic(&self, on: bool) {
        self.inner.lock().fail_semantic = on;
    }

    pub fn set_verifiable(&self, on: bool) {
        self.inner.lock().verifiable = on;
    }

    pub fn semantic_fallback(&self) -> u32 {
        self.inner.lock().semantic_fallback
    }

    pub fn last_dispatch_scope(&self) -> Option<ActionScope> {
        self.inner.lock().last_dispatch_scope
    }

    pub fn last_dispatch_target_id(&self) -> String {
        self.inner.lock().last_dispatch_target_id.clone()
    }

    pub fn last_dispatch_target(&self) -> Option<crate::protocol::ActionTarget> {
        self.inner.lock().last_dispatch_target.clone()
    }

    pub fn desktop_fallback(&self) -> u32 {
        self.inner.lock().desktop_fallback
    }

    pub fn observe_calls(&self) -> u32 {
        self.inner.lock().observe_calls
    }

    pub fn preview_ticks(&self) -> u32 {
        self.inner.lock().preview_ticks
    }

    pub fn abort_called(&self) -> u32 {
        self.inner.lock().abort_called
    }

    pub fn set_abort_blocked(&self, blocked: bool) {
        self.inner.lock().abort_blocked = blocked;
        if !blocked {
            self.cv.notify_all();
        }
    }

    pub fn wait_for_abort_call(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut guard = self.inner.lock();
        while guard.abort_called == 0 {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            self.cv.wait_for(&mut guard, deadline - now);
        }
        true
    }

    pub fn release_called(&self) -> u32 {
        self.inner.lock().release_called
    }

    pub fn adapter_in_flight(&self) -> u32 {
        self.inner.lock().in_flight
    }

    pub fn finish_in_flight(&self) {
        let mut g = self.inner.lock();
        g.in_flight = 0;
        g.hang = false;
        g.cancelled = true;
        self.cv.notify_all();
    }

    pub fn drive_preview_tick(&self) {
        let mut g = self.inner.lock();
        if g.preview_active {
            g.preview_ticks += 1;
        }
    }

    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Default for FakeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputerUseAdapter for FakeAdapter {
    fn backend_id(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::for_surface(crate::adapter::SurfaceKind::Desktop, "fake");
        caps.observe_ax = true;
        caps.notes = vec!["test adapter".into()];
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        let mut g = self.inner.lock();
        g.list_calls += 1;
        if g.fail_listing {
            return Err("fixture desktop enumeration unavailable".into());
        }
        if g.iconic {
            return Ok(Vec::new());
        }
        let mut out = vec![
            TargetInfo {
                target_id: g.fixture_id.clone(),
                title: "GrokCuFixture".into(),
                app_name: "fixture".into(),
                kind: "window".into(),
                pid: Some(std::process::id()),
                backend: "fake".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "fake-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "test window on fake desktop".into(),
            },
            TargetInfo {
                target_id: g.host_id.clone(),
                title: "Grok Dev".into(),
                app_name: "grok-app".into(),
                kind: "window".into(),
                pid: Some(std::process::id()),
                backend: "fake".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "fake-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "host window (not operable)".into(),
            },
        ];
        if g.include_decoy {
            out.push(TargetInfo {
                target_id: "fake:decoy:other".into(),
                title: "OtherApp".into(),
                app_name: "other".into(),
                kind: "window".into(),
                pid: Some(1),
                backend: "fake".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "fake-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "other window on fake desktop".into(),
            });
        }
        if g.include_overlay {
            out.push(TargetInfo {
                target_id: "fake:overlay:nv".into(),
                title: "NVIDIA GeForce Overlay".into(),
                app_name: "overlay".into(),
                kind: "window".into(),
                pid: Some(2),
                backend: "fake".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "fake-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "overlay chrome (skipped)".into(),
            });
        }
        if g.include_statusbar {
            out.push(TargetInfo {
                target_id: "fake:bar:status".into(),
                title: "StatusBarWnd".into(),
                app_name: "win32".into(),
                kind: "window".into(),
                pid: Some(3),
                backend: "fake".into(),
                execution_mode: "exclusive".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: "fake-display".into(),
                coordinate_space: "image-pixels".into(),
                scope_label: "status chrome (skipped)".into(),
            });
        }
        Ok(out)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        let g = self.inner.lock();
        g.target_alive && target_id == g.fixture_id
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        let mut g = self.inner.lock();
        if !g.target_alive || target_id != g.fixture_id {
            return Err("dead target".into());
        }
        g.observe_calls += 1;
        g.snapshot_seq += 1;
        let snap = format!("snap-{}", g.snapshot_seq);
        let geo = g.geometry;
        let blank = g.blank_image;
        let include_png = g.include_png;
        let node_count = g.node_count;
        let huge_png = g.huge_png;
        let origin = g.origin;
        let dpi_scale = g.dpi_scale;
        let truncate_ref = g.truncate_ref.clone();
        let stale = g.stale_observe_revision;
        drop(g);
        const TINY_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let png = if blank {
            None
        } else if huge_png {
            Some("A".repeat(crate::protocol::OBSERVATION_PNG_B64_CAP + 1))
        } else if include_png {
            Some(TINY_PNG.into())
        } else {
            None
        };
        let image = if blank {
            ObservationImage {
                width: 0,
                height: 0,
                content_id: "blank".into(),
                png_base64: None,
            }
        } else {
            ObservationImage {
                width: 64,
                height: 32,
                content_id: "fake-img".into(),
                png_base64: png,
            }
        };
        let nodes = (1..=node_count)
            .map(|i| {
                let node_ref = format!("n{i}");
                let truncated = truncate_ref.as_deref() == Some(node_ref.as_str());
                ObservationNode {
                    node_ref,
                    role: "button".into(),
                    name: if i == 1 {
                        "Count".into()
                    } else {
                        format!("N{i}")
                    },
                    actions: vec!["click".into()],
                    truncated,
                    x: if i == 1 { Some(0.0) } else { None },
                    y: if i == 1 { Some(0.0) } else { None },
                    width: if i == 1 { Some(20.0) } else { None },
                    height: if i == 1 { Some(20.0) } else { None },
                }
            })
            .collect();
        Ok(Observation {
            text: String::new(),
            version: PROTOCOL_VERSION,
            run_id: String::new(),
            target_id: target_id.to_string(),
            target_generation: 0,
            snapshot_id: snap,
            captured_at: chrono::Utc::now().to_rfc3339(),
            geometry_revision: if stale { geo.wrapping_add(99) } else { geo },
            coordinate_space: "image_pixels".into(),
            image,
            nodes,
            truncated: false,
            crop_x: 0,
            crop_y: 0,
            crop_width: 0,
            crop_height: 0,
            scale: dpi_scale,
            dpi: 96.0 * dpi_scale,
            origin_x: origin.0,
            origin_y: origin.1,
            topology_revision: 0,
        })
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        req.admit(self)?;
        if req.action == crate::protocol::ActionKind::Wait {
            return Ok(AdapterActResult {
                applied: false,
                outcome: None,
                postcondition_ok: false,
                verifiable: false,
                detail: "wait is host-owned".into(),
            });
        }
        let fail_semantic = self.inner.lock().fail_semantic;
        let mut effective = req.clone();
        let mut used_fallback = false;
        if matches!(
            effective.target,
            crate::protocol::ActionTarget::Element { .. }
        ) && fail_semantic
        {
            if effective.scope != ActionScope::Directed {
                self.inner.lock().desktop_fallback += 1;
                return Err("desktop fallback is forbidden".into());
            }
            effective.target = crate::protocol::ActionTarget::Coord { x: 32.0, y: 16.0 };
            used_fallback = true;
        }
        if effective.scope == ActionScope::Desktop {
            self.inner.lock().desktop_fallback += 1;
            return Err("desktop fallback is forbidden".into());
        }
        let delay;
        let hang;
        {
            let mut g = self.inner.lock();
            if !g.target_alive {
                return Err("dead target".into());
            }
            g.last_generation = effective.generation;
            g.last_dispatch_scope = Some(effective.scope);
            g.last_dispatch_target_id = effective.target_id.clone();
            g.last_dispatch_target = Some(effective.target.clone());
            if used_fallback {
                g.semantic_fallback += 1;
            }
            g.executions.push(effective.action_id.clone());
            if effective.action == crate::protocol::ActionKind::Click {
                g.last_click_button =
                    crate::protocol::click_button(&effective.parameters).to_string();
                g.last_click_count = crate::protocol::click_count(&effective.parameters);
            }
            if effective.action == crate::protocol::ActionKind::Key {
                g.last_key = effective
                    .parameters
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            let (ix, iy) = match &effective.target {
                crate::protocol::ActionTarget::Coord { x, y } => (*x, *y),
                crate::protocol::ActionTarget::Element { .. } => (32.0, 16.0),
            };
            g.last_virtual = Some((
                g.origin.0 + (ix * g.dpi_scale).round() as i32,
                g.origin.1 + (iy * g.dpi_scale).round() as i32,
            ));
            g.in_flight += 1;
            g.cancelled = false;
            delay = g.delay;
            hang = g.hang;
            self.cv.notify_all();
        }
        if hang {
            let mut g = self.inner.lock();
            while g.hang && !g.cancelled {
                self.cv.wait(&mut g);
            }
        } else if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let mut g = self.inner.lock();
        if !g.abort_does_not_quiesce {
            g.in_flight = g.in_flight.saturating_sub(1);
        }
        let cancelled = g.cancelled;
        let ok = g.verify_ok;
        let verifiable = g.verifiable;
        drop(g);
        if cancelled {
            return Err("action cancelled; dispatch revoked".into());
        }
        effective.cancellation.check()?;
        Ok(AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: ok && verifiable,
            verifiable,
            detail: "fake".into(),
        })
    }

    fn abort(&self, _run_id: &str, generation: u64) -> Result<(), String> {
        let mut g = self.inner.lock();
        g.abort_called += 1;
        g.last_generation = generation;
        self.cv.notify_all();
        while g.abort_blocked {
            self.cv.wait(&mut g);
        }
        g.cancelled = true;
        if !g.abort_does_not_quiesce {
            g.hang = false;
            g.in_flight = 0;
        }
        self.cv.notify_all();
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        self.inner.lock().in_flight == 0
    }

    fn start_periodic_preview(&self, _target_id: &str) {
        self.inner.lock().preview_active = true;
    }

    fn stop_periodic_preview(&self) {
        self.inner.lock().preview_active = false;
    }

    fn periodic_preview_active(&self) -> bool {
        self.inner.lock().preview_active
    }

    fn release_target_for_run(&self, _run_id: &str, _target_id: &str) {
        self.inner.lock().release_called += 1;
    }

    fn input_available(&self) -> bool {
        !self.inner.lock().session_locked
    }

    fn foreground_input_available(&self, target_id: &str) -> bool {
        let g = self.inner.lock();
        g.focus_on_fixture && target_id == g.fixture_id
    }

    fn current_geometry_revision(&self) -> u64 {
        self.inner.lock().geometry
    }

    fn user_input_active(&self) -> bool {
        self.inner.lock().user_input
    }
}
