//! A real GTK3 Wayland parent export, owned and retired only on the GTK thread.
//! No parent string supplied by a model, display-environment guess, or X11 fallback.
#![cfg(target_os = "linux")]

use glib::translate::ToGlibPtr;
use grok_computer_use_core::session_grants::AuthorizationTicket;
use gtk::{gdk, glib, prelude::*};
use std::{
    ffi::CStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentState {
    Pending,
    Ready {
        handle: String,
    },
    Closing,
    /// This export's GDK reference has been released on its original UI thread.
    /// Not a claim about portal session cleanup or compositor acknowledgement.
    Closed,
}

struct Shared {
    ticket: AuthorizationTicket,
    stop: AtomicBool,
    lost: AtomicBool,
    state: watch::Sender<ParentState>,
}

impl Shared {
    fn revoked(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
            || self.lost.load(Ordering::SeqCst)
            || self.ticket.is_preparation_cancelled()
    }
    fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.state.send_if_modified(|state| {
            if matches!(state, ParentState::Closed | ParentState::Closing) {
                return false;
            }
            *state = ParentState::Closing;
            true
        });
    }
}

/// Sendable Host-only lease. Dropping it requests cleanup; only the retained
/// GTK owner may publish Closed. Never serialize this lease into a model tool.
pub struct ParentLease {
    shared: Arc<Shared>,
}

impl ParentLease {
    pub fn state(&self) -> ParentState {
        let state = self.shared.state.borrow().clone();
        if self.shared.revoked() && !matches!(state, ParentState::Closed) {
            ParentState::Closing
        } else {
            state
        }
    }
    pub fn handle(&self) -> Result<Option<String>, String> {
        match self.state() {
            ParentState::Ready { handle } => Ok(Some(handle)),
            ParentState::Pending => Ok(None),
            _ => Err("native parent export was revoked".into()),
        }
    }
    pub fn request_close(&self) {
        self.shared.close();
    }
    pub async fn ready(&self, deadline: Duration) -> Result<String, String> {
        let mut state = self.shared.state.subscribe();
        let wait = async {
            loop {
                if let Some(handle) = self.handle()? {
                    return Ok(handle);
                }
                tokio::select! {
                    biased;
                    _ = self.shared.ticket.preparation_cancelled() => return Err("parent authorization cancelled".into()),
                    result = state.changed() => result.map_err(|_| "native parent owner unavailable".to_owned())?,
                }
            }
        };
        match tokio::time::timeout(deadline, wait).await {
            Ok(Ok(handle)) => Ok(handle),
            result => {
                self.request_close();
                match result {
                    Ok(Err(error)) => Err(error),
                    _ => Err("native parent export deadline".into()),
                }
            }
        }
    }
    pub async fn closed(&self) -> Result<(), String> {
        let mut state = self.shared.state.subscribe();
        loop {
            if matches!(self.state(), ParentState::Closed) {
                return Ok(());
            }
            state
                .changed()
                .await
                .map_err(|_| "native parent retirement unproven".to_owned())?;
        }
    }
}
impl Drop for ParentLease {
    fn drop(&mut self) {
        self.request_close();
    }
}

impl grok_computer_use_core::native_parent::NativeParent for ParentLease {
    fn authorization(&self) -> &AuthorizationTicket {
        &self.shared.ticket
    }
    fn handle(&self) -> Result<Option<String>, String> {
        ParentLease::handle(self)
    }
    fn is_revoked(&self) -> bool {
        self.shared.revoked()
    }
    fn request_close(&self) {
        ParentLease::request_close(self);
    }
    fn is_closed(&self) -> bool {
        self.state() == ParentState::Closed
    }
}

// GTK3's exported closure destroy_notify also runs immediately after successful
// callback delivery. It does NOT mean the export has been unexported. Balance
// exactly one successful export with one unexport, independently of user_data.
#[link(name = "gdk-3")]
extern "C" {
    fn gdk_wayland_window_get_type() -> glib::ffi::GType;
    fn gdk_wayland_window_export_handle(
        window: *mut gdk::ffi::GdkWindow,
        callback: Option<
            unsafe extern "C" fn(*mut gdk::ffi::GdkWindow, *const libc::c_char, *mut libc::c_void),
        >,
        data: *mut libc::c_void,
        destroy: Option<unsafe extern "C" fn(*mut libc::c_void)>,
    ) -> libc::c_int;
    fn gdk_wayland_window_unexport_handle(window: *mut gdk::ffi::GdkWindow);
}

