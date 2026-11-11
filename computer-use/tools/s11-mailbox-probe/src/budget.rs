//! Command and attempt caps from ADR 0019 §2, shortened by the §5 subcase rules.
//! Sub-budget maxima are not additive. A shortfall stops the attempt.

/// Install/query commands.
pub const MS_QUERY: u64 = 5_000;
/// `tart clone`.
pub const MS_CLONE: u64 = 60_000;
/// `hdiutil create`.
pub const MS_CREATE: u64 = 20_000;
/// `hdiutil attach`.
pub const MS_ATTACH: u64 = 10_000;
/// Each SSH command, including upload.
pub const MS_SSH: u64 = 60_000;
/// TCP connect cap, also passed as `ConnectTimeout`.
pub const CONNECT_SEC: u64 = 5;
/// `tart stop` inside the cleanup stop/confirm slice.
pub const MS_STOP: u64 = 20_000;
/// VM delete confirmation slice.
pub const MS_DELETE: u64 = 5_000;
/// Normal detach.
pub const MS_DETACH: u64 = 5_000;
/// The single allowed force detach.
pub const MS_FORCE: u64 = 5_000;
/// Unlink plus absence check.
pub const MS_UNLINK: u64 = 5_000;
/// Per-subcase mailbox load.
pub const MS_LOAD: u64 = 60_000;
/// Per-subcase teardown.
pub const MS_TEARDOWN: u64 = 60_000;
/// Tool stdout/stderr cap. Overflow is untrusted, not a short read.
pub const OUT_CAP: usize = 1024 * 1024;
/// Test result slot and production frame cap.
pub const SLOT_CAP: usize = 4096;
/// Uploaded probe cap.
pub const UPLOAD_CAP: usize = 8 * 1024 * 1024;
/// Dispatcher script cap for the one-time install.
pub const DISPATCHER_CAP: usize = 256 * 1024;
/// C1 frame size.
pub const FRAME_LEN: usize = 4096;
/// C1 iterations per direction.
pub const C1_ITERS: u8 = 8;
/// Visibility wait per publish.
pub const VISIBILITY_MS: u64 = 3_000;
/// Round trip at or above this blocks a real mailbox implementation.
pub const RTT_BLOCK_US: u64 = 2_000_000;
/// Open/read/publish slice observation threshold.
pub const SLICE_NOTE_US: u64 = 100_000;
/// Ledger file cap.
pub const LEDGER_CAP: usize = 256 * 1024;
/// Report file cap.
pub const REPORT_CAP: usize = 64 * 1024;

pub const EVIDENCE: &str = "DEFAULT-network/mechanics-only";
pub const GOLDEN: &str = "macos-s2-eval";
pub const GUEST_ROLE: &str = "eval";
pub const GUEST_DIR: &str = "/Users/eval/.s11-0c";
pub const DISPATCHER_PATH: &str = "/Users/eval/.s11-0c/dispatcher";
pub const SHARE_VOLUME: &str = "/Volumes/My Shared Files";
pub const SLOT_NAME: &str = "probe-result";
pub const VOLNAME: &str = "GCU-MAILBOX";

/// A closed error/reason code: ASCII, no path characters.
pub fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
