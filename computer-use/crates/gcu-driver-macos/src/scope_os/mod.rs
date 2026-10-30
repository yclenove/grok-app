//! macOS instance evidence.
//!
//! Facts are re-read at the moment of use. A fact the OS cannot re-derive is
//! `None`. Delivery checks the same facts immediately before `CGEvent.post`
//! and before an accessibility mutation. A missing permission or a missing
//! private symbol stays closed. Title, geometry, and a cached pid are not
//! substitutes.

mod deliver;
mod evidence;
mod facts;
mod spi;
mod table;

#[cfg(target_os = "macos")]
mod live;

#[cfg(test)]
mod round2_tests;
#[cfg(test)]
mod tests;

pub(crate) use deliver::PostView;
pub(crate) use evidence::{not_proved, Evidence};
pub(crate) use spi::ax_window_status;

#[cfg(target_os = "macos")]
pub(crate) use facts::{ElementFact, OsWindow, Snapshot, TokenBook};
#[cfg(target_os = "macos")]
pub(crate) use live::{live_evidence, snapshot_from_rows};
#[cfg(target_os = "macos")]
pub(crate) use spi::cg_id_from_ax;
