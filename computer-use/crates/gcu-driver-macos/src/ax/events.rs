//! Accessibility notifications used to settle a tree.
//!
//! [`EventHub`] is the in-process stream. [`FakeAxEvents`] feeds it in tests.
//! On macOS, [`ObserverPool`] registers an `AXObserver` on `gcu-macos-ax-observer`
//! and runs a `CFRunLoop` until [`ObserverPool::stop`] or Drop. The run-loop
//! source returned by AX is +0 and is not released. The observer and the
//! application element are +1 and are released after the loop stops, and only
//! then is the callback's `Arc` reclaimed.
//!
//! `AXValueChanged`, `AXFocusedUIElementChanged`, and `AXWindowCreated` become
//! [`UiEvent::Changed`]. `AXLayoutChanged` becomes [`UiEvent::GeometryChanged`].
//! Element destruction is not mapped to `WindowGone`.

use async_trait::async_trait;
use futures_core::Stream;
use gcu_core::driver::{BoxStream, UiEvent};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    thread::JoinHandle,
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Subscribes to accessibility changes for one window.
#[async_trait]
pub trait AxEventSource: Send + Sync {
    /// A stream of settle events. `None` only when the caller passed an empty id
    /// to a source that requires one. The hub itself always returns a stream.
    fn subscribe(&self, window_id: &str) -> Option<BoxStream<'static, UiEvent>>;
}

/// Fan-out for UI events. Cloning shares the sender list.
#[derive(Clone, Debug, Default)]
pub struct EventHub {
    senders: Arc<Mutex<Vec<mpsc::UnboundedSender<UiEvent>>>>,
}

impl EventHub {
    /// No subscribers yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// A stream that receives later [`EventHub::emit`] calls.
    pub fn subscribe(&self) -> BoxStream<'static, UiEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        if let Ok(mut senders) = self.senders.lock() {
            senders.retain(|sender| !sender.is_closed());
            senders.push(tx);
        }
        Box::pin(EventStream { rx })
    }

    /// Deliver one event. Closed subscribers are dropped.
    pub fn emit(&self, event: UiEvent) {
        if let Ok(mut senders) = self.senders.lock() {
            senders.retain(|sender| sender.send(event.clone()).is_ok());
        }
    }
}

/// Test source. [`FakeAxEvents::emit`] wakes every active subscriber.
#[derive(Clone, Debug, Default)]
pub struct FakeAxEvents {
    hub: EventHub,
}

impl FakeAxEvents {
    /// Empty hub.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push `event` to current subscribers.
    pub fn emit(&self, event: UiEvent) {
        self.hub.emit(event);
    }
}

#[async_trait]
impl AxEventSource for FakeAxEvents {
    fn subscribe(&self, _window_id: &str) -> Option<BoxStream<'static, UiEvent>> {
        Some(self.hub.subscribe())
    }
}

struct EventStream {
    rx: mpsc::UnboundedReceiver<UiEvent>,
}

