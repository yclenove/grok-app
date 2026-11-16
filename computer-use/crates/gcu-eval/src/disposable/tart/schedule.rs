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
//! can slip one more step. The grant window is [`LEASE_WINDOW_MS`] (5000 ms,
//! owner decision 2026-11-15). None of these figures is a Stop limit. The
//! outer owner proof, the guest supervisor renewal, and the S3 scope lease
//! stay as they are.
//!
//! The worst-case sample age when the grant is visible is guest→host, plus
//! one service step, plus host→guest: `1023 + 100 + 1023 = 2146`. The gap
//! until the next grant is visible adds the 800 ms publish interval, the
//! 100 ms `due` slip, and one one-way of visibility variation:
//! `800 + 100 + 1023 = 1923`. Continuity needs `2146 + 1923 = 4069 < 5000`.
//! The margin is 931 ms. The single-round-trip reading
//! `1023 + 100 + 800 + 100 = 2023` also fits. [`prove`] returns a
//! [`ScheduleProof`] for the attempt window.
//!
//! The longest window is prep plus task plus cleanup (login sits inside
//! prep): 1_560_000 ms. The 800 ms grid is `1_560_000 / 800 + 1 = 1951`
//! samples, plus eight off-grid completion samples, 1959 against the 2048
//! cap. A 2000 ms grant, or a larger age or gap, still returns
//! [`ScheduleError::Infeasible`]. So does a grid above 2048 requests. Under
//! the historical `g+2s` window that same age is already past 2000 ms, and
//! both `2146 + 1923` and `2023` miss it. [`prove_model`] keeps that closed
//! case so the gate is not a constant success.

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

/// Grant, age, and gap a feasibility check evaluates.
///
/// [`prove`] fills this from the measured constants and [`LEASE_WINDOW_MS`].
/// Tests pass a shorter grant or a larger age or gap. That does not change
/// the public lease constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CycleModel {
    /// Attempt length, in milliseconds. This sizes the request grid.
    pub window_ms: u64,
    /// Guest grant. `until_ms` is the sample plus this.
    pub grant_ms: u64,
    /// Worst-case sample age when the grant is visible.
    pub age_ms: u64,
    /// Worst-case gap between grants becoming visible.
    pub gap_ms: u64,
}

/// Prove `window_ms` fits the measured grant cycle and the 2048 cap.
///
/// Does not sleep, renew, or start a thread. The grant is [`LEASE_WINDOW_MS`].
/// A cycle that misses that grant, or a grid past [`MAX_POLLS`], is
/// [`ScheduleError::Infeasible`]. The function does not return a proof that
/// merely names the publish period.
pub fn prove(window_ms: u64) -> Result<ScheduleProof, ScheduleError> {
    prove_model(CycleModel {
        window_ms,
        grant_ms: LEASE_WINDOW_MS,
        age_ms: ACCEPT_AGE_MS,
        gap_ms: GRANT_GAP_MS,
    })
}

