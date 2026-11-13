//! Fresh-sample and grant schedule for one attempt.
//!
//! Feasibility follows the protocol cycle, not a nominal age. One cycle is:
//! the host publishes an inventory request, the guest samples when that
//! request is visible, the reply becomes visible to the host, the host
//! publishes a grant on the next service step, and the grant becomes visible
//! to the guest. Host→guest and guest→host are separate legs.
//!
//! ADR 0019 records the C1 ping-pong maxima: host→guest 1023 ms, guest→host
//! 1021 ms. A one-way delay was not measured on its own. Its only upper bound
//! is the larger round trip, 1023 ms, so each leg uses 1023 ms. The service
//! step is 100 ms. `due` is sampled on that same step grid, so a publication
//! can slip one more step. The grant window stays 2000 ms. None of these
//! figures is a new lease or Stop limit.
//!
//! The worst-case sample age when the grant is visible is guest→host, plus
//! one service step, plus host→guest: `1023 + 100 + 1023 = 2146`. That is
//! already past 2000 ms, so the first grant can miss its own deadline. The
//! gap until the next grant is visible adds the 800 ms publish interval, the
//! 100 ms `due` slip, and one one-way of visibility variation:
//! `800 + 100 + 1023 = 1923`. Continuity needs the next grant to be visible
//! before the old deadline, which is `age + gap < 2000`. It is not. Even the
//! tighter reading that charges the whole exchange as one 1023 ms round trip
//! still misses: `1023 + 100 + 800 + 100 = 2023`. `prove` returns
//! [`ScheduleError::Infeasible`] and does not hand back a nominal proof.
//!
//! The longest window is prep plus task plus cleanup (login sits inside
//! prep): 1_560_000 ms. The 800 ms grid would be `1_560_000 / 800 + 1 = 1951`
//! samples, plus eight off-grid completion samples, 1959 against the 2048
//! cap. That count is not a schedule. The timing bound fails first.

use std::time::Duration;

use super::budgets::ATTEMPT_BUDGET;
use super::deadline::{DeadlineError, MonoInstant};
use super::mailbox::{LEASE_WINDOW_MS, MAX_POLLS};

/// Longest gap between service checks, in milliseconds.
pub const STEP_MS: u64 = 100;

/// Larger C1 ping-pong maximum (ADR 0019, host→guest direction), in milliseconds.
pub const ROUND_TRIP_MAX_MS: u64 = 1023;

/// One-way visibility upper bound. Only known to be at most one round trip.
pub const ONE_WAY_MS: u64 = ROUND_TRIP_MAX_MS;

/// Fresh inventory and lease publication period, in milliseconds.
pub const RENEW_MS: u64 = 800;

/// Off-grid completion and consume samples budgeted inside [`MAX_POLLS`].
pub const EXTRA_SAMPLES: u64 = 8;

/// Guest-slot bytes one bounded read call may consume, including its rereads.
///
/// This is not an attempt total. Stale and discarded frames in that call count.
pub const POLL_BYTE_BUDGET: usize = 128 * 1024;

/// Worst-case age of a sample when its grant is visible to the guest.
///
/// Guest→host reply, one service step, then host→guest grant visibility.
/// The inventory's host→guest leg is before the sample, so it is not in the age.
pub const ACCEPT_AGE_MS: u64 = ONE_WAY_MS + STEP_MS + ONE_WAY_MS;

/// Worst-case gap between two grants becoming visible to the guest.
///
/// Publish period, plus one step of [`due`] slip, plus a full one-way when the
/// previous grant was immediate and this one is not.
pub const GRANT_GAP_MS: u64 = RENEW_MS + STEP_MS + ONE_WAY_MS;

/// Why no schedule meets the lease window and the poll cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// The measured cycle does not land inside the grant window, or the
    /// window needs more than [`MAX_POLLS`] inventory or lease requests.
    #[error("no lease schedule fits this attempt window")]
    Infeasible {
        /// Window that was offered, in milliseconds.
        window_ms: u64,
        /// Inventory or lease requests that window's grid would take.
        count: u64,
    },
}

