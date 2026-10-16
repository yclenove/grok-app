//! Probe policy shared by the native sender and disposable-desktop diagnostics.
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // The last three candidates are exercised only by CI diagnostics.
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