unsafe extern "C" fn exported(
    _: *mut gdk::ffi::GdkWindow,
    handle: *const libc::c_char,
    data: *mut libc::c_void,
) {
    let shared = &*data.cast::<Arc<Shared>>();
    if handle.is_null() || shared.revoked() {
        shared.close();
        return;
    }
    // The native ABI guarantees a NUL-terminated string for this callback.
    let bytes = CStr::from_ptr(handle).to_bytes();
    let value = std::str::from_utf8(bytes)
        .ok()
        .filter(|s| !s.is_empty() && s.len() <= 4088 && !s.chars().any(char::is_control));
    match value {
        Some(handle) => {
            shared.state.send_if_modified(|state| {
                if shared.revoked() || !matches!(state, ParentState::Pending) {
                    return false;
                }
                *state = ParentState::Ready {
                    handle: format!("wayland:{handle}"),
                };
                true
            });
        }
        None => shared.close(),
    }
}
unsafe extern "C" fn destroy_callback(data: *mut libc::c_void) {
    drop(Box::from_raw(data.cast::<Arc<Shared>>()));
}

struct Owner {
    window: gtk::Window,
    native: gdk::Window,
    shared: Arc<Shared>,
    destroyed: Option<glib::SignalHandlerId>,
    unmapped: Option<glib::SignalHandlerId>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.close();
        for handler in [self.destroyed.take(), self.unmapped.take()]
            .into_iter()
            .flatten()
        {
            // GtkWidget destruction can already disconnect these signals.
            // Check on the same UI thread before disconnecting our exact ID.
            let connected = unsafe {
                glib::gobject_ffi::g_signal_handler_is_connected(
                    self.window.as_ptr().cast(),
                    handler.as_raw(),
                ) != 0
            };
            if connected {
                self.window.disconnect(handler);
            }
        }
        // Destroying/unmapping a GdkWindow destroys its wl_surface, not its
        // export references. Our strong reference prevents GDK finalization;
        // balance this owner's successful export even after window.destroy().
        unsafe {
            gdk_wayland_window_unexport_handle(self.native.to_glib_none().0);
        }
        self.shared.state.send_replace(ParentState::Closed);
    }
}

/// Must be called by the real GTK main thread with the actual mapped Host
/// window. The returned lease is Send; GTK objects never leave this thread.
pub fn export(window: &gtk::Window, ticket: &AuthorizationTicket) -> Result<ParentLease, String> {
    if !gtk::is_initialized_main_thread() {
        return Err("parent export requires GTK main thread".into());
    }
    ticket.check_preparation()?;
    // xdg-foreign exports toplevels. A popup has a GdkWaylandWindow too, but
    // exporting its surface can terminate the Wayland connection.
    if window.window_type() != gtk::WindowType::Toplevel {
        return Err("native parent must be a GTK toplevel".into());
    }
    if !window.is_mapped() {
        return Err("native parent is not mapped".into());
    }
    let native = window.window().ok_or("native parent has no GDK window")?;
    if native.is_destroyed() {
        return Err("native parent is destroyed".into());
    }
    let ptr: *mut gdk::ffi::GdkWindow = native.to_glib_none().0;
    if unsafe {
        glib::gobject_ffi::g_type_check_instance_is_a(ptr.cast(), gdk_wayland_window_get_type())
    } == 0
    {
        return Err("native parent is not Wayland; X11 fallback is forbidden".into());
    }
    let (state, _) = watch::channel(ParentState::Pending);
    let shared = Arc::new(Shared {
        ticket: ticket.clone(),
        stop: AtomicBool::new(false),
        lost: AtomicBool::new(false),
        state,
    });
    let data = Box::into_raw(Box::new(shared.clone())).cast();
    let accepted = unsafe {
        gdk_wayland_window_export_handle(ptr, Some(exported), data, Some(destroy_callback))
    } != 0;
    if !accepted {
        // GDK transfers user_data only on TRUE; no native export was acquired.
        unsafe {
            destroy_callback(data);
        }
        return Err("compositor did not accept native parent export".into());
    }
    let lost = shared.clone();
    let destroyed = window.connect_destroy(move |_| {
        lost.lost.store(true, Ordering::SeqCst);
        lost.close();
    });
    let lost = shared.clone();
    let unmapped = window.connect_unmap(move |_| {
        lost.lost.store(true, Ordering::SeqCst);
        lost.close();
    });
    let owner = Owner {
        window: window.clone(),
        native,
        shared: shared.clone(),
        destroyed: Some(destroyed),
        unmapped: Some(unmapped),
    };
    let mut owner = Some(owner);
    glib::timeout_add_local(Duration::from_millis(10), move || {
        let Some(current) = owner.as_ref() else {
            return glib::ControlFlow::Break;
        };
        if current.shared.revoked() || current.native.is_destroyed() {
            drop(owner.take());
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    Ok(ParentLease { shared })
}

#[cfg(feature = "native-probe")]
pub mod probe;
