//! Whether an injected absolute move has reached the cursor.
//! Portable so admission after a move can be tested without a desktop.
use gcu_core::driver::DriverError;

/// Outcome of waiting for the cursor after an absolute move that was already inserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Settle {
    /// The cursor is within the arrival tolerance.
    Arrived,
    /// A position was read, and it was not the target before the deadline.
    TimedOut,
    /// No position could be read before the deadline.
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettleProgress {
    Pending { saw_position: bool },
    Done(Settle),
}

impl SettleProgress {
    pub(crate) fn pending() -> Self {
        Self::Pending {
            saw_position: false,
        }
    }
    pub(crate) fn finish(self) -> Settle {
        match self {
            Self::Done(done) => done,
            Self::Pending { saw_position: true } => Settle::TimedOut,
            Self::Pending {
                saw_position: false,
            } => Settle::Unreadable,
        }
    }
}

/// How far a sampled cursor may sit from the inverse pixel and still count as arrived.
///
/// Default is 0: the cursor must hit the pixel `denormalize_axis` produces from the same
/// normalization `inputs()` sends. win-test on 2026-10-23 showed that Windows maps
/// `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` with
/// `pixel = origin + floor(norm * extent / 65536)`, so that pixel is the requested pixel.
/// Do not raise this constant to hide a miss.
pub(crate) const CURSOR_ARRIVAL_TOLERANCE_PX: i32 = 0;

/// Minimum absolute coordinate in `0..=65535` that the Windows floor map sends to `pixel`.
///
/// `P = round(pixel - origin)` clamped to `[0, extent - 1]`, then
/// `ceil(P * 65536 / extent)` in integer arithmetic. A desktop wider than 65536 pixels
/// cannot name every pixel; the coordinate saturates at 65535 and a later settle timeout
/// stays fail-closed. `extent <= 1` is not a usable axis and returns 0.
///
/// `hook_probe::normalize` uses these same floor bins but publishes the bin midpoint.
/// This function publishes the minimum norm. Both land on `P`.
pub(crate) fn normalize_axis(pixel: f64, origin: i32, extent: i32) -> i32 {
    if extent <= 1 {
        return 0;
    }
    let offset = (pixel - f64::from(origin)).round();
    let max_offset = f64::from(extent - 1);
    let clamped = if offset.is_finite() {
        offset.clamp(0.0, max_offset)
    } else if offset.is_sign_positive() {
        max_offset
    } else {
        0.0
    };
    // `clamped` is a non-negative integer screen offset, exact in f64.
    // Unsigned `div_ceil` is the stable ceil; signed `div_ceil` is not.
    let p = clamped as u64;
    let extent = u64::try_from(extent).unwrap_or(1);
    let norm = (p * 65536).div_ceil(extent);
    i32::try_from(norm.min(65535)).unwrap_or(0)
}

/// Pixel the Windows floor map produces: `origin + floor(norm * extent / 65536)`.
///
/// Arrival compares the cursor with this pixel. For a `norm` from [`normalize_axis`]
/// on a desktop at most 65536 pixels on the axis, it is the requested pixel.
pub(crate) fn denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    if extent <= 0 {
        return origin;
    }
    let norm = i64::from(norm).clamp(0, 65535);
    let landed = i64::from(origin) + norm * i64::from(extent) / 65536;
    i32::try_from(landed).unwrap_or(if landed.is_positive() {
        i32::MAX
    } else {
        i32::MIN
    })
}

pub(crate) fn cursor_arrived(actual: (i32, i32), target: (i32, i32)) -> bool {
    (actual.0 - target.0).abs() <= CURSOR_ARRIVAL_TOLERANCE_PX
        && (actual.1 - target.1).abs() <= CURSOR_ARRIVAL_TOLERANCE_PX
}

pub(crate) fn fold_sample(
    progress: SettleProgress,
    sample: Result<(i32, i32), ()>,
    target: (i32, i32),
) -> SettleProgress {
    let SettleProgress::Pending { mut saw_position } = progress else {
        return progress;
    };
    match sample {
        Ok(position) if cursor_arrived(position, target) => SettleProgress::Done(Settle::Arrived),
        Ok(_) => {
            saw_position = true;
            SettleProgress::Pending { saw_position }
        }
        Err(()) => SettleProgress::Pending { saw_position },
    }
}

