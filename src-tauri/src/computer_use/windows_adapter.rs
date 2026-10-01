//! Windows desktop adapter (Win32). Directed targets only; never falls back to the desktop.

#![cfg(target_os = "windows")]

use std::cell::Cell;
use std::mem::size_of;
use std::sync::Once;

use grok_computer_use_core::native_action::NativeActionSlot;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, GetDIBits, GetWindowDC, MapWindowPoints, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPoint, EnumWindows, GetAncestor, GetClientRect, GetDesktopWindow,
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SendMessageW, SetCursorPos, GA_ROOT, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SETTEXT,
};

use super::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use super::protocol::{
    ActionKind, ActionTarget, Observation, ObservationImage, ObservationNode, PROTOCOL_VERSION,
};

pub struct WindowsAdapter {
    native: NativeActionSlot,
}

impl WindowsAdapter {
    pub fn new() -> Self {
        ensure_dpi_aware();
        Self {
            native: NativeActionSlot::default(),
        }
    }

    fn fallback_input(
        &self,
        req: &DispatchRequest,
        hwnd: HWND,
        pid: u32,
        stamp: u64,
        uncertain: &Cell<bool>,
    ) -> Result<String, String> {
        let gate = input::InputGate {
            adapter: self,
            request: req,
            uncertain,
        };
        gate.check()?;
        req.cancellation.check()?;
        if !self.target_alive(&req.target_id) {
            return Err("dead target".into());
        }
        if !self.input_available() {
            return Err("session locked; secure desktop owns input".into());
        }
        if !self.foreground_input_available(&req.target_id) {
            return Err("focus drifted; foreground input paused".into());
        }
        if self.user_input_active() {
            return Err("user input; run paused".into());
        }
        if !super::windows_identity::can_control_process(pid) {
            return Err("UIPI: elevated or inaccessible target".into());
        }
        if req.geometry_revision != 0
            && self.current_geometry_revision_for(&req.target_id) != req.geometry_revision
        {
            return Err("stale geometryRevision".into());
        }
        match req.action {
            ActionKind::Click => {
                resolve_client_point(hwnd, pid, stamp, &req.target).and_then(|(x, y)| {
                    send_click_client(
                        &gate,
                        hwnd,
                        x,
                        y,
                        super::protocol::click_button(&req.parameters),
                        super::protocol::click_count(&req.parameters),
                    )
                })
            }
            ActionKind::SetValue => {
                let text = req
                    .parameters
                    .get("text")
                    .or_else(|| req.parameters.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let child = resolve_child(hwnd, pid, stamp, &req.target)?;
                set_child_text(&gate, child, text)
            }
            ActionKind::TypeText => {
                let text = req
                    .parameters
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let via_clipboard =
                    req.parameters.get("via").and_then(|v| v.as_str()) == Some("clipboard");
                let child = resolve_child(hwnd, pid, stamp, &req.target)?;
                type_into_child(&gate, child, text, via_clipboard)
            }
            ActionKind::Key => {
                let vk = vk_from_params(&req.parameters)?;
                let child = resolve_child(hwnd, pid, stamp, &req.target)?;
                send_key(&gate, child, vk)
            }
            ActionKind::Scroll => {
                resolve_client_point(hwnd, pid, stamp, &req.target).and_then(|(x, y)| {
                    let delta = super::protocol::ScrollDelta::parse(&req.parameters)?;
                    send_scroll_client(&gate, hwnd, x, y, delta)
                })
            }
            ActionKind::Drag => {
                resolve_client_point(hwnd, pid, stamp, &req.target).and_then(|(x0, y0)| {
                    super::protocol::drag_destination(&req.parameters)
                        .and_then(|(x1, y1)| send_drag_client(&gate, hwnd, x0, y0, x1, y1))
                })
            }
            ActionKind::Wait => Err("wait is host-owned".into()),
        }
        .map(|_| "windows sendinput".into())
    }
}

/// Must run before creating any HWND so client pixels match SendInput.
pub fn ensure_dpi_aware() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    });
}

impl Default for WindowsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

