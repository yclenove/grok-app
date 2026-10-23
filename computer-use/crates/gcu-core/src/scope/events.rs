//! Scoped event stream. Pending polls wake on revoke, rebind, and Stop.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;
use gcu_host_api::UserActivity;

use super::model::{FixtureScope, InstanceEvidence, ScopeLease, WindowBinding};
use crate::driver::{BoxStream, UiEvent, WindowRef};

pub(super) struct ScopedEvents<D> {
    inner: Option<BoxStream<'static, UiEvent>>,
    window: WindowRef,
    admitted: WindowBinding,
    evidence: Arc<D>,
    run_id: String,
    generation: u64,
    lease: ScopeLease,
    activity: Arc<dyn UserActivity>,
}

impl<D> ScopedEvents<D> {
    pub(super) fn new(
        inner: BoxStream<'static, UiEvent>,
        window: WindowRef,
        admitted: WindowBinding,
        evidence: Arc<D>,
        scope: &FixtureScope,
        lease: ScopeLease,
        activity: Arc<dyn UserActivity>,
    ) -> Self {
        Self {
            inner: Some(inner),
            window,
            admitted,
            evidence,
            run_id: scope.run_id().to_owned(),
            generation: scope.generation(),
            lease,
            activity,
        }
    }

    fn alive(&self) -> bool
    where
        D: InstanceEvidence,
    {
        if self.activity.stop_requested() || !self.lease.same_epoch(&self.run_id, self.generation) {
            return false;
        }
        let Some(scope) = self.lease.current() else {
            return false;
        };
        if !scope.admits_window(&self.admitted) {
            return false;
        }
        self.evidence.window_binding(&self.window).as_ref() == Some(&self.admitted)
    }
}

impl<D> Stream for ScopedEvents<D>
where
    D: InstanceEvidence,
{
    type Item = UiEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<UiEvent>> {
        let this = self.get_mut();
        this.activity.register_stop_waker(cx.waker());
        this.lease.register_waker(cx.waker());
        if !this.alive() {
            this.inner = None;
            return Poll::Ready(None);
        }
        let Some(inner) = this.inner.as_mut() else {
            return Poll::Ready(None);
        };
        match Pin::new(inner).poll_next(cx) {
            Poll::Ready(Some(event)) => {
                if this.alive() {
                    Poll::Ready(Some(event))
                } else {
                    this.inner = None;
                    Poll::Ready(None)
                }
            }
            Poll::Ready(None) => {
                this.inner = None;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
