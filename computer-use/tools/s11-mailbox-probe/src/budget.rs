//! Caps for one C1/C2/C8 attempt. Sub-budgets are not additive.

use std::time::{Duration, Instant};

pub const MS_QUERY: u64 = 5_000;
pub const MS_CLONE: u64 = 60_000;
pub const MS_SSH: u64 = 60_000;
pub const CONNECT_SEC: u64 = 5;
pub const MS_STOP: u64 = 20_000;
pub const MS_DELETE: u64 = 5_000;
pub const MS_KEYGEN: u64 = 5_000;
pub const ATTEMPT_MS: u64 = 240_000;
pub const RESERVE_CLEANUP_MS: u64 = 60_000;
pub const GRACE_MS: u64 = 200;
pub const REAP_MS: u64 = 300;
pub const OUT_CAP: usize = 1024 * 1024;
pub const SLOT_CAP: usize = 4096;
pub const UPLOAD_CAP: usize = 8 * 1024 * 1024;
pub const DISPATCHER_CAP: usize = 256 * 1024;
pub const FRAME_LEN: usize = 4096;
pub const C1_ITERS: u8 = 8;
pub const VISIBILITY_MS: u64 = 3_000;
pub const RTT_BLOCK_US: u64 = 2_000_000;
pub const SLICE_NOTE_US: u64 = 100_000;
pub const REPORT_CAP: usize = 64 * 1024;
pub const REMOTE_CAP: usize = 8192;

pub const EVIDENCE: &str = "DEFAULT-network/mechanics-only";
pub const EXPORT_LABEL: &str = "host-private-directory (not the capped HFSX volume)";
pub const GOLDEN: &str = "macos-s2-eval";
pub const GUEST_ROLE: &str = "eval";
pub const GUEST_DIR: &str = "/Users/eval/.s11-0c";
pub const DISPATCHER_PATH: &str = "/Users/eval/.s11-0c/dispatcher";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const SHARE_VOLUME: &str = "/Volumes/My Shared Files";
pub const EVAL_KEY_FILE: &str = ".ssh/id_ed25519_tart";

pub fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Absolute end of one attempt. Command timeouts are `min(cap, remaining)` and
/// are never restarted from a fresh full cap.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    end: Instant,
}

impl Deadline {
    pub fn fresh() -> Self {
        Self {
            end: Instant::now() + Duration::from_millis(ATTEMPT_MS),
        }
    }

    pub fn expired() -> Self {
        Self {
            end: Instant::now() - Duration::from_millis(1),
        }
    }

    pub fn within(ms: u64) -> Self {
        Self {
            end: Instant::now() + Duration::from_millis(ms),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.end.saturating_duration_since(Instant::now())
    }

    pub fn past(&self) -> bool {
        Instant::now() >= self.end
    }

    /// Experiment work may use only the time above the cleanup reserve.
    pub fn experiment(&self, cap_ms: u64) -> Option<Duration> {
        let left = self.remaining();
        let reserve = Duration::from_millis(RESERVE_CLEANUP_MS);
        if left <= reserve {
            return None;
        }
        Some(cap(left - reserve, cap_ms))
    }

    /// Cleanup commands may use whatever remains until the absolute end.
    pub fn cleanup(&self, cap_ms: u64) -> Option<Duration> {
        let left = self.remaining();
        if left.is_zero() {
            None
        } else {
            Some(cap(left, cap_ms))
        }
    }
}

fn cap(left: Duration, cap_ms: u64) -> Duration {
    let cap = Duration::from_millis(cap_ms);
    if left < cap {
        left
    } else {
        cap
    }
}
