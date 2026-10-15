//! Linux AT-SPI, X11, and Wayland computer-use driver.

#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

/// AT-SPI role mapping, tree construction, event subscriptions, and connection management.
pub mod atspi;
/// Linux computer-use driver implementation adhering to `gcu_core::driver::Driver`.
pub mod driver;
/// Linux application identity, filesystem provenance, and re-authorization checks.
pub mod identity;
/// Linux driver action routing and dispatching across AT-SPI and X11.
pub mod perform;
/// Wayland environment detection, compositor classification, and portal probing.
pub mod wayland;
/// X11 window management, EWMH alignment, and window capture.
pub mod x11;

pub use driver::LinuxDriver;
pub use identity::{needs_reauth, same_process_instance, AppIdentity};
pub use wayland::{
    detect_environment, CaptureError, Compositor, EnvSource, FakeFrameReceiver, FakePortalChecker,
    FakePortalClient, FakePortalSession, LinuxEnvironment, LinuxSessionType, MockEnv,
    OriginBinding, PipeWireFrameReceiver, PixelRegion, PortalChecker, PortalCursorMode,
    PortalError, PortalInfo, PortalScreenCastClient, PortalScreenCastResult,
    PortalScreenCastSession, PortalSourceKind, PortalStream, RawCapturedFrame, RestoreError,
    RestoreRecord, RestoreTokenStore, SystemEnv, TargetEnvironment, VideoTransform,
    WaylandCapturePipeline, MAX_TOKEN_LIFETIME_SECS,
};
pub use x11::{FakeX, InputLedger, RealX, XServer};
