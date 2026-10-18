//! Architecture §14 and quality gates §4. Budget changes require an approved ADR
//! (变更须 ADR); never raise a limit to turn a failing measurement green.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Version of the native latency report contract, independent of engine protocols.
pub const SCHEMA_VERSION: u32 = 1;
/// Minimum real wall-clock samples per operation and platform. Changes require ADR.
pub const MIN_SAMPLES: usize = 50;
/// Exact UTF-8 tools/list array budget, retaining the existing 14000-byte limit.
/// Changes require ADR. All tool profiles must fit this same ceiling.
pub const TOOL_CATALOG_BYTES: usize = 14_000;

/// Required native desktop, not the operating system of the report checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Platform {
    /// macOS at 2x scaling.
    #[serde(rename = "macos")]
    Macos,
    /// Windows at 150% scaling.
    #[serde(rename = "windows")]
    Windows,
    /// Linux X11 at 1x scaling; Wayland is not a substitute.
    #[serde(rename = "linux-x11")]
    LinuxX11,
}

impl Platform {
    /// Complete mandatory platform set. No command-line subset bypass is allowed.
    pub const ALL: [Self; 3] = [Self::Macos, Self::Windows, Self::LinuxX11];

    /// Stable identifier used in report filenames and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::LinuxX11 => "linux-x11",
        }
    }

    /// Required desktop scale in percent, from quality gates §4.
    pub const fn scale_percent(self) -> u32 {
        match self {
            Self::Macos => 200,
            Self::Windows => 150,
            Self::LinuxX11 => 100,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// End-to-end latency case. Action durations include the real settle engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// State of at most 1200 nodes, including the safe screenshot pipeline.
    GetAppState,
    /// Element click through the router, until its response including settle.
    Click,
    /// Fifty characters through the router, until its response including settle.
    TypeText,
    /// App discovery after cache warm-up (required on Windows).
    ListApps,
}

impl Operation {
    /// Every report must contain each case exactly once.
    pub const ALL: [Self; 4] = [
        Self::GetAppState,
        Self::Click,
        Self::TypeText,
        Self::ListApps,
    ];

    /// Tool name used in diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GetAppState => "get_app_state",
            Self::Click => "click",
            Self::TypeText => "type_text",
            Self::ListApps => "list_apps",
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Inclusive nearest-rank percentile limits, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyBudget {
    /// Median ceiling.
    pub p50_ms: u64,
    /// Ninety-fifth percentile ceiling.
    pub p95_ms: u64,
}

/// Single latency budget table, transcribed from architecture §14.
/// Pure lookup; no blocking, timeout or cancellation. Changes require ADR.
pub const fn latency_budget(platform: Platform, operation: Operation) -> LatencyBudget {
    use Operation::{Click, GetAppState, ListApps, TypeText};
    use Platform::{LinuxX11, Macos, Windows};
    let (p50_ms, p95_ms) = match (platform, operation) {
        (Macos, GetAppState) => (250, 600),
        (Windows, GetAppState) => (450, 900),
        (LinuxX11, GetAppState) => (400, 900),
        (Macos, Click) => (300, 900),
        (Windows | LinuxX11, Click) => (400, 1100),
        (Macos, TypeText) => (400, 1200),
        (Windows | LinuxX11, TypeText) => (600, 1500),
        (Macos, ListApps) => (150, 400),
        (Windows, ListApps) => (300, 800),
        (LinuxX11, ListApps) => (200, 500),
    };
    LatencyBudget { p50_ms, p95_ms }
}