/// Counts and ages for one feasible window.
///
/// Constructed only when the measured cycle fits. An infeasible window is
/// [`ScheduleError`], not a proof filled with these fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleProof {
    /// Window covered, in milliseconds.
    pub window_ms: u64,
    /// Publish period.
    pub period_ms: u64,
    /// Grid samples, counting both endpoints.
    pub periodic: u64,
    /// Grid samples plus [`EXTRA_SAMPLES`].
    pub inventory_count: u64,
    /// Same cap as inventory. Lease uses the same grid.
    pub lease_count: u64,
    /// [`MAX_POLLS`] minus [`Self::inventory_count`].
    pub margin: u64,
    /// Worst-case sample age when the grant is visible to the guest.
    pub age_ms: u64,
    /// Worst-case gap between grants becoming visible to the guest.
    pub gap_ms: u64,
    /// `grant - age - gap`. Positive only when continuity holds.
    pub slack_ms: u64,
}

/// Prove `window_ms` fits the measured grant cycle and the 2048 cap.
///
/// Does not sleep, renew, or start a thread. A window whose cycle misses the
/// grant deadline is [`ScheduleError::Infeasible`], including a short window.
/// The function does not return a proof that merely names the publish period.
pub fn prove(window_ms: u64) -> Result<ScheduleProof, ScheduleError> {
    let period = RENEW_MS;
    let age = ACCEPT_AGE_MS;
    let gap = GRANT_GAP_MS;
    let grant = LEASE_WINDOW_MS;
    let periodic = window_ms / period + 1;
    let count = periodic.saturating_add(EXTRA_SAMPLES);
    let fresh = age < grant;
    let continuous = age.checked_add(gap).is_some_and(|sum| sum < grant);
    if !fresh || !continuous || count > MAX_POLLS {
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
        gap_ms: gap,
        slack_ms: grant - age - gap,
    })
}

/// ADR attempt cap: prep plus task plus cleanup, in milliseconds.
#[must_use]
pub fn attempt_window_ms() -> u64 {
    u64::try_from(ATTEMPT_BUDGET.as_millis()).unwrap_or(u64::MAX)
}

/// A new sample or grant is due when none has been sent, or `period` has elapsed.
///
/// A caller that samples this on the service grid can be up to [`STEP_MS`]
/// late. [`prove`] charges that slip. This function does not hide it.
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

#[cfg(test)]
mod tests {
    use super::ScheduleError;
    use super::{
        attempt_window_ms, prove, ACCEPT_AGE_MS, EXTRA_SAMPLES, GRANT_GAP_MS, LEASE_WINDOW_MS,
        MAX_POLLS, ONE_WAY_MS, RENEW_MS, ROUND_TRIP_MAX_MS, STEP_MS,
    };

    #[test]
    fn measured_bounds_reject_the_attempt_window() {
        if ROUND_TRIP_MAX_MS != 1023 || ONE_WAY_MS != ROUND_TRIP_MAX_MS {
            panic!("one-way bound is the round-trip max, got {ONE_WAY_MS}");
        }
        let age = ONE_WAY_MS + STEP_MS + ONE_WAY_MS;
        let gap = RENEW_MS + STEP_MS + ONE_WAY_MS;
        if ACCEPT_AGE_MS != age || GRANT_GAP_MS != gap {
            panic!("age {ACCEPT_AGE_MS} gap {GRANT_GAP_MS}, want {age} and {gap}");
        }
        if age < LEASE_WINDOW_MS && age + gap < LEASE_WINDOW_MS {
            panic!("bounds fit the grant; the gate would return a proof");
        }
        // One round trip for the whole exchange, plus service, period, and due slip.
        let single_round_trip = ROUND_TRIP_MAX_MS + STEP_MS + RENEW_MS + STEP_MS;
        if single_round_trip <= LEASE_WINDOW_MS {
            panic!("single round trip still fits: {single_round_trip}");
        }
        let window = attempt_window_ms();
        if window != 1_560_000 {
            panic!("attempt window {window}");
        }
        let periodic = window / RENEW_MS + 1;
        let count = periodic + EXTRA_SAMPLES;
        if periodic != 1951 || count != 1959 || count > MAX_POLLS {
            panic!("count formula changed: periodic {periodic} count {count}");
        }
        match prove(window) {
            Err(ScheduleError::Infeasible {
                window_ms,
                count: got,
            }) if window_ms == window && got == count => {}
            other => panic!("nominal proof: {other:?}"),
        }
        match prove(1) {
            Err(ScheduleError::Infeasible { window_ms: 1, .. }) => {}
            other => panic!("short window returned a proof: {other:?}"),
        }
    }
}
