//! Complete raw-tree evidence and cancellation ownership of an in-flight read.
use crate::action::observe::{digest, signature};
use crate::{
    driver::{Cancel, Driver, TreeOpts, WindowRef},
    model::{RawNode, RawTree, States},
};
use gcu_protocol::ErrorCode;
use std::{future::Future, pin::Pin, time::Duration};

pub(crate) fn usable(raw: &RawTree) -> bool {
    fn busy(node: &RawNode) -> bool {
        node.states.contains(States::BUSY) || node.children.iter().any(busy)
    }
    raw.incomplete.is_none() && !raw.root.as_ref().is_some_and(busy)
}
pub(crate) fn hash(raw: &RawTree) -> Result<u64, ErrorCode> {
    Ok(digest(signature(raw)?.as_bytes()))
}
pub(super) struct Evidence {
    baseline: Option<u64>,
    previous: Option<u64>,
    pub changed: bool,
    stable: bool,
    last: Option<Duration>,
}
impl Evidence {
    pub fn new(before: &RawTree) -> Result<Self, ErrorCode> {
        Ok(Self {
            baseline: if usable(before) {
                Some(hash(before)?)
            } else {
                None
            },
            previous: None,
            changed: false,
            stable: false,
            last: None,
        })
    }
    pub fn invalidate(&mut self) {
        self.previous = None;
        self.stable = false;
    }
    // True when a read invalidates event quiet (including missing native regions).
    pub fn observe(&mut self, raw: &RawTree, now: Duration) -> Result<bool, ErrorCode> {
        if !usable(raw) {
            self.invalidate();
            self.last = None;
            return Ok(true);
        }
        let current = hash(raw)?;
        let different = self.previous.is_some_and(|old| old != current);
        self.changed |= self.baseline.is_some_and(|old| old != current);
        self.stable = self.previous == Some(current);
        self.previous = Some(current);
        self.last = Some(now);
        Ok(different)
    }
    pub fn confirmed(&self, now: Duration, interval: Duration) -> bool {
        self.stable
            && self
                .last
                .is_some_and(|at| now.saturating_sub(at) <= interval)
    }
}

pub(crate) struct Read<'a> {
    pub work: Pin<Box<dyn Future<Output = crate::driver::Result<RawTree>> + Send + 'a>>,
    pub deadline: Duration,
    cancel: Cancel,
}
impl<'a> Read<'a> {
    pub fn new(
        driver: &'a dyn Driver,
        win: &'a WindowRef,
        mut opts: TreeOpts,
        now: Duration,
        deadline: Duration,
    ) -> Self {
        opts.timeout = opts
            .timeout
            .min(crate::driver::DRIVER_DEADLINE)
            .min(deadline.saturating_sub(now));
        let cancel = Cancel::default();
        let read_cancel = cancel.clone();
        Self {
            deadline: now + opts.timeout,
            work: Box::pin(async move { driver.snapshot_tree(win, &opts, &read_cancel).await }),
            cancel,
        }
    }
}
impl Drop for Read<'_> {
    fn drop(&mut self) {
        // Runs before `work` is dropped, including caller cancellation and Stop.
        self.cancel.cancel();
    }
}
