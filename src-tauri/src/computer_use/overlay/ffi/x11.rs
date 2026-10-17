//! The same _GCU_OVERLAY capture-lease contract as route A. All calls are on
//! GTK's main thread, on its Display; no server grab or action-queue dependency.
use gtk::{glib::object::ObjectType, prelude::*};
use std::{
    ffi::{c_char, c_int, c_long, c_ulong, c_void, CStr},
    ptr,
};
use tauri::WebviewWindow;
type Display = *mut c_void;
type Xid = c_ulong;

#[link(name = "gdk-3")]
extern "C" {
    fn gdk_x11_window_get_xid(window: *mut gtk::gdk::ffi::GdkWindow) -> Xid;
    fn gdk_x11_display_get_xdisplay(display: *mut gtk::gdk::ffi::GdkDisplay) -> Display;
    fn gdk_x11_display_error_trap_push(display: *mut gtk::gdk::ffi::GdkDisplay);
    fn gdk_x11_display_error_trap_pop(display: *mut gtk::gdk::ffi::GdkDisplay) -> c_int;
}
#[link(name = "X11")]
extern "C" {
    fn XInternAtom(display: Display, name: *const c_char, only: c_int) -> Xid;
    fn XDefaultRootWindow(display: Display) -> Xid;
    fn XQueryTree(
        display: Display,
        window: Xid,
        root: *mut Xid,
        parent: *mut Xid,
        children: *mut *mut Xid,
        count: *mut u32,
    ) -> c_int;
    fn XGetWindowProperty(
        display: Display,
        window: Xid,
        property: Xid,
        offset: c_long,
        length: c_long,
        delete: c_int,
        requested_type: Xid,
        actual_type: *mut Xid,
        format: *mut c_int,
        items: *mut c_ulong,
        remaining: *mut c_ulong,
        data: *mut *mut u8,
    ) -> c_int;
    fn XChangeProperty(
        display: Display,
        window: Xid,
        property: Xid,
        kind: Xid,
        format: c_int,
        mode: c_int,
        data: *const u8,
        count: c_int,
    ) -> c_int;
    fn XUnmapWindow(display: Display, window: Xid) -> c_int;
    fn XMapRaised(display: Display, window: Xid) -> c_int;
    fn XSync(display: Display, discard: c_int) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
}
struct Window {
    gdk: gtk::gdk::Window,
    display: gtk::gdk::Display,
    x: Display,
    id: Xid,
}
fn window(win: &WebviewWindow) -> Result<Window, String> {
    if std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
    {
        return Err("overlay unavailable on Wayland/XWayland".into());
    }
    let gtk = win.gtk_window().map_err(|e| e.to_string())?;
    gtk.realize();
    let gdk = gtk.window().ok_or("missing overlay GDK window")?;
    if !gdk.type_().name().contains("X11") {
        return Err("overlay unavailable on Wayland".into());
    }
    let display = gdk.display();
    // SAFETY: The actual GDK instance was checked to be X11 and is retained.
    let (x, id) = unsafe {
        (
            gdk_x11_display_get_xdisplay(display.as_ptr()),
            gdk_x11_window_get_xid(gdk.as_ptr()),
        )
    };
    if x.is_null() || id == 0 {
        return Err("missing overlay X11 handles".into());
    }
    Ok(Window {
        gdk,
        display,
        x,
        id,
    })
}
fn atom(w: &Window, name: &CStr) -> Xid {
    // SAFETY: Valid main-thread Display and NUL-terminated static name.
    unsafe { XInternAtom(w.x, name.as_ptr(), 0) }
}
fn property(w: &Window, id: Xid, key: Xid) -> Result<Vec<Xid>, String> {
    let (mut kind, mut format, mut items, mut left, mut data) = (0, 0, 0, 0, ptr::null_mut());
    // SAFETY: Out pointers are initialized and live. GDK's error trap handles
    // windows disappearing between QueryTree and GetProperty. XFree owns data.
    unsafe {
        gdk_x11_display_error_trap_push(w.display.as_ptr());
        let status = XGetWindowProperty(
            w.x,
            id,
            key,
            0,
            4,
            0,
            6,
            &mut kind,
            &mut format,
            &mut items,
            &mut left,
            &mut data,
        );
        let trapped = gdk_x11_display_error_trap_pop(w.display.as_ptr());
        let result = if trapped == 3 {
            Ok(Vec::new())
        }
        // BadWindow: a retired capture lease.
        else if status != 0 || trapped != 0 {
            Err("overlay X11 property failed".into())
        } else if kind == 6 && format == 32 && items == 4 && left == 0 && !data.is_null() {
            Ok(std::slice::from_raw_parts(data.cast::<Xid>(), items as usize).to_vec())
        } else {
            Ok(Vec::new())
        };
        if !data.is_null() {
            XFree(data.cast());
        }
        result
    }
}
fn write(w: &Window, key: Xid, values: &[Xid]) -> Result<(), String> {
    // SAFETY: Xlib format=32 consumes a native-long array. The retained window
    // is owned by this main-thread GTK instance; the slice survives the call.
    unsafe {
        gdk_x11_display_error_trap_push(w.display.as_ptr());
        XChangeProperty(
            w.x,
            w.id,
            key,
            6,
            32,
            0,
            values.as_ptr().cast(),
            values.len() as c_int,
        );
        XSync(w.x, 0);
        if gdk_x11_display_error_trap_pop(w.display.as_ptr()) != 0 {
            return Err("overlay X11 acknowledgment failed".into());
        }
    }
    Ok(())
}
pub fn configure(win: &WebviewWindow) -> Result<(), String> {
    let w = window(win)?;
    if !w.gdk.screen().is_composited() {
        return Err("transparent overlay requires an X11 compositor".into());
    }
    // Bypass WM focus policies. The WebView still receives pointer events;
    // WM_HINTS input=False and override-redirect keep keyboard focus in chat.
    w.gdk.set_override_redirect(true);
    let gtk = win.gtk_window().map_err(|e| e.to_string())?;
    gtk.set_accept_focus(false);
    gtk.set_focus_on_map(false);
    gtk.set_show_menubar(false);
    Ok(())
}
pub fn passthrough(win: &WebviewWindow, cursor: bool) -> Result<(), String> {
    let w = window(win)?;
    if cursor {
        // Unlike Tao's asynchronous 1x1 workaround, this is an empty native
        // input region applied before the first map, with no intercepted pixel.
        w.gdk
            .input_shape_combine_region(&gtk::cairo::Region::create(), 0, 0);
    }
    // SAFETY: Flush the main-thread Display before a surface may be shown.
    unsafe {
        XSync(w.x, 0);
    }
    Ok(())
}
pub fn register(win: &WebviewWindow) -> Result<(), String> {
    let w = window(win)?;
    write(&w, atom(&w, c"_GCU_OVERLAY"), &[std::process::id().into()])?;
    write(
        &w,
        atom(&w, c"_GCU_OVERLAY_HOST"),
        &[std::process::id().into()],
    )
}
pub fn present(draw: &WebviewWindow, controls: &WebviewWindow) -> Result<(), String> {
    let a = window(draw)?;
    let b = window(controls)?;
    let mut leases = Vec::new();
    let (mut root, mut parent, mut count, mut children) = (0, 0, 0, ptr::null_mut());
    // SAFETY: Display retained by GDK, initialized output pointers. XQueryTree
    // returns an XFree-owned array; it is copied/freed before any early return.
    let ids = unsafe {
        if XQueryTree(
            a.x,
            XDefaultRootWindow(a.x),
            &mut root,
            &mut parent,
            &mut children,
            &mut count,
        ) == 0
        {
            return Err("overlay X11 capture query failed".into());
        }
        let ids = if children.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(children, count as usize).to_vec()
        };
        if !children.is_null() {
            XFree(children.cast());
        }
        ids
    };
    let capture = atom(&a, c"_GCU_OVERLAY_CAPTURE");
    for id in ids {
        let nonce = property(&a, id, capture)?;
        if nonce.len() == 4 {
            if leases.len() >= 64 * 5 {
                return Err("too many capture leases".into());
            }
            leases.push(id);
            leases.extend(nonce);
        }
    }
    let proofs: Vec<[u32; 5]> = leases
        .as_chunks::<5>()
        .0
        .iter()
        .map(|p| (*p).map(|v| v as u32))
        .collect();
    gcu_host_protocol::overlay::capture::capture_barrier(
        &mut CapturePair {
            draw,
            controls,
            a,
            b,
        },
        &proofs,
    )
}
struct CapturePair<'a> {
    draw: &'a WebviewWindow,
    controls: &'a WebviewWindow,
    a: Window,
    b: Window,
}
impl gcu_host_protocol::overlay::capture::CaptureSurface for CapturePair<'_> {
    type Error = String;
    fn hide_all(&mut self) -> Result<(), String> {
        // SAFETY: Both hides and nonce acknowledgments use the same retained
        // main-thread Display. Sync orders unmaps before any capture proof.
        unsafe {
            gdk_x11_display_error_trap_push(self.a.display.as_ptr());
            XUnmapWindow(self.a.x, self.a.id);
            XUnmapWindow(self.b.x, self.b.id);
            XSync(self.a.x, 0);
            if gdk_x11_display_error_trap_pop(self.a.display.as_ptr()) != 0 {
                return Err("overlay hide failed; capture cannot proceed".into());
            }
        }
        Ok(())
    }
    fn acknowledge(&mut self, proofs: &[[u32; 5]]) -> Result<(), String> {
        let leases: Vec<Xid> = proofs.iter().flatten().map(|v| Xid::from(*v)).collect();
        write(&self.a, atom(&self.a, c"_GCU_OVERLAY_ACK"), &leases)?;
        write(&self.b, atom(&self.b, c"_GCU_OVERLAY_ACK"), &leases)
    }
    fn show_all(&mut self) -> Result<(), String> {
        for win in [self.draw, self.controls] {
            if !win.is_visible().map_err(|e| e.to_string())? {
                win.show().map_err(|e| e.to_string())?;
            }
        }
        // SAFETY: No active capture lease. Mapping override-redirect windows
        // changes only stacking and visibility, never keyboard focus.
        unsafe {
            XMapRaised(self.a.x, self.a.id);
            XMapRaised(self.b.x, self.b.id);
            XSync(self.a.x, 0);
        }
        Ok(())
    }
}
