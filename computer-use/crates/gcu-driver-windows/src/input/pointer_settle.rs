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

/// Arrival tolerance for an absolute move.
///
/// The 0..65535 virtual-desktop normalization has been measured to land one
/// physical pixel off the requested point, so exact equality is too tight.
/// ±2 covers that rounding on both axes.
///
/// `plan::approach_point` stands only one pixel away from the target. A cursor
/// still sitting on that approach point is therefore inside this tolerance and
/// counts as arrived even when the final move has not been applied. Shrinking
/// the tolerance would reject real rounded arrivals. Timeout and unreadable
/// samples stay failures; this false arrival is the accepted tradeoff.
pub(crate) fn cursor_arrived(actual: (i32, i32), target: (i32, i32)) -> bool {
    (actual.0 - target.0).abs() <= 2 && (actual.1 - target.1).abs() <= 2
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
    fn cursor_arrival_tolerates_two_pixels_of_normalization_rounding() {
        assert!(cursor_arrived((431, 607), (431, 608)));
        assert!(cursor_arrived((433, 610), (431, 608)));
        assert!(!cursor_arrived((434, 608), (431, 608)));
        assert!(!cursor_arrived((-2460, 100), (431, 608)));
    }

    #[test]
    fn one_pixel_approach_counts_as_arrived_and_farther_samples_do_not() {
        // approach_point steps one physical pixel. Measured normalization error
        // is also one pixel, so ±2 treats the unmoved approach point as arrived.
        let target: (i32, i32) = (100, 100);
        let approach: (i32, i32) = (101, 101);
        assert_eq!((approach.0 - target.0).abs(), 1);
        assert_eq!((approach.1 - target.1).abs(), 1);
        assert!(cursor_arrived(approach, target));
        assert_eq!(
            settle_samples(&[Ok(approach)], target),
            Settle::Arrived,
            "a cursor still on the one-pixel approach is inside the rounding tolerance"
        );
        assert!(!cursor_arrived((103, 100), target));
        assert_eq!(
            settle_samples(&[Ok((40, 50)), Err(()), Ok((40, 50))], target),
            Settle::TimedOut
        );
        assert_eq!(
            settle_samples(&[Err(()), Err(())], target),
            Settle::Unreadable
        );
        assert_eq!(
            settle_samples(&[Err(()), Ok((100, 101))], target),
            Settle::Arrived
        );
    }

    #[test]
    fn timeout_or_unreadable_keeps_the_sent_move_uncertain() {
        let timeout = failure(Settle::TimedOut).expect("timeout is not success");
        let unread = failure(Settle::Unreadable).expect("an unread cursor is not success");
        assert!(timeout.maybe_executed());
        assert!(unread.maybe_executed());
        assert!(failure(Settle::Arrived).is_none());
        match timeout {
            DriverError::Native {
                ref message,
                maybe_executed: true,
            } => assert!(message.contains("following button")),
            other => panic!("expected an uncertain native error, got {other:?}"),
        }
    }
}
