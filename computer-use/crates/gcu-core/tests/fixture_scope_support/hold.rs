//! Bounded fences for round-3 scope races.
//!
//! A missed release fails the test after two seconds. The wait sleeps instead of
//! spinning so a stuck fence does not burn the host.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{lock, HoldOp, ScopeLease, Spy};

impl Spy {
    pub fn arm_hold(&self, op: HoldOp, started: bool) {
        self.arm_hold_after(op, 0, started);
    }

    pub fn arm_hold_after(&self, op: HoldOp, skip: usize, started: bool) {
        let mut ev = lock(&self.evidence);
        ev.hold = Some(op);
        ev.hold_started = started;
        ev.hold_skip = skip;
        ev.hold_go = false;
        self.counts.held.store(false, Ordering::SeqCst);
    }

    pub fn arm_snapshot_fault_after(&self, skip: usize, lease: ScopeLease) {
        let mut ev = lock(&self.evidence);
        ev.snapshot_fault = Some(skip);
        ev.fault_lease = Some(lease);
    }

    pub async fn wait_held(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.counts.held.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                panic!("scope fence was not reached within 2s");
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    pub fn release_hold(&self) {
        lock(&self.evidence).hold_go = true;
    }

    pub fn started(&self) -> usize {
        self.counts.started.load(Ordering::SeqCst)
    }

    pub fn held_input(&self) -> usize {
        self.counts.held_input.load(Ordering::SeqCst)
    }

    pub fn unsubscribed(&self) -> usize {
        self.counts.unsubscribed.load(Ordering::SeqCst)
    }

    pub fn events_parked(&self) -> bool {
        self.counts.events_parked.load(Ordering::SeqCst)
    }

    pub(super) fn fire_snapshot_fault(&self) -> bool {
        let lease = {
            let mut ev = lock(&self.evidence);
            let Some(skip) = ev.snapshot_fault else {
                return false;
            };
            if skip > 0 {
                ev.snapshot_fault = Some(skip - 1);
                return false;
            }
            ev.snapshot_fault = None;
            ev.fault_lease.clone()
        };
        self.activity.request_stop();
        if let Some(lease) = lease {
            lease.revoke();
        }
        true
    }

    pub(super) async fn pause(&self, op: HoldOp) {
        let started = {
            let mut ev = lock(&self.evidence);
            if ev.hold != Some(op) {
                return;
            }
            if ev.hold_skip > 0 {
                ev.hold_skip -= 1;
                return;
            }
            op == HoldOp::Perform && ev.hold_started
        };
        if started {
            self.counts.started.fetch_add(1, Ordering::SeqCst);
            self.counts.held_input.fetch_add(1, Ordering::SeqCst);
        }
        self.counts.held.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if lock(&self.evidence).hold_go {
                break;
            }
            if Instant::now() >= deadline {
                panic!("scope fence wait exceeded 2s");
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        self.counts.held.store(false, Ordering::SeqCst);
        lock(&self.evidence).hold = None;
    }
}