struct EnumState {
    out: Vec<TargetInfo>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut EnumState);
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return true.into();
    }
    let mut title = [0u16; 512];
    let n = GetWindowTextW(hwnd, &mut title);
    if n <= 0 {
        return true.into();
    }
    let title = String::from_utf16_lossy(&title[..n as usize]);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let app_name =
        super::windows_identity::process_image_stem(pid).unwrap_or_else(|| "win32".into());
    if title.is_empty() || super::adapter::skip_desktop_target(&title, &app_name) {
        return true.into();
    }
    if super::windows_identity::is_protected_window(hwnd, pid) {
        return true.into();
    }
    if !super::windows_identity::can_control_process(pid) {
        return true.into();
    }
    let stamp = super::windows_identity::stamp_window(hwnd);
    state.out.push(TargetInfo {
        target_id: super::windows_identity::format_target_id(pid, hwnd, stamp),
        title: title.clone(),
        app_name,
        kind: "window".into(),
        pid: Some(pid),
        backend: "windows".into(),
        execution_mode: "exclusive".into(),
        replay_policy: "never".into(),
        lifecycle_stamp: stamp,
        display_id: "win32".into(),
        coordinate_space: "image-pixels".into(),
        scope_label: format!("Windows window “{title}”"),
    });
    true.into()
}

fn parse_hwnd(target_id: &str) -> Option<(u32, HWND, u64)> {
    super::windows_identity::parse_target_id(target_id)
}

const PW_RENDERFULLCONTENT: u32 = 2;

unsafe extern "system" {
    fn PrintWindow(hwnd: HWND, hdc_blt: HDC, n_flags: u32) -> i32;
}

fn mix_u64(h: u64, v: u64) -> u64 {
    h.wrapping_mul(31).wrapping_add(v)
}

pub fn topology_revision() -> u64 {
    unsafe {
        let mut h = 0x9e37_79b9_7f4a_7c15u64;
        for metric in [
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        ] {
            h = mix_u64(h, metric as u64);
        }
        let dpi = GetDpiForWindow(GetDesktopWindow()) as u64;
        mix_u64(h, dpi)
    }
}

/// Origin, client size, per-window DPI, minimized bit, and display topology.
pub fn window_geometry_revision(hwnd: HWND) -> u64 {
    unsafe {
        let mut h = topology_revision();
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let dpi = GetDpiForWindow(hwnd) as u64;
        h = mix_u64(h, origin.x as u64);
        h = mix_u64(h, origin.y as u64);
        h = mix_u64(h, rc.right as u64);
        h = mix_u64(h, rc.bottom as u64);
        h = mix_u64(h, dpi);
        mix_u64(h, u64::from(IsIconic(hwnd).as_bool()))
    }
}

fn target_is_foreground(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        if fg == hwnd {
            return true;
        }
        GetAncestor(fg, GA_ROOT) == hwnd
    }
}

fn encode_bgra_png(w: u32, h: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for chunk in bgra.as_chunks::<4>().0 {
        rgb.push(chunk[2]);
        rgb.push(chunk[1]);
        rgb.push(chunk[0]);
    }
    let img = image::RgbImage::from_raw(w, h, rgb).ok_or_else(|| "rgb buffer".to_string())?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

fn read_dib(mem: HDC, bmp: windows::Win32::Graphics::Gdi::HBITMAP, w: i32, h: i32) -> Vec<u8> {
    unsafe {
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );
        bgra
    }
}

fn capture_bitblt_client(hwnd: HWND) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let w = rect.right.max(1);
        let h = rect.bottom.max(1);
        let hdc = GetDC(Some(hwnd));
        if hdc.0.is_null() {
            return Err("GetDC failed".into());
        }
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        let _ = BitBlt(mem, 0, 0, w, h, Some(hdc), 0, 0, SRCCOPY);
        let bgra = read_dib(mem, bmp, w, h);
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), hdc);
        let png = encode_bgra_png(w as u32, h as u32, &bgra)?;
        Ok((w as u32, h as u32, png))
    }
}

fn capture_png(hwnd: HWND) -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            return Err("window is minimized; capture refused".into());
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return Err("window is not visible; capture refused".into());
        }
        let mut crect = RECT::default();
        GetClientRect(hwnd, &mut crect).map_err(|e| e.to_string())?;
        let cw = crect.right.max(1);
        let ch = crect.bottom.max(1);
        let mut wrect = RECT::default();
        GetWindowRect(hwnd, &mut wrect).map_err(|e| e.to_string())?;
        let ww = (wrect.right - wrect.left).max(1);
        let wh = (wrect.bottom - wrect.top).max(1);
        let hdc_win = GetWindowDC(Some(hwnd));
        if hdc_win.0.is_null() {
            return capture_bitblt_client(hwnd);
        }
        let mem = CreateCompatibleDC(Some(hdc_win));
        let bmp = CreateCompatibleBitmap(hdc_win, ww, wh);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        let mut printed = PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT);
        if printed == 0 {
            printed = PrintWindow(hwnd, mem, 0);
        }
        let bgra = read_dib(mem, bmp, ww, wh);
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), hdc_win);
        if printed == 0 {
            return capture_bitblt_client(hwnd);
        }
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let crop_x = (origin.x - wrect.left).clamp(0, ww - 1);
        let crop_y = (origin.y - wrect.top).clamp(0, wh - 1);
        let crop_w = cw.min(ww - crop_x).max(1);
        let crop_h = ch.min(wh - crop_y).max(1);
        let mut client = Vec::with_capacity((crop_w * crop_h * 4) as usize);
        for y in 0..crop_h as usize {
            let src = ((crop_y as usize + y) * ww as usize + crop_x as usize) * 4;
            let end = src + crop_w as usize * 4;
            if end > bgra.len() {
                return capture_bitblt_client(hwnd);
            }
            client.extend_from_slice(&bgra[src..end]);
        }
        let png = encode_bgra_png(crop_w as u32, crop_h as u32, &client)?;
        Ok((crop_w as u32, crop_h as u32, png))
    }
}

