//! Bounded event draining. A permanently ready stream must yield to timers/Stop.
use crate::driver::{BoxStream, UiEvent};
use std::{future::poll_fn, task::Poll, time::Duration};

const MAX_BATCH: usize = 64;
const EVENT_YIELD: Duration = Duration::from_millis(1);

pub(crate) struct Events {
    stream: Option<BoxStream<'static, UiEvent>>,
    pub quiet_at: Duration,
    pub check_at: Duration,
    pub poll_at: Duration,
    quiet: Duration,
}
#[derive(Default)]
pub(crate) struct Batch {
    pub changed: bool,
    pub gone: bool,
    pub closed: bool,
}
impl Events {
    pub fn new(
        stream: Option<BoxStream<'static, UiEvent>>,
        start: Duration,
        quiet: Duration,
    ) -> Self {
        Self {
            stream,
            quiet_at: start + quiet,
            check_at: start + quiet,
            poll_at: start,
            quiet,
        }
    }
    pub fn active(&self) -> bool {
        self.stream.is_some()
    }
    pub fn invalidate(&mut self, now: Duration) {
        self.quiet_at = now + self.quiet;
        self.check_at = self.quiet_at;
    }
    pub async fn next(&mut self) -> Batch {
        poll_fn(|cx| {
            let Some(stream) = &mut self.stream else {
                return Poll::Pending;
            };
            let mut batch = Batch::default();
            for _ in 0..MAX_BATCH {
                match stream.as_mut().poll_next(cx) {
                    Poll::Ready(Some(UiEvent::WindowGone)) => batch.gone = true,
                    Poll::Ready(Some(_)) => batch.changed = true,
                    Poll::Ready(None) => {
                        batch.closed = true;
                        break;
                    }
                    Poll::Pending if batch.changed || batch.gone => break,
                    Poll::Pending => return Poll::Pending,
                }
            }
            Poll::Ready(batch)
        })
        .await
    }
    /// One non-blocking drain. `None` means the stream was pending; the caller must
    /// then wait, or a ready flood would spin.
    pub async fn try_ready(&mut self) -> Option<Batch> {
        std::future::poll_fn(|cx| {
            let next = self.next();
            tokio::pin!(next);
            match std::future::Future::poll(next.as_mut(), cx) {
                std::task::Poll::Ready(batch) => std::task::Poll::Ready(Some(batch)),
                std::task::Poll::Pending => std::task::Poll::Ready(None),
            }
        })
        .await
    }

    pub fn received(&mut self, batch: &Batch, now: Duration) {
        if batch.changed {
            self.invalidate(now);
        }
        if batch.closed {
            self.stream = None;
        }
        self.poll_at = now + EVENT_YIELD;
    }
}
