//! AT-SPI event subscription skeleton for settle and wait_for operations.

use async_trait::async_trait;
use futures_core::Stream;
use gcu_core::driver::{BoxStream, UiEvent};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::mpsc;

/// Interface for subscribing to native accessibility events.
#[async_trait]
pub trait AtspiEventSubscriber: Send + Sync {
    /// Subscribe to accessibility changes for a specific window or all windows.
    fn subscribe(&self, window_id: &str) -> Option<BoxStream<'static, UiEvent>>;
}

/// Fake event subscriber for testing settle and event wait logic.
#[derive(Clone, Default)]
pub struct FakeEventSubscriber {
    senders: Arc<Mutex<Vec<mpsc::UnboundedSender<UiEvent>>>>,
}

impl FakeEventSubscriber {
    /// Create a new fake event subscriber.
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit a fake UI event to all active subscribers.
    pub fn emit(&self, event: UiEvent) {
        if let Ok(mut senders) = self.senders.lock() {
            senders.retain(|tx| tx.send(event.clone()).is_ok());
        }
    }
}

#[async_trait]
impl AtspiEventSubscriber for FakeEventSubscriber {
    fn subscribe(&self, _window_id: &str) -> Option<BoxStream<'static, UiEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        if let Ok(mut senders) = self.senders.lock() {
            senders.retain(|sender| !sender.is_closed());
            senders.push(tx);
        }
        Some(Box::pin(EventStream { rx }))
    }
}

/// Stream adapter for `mpsc::UnboundedReceiver<UiEvent>`.
pub struct EventStream {
    rx: mpsc::UnboundedReceiver<UiEvent>,
}

impl Stream for EventStream {
    type Item = UiEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Real AT-SPI D-Bus event subscriber skeleton.
pub struct LiveEventSubscriber {
    connection: zbus::Connection,
}

impl LiveEventSubscriber {
    /// Create an event subscriber over an AT-SPI connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    /// Access underlying connection.
    pub fn connection(&self) -> &zbus::Connection {
        &self.connection
    }
}

#[async_trait]
impl AtspiEventSubscriber for LiveEventSubscriber {
    fn subscribe(&self, _window_id: &str) -> Option<BoxStream<'static, UiEvent>> {
        // Event streaming skeleton; returns None when active listeners are not yet initialized
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_subscriber_emits_events() {
        use std::future::poll_fn;
        let subscriber = FakeEventSubscriber::new();
        let stream = subscriber.subscribe("win-1");
        assert!(stream.is_some());
        let mut stream = stream.expect("stream should be present");

        subscriber.emit(UiEvent::Changed);
        let ev = poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
        assert_eq!(ev, Some(UiEvent::Changed));
    }
}
