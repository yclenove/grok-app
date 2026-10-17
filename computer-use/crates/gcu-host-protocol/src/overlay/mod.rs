//! Route B overlay state and layout. No Tauri, core or driver dependencies.
//! Memory-only methods are synchronous, bounded, and require no cancellation.
pub mod capture;
mod layout;
pub mod safety;
use crate::messages::{OverlayShow, OverlayStatus};
pub use layout::{backing_placement, placement, Monitor, Placement, Point};
use serde::Serialize;
use std::collections::BTreeMap;

/// Route A/B cursor spring response, in milliseconds.
pub const RESPONSE_MS: u32 = 350;
/// Idle pointers disappear after thirty seconds of monotonic Host time.
pub const CURSOR_IDLE_MS: u64 = 30_000;

/// App-owned visual state; engine notifications cannot clear Stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No task.
    Hidden,
    /// Background action.
    Running,
    /// Foreground action.
    Foreground,
    /// Explicit user Stop.
    Stopped,
}
/// Bounded scene sent only to the local overlay webviews.
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    /// Visual state, never an authorization decision.
    pub phase: Phase,
    /// Most recently active target, already validated.
    pub target: Option<OverlayShow>,
    /// Short engine status, rendered as plain text.
    pub message: String,
    /// False after Stop, disconnect, or thirty idle seconds.
    pub cursor_visible: bool,
}
#[derive(Default)]
struct Entry {
    revision: u64,
    target: Option<OverlayShow>,
    message: String,
    foreground: bool,
    cursor_at_ms: u64,
}
/// At most 64 live sessions. Stop is owned by the trusted control path.
#[derive(Default)]
pub struct Desk {
    entries: BTreeMap<String, Entry>,
    revision: u64,
    stopped: bool,
}
impl Desk {
    fn entry(&mut self, session: &str) -> Result<&mut Entry, crate::Error> {
        if session.is_empty() || session.len() > 256 {
            return Err(crate::Error::Invalid);
        }
        if !self.entries.contains_key(session) && self.entries.len() >= 64 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.revision)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        let entry = self.entries.entry(session.to_owned()).or_default();
        entry.revision = self.revision;
        Ok(entry)
    }
    /// Validate physical geometry before retaining it. Invalid updates have no effect.
    pub fn show(&mut self, target: OverlayShow) -> Result<(), crate::Error> {
        self.show_at(target, 0)
    }
    /// Same validation with injected monotonic time for idle expiry.
    pub fn show_at(&mut self, mut target: OverlayShow, now_ms: u64) -> Result<(), crate::Error> {
        if target.window_id.len() > 256
            || !layout::valid_rect(target.frame)
            || !layout::valid_scale(target.scale)
            || ![target.x, target.y]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 10_000_000.)
        {
            return Err(crate::Error::Invalid);
        }
        target.title = clean(&target.title);
        let session = target.session_id.clone();
        let entry = self.entry(&session)?;
        entry.target = Some(target);
        entry.cursor_at_ms = now_ms;
        Ok(())
    }
    /// Update the capsule; status is informational and cannot change the latch.
    pub fn status(&mut self, status: OverlayStatus) -> Result<(), crate::Error> {
        let entry = self.entry(&status.session_id)?;
        entry.message = clean(&status.message);
        entry.foreground = status.foreground;
        Ok(())
    }
    /// Turn/session end hides that entry, preserving Stop.
    pub fn hide(&mut self, session: &str) {
        self.entries.remove(session);
    }
    /// Engine replacement drops stale targets, preserving Stop.
    pub fn disconnect(&mut self) {
        self.entries.clear();
    }
    /// Mirror the independent trusted control latch.
    pub fn set_stopped(&mut self, stopped: bool) {
        self.stopped = stopped;
    }
    /// Take a bounded immutable visual snapshot.
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_at(0)
    }
    /// Pure timed snapshot; missed render frames never extend cursor lifetime.
    pub fn snapshot_at(&self, now_ms: u64) -> Snapshot {
        let active = self.entries.values().max_by_key(|e| e.revision);
        Snapshot {
            phase: if self.stopped {
                Phase::Stopped
            } else if active.is_some_and(|e| e.foreground) {
                Phase::Foreground
            } else if active.is_some() {
                Phase::Running
            } else {
                Phase::Hidden
            },
            target: if self.stopped {
                None
            } else {
                active.and_then(|e| e.target.clone())
            },
            message: active.map(|e| e.message.clone()).unwrap_or_default(),
            cursor_visible: !self.stopped
                && active.is_some_and(|e| {
                    e.target.is_some() && now_ms.saturating_sub(e.cursor_at_ms) < CURSOR_IDLE_MS
                }),
        }
    }
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(160)
        .collect()
}

/// Native safety boundary, implemented by Tauri and faked in portable tests.
pub trait Surface {
    /// Platform error.
    type Error;
    /// Prevent showing/clicking the surface from activating its application.
    fn nonactivating(&mut self) -> Result<(), Self::Error>;
    /// Entire drawing surface passes clicks; a separate capsule alone receives input.
    fn passthrough(&mut self, cursor: bool) -> Result<(), Self::Error>;
    /// Exclude pixels using an OS API or the X11 hide/acknowledge capture lease.
    fn exclude_capture(&mut self) -> Result<(), Self::Error>;
    /// Fail closed after any safety setup failure.
    fn hide(&mut self);
}
/// Configure while hidden. Never show after a failed safety operation.
pub fn prepare_surface<S: Surface>(surface: &mut S, cursor: bool) -> Result<(), S::Error> {
    let result = surface
        .nonactivating()
        .and_then(|()| surface.passthrough(cursor))
        .and_then(|()| surface.exclude_capture());
    if result.is_err() {
        surface.hide();
    }
    result
}

/// Local overlay command boundary, using the actual Rust webview label.
/// All commands from the remote browser are rejected. Memory-only, no waits.
pub fn command_allowed(label: &str, command: &str) -> bool {
    match label {
        "cu-overlay" => command == "cu_overlay_snapshot",
        "cu-overlay-controls" => matches!(command, "cu_overlay_snapshot" | "cu_stop" | "cu_resume"),
        "cu-browser" => false,
        _ => true,
    }
}