mod input;
mod native_target;
mod text;
#[cfg(feature = "computer-use-probe")]
pub(super) mod text_probe;
use self::input::*;

impl ComputerUseAdapter for WindowsAdapter {
    fn backend_id(&self) -> &'static str {
        "windows"
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::for_surface(super::adapter::SurfaceKind::Desktop, "windows");
        caps.observe_ax = true;
        caps.scroll = super::adapter::ActionSupport::BOTH;
        caps.notes = vec![
            "UIA Invoke/Value/Scroll first; SendInput only after foreground recheck".into(),
            "PrintWindow capture; minimized/hidden refused; geometry includes window origin/DPI"
                .into(),
            "UIPI / secure desktop not supported".into(),
        ];
        caps
    }

    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        if !self.input_available() {
            return Err("session locked; secure desktop owns input".into());
        }
        let mut state = EnumState { out: Vec::new() };
        unsafe {
            EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut _ as isize))
                .map_err(|e| e.to_string())?;
        }
        Ok(state.out)
    }

    fn target_alive(&self, target_id: &str) -> bool {
        let Some((pid, hwnd, stamp)) = parse_hwnd(target_id) else {
            return false;
        };
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            let mut live_pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut live_pid));
            if live_pid != pid {
                return false;
            }
            if super::windows_identity::is_protected_window(hwnd, live_pid) {
                return false;
            }
            if !super::windows_identity::can_control_process(live_pid) {
                return false;
            }
            super::windows_identity::read_stamp(hwnd) == Some(stamp)
        }
    }

    fn observe(&self, target_id: &str) -> Result<Observation, String> {
        let Some((pid, hwnd, stamp)) = parse_hwnd(target_id) else {
            return Err("invalid target".into());
        };
        if !self.target_alive(target_id) {
            return Err("dead target".into());
        }
        if !self.input_available() {
            return Err("session locked; secure desktop owns input".into());
        }
        if !super::windows_identity::can_control_process(pid) {
            return Err("UIPI: elevated or inaccessible target".into());
        }
        if unsafe { IsIconic(hwnd).as_bool() } {
            return Err("window is minimized; capture refused".into());
        }
        let (w, h, png) = {
            let mut last = "capture failed".to_string();
            let mut grabbed = None;
            for _ in 0..3 {
                match capture_png(hwnd) {
                    Ok(frame) => {
                        grabbed = Some(frame);
                        break;
                    }
                    Err(e) => {
                        last = e;
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                }
            }
            grabbed.ok_or(last)?
        };
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&png)
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let mut nodes = super::windows_uia::collect_nodes(hwnd, pid, stamp)?;
        nodes.push(ObservationNode {
            node_ref: "meta:dpi".into(),
            role: "meta".into(),
            name: format!("{dpi}"),
            actions: vec![],
            truncated: true,
            ..Default::default()
        });
        let origin = unsafe {
            let mut pt = POINT { x: 0, y: 0 };
            let _ = ClientToScreen(hwnd, &mut pt);
            (pt.x, pt.y)
        };
        Ok(Observation {
            text: String::new(),
            version: PROTOCOL_VERSION,
            run_id: String::new(),
            target_id: target_id.to_string(),
            target_generation: 0,
            snapshot_id: format!("win-snap-{}", uuid::Uuid::new_v4()),
            captured_at: chrono::Utc::now().to_rfc3339(),
            geometry_revision: window_geometry_revision(hwnd),
            coordinate_space: crate::computer_use::protocol::COORDINATE_SPACE_IMAGE_PIXELS.into(),
            image: ObservationImage {
                width: w,
                height: h,
                content_id: format!("png-{}", png.len()),
                png_base64: Some(b64),
            },
            nodes,
            truncated: false,
            crop_x: 0,
            crop_y: 0,
            crop_width: w,
            crop_height: h,
            scale: dpi as f64 / 96.0,
            dpi: dpi as f64,
            origin_x: origin.0,
            origin_y: origin.1,
            topology_revision: topology_revision(),
        })
    }

    fn act(&self, req: &DispatchRequest) -> Result<AdapterActResult, String> {
        let mut owner = self
            .native
            .begin(&req.run_id, req.generation, &req.cancellation)?;
        let uncertain = Cell::new(false);
        let gate = input::InputGate {
            adapter: self,
            request: req,
            uncertain: &uncertain,
        };
        req.admit(self)?;
        if !self.input_available() {
            return Err("session locked; secure desktop owns input".into());
        }
        if self.user_input_active() {
            return Err("user input; run paused".into());
        }
        let (pid, hwnd, stamp) = parse_hwnd(&req.target_id).ok_or("invalid target")?;
        if !super::windows_identity::can_control_process(pid) {
            return Err("UIPI: elevated or inaccessible target".into());
        }
        if req.action == ActionKind::Scroll
            && super::protocol::ScrollDelta::parse(&req.parameters)?.value() == 0
        {
            return Ok(AdapterActResult {
                applied: false,
                outcome: None,
                postcondition_ok: false,
                verifiable: false,
                detail: "zero scroll delta; no native event posted".into(),
            });
        }
        let uia = super::windows_uia::perform(
            hwnd,
            pid,
            stamp,
            req.action,
            &req.target,
            &req.parameters,
            &|| gate.check(),
        );
        let result = match uia {
            Ok(Some(detail)) => {
                return Ok(AdapterActResult {
                    applied: true,
                    outcome: None,
                    postcondition_ok: false,
                    verifiable: false,
                    detail: detail.into(),
                });
            }
            Ok(None) => self.fallback_input(req, hwnd, pid, stamp, &uncertain),
            Err(e) => Err(e),
        };
        if uncertain.get() {
            owner.retain_until_native_recovery();
        }
        result.map(|detail| AdapterActResult {
            applied: true,
            outcome: None,
            postcondition_ok: false,
            verifiable: false,
            detail,
        })
    }

    fn abort(&self, run_id: &str, generation: u64) -> Result<(), String> {
        // Revocation is not native completion. Only the executing owner may
        // release its slot or finish an input pair that it actually started.
        self.native.cancel_before(run_id, generation);
        Ok(())
    }

    fn is_idle(&self, _run_id: &str) -> bool {
        self.native.is_idle()
    }

    fn start_periodic_preview(&self, _target_id: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }

    fn input_available(&self) -> bool {
        super::windows_identity::session_input_available()
    }

    fn user_input_active(&self) -> bool {
        input::user_input_held()
    }

    fn foreground_input_available(&self, target_id: &str) -> bool {
        let Some((_, hwnd, _)) = parse_hwnd(target_id) else {
            return false;
        };
        if !self.target_alive(target_id) {
            return false;
        }
        target_is_foreground(hwnd)
    }

    fn current_geometry_revision(&self) -> u64 {
        topology_revision()
    }

    fn current_geometry_revision_for(&self, target_id: &str) -> u64 {
        parse_hwnd(target_id)
            .map(|(_, hwnd, _)| window_geometry_revision(hwnd))
            .unwrap_or_else(topology_revision)
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use grok_computer_use_core::execution::ActionCancellation;

    #[test]
    fn adapter_stop_fences_only_its_older_generation_without_claiming_idle() {
        let adapter = WindowsAdapter::new();
        let token = ActionCancellation::default();
        let owner = adapter.native.begin("run", 5, &token).unwrap();
        adapter.abort("other-run", 100).unwrap();
        adapter.abort("run", 5).unwrap();
        assert!(token.check().is_ok());
        adapter.abort("run", 6).unwrap();
        assert!(token.check().is_err());
        assert!(!adapter.is_idle("run"));
        assert!(!adapter.is_idle("other-run"));
        drop(owner);
        assert!(adapter.is_idle("run"));
    }

    #[test]
    fn failed_cleanup_cannot_be_cleared_by_adapter_stop() {
        let adapter = WindowsAdapter::new();
        let token = ActionCancellation::default();
        let mut owner = adapter.native.begin("run", 1, &token).unwrap();
        owner.retain_until_native_recovery();
        drop(owner);
        adapter.abort("run", 2).unwrap();
        assert!(token.check().is_err());
        assert!(!adapter.is_idle("run"));
        assert!(adapter.native.begin("run", 3, &Default::default()).is_err());
    }
}