impl Stream for EventStream {
    type Item = UiEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Portable stand-in for the observer thread's start/stop/Drop lifecycle.
///
/// The thread loops until [`RunLoopControl::stop`] or Drop, then sets
/// [`RunLoopControl::finished`].
pub struct RunLoopControl {
    stop: Arc<std::sync::atomic::AtomicBool>,
    finished: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RunLoopControl {
    /// Start the loop thread.
    pub fn start() -> Self {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let done = Arc::clone(&finished);
        let thread = std::thread::Builder::new()
            .name("gcu-macos-ax-observer".into())
            .spawn(move || {
                while !stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                done.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();
        Self {
            stop,
            finished,
            thread,
        }
    }

    /// Ask the thread to leave the loop and wait for it.
    pub fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// True after the thread has left its loop.
    pub fn finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Drop for RunLoopControl {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `cg:{window}:{pid}`.
pub fn parse_window_pid(id: &str) -> Option<u32> {
    let mut parts = id.split(':');
    if parts.next() != Some("cg") {
        return None;
    }
    let _window = parts.next()?;
    parts.next()?.parse().ok()
}

/// Live `AXObserver` threads, one per pid. Compiled on macOS.
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct ObserverPool {
    handles: Mutex<HashMap<u32, ObserverHandle>>,
}

#[cfg(target_os = "macos")]
struct ObserverHandle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    finished: Arc<std::sync::atomic::AtomicBool>,
    loop_ptr: Arc<std::sync::atomic::AtomicPtr<std::ffi::c_void>>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl ObserverPool {
    /// No observers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the run loop for `pid` if it is not already running.
    ///
    /// Failure to create the observer leaves the hub stream quiet. This does
    /// not block on the first AX call beyond spawning the thread.
    pub fn start(&self, pid: u32, hub: EventHub) {
        let Ok(mut handles) = self.handles.lock() else {
            return;
        };
        if handles.contains_key(&pid) {
            return;
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let loop_ptr = Arc::new(std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()));
        let stop_flag = Arc::clone(&stop);
        let done = Arc::clone(&finished);
        let loop_slot = Arc::clone(&loop_ptr);
        let thread = std::thread::Builder::new()
            .name("gcu-macos-ax-observer".into())
            .spawn(move || {
                let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    live::run(pid, hub, &stop_flag, &loop_slot);
                }));
                if joined.is_err() {
                    tracing::debug!(pid, "accessibility observer thread panicked");
                }
                done.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();
        handles.insert(
            pid,
            ObserverHandle {
                stop,
                finished,
                loop_ptr,
                thread,
            },
        );
    }

    /// Stop every observer and join its thread.
    pub fn stop(&self) {
        let Ok(mut handles) = self.handles.lock() else {
            return;
        };
        for handle in handles.values_mut() {
            stop_handle(handle);
        }
        handles.clear();
    }
}

#[cfg(target_os = "macos")]
impl Drop for ObserverPool {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
fn stop_handle(handle: &mut ObserverHandle) {
    use core_foundation::base::TCFType;
    handle.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    if !handle.finished.load(std::sync::atomic::Ordering::SeqCst) {
        let ptr = handle.loop_ptr.load(std::sync::atomic::Ordering::SeqCst);
        if !ptr.is_null() {
            // SAFETY: the observer thread published this run-loop pointer and
            // has not set `finished` yet, so the run loop is still alive.
            // Get-rule retains for this call; Drop releases that retain only.
            let run_loop =
                unsafe { core_foundation::runloop::CFRunLoop::wrap_under_get_rule(ptr as _) };
            run_loop.stop();
        }
    }
    if let Some(thread) = handle.thread.take() {
        let _ = thread.join();
    }
}

#[cfg(target_os = "macos")]
mod live {
    use super::EventHub;
    use accessibility_sys::{
        kAXErrorSuccess, kAXFocusedUIElementChangedNotification, kAXLayoutChangedNotification,
        kAXValueChangedNotification, kAXWindowCreatedNotification, AXObserverAddNotification,
        AXObserverCreate, AXObserverGetRunLoopSource, AXObserverRef, AXObserverRemoveNotification,
        AXUIElementCreateApplication, AXUIElementRef, AXUIElementSetMessagingTimeout,
    };
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
    use core_foundation::string::CFString;
    use gcu_core::driver::UiEvent;
    use std::ffi::c_void;
    use std::sync::{
        atomic::{AtomicBool, AtomicPtr, Ordering},
        Arc,
    };
    use std::time::Duration;

    const NOTES: &[&str] = &[
        kAXValueChangedNotification,
        kAXFocusedUIElementChangedNotification,
        kAXWindowCreatedNotification,
        kAXLayoutChangedNotification,
    ];

    pub(super) fn run(pid: u32, hub: EventHub, stop: &AtomicBool, loop_ptr: &AtomicPtr<c_void>) {
        let Some(pid) = i32::try_from(pid).ok() else {
            return;
        };
        // SAFETY: the rest of this function creates the observer, and `Guard`
        // releases every +1 before it returns, including on early return.
        unsafe { run_inner(pid, hub, stop, loop_ptr) }
    }

    struct Guard {
        app: AXUIElementRef,
        observer: AXObserverRef,
        hub: *const EventHub,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.observer.is_null() {
                // SAFETY: AXObserverCreate returned +1. Releasing it stops callbacks.
                unsafe { CFRelease(self.observer as CFTypeRef) };
                self.observer = std::ptr::null_mut();
            }
            if !self.app.is_null() {
                // SAFETY: AXUIElementCreateApplication returned +1.
                unsafe { CFRelease(self.app as CFTypeRef) };
                self.app = std::ptr::null_mut();
            }
            if !self.hub.is_null() {
                // SAFETY: `hub` came from Arc::into_raw and callbacks are gone
                // because the observer was released first.
                drop(unsafe { Arc::from_raw(self.hub) });
                self.hub = std::ptr::null();
            }
        }
    }

    unsafe fn run_inner(pid: i32, hub: EventHub, stop: &AtomicBool, loop_ptr: &AtomicPtr<c_void>) {
        // SAFETY: pid names a process. Create returns +1, owned by `guard`.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        let mut guard = Guard {
            app,
            observer: std::ptr::null_mut(),
            hub: std::ptr::null(),
        };
        if app.is_null() {
            tracing::debug!(pid, "accessibility observer application element was null");
            return;
        }
        // SAFETY: app is the +1 element above. 1s caps a hung notification port.
        unsafe { AXUIElementSetMessagingTimeout(app, 1.0) };
        let mut observer: AXObserverRef = std::ptr::null_mut();
        // SAFETY: callback is `extern "C"` and only reads a live CFString plus the
        // hub pointer installed below. out-parameter receives +1 on success.
        let status = unsafe { AXObserverCreate(pid, on_notification, &mut observer) };
        if status != kAXErrorSuccess || observer.is_null() {
            tracing::debug!(pid, status, "accessibility observer was not created");
            return;
        }
        guard.observer = observer;
        let leaked = Arc::into_raw(Arc::new(hub));
        guard.hub = leaked;
        for name in NOTES {
            let cf = CFString::new(name);
            // SAFETY: observer and app are live. cf lives for the call. refcon is
            // the leaked Arc, reclaimed only after the observer is released.
            let added = unsafe {
                AXObserverAddNotification(
                    observer,
                    app,
                    cf.as_concrete_TypeRef(),
                    leaked as *mut c_void,
                )
            };
            if added != kAXErrorSuccess {
                tracing::debug!(pid, added, name, "accessibility notification was not added");
            }
        }
        // SAFETY: AXObserverGetRunLoopSource is +0; the observer owns it. Get-rule
        // retains a +1 that `source`'s Drop releases. This does not free the original.
        let source_ref = unsafe { AXObserverGetRunLoopSource(observer) };
        if source_ref.is_null() {
            return;
        }
        let source = unsafe { CFRunLoopSource::wrap_under_get_rule(source_ref) };
        let run_loop = CFRunLoop::get_current();
        loop_ptr.store(
            run_loop.as_concrete_TypeRef() as *mut c_void,
            Ordering::SeqCst,
        );
        // SAFETY: kCFRunLoopDefaultMode is a process-lifetime CFString.
        let mode = unsafe { kCFRunLoopDefaultMode };
        run_loop.add_source(&source, mode);
        while !stop.load(Ordering::SeqCst) {
            let _ = CFRunLoop::run_in_mode(mode, Duration::from_millis(50), false);
        }
        run_loop.remove_source(&source, mode);
        for name in NOTES {
            let cf = CFString::new(name);
            // SAFETY: same live observer and app. Removing a missing note is fine.
            unsafe {
                AXObserverRemoveNotification(observer, app, cf.as_concrete_TypeRef());
            }
        }
        loop_ptr.store(std::ptr::null_mut(), Ordering::SeqCst);
    }

    unsafe extern "C" fn on_notification(
        _observer: AXObserverRef,
        _element: AXUIElementRef,
        notification: core_foundation::string::CFStringRef,
        refcon: *mut c_void,
    ) {
        if refcon.is_null() || notification.is_null() {
            return;
        }
        // SAFETY: refcon is the Arc leaked in `run_inner` and is not reclaimed
        // until this observer has been released.
        let hub = unsafe { &*(refcon as *const EventHub) };
        // SAFETY: notification is valid for this callback. Get-rule retains a
        // copy; Drop releases that retain and not the caller's string.
        let name = unsafe { CFString::wrap_under_get_rule(notification) }.to_string();
        hub.emit(map_name(&name));
    }

    fn map_name(name: &str) -> UiEvent {
        if name == kAXLayoutChangedNotification {
            UiEvent::GeometryChanged
        } else {
            UiEvent::Changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;

    #[tokio::test]
    async fn fake_events_emit_changed() {
        let events = FakeAxEvents::new();
        let mut stream = events.subscribe("cg:1:2").expect("hub stream");
        events.emit(UiEvent::Changed);
        let event = poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
        assert_eq!(event, Some(UiEvent::Changed));
    }

    #[test]
    fn run_loop_stop_joins_and_finishes() {
        let mut control = RunLoopControl::start();
        assert!(!control.finished());
        control.stop();
        assert!(control.finished());
    }

    #[test]
    fn parses_window_pid() {
        assert_eq!(parse_window_pid("cg:7:4"), Some(4));
        assert_eq!(parse_window_pid("not-a-window"), None);
    }
}