/// `None` only when the cursor arrived. Otherwise the move was inserted and a
/// following button or drag packet must not be sent. `maybe_executed` stays set
/// so the ledger keeps the uncertainty.
pub(crate) fn failure(settle: Settle) -> Option<DriverError> {
    let message = match settle {
        Settle::Arrived => return None,
        Settle::TimedOut => {
            "pointer move was sent but the cursor did not arrive; following button and drag packets were not sent"
        }
        Settle::Unreadable => {
            "pointer move was sent but the cursor could not be read; following button and drag packets were not sent"
        }
    };
    Some(DriverError::Native {
        message: message.into(),
        maybe_executed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle_samples(samples: &[Result<(i32, i32), ()>], target: (i32, i32)) -> Settle {
        let mut progress = SettleProgress::pending();
        for sample in samples {
            progress = fold_sample(progress, *sample, target);
            if matches!(progress, SettleProgress::Done(_)) {
                return progress.finish();
            }
        }
        progress.finish()
    }

    #[test]
    fn denormalize_is_the_inverse_of_normalize_axis() {
        let origin = -1920;
        let extent = 3840;
        let norm = normalize_axis(100.0, origin, extent);
        let back = denormalize_axis(norm, origin, extent);
        assert_eq!(back, 100);
        assert!(cursor_arrived((back, back), (100, 100)));
        assert!(!cursor_arrived((back + 1, back), (back, back)));
        assert_eq!(CURSOR_ARRIVAL_TOLERANCE_PX, 0);
    }

    #[test]
    fn a_delayed_neighbor_does_not_count_as_arrived() {
        // approach_point steps one physical pixel. That old position must not open the gate.
        let target: (i32, i32) = (100, 100);
        let approach: (i32, i32) = (101, 101);
        assert!(!cursor_arrived(approach, target));
        assert!(!cursor_arrived((100, 101), target));
        assert_eq!(settle_samples(&[Ok(approach)], target), Settle::TimedOut);
        assert_eq!(
            settle_samples(&[Ok(approach), Ok(target)], target),
            Settle::Arrived,
            "the target sample after the old neighbor is the arrival"
        );
        assert_eq!(
            settle_samples(&[Ok((40, 50)), Err(()), Ok((40, 50))], target),
            Settle::TimedOut
        );
        assert_eq!(
            settle_samples(&[Err(()), Err(())], target),
            Settle::Unreadable
        );
    }

    #[test]
    fn timeout_or_unreadable_keeps_the_sent_move_uncertain() {
        let timeout = failure(Settle::TimedOut).expect("timeout is not success");
        let unread = failure(Settle::Unreadable).expect("an unread cursor is not success");
        assert!(timeout.maybe_executed());
        assert!(unread.maybe_executed());
        assert_eq!(timeout.error_code(), gcu_protocol::ErrorCode::DriverError);
        assert_eq!(unread.error_code(), gcu_protocol::ErrorCode::DriverError);
        assert!(failure(Settle::Arrived).is_none());
        match timeout {
            DriverError::Native {
                ref message,
                maybe_executed: true,
            } => assert!(message.contains("following button")),
            other => panic!("expected an uncertain native error, got {other:?}"),
        }
    }

    /// Windows' absolute map: `origin + floor(norm * extent / 65536)`. Independent of
    /// `denormalize_axis`, so a round-model norm still fails this oracle.
    fn floor_model(norm: i32, origin: i32, extent: i32) -> i32 {
        origin + (i64::from(norm) * i64::from(extent) / 65536) as i32
    }

    #[test]
    fn normalize_axis_win_test_virtual_desktop_lands_on_the_requested_pixel() {
        // win-test 2026-10-23, virtual desktop origin -2560, extent 6000.
        // Requested (-2553, 720) was sent as 76 and landed on -2554.
        let origin = -2560i32;
        let extent = 6000i32;
        let norm = normalize_axis(-2553.0, origin, extent);
        assert_eq!(norm, 77, "minimum norm whose floor bin is offset 7");
        assert!((0..=65535).contains(&norm));
        assert_eq!(floor_model(norm, origin, extent), -2553);
        assert_eq!(denormalize_axis(norm, origin, extent), -2553);
        let norm = normalize_axis(-2552.0, origin, extent);
        assert_eq!(norm, 88);
        assert_eq!(floor_model(norm, origin, extent), -2552);
        assert_eq!(denormalize_axis(norm, origin, extent), -2552);
        // Half-pixels round to an integer offset before the floor bin is chosen.
        assert_eq!(normalize_axis(-2553.4, origin, extent), 77);
        assert_eq!(
            floor_model(normalize_axis(-2553.6, origin, extent), origin, extent),
            -2554
        );
        // y = 720 on a 1440-tall desktop is the center bin, norm 32768, not the old 32790.
        let y = normalize_axis(720.0, 0, 1440);
        assert_eq!(y, 32768);
        assert_eq!(floor_model(y, 0, 1440), 720);
        assert_eq!(denormalize_axis(y, 0, 1440), 720);
    }

    #[test]
    fn normalize_axis_round_trips_every_pixel_through_the_floor_bin() {
        for extent in [1920, 2560, 3440, 3840, 6000, 7680, 1440, 1080] {
            let mut previous = -1i32;
            for offset in 0..extent {
                let norm = normalize_axis(f64::from(offset), 0, extent);
                assert!(
                    (0..=65535).contains(&norm),
                    "extent {extent} offset {offset} norm {norm}"
                );
                assert!(
                    norm >= previous,
                    "extent {extent} offset {offset} norm {norm} went backwards from {previous}"
                );
                previous = norm;
                assert_eq!(
                    floor_model(norm, 0, extent),
                    offset,
                    "extent {extent} offset {offset} norm {norm}"
                );
                assert_eq!(
                    denormalize_axis(norm, 0, extent),
                    offset,
                    "extent {extent} offset {offset} norm {norm}"
                );
                let shifted = -2560 + offset;
                let shifted_norm = normalize_axis(f64::from(shifted), -2560, extent);
                assert_eq!(shifted_norm, norm);
                assert_eq!(denormalize_axis(shifted_norm, -2560, extent), shifted);
                assert_eq!(floor_model(shifted_norm, -2560, extent), shifted);
            }
        }
    }
}
