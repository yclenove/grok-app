//! Injectable input boundary. Native implementations use a dedicated MTA worker.
use super::ledger::InputLedger;
use crate::{api::NativeWindow, uia::patterns::ActionContext};
use async_trait::async_trait;
use gcu_core::{
    driver::*,
    model::{NativeHandle, WindowPoint},
};
use std::sync::Arc;

/// A resolved child HWND and its owning stamped top-level window.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Exact top-level window provenance.
    pub window: NativeWindow,
    /// Child recipient; never a window found by name.
    pub hwnd: u64,
    /// Recipient class used to reject unsupported message routes before delivery.
    pub class: String,
    /// Whether the exact focus element has a native message recipient.
    pub message_capable: bool,
}
/// One physical/Unicode input. Debug output excludes text payloads.
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// Absolute screen position, physical pixels.
    Move(WindowPoint, bool),
    /// Mouse button transition, screen position and double-click marker.
    Button(#[serde(with = "ButtonDef")] Button, bool, WindowPoint, bool),
    /// Horizontal/vertical wheel delta at a screen position.
    Wheel(i32, i32, WindowPoint),
    /// Virtual-key transition, with extended-key and Alt-context flags.
    Key(u16, bool, bool, bool),
    /// UTF-16 unit transition. Targeted text posts WM_CHAR only on down.
    Unicode(u16, bool),
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "Button", rename_all = "snake_case")]
enum ButtonDef {
    Left,
    Right,
    Middle,
}

/// Independent parent-side cleanup; never reads a provider or queues worker RPC.
#[async_trait]
pub trait InputReleaser: Send + Sync + 'static {
    /// Release recorded ups only. Failed releases remain pending. Bounded by 100ms.
    async fn release(&self, ledger: Arc<InputLedger>);
}
/// Child-to-parent accounting barrier. No packet may send before preparation is
/// acknowledged; completion retires only acknowledged up events.
#[async_trait]
pub trait InputTracker: Send + Sync + 'static {
    /// Reserve every possible up before this packet can reach the OS.
    async fn prepare(&self, target: Target, events: Vec<Event>, delivery: Delivery) -> Result<()>;
    /// Acknowledge full insertion or proven zero insertion; uncertainty retains ups.
    async fn complete(&self, delivered: bool) -> Result<()>;
}
impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Move(..) => "Move",
            Self::Button(..) => "Button",
            Self::Wheel(..) => "Wheel",
            Self::Key(..) => "Key",
            Self::Unicode(..) => "Unicode",
        })
    }
}
impl Event {
    pub(crate) fn up(self) -> Option<Self> {
        match self {
            Self::Button(b, true, p, _) => Some(Self::Button(b, false, p, false)),
            Self::Key(k, true, e, a) => Some(Self::Key(k, false, e, a)),
            Self::Unicode(u, true) => Some(Self::Unicode(u, false)),
            _ => None,
        }
    }
    pub(crate) fn is_up(self) -> bool {
        matches!(
            self,
            Self::Button(_, false, ..) | Self::Key(_, false, ..) | Self::Unicode(_, false)
        )
    }
}
/// All OS operations sit behind this boundary. Reads are abandoned when their
/// futures drop. `send` must use ledger dispatch immediately around the native
/// call, restore uncertainty only on proven zero delivery, and never retry.
/// `release` bypasses the action worker and cancelled tokens; it is bounded.
#[async_trait]
pub trait InputApi: Send + Sync + 'static {
    /// Resolve a pointer hit or exact focused/observed native element.
    async fn resolve(
        &self,
        window: NativeWindow,
        point: Option<WindowPoint>,
        focus: Option<NativeHandle>,
    ) -> Result<Target>;
    /// Resolve a character using the recipient keyboard layout before dispatch.
    async fn character_key(
        &self,
        target: Target,
        character: char,
    ) -> Result<(u16, Vec<gcu_protocol::keys::Modifier>)>;
    /// Refresh stamped identity, integrity, desktop and child ownership.
    async fn validate(&self, target: Target, foreground: bool) -> Result<()>;
    /// Attach/allow/set/detach activation sequence, using dispatch accounting.
    async fn activate(&self, target: Target, context: ActionContext) -> Result<()>;
    /// Actual foreground HWND; errors must prevent global input.
    async fn foreground(&self) -> Result<u64>;
    /// Focus an exact UIA handle after verified, authorized activation.
    async fn focus(
        &self,
        target: Target,
        focus: NativeHandle,
        context: ActionContext,
    ) -> Result<()>;
    /// Deliver one packet with press-before-send accounting.
    async fn send(
        &self,
        target: Target,
        events: Vec<Event>,
        delivery: Delivery,
        ledger: Arc<InputLedger>,
        context: ActionContext,
    ) -> Result<()>;
    /// Sample state/pixel evidence. Core owns the final tree and redacted diff.
    async fn sample(&self, target: Target) -> Result<u64>;
    /// Release held inputs on an independent path, including after Stop.
    async fn release(&self, ledger: Arc<InputLedger>);
}
