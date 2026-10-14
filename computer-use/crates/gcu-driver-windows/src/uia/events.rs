//! Bounded UIA event streams. Dropping a stream drops its native registration.
use futures_core::Stream;
use gcu_core::driver::{BoxStream, Result, UiEvent};
use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};
use tokio::sync::mpsc;

/// Native event category; all invalidate the tree version without retaining payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// StructureChanged.
    Structure,
    /// PropertyChanged.
    Property,
    /// FocusChanged (conservatively invalidates when another window gains focus).
    Focus,
}
/// Shared monotonic invalidation counter. No native handles or payloads.
#[derive(Clone, Default)]
pub struct TreeVersion(Arc<AtomicU64>);
impl TreeVersion {
    /// Current version, with no blocking.
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
    fn invalidate(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}
/// Callback sink. Capacity one coalesces bursts; versions still count every event.
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<UiEvent>,
    version: TreeVersion,
}
impl EventSink {
    /// Invalidate and wake observers. Never blocks or stores COM event payloads.
    pub fn emit(&self, _: EventKind) {
        self.version.invalidate();
        let _ = self.tx.try_send(UiEvent::Changed);
    }
    /// Whether the receiving stream has been dropped.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
/// RAII registration. Drop must request unregistration without blocking Stop.
pub trait EventRegistration: Send {}
/// Portable subscription source. Register may queue native work but must not block.
pub trait UiaEventSource: Send + Sync + 'static {
    /// Register all three native categories for an exact HWND.
    /// Errors/queue exhaustion are reported; no empty implementation is registered.
    fn register(&self, hwnd: u64, sink: EventSink) -> Result<Box<dyn EventRegistration>>;
    /// Shared invalidation version for this source. No IO.
    fn version(&self) -> TreeVersion;
}
/// Subscribe with bounded buffering and a registration tied to stream ownership.
/// Returns None on immediate registration failure. Dropping never joins a worker.
pub fn subscribe(source: &dyn UiaEventSource, hwnd: u64) -> Option<BoxStream<'static, UiEvent>> {
    let (tx, rx) = mpsc::channel(1);
    let registration = source
        .register(
            hwnd,
            EventSink {
                tx,
                version: source.version(),
            },
        )
        .ok()?;
    Some(Box::pin(EventStream {
        rx,
        _registration: registration,
    }))
}
struct EventStream {
    rx: mpsc::Receiver<UiEvent>,
    _registration: Box<dyn EventRegistration>,
}
impl Stream for EventStream {
    type Item = UiEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

#[derive(Default)]
struct FakeState {
    next: u64,
    entries: BTreeMap<u64, (u64, EventSink)>,
}
/// Fake subscription source, with the same RAII and coalescing as real UIA.
#[derive(Clone, Default)]
pub struct FakeUiaEvents {
    state: Arc<Mutex<FakeState>>,
    version: TreeVersion,
}
impl FakeUiaEvents {
    /// Emit an event only to matching HWND subscriptions.
    pub fn emit(&self, hwnd: u64, kind: EventKind) {
        if let Ok(s) = self.state.lock() {
            for (window, sink) in s.entries.values() {
                if *window == hwnd {
                    sink.emit(kind);
                }
            }
        }
    }
    /// Number of registered native subscriptions (each includes all three kinds).
    pub fn registrations(&self) -> usize {
        self.state
            .lock()
            .map(|s| s.entries.len())
            .unwrap_or_default()
    }
}
struct FakeRegistration {
    state: Arc<Mutex<FakeState>>,
    id: u64,
}
impl EventRegistration for FakeRegistration {}
impl Drop for FakeRegistration {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.entries.remove(&self.id);
        }
    }
}
impl UiaEventSource for FakeUiaEvents {
    fn register(&self, hwnd: u64, sink: EventSink) -> Result<Box<dyn EventRegistration>> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| gcu_core::driver::DriverError::Crashed {
                maybe_executed: false,
            })?;
        s.next += 1;
        let id = s.next;
        s.entries.insert(id, (hwnd, sink));
        Ok(Box::new(FakeRegistration {
            state: self.state.clone(),
            id,
        }))
    }
    fn version(&self) -> TreeVersion {
        self.version.clone()
    }
}

// Driver::subscribe is synchronous. Gate the exact window on the discovery
// worker, then hand its registered stream over without blocking this caller.
pub(crate) fn deferred(
    receiver: tokio::sync::oneshot::Receiver<BoxStream<'static, UiEvent>>,
) -> BoxStream<'static, UiEvent> {
    Box::pin(DeferredStream {
        pending: Some(Box::pin(async move {
            tokio::time::timeout(gcu_core::driver::DRIVER_DEADLINE, receiver)
                .await
                .ok()?
                .ok()
        })),
        ready: None,
    })
}
type Pending =
    Pin<Box<dyn std::future::Future<Output = Option<BoxStream<'static, UiEvent>>> + Send>>;
struct DeferredStream {
    pending: Option<Pending>,
    ready: Option<BoxStream<'static, UiEvent>>,
}
impl Stream for DeferredStream {
    type Item = UiEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(pending) = &mut self.pending {
            match pending.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(stream) => {
                    self.ready = stream;
                    self.pending = None;
                }
            }
        }
        self.ready
            .as_mut()
            .map_or(Poll::Ready(None), |stream| stream.as_mut().poll_next(cx))
    }
}
