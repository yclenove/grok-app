//! In-memory event queue for scoped subscribe tests.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use gcu_core::driver::{Stream, UiEvent};

pub(super) struct LiveEvents {
    pub(super) events: Arc<Mutex<VecDeque<UiEvent>>>,
    pub(super) waker: Arc<Mutex<Option<Waker>>>,
}

impl Stream for LiveEvents {
    type Item = UiEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<UiEvent>> {
        let mut queue = self.events.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(event) = queue.pop_front() {
            return Poll::Ready(Some(event));
        }
        *self.waker.lock().unwrap_or_else(|err| err.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }
}
