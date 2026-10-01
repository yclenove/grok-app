//! Native Wayland portal session ownership, shared by production and transport tests.
//!
//! A portal grant is not a screenshot, an EI device, or model input authority.
//! Supervised CPU capture, low-level EI input, frame/generation binding and
//! upright screenshot-pixel motion are implemented. They do not enable
//! App/ACP/MCP actions without Host intent, permission, target and effect checks.
//! In particular, it never falls back to XWayland.

#![cfg(target_os = "linux")]

mod adapter;
mod capture;
mod ei_ffi;
mod ei_owner;
mod frame;
mod gnome_control;
mod gnome_native;
mod gnome_session;
mod grant;
mod host_session;
mod input;
mod logind;
mod observation;
mod policy;
mod presentation;
mod registry;
mod registry_adapter;
mod sequence;
mod session;
mod transport;

pub use adapter::{PortalAdapter, PortalInputPolicy};
pub use frame::{CapturedFrame, PixelRegion, VideoTransform};
pub use gnome_control::{
    GnomeHelperControl, HelperExtension, HelperHealth, HelperSnapshot, GNOME_HELPER_UUID,
};
pub use gnome_native::{GnomeNativePolicyWatch, GnomePolicyActivation};
pub use gnome_session::GnomeSessionWatch;
pub use grant::{PortalGrant, SourceKind, StreamGrant};
pub use host_session::PortalHostSession;
pub use input::{InputAction, InputCapabilities, InputRegion, InputState, InputSubmission};
pub use logind::LogindSessionWatch;
pub use observation::ObservedFrame;
pub use presentation::{PresentedFrame, PresentedImage};
pub use registry::{PortalRegistry, PortalSelection, PortalSelectionState};
pub use session::{CloseReason, PortalOptions, PortalSession, SessionState};

#[cfg(test)]
mod tests;
