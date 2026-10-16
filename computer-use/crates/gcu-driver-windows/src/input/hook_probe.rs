//! Probe policy shared by the native sender and disposable-desktop diagnostics.
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)] // Visible candidates require the debug/test-only empirical CI selection.
pub(crate) enum MouseProbe {
    ZeroMove,
    ZeroWheel,
    ZeroHWheel,
    AbsoluteSamePosition,
    RelativePair,
    XButtonPair,
}

pub(crate) fn mode_at(elapsed: Duration) -> MouseProbe {
    if elapsed < Duration::from_millis(250) {
        MouseProbe::ZeroMove
    } else if elapsed < Duration::from_millis(500) {
        MouseProbe::ZeroWheel
    } else {
        MouseProbe::ZeroHWheel
    }
}

impl MouseProbe {
    pub(crate) fn requested(self) -> u32 {
        if matches!(self, Self::RelativePair | Self::XButtonPair) {
            2
        } else {
            1
        }
    }
}
