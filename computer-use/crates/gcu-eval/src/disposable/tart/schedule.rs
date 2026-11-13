//! Fresh-sample and grant schedule for one attempt.
//!
//! The measured host/guest round trip is about 1000 ms. That figure is one
//! visibility budget, not two independent one-way legs. A grant is served on
//! the next 100 ms step, so the sample age at accept is visibility plus one
//! service slice (1100 ms). Renewal is anchored to publication, not to ACK
//! arrival, so the gap stays 800 ms. Slack is `2000 - 1100 - 800 = 100`.
//! Waiting for the ACK before the next publication is `1000 + 800 + 1000`
//! and is rejected here instead of being returned as a proof.
//!
//! The longest window is prep plus task plus cleanup (login sits inside
//! prep): 1_560_000 ms. Samples on an 800 ms grid, counting both endpoints,
//! are `1_560_000 / 800 + 1 = 1951`. Eight off-grid completion samples fit in
//! the same 2048 cap, leaving 89 requests. A wider window fails closed.

use std::time::Duration;

use super::budgets::ATTEMPT_BUDGET;
use super::deadline::{DeadlineError, MonoInstant};
use super::mailbox::{LEASE_WINDOW_MS, MAX_POLLS};

/// Longest gap between service checks, in milliseconds.
pub const STEP_MS: u64 = 100;

/// Host-to-guest and guest-to-host visibility budget, in milliseconds.
pub const VISIBILITY_MS: u64 = 1000;

/// Fresh inventory and lease period, in milliseconds.
pub const RENEW_MS: u64 = 800;

/// Off-grid completion and consume samples budgeted inside [`MAX_POLLS`].
pub const EXTRA_SAMPLES: u64 = 8;

/// Guest-slot bytes one bounded read call may consume, including its rereads.
///
/// This is not an attempt total. Stale and discarded frames in that call count.
pub const POLL_BYTE_BUDGET: usize = 128 * 1024;

/// Sample age at accept: one measured round trip plus one service slice.
pub const ACCEPT_AGE_MS: u64 = VISIBILITY_MS + STEP_MS;

/// Why no schedule meets the lease window and the poll cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// The window needs more than [`MAX_POLLS`] inventory or lease requests,
    /// or the renew period cannot land inside the measured grant window.
    #[error("no lease schedule fits this attempt window")]
    Infeasible {
        /// Window that was offered, in milliseconds.
        window_ms: u64,
        /// Inventory or lease requests that window would take.
        count: u64,
    },
}

/// Counts and ages for one feasible window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleProof {
    /// Window covered, in milliseconds.
    pub window_ms: u64,
    /// Grid period. Also the grant gap.
    pub period_ms: u64,
    /// Grid samples, counting both endpoints.
    pub periodic: u64,
    /// Grid samples plus [`EXTRA_SAMPLES`].
    pub inventory_count: u64,
    /// Same cap as inventory. Lease uses the same grid.
    pub lease_count: u64,
    /// [`MAX_POLLS`] minus [`Self::inventory_count`].
    pub margin: u64,
    /// Sample age when a grant is accepted. Visibility plus one service slice.
    pub age_ms: u64,
    /// Time between publications. Equal to [`RENEW_MS`]. ACK wait is not included.
    pub gap_ms: u64,
    /// `2000 - age - gap`. The next grant is visible before the old deadline.
    pub slack_ms: u64,
}

/// Prove `window_ms` fits the measured grant and the 2048 cap.
///
/// Does not sleep, renew, or start a thread. An infeasible window is an error.
pub fn prove(window_ms: u64) -> Result<ScheduleProof, ScheduleError> {
    let period = RENEW_MS;
    let age = ACCEPT_AGE_MS;
    let grant = LEASE_WINDOW_MS;
    // ACK-wait (`visibility + period + visibility`) is larger than the grant
    // and must not be substituted when this inequality fails.
    let timing = period > 0 && period <= 1_000 && age < grant && period.saturating_add(age) < grant;
    let periodic = window_ms / period + 1;
    let count = periodic.saturating_add(EXTRA_SAMPLES);
    if !timing || count > MAX_POLLS {
        return Err(ScheduleError::Infeasible { window_ms, count });
    }
    Ok(ScheduleProof {
        window_ms,
        period_ms: period,
        periodic,
        inventory_count: count,
        lease_count: count,
        margin: MAX_POLLS - count,
        age_ms: age,
        gap_ms: period,
        slack_ms: grant - age - period,
    })
}

/// ADR attempt cap: prep plus task plus cleanup, in milliseconds.
#[must_use]
pub fn attempt_window_ms() -> u64 {
    u64::try_from(ATTEMPT_BUDGET.as_millis()).unwrap_or(u64::MAX)
}

/// A new sample or grant is due when none has been sent, or `period` has elapsed.
#[must_use]
pub fn due(last: Option<MonoInstant>, now: MonoInstant) -> bool {
    match last {
        None => true,
        Some(at) => match at.checked_add(Duration::from_millis(RENEW_MS)) {
            Ok(next) => now >= next,
            Err(_) => true,
        },
    }
}

/// Instant `STEP_MS` after `now`.
pub fn step_end(now: MonoInstant) -> Result<MonoInstant, DeadlineError> {
    now.checked_add(Duration::from_millis(STEP_MS))
}
