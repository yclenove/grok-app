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

/// Admitted window copied into one scoped clipboard or input job.
///
/// Later enumeration must not replace these fields. Absent on an unscoped job.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmittedTarget {
    /// Top-level HWND.
    pub hwnd: u64,
    /// Window creation generation.
    pub incarnation: u64,
    /// Owner process id.
    pub pid: u32,
    /// Owner process creation time.
    pub created: u64,
    /// Boot-session identity. Empty is unproved.
    pub boot: String,
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
    /// Parent-owned clipboard recovery, independent of the killable child.
    fn clipboard(&self) -> Option<Arc<dyn super::clipboard::ClipboardApi>> {
        None
    }
}
/// Child-to-parent accounting barrier. No packet may send before preparation is
/// acknowledged; completion retires only acknowledged up events.
#[async_trait]
pub trait InputTracker: Send + Sync + 'static {
    /// Reserve every possible up before this packet can reach the OS.
    async fn prepare(&self, target: Target, events: Vec<Event>, delivery: Delivery) -> Result<()>;
    /// Acknowledge full insertion or proven zero insertion; uncertainty retains ups.
    async fn complete(&self, delivered: bool) -> Result<()>;
    /// Stage text in the parent before acknowledging any paste. No retry.
    async fn clipboard_stage(&self, _target: Target, _text: String) -> Result<u32> {
        Err(DriverError::CapabilityUnavailable)
    }
    /// Stage scoped text only when `admitted` can be proved at publication.
    ///
    /// `Some` is the claim copied at admission. The default refuses it and does
    /// not publish. `None` is the unscoped [`InputTracker::clipboard_stage`] route.
    async fn clipboard_stage_admitted(
        &self,
        target: Target,
        text: String,
        admitted: Option<AdmittedTarget>,
    ) -> Result<u32> {
        if admitted.is_some() {
            return Err(DriverError::ScopeDenied);
        }
        self.clipboard_stage(target, text).await
    }
    /// Restore the parent's transaction, including after cancelled dispatch.
    async fn clipboard_restore(&self) -> Result<()> {
        Err(DriverError::CapabilityUnavailable)
    }
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
    /// Fail closed unless the exact focus/ancestry and core sensitivity detector
    /// prove clipboard fallback suitable. Reads only; no activation or mutation.
    async fn clipboard_safe(
        &self,
        _target: Target,
        _focus: Option<NativeHandle>,
        _text: String,
    ) -> Result<bool> {
        Ok(false)
    }
    /// Compare the current precise focus to `focus` and check cancellation.
    /// The paste coordinator checks its attached parent Stop latch before/after
    /// this read; isolated workers are stopped through the independent parent
    /// channel and cannot send before the parent's guarded InputPrepare ack.
    async fn before_paste(
        &self,
        _target: Target,
        _focus: Option<NativeHandle>,
        _context: ActionContext,
    ) -> Result<()> {
        Err(DriverError::CapabilityUnavailable)
    }
    /// Resolve a pointer hit or exact focused/observed native element.
    async fn resolve(
        &self,
        window: NativeWindow,
        point: Option<WindowPoint>,
        focus: Option<NativeHandle>,
    ) -> Result<Target>;
    /// Resolve under an admitted window copied by the caller.
    ///
    /// The default is [`InputApi::resolve`]. A scoped focus element must be
    /// proved before any of its content is read. `admitted_pid` and
    /// `admitted_hwnd` are that copy, not a fresh lookup.
    async fn resolve_scoped(
        &self,
        window: NativeWindow,
        point: Option<WindowPoint>,
        focus: Option<NativeHandle>,
        _admitted_pid: u32,
        _admitted_hwnd: u64,
    ) -> Result<Target> {
        self.resolve(window, point, focus).await
    }
    /// Top-level window under `point`, or the captured window when capture is set.
    ///
    /// `Ok(None)` means the receiver cannot be proved. Scoped foreground pointer
    /// packets refuse that result. The default cannot prove a receiver.
    async fn pointer_receiver(&self, _point: WindowPoint) -> Result<Option<u64>> {
        Ok(None)
    }
    /// Live identity at a publication boundary. The default cannot prove it.
    ///
    /// Memory or a metadata read the implementor bounds. Not inside the pointer gate.
    fn recheck_publish(
        &self,
        _hwnd: u64,
        _incarnation: u64,
        _pid: u32,
        _created: u64,
        _boot: &str,
    ) -> Result<()> {
        Err(DriverError::ScopeDenied)
    }
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
    ///
    /// `claim` is the admitted window copied by value. `None` is an unscoped send
    /// and does not read a recipient. A scoped send compares the live child
    /// process inside this call, before any message is posted.
    async fn send(
        &self,
        target: Target,
        events: Vec<Event>,
        delivery: Delivery,
        ledger: Arc<InputLedger>,
        context: ActionContext,
        claim: Option<AdmittedTarget>,
    ) -> Result<()>;
    /// Sample state or pixel evidence. Core owns the final tree and redacted diff.
    ///
    /// `claim` is compared inside this call before content is read. `None` skips
    /// that comparison. Each settle sample carries the original claim.
    async fn sample(&self, target: Target, claim: Option<AdmittedTarget>) -> Result<u64>;
    /// Release held inputs on an independent path, including after Stop.
    async fn release(&self, ledger: Arc<InputLedger>);
}
