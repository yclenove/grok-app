//! Only ImportLock worker/publish OS acquisition, NOT all locks or SQLite busy.

use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

const ACTIVE_SLOTS: usize = 128;

#[derive(Clone, Copy)]
pub(crate) enum ImportOsLockKind {
    Publish,
    Worker,
}

impl ImportOsLockKind {
    fn index(self) -> usize {
        match self {
            Self::Publish => 0,
            Self::Worker => 1,
        }
    }
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LockSnapshot {
    pub acquire_count: u64,
    pub failed_count: u64,
    pub total_acquire_us: u64,
    pub max_acquire_us: u64,
    pub active_wait_count: u64,
    pub oldest_active_wait_us: u64,
    pub active_tracking_overflows: u64,
    pub untracked_active_waits: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImportOsLocksSnapshot {
    pub publish: LockSnapshot,
    pub worker: LockSnapshot,
}

struct Bucket {
    totals: LockSnapshot,
    active: [Option<Instant>; ACTIVE_SLOTS],
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            totals: LockSnapshot::default(),
            active: [None; ACTIVE_SLOTS],
        }
    }
}

#[derive(Default)]
pub(super) struct LockMetrics(Mutex<[Bucket; 2]>);

pub(super) struct Acquisition<'a> {
    metrics: &'a LockMetrics,
    kind: ImportOsLockKind,
    slot: Option<usize>,
    started: Instant,
    succeeded: bool,
}

fn micros(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

impl LockMetrics {
    pub(super) fn begin(&self, kind: ImportOsLockKind) -> Acquisition<'_> {
        let mut buckets = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let bucket = &mut buckets[kind.index()];
        let started = Instant::now();
        let slot = bucket.active.iter().position(Option::is_none);
        if let Some(slot) = slot {
            bucket.active[slot] = Some(started);
        } else {
            bucket.totals.active_tracking_overflows =
                bucket.totals.active_tracking_overflows.saturating_add(1);
            bucket.totals.untracked_active_waits =
                bucket.totals.untracked_active_waits.saturating_add(1);
        }
        bucket.totals.active_wait_count = bucket.totals.active_wait_count.saturating_add(1);
        Acquisition {
            metrics: self,
            kind,
            slot,
            started,
            succeeded: false,
        }
    }

    pub(super) fn snapshot(&self) -> ImportOsLocksSnapshot {
        let buckets = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let snapshot = |bucket: &Bucket| LockSnapshot {
            oldest_active_wait_us: bucket
                .active
                .iter()
                .flatten()
                .copied()
                .map(micros)
                .max()
                .unwrap_or(0),
            ..bucket.totals
        };
        ImportOsLocksSnapshot {
            publish: snapshot(&buckets[0]),
            worker: snapshot(&buckets[1]),
        }
    }

    pub(super) fn observe<T, E>(
        &self,
        kind: ImportOsLockKind,
        acquire: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let mut observation = self.begin(kind);
        let result = acquire();
        observation.succeeded = result.is_ok();
        // Finish before diagnostics can wait for the metrics mutex.
        observation.finish();
        result
    }
}

impl Acquisition<'_> {
    fn finish(self) {
        drop(self);
    }
}

impl Drop for Acquisition<'_> {
    fn drop(&mut self) {
        let elapsed = micros(self.started);
        let mut buckets = self
            .metrics
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let bucket = &mut buckets[self.kind.index()];
        let totals = &mut bucket.totals;
        totals.acquire_count = totals.acquire_count.saturating_add(1);
        totals.failed_count = totals
            .failed_count
            .saturating_add(u64::from(!self.succeeded));
        totals.total_acquire_us = totals.total_acquire_us.saturating_add(elapsed);
        totals.max_acquire_us = totals.max_acquire_us.max(elapsed);
        totals.active_wait_count = totals.active_wait_count.saturating_sub(1);
        if let Some(slot) = self.slot {
            bucket.active[slot] = None;
        } else {
            totals.untracked_active_waits = totals.untracked_active_waits.saturating_sub(1);
        }
    }
}