/// Evaluate `model` against the publish grid and the 2048 cap.
///
/// Does not sleep, renew, or start a thread. Public callers use [`prove`].
pub(crate) fn prove_model(model: CycleModel) -> Result<ScheduleProof, ScheduleError> {
    let period = RENEW_MS;
    let periodic = model.window_ms / period + 1;
    let count = periodic.saturating_add(EXTRA_SAMPLES);
    let fresh = model.age_ms < model.grant_ms;
    let continuous = model
        .age_ms
        .checked_add(model.gap_ms)
        .is_some_and(|sum| sum < model.grant_ms);
    if !fresh || !continuous || count > MAX_POLLS {
        return Err(ScheduleError::Infeasible {
            window_ms: model.window_ms,
            count,
        });
    }
    Ok(ScheduleProof {
        window_ms: model.window_ms,
        period_ms: period,
        periodic,
        inventory_count: count,
        lease_count: count,
        margin: MAX_POLLS - count,
        age_ms: model.age_ms,
        gap_ms: model.gap_ms,
        slack_ms: model.grant_ms - model.age_ms - model.gap_ms,
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
        attempt_window_ms, prove, prove_model, CycleModel, ACCEPT_AGE_MS, EXTRA_SAMPLES,
        GRANT_GAP_MS, LEASE_WINDOW_MS, MAX_POLLS, ONE_WAY_MS, RENEW_MS, ROUND_TRIP_MAX_MS, STEP_MS,
    };

    fn grid_count(window_ms: u64) -> u64 {
        window_ms / RENEW_MS + 1 + EXTRA_SAMPLES
    }

    #[test]
    fn measured_bounds_fit_the_attempt_window() {
        if ROUND_TRIP_MAX_MS != 1023 || ONE_WAY_MS != ROUND_TRIP_MAX_MS {
            panic!("one-way bound is the round-trip max, got {ONE_WAY_MS}");
        }
        let age = ONE_WAY_MS + STEP_MS + ONE_WAY_MS;
        let gap = RENEW_MS + STEP_MS + ONE_WAY_MS;
        if ACCEPT_AGE_MS != 2_146
            || age != ACCEPT_AGE_MS
            || GRANT_GAP_MS != 1_923
            || gap != GRANT_GAP_MS
        {
            panic!("age {ACCEPT_AGE_MS} gap {GRANT_GAP_MS}, want {age} and {gap}");
        }
        if LEASE_WINDOW_MS != 5_000 {
            panic!("grant window {LEASE_WINDOW_MS}");
        }
        let sum = age + gap;
        if sum != 4_069 || LEASE_WINDOW_MS - sum != 931 || sum >= LEASE_WINDOW_MS {
            panic!("continuity {sum} against {LEASE_WINDOW_MS}");
        }
        let single = ROUND_TRIP_MAX_MS + STEP_MS + RENEW_MS + STEP_MS;
        if single != 2_023 || single > LEASE_WINDOW_MS {
            panic!("single round trip {single}");
        }
        let window = attempt_window_ms();
        if window != 1_560_000 {
            panic!("attempt window {window}");
        }
        let periodic = window / RENEW_MS + 1;
        let count = periodic + EXTRA_SAMPLES;
        if periodic != 1_951 || count != 1_959 || count > MAX_POLLS {
            panic!("count formula changed: periodic {periodic} count {count}");
        }
        match prove(window) {
            Ok(proof)
                if proof.window_ms == window
                    && proof.period_ms == RENEW_MS
                    && proof.periodic == periodic
                    && proof.inventory_count == count
                    && proof.lease_count == count
                    && proof.margin == MAX_POLLS - count
                    && proof.age_ms == age
                    && proof.gap_ms == gap
                    && proof.slack_ms == 931 => {}
            other => panic!("attempt window was not proved: {other:?}"),
        }
    }

    #[test]
    fn historical_2000_grant_is_infeasible() {
        let window = attempt_window_ms();
        let count = grid_count(window);
        match prove_model(CycleModel {
            window_ms: window,
            grant_ms: 2_000,
            age_ms: ACCEPT_AGE_MS,
            gap_ms: GRANT_GAP_MS,
        }) {
            Err(ScheduleError::Infeasible {
                window_ms,
                count: got,
            }) if window_ms == window && got == count => {}
            other => panic!("2000 ms grant returned a proof: {other:?}"),
        }
    }

    #[test]
    fn larger_jitter_is_infeasible() {
        let window = attempt_window_ms();
        let count = grid_count(window);
        // 930 ms of extra gap still fits: 4069 + 930 = 4999, slack 1 ms.
        let inside = GRANT_GAP_MS + 930;
        match prove_model(CycleModel {
            window_ms: window,
            grant_ms: LEASE_WINDOW_MS,
            age_ms: ACCEPT_AGE_MS,
            gap_ms: inside,
        }) {
            Ok(proof) if proof.slack_ms == 1 && proof.gap_ms == inside => {}
            other => panic!("jitter inside the margin was rejected: {other:?}"),
        }
        // One more millisecond lands on the grant. Continuity is strict.
        let edge = GRANT_GAP_MS + 931;
        if ACCEPT_AGE_MS + edge != LEASE_WINDOW_MS {
            panic!("edge jitter did not land on the grant");
        }
        match prove_model(CycleModel {
            window_ms: window,
            grant_ms: LEASE_WINDOW_MS,
            age_ms: ACCEPT_AGE_MS,
            gap_ms: edge,
        }) {
            Err(ScheduleError::Infeasible {
                window_ms,
                count: got,
            }) if window_ms == window && got == count => {}
            other => panic!("edge jitter returned a proof: {other:?}"),
        }
    }

    #[test]
    fn request_count_above_2048_is_infeasible() {
        // 1_631_200 / 800 + 1 + 8 = 2048, still inside the cap.
        let edge = 1_631_200;
        match prove(edge) {
            Ok(proof) if proof.inventory_count == MAX_POLLS && proof.margin == 0 => {}
            other => panic!("2048 requests were rejected: {other:?}"),
        }
        // One more period: 1_632_000 / 800 + 1 + 8 = 2049.
        let over = 1_632_000;
        match prove(over) {
            Err(ScheduleError::Infeasible {
                window_ms,
                count: 2_049,
            }) if window_ms == over => {}
            other => panic!("2049 requests were accepted: {other:?}"),
        }
    }
}
