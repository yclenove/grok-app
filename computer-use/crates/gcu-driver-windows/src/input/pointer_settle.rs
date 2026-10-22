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
/// normalization `inputs()` sends. Windows' rounding of that normalized value onto a physical
/// pixel is not confirmed. `live::live_move_landing_probe` records it. Do not merge, and do not
/// raise this constant, until that histogram shows the inverse error.
pub(crate) const CURSOR_ARRIVAL_TOLERANCE_PX: i32 = 0;

/// `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` axis, matching `native::inputs`.
pub(crate) fn normalize_axis(pixel: f64, origin: i32, extent: i32) -> i32 {
    ((pixel - f64::from(origin)) * 65535. / f64::from(extent - 1)).round() as i32
}

/// Inverse of `normalize_axis`. This is the pixel arrival compares against, not the request.
pub(crate) fn denormalize_axis(norm: i32, origin: i32, extent: i32) -> i32 {
    origin + (f64::from(norm) * f64::from(extent - 1) / 65535.).round() as i32
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
    fn denormalize_is_the_inverse_rounding_of_normalize_axis() {
        let origin = -1920;
        let extent = 3840;
        let norm = normalize_axis(100.0, origin, extent);
        let back = denormalize_axis(norm, origin, extent);
        assert!(cursor_arrived((back, back), (back, back)));
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

    /// `floor(norm * extent / 65536)` placed back on the axis. Not what `denormalize_axis` returns.
    fn bin_axis(norm: i32, origin: i32, extent: i32) -> i32 {
        origin + (i64::from(norm) * i64::from(extent) / 65536) as i32
    }

    fn disagreement_offsets(extent: i32) -> Vec<i32> {
        if extent <= 1 {
            return Vec::new();
        }
        (0..extent)
            .filter(|offset| {
                let norm = normalize_axis(f64::from(*offset), 0, extent);
                denormalize_axis(norm, 0, extent) != bin_axis(norm, 0, extent)
            })
            .collect()
    }

    #[test]
    fn inverse_and_pixel_bin_disagree_on_known_extents() {
        assert_eq!(disagreement_offsets(1920), vec![1, 2, 3, 8, 9, 16, 23]);
        let norm = normalize_axis(1.0, 0, 1920);
        assert_eq!(norm, 34);
        assert_eq!(denormalize_axis(norm, 0, 1920), 1);
        assert_eq!(bin_axis(norm, 0, 1920), 0);
        assert_eq!(disagreement_offsets(2560), vec![2, 4, 7, 9, 12, 17, 22, 27]);
        assert_eq!(
            disagreement_offsets(3840),
            vec![
                1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 31, 32, 33, 34, 35, 46, 47, 48, 49,
                61, 62, 63, 76, 77, 91
            ]
        );
        let wide = disagreement_offsets(7680);
        assert_eq!(wide.len(), 110);
        assert!(wide.contains(&100));
        let norm = normalize_axis(100.0, 0, 7680);
        assert_eq!(norm, 853);
        assert_eq!(denormalize_axis(norm, 0, 7680), 100);
        assert_eq!(bin_axis(norm, 0, 7680), 99);
        let shifted: Vec<i32> = disagreement_offsets(3840)
            .into_iter()
            .map(|o| o - 1920)
            .collect();
        let absolute: Vec<i32> = (0..3840)
            .filter(|offset| {
                let pixel = -1920 + offset;
                let norm = normalize_axis(f64::from(pixel), -1920, 3840);
                denormalize_axis(norm, -1920, 3840) != bin_axis(norm, -1920, 3840)
            })
            .map(|offset| -1920 + offset)
            .collect();
        assert_eq!(shifted, absolute);
    }
}
