//! Parent-held ledger and child acknowledgement barrier; cleanup never uses RPC.
use super::{codec, wire::WireDelivery, Action, Reply, Request, Response};
use crate::input::{
    api::{Event, InputReleaser, InputTracker, Target},
    ledger::InputLedger,
};
use async_trait::async_trait;
use gcu_core::driver::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot},
};

/// Bounded packet accounting metadata. The child still performs native delivery.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPacket {
    /// Exact stamped root and resolved child recipient.
    pub target: Target,
    /// Possible downs and acknowledged ups, at most 512 records.
    pub events: Vec<Event>,
    /// Approved delivery path.
    pub delivery: WireDelivery,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    version: u32,
    id: u64,
    epoch: u64,
    clipboard_sequence: Option<u32>,
}
pub(super) struct Notice {
    reply: Reply,
    done: oneshot::Sender<Result<Option<u32>>>,
}
struct Tracker(mpsc::Sender<Notice>);
impl Tracker {
    async fn notify(&self, reply: Reply) -> Result<Option<u32>> {
        let (done, wait) = oneshot::channel();
        self.0
            .send(Notice { reply, done })
            .await
            .map_err(|_| crashed())?;
        wait.await.map_err(|_| crashed())?
    }
}
#[async_trait]
impl InputTracker for Tracker {
    async fn clipboard_stage(&self, target: Target, text: String) -> Result<u32> {
        self.notify(Reply::ClipboardStage { target, text })
            .await?
            .ok_or_else(crashed)
    }
    async fn clipboard_restore(&self) -> Result<()> {
        self.notify(Reply::ClipboardRestore).await.map(|_| ())
    }
    async fn prepare(&self, target: Target, events: Vec<Event>, delivery: Delivery) -> Result<()> {
        self.notify(Reply::InputPrepare(InputPacket {
            target,
            events,
            delivery: delivery.into(),
        }))
        .await
        .map(|_| ())
    }
    async fn complete(&self, delivered: bool) -> Result<()> {
        self.notify(Reply::InputComplete(delivered))
            .await
            .map(|_| ())
    }
}
pub(super) fn channel() -> (Arc<dyn InputTracker>, mpsc::Receiver<Notice>) {
    let (tx, rx) = mpsc::channel(1);
    (Arc::new(Tracker(tx)), rx)
}
pub(super) async fn notify<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    notice: Notice,
    input: &mut R,
    output: &mut W,
    id: u64,
    epoch: u64,
) -> Result<()> {
    codec::write_frame(
        output,
        &Response {
            version: super::wire::VERSION,
            id,
            epoch,
            result: Ok(notice.reply),
        },
    )
    .await
    .map_err(|_| crashed())?;
    let ack: Ack = codec::read_frame(input)
        .await
        .map_err(|_| crashed())?
        .ok_or_else(crashed)?;
    if ack.version != super::wire::VERSION || ack.id != id || ack.epoch != epoch {
        return Err(crashed());
    }
    let _ = notice.done.send(Ok(ack.clipboard_sequence));
    Ok(())
}
fn crashed() -> DriverError {
    DriverError::Crashed {
        maybe_executed: true,
    }
}

pub(super) struct Recovery {
    ledger: Arc<InputLedger>,
    release: Arc<dyn InputReleaser>,
    pending: Mutex<Option<(InputPacket, usize)>>,
    clipboard: Option<Arc<dyn crate::input::clipboard::ClipboardApi>>,
    safety: Arc<Mutex<Option<Arc<crate::input::hotkey::SafetyGuard>>>>,
}
impl Recovery {
    pub(super) fn new(
        release: Arc<dyn InputReleaser>,
        safety: Arc<Mutex<Option<Arc<crate::input::hotkey::SafetyGuard>>>>,
    ) -> Self {
        Self {
            clipboard: release.clipboard(),
            ledger: Arc::default(),
            release,
            pending: Mutex::default(),
            safety,
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.ledger.is_empty() && self.clipboard.as_ref().is_none_or(|c| !c.pending())
    }
    pub(super) fn check_request(&self, request: &Request) -> Result<()> {
        if !request.is_read_only() {
            if let Some(guard) = self
                .safety
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
            {
                guard.check(foreground_request(request))?;
            }
        }
        Ok(())
    }
    fn check_delivery(&self, foreground: bool) -> Result<()> {
        if let Some(guard) = self
            .safety
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            if guard.check(foreground).is_err() {
                // Earlier activation/publication may already have executed.
                // Kill the child awaiting this ack even if its ledger is empty.
                guard.request_stop();
                return Err(DriverError::Cancelled {
                    maybe_executed: true,
                });
            }
        }
        Ok(())
    }
    pub(super) async fn receive<W: AsyncWrite + Unpin>(
        &self,
        reply: Reply,
        request: &Request,
        output: &mut W,
        id: u64,
        epoch: u64,
    ) -> Result<Option<Reply>> {
        let mut clipboard_sequence = None;
        match reply {
            Reply::ClipboardStage { target, text } => {
                self.check_delivery(true)?;
                let Request::Perform(Action::TypeText {
                    win,
                    text: approved,
                    route: KeyRoute::AllowForeground,
                    ..
                }) = request
                else {
                    return Err(crashed());
                };
                if approved != &text
                    || crate::desktop::HwndStamp::from(&target.window).id() != win.id
                {
                    return Err(crashed());
                }
                let clipboard = self.clipboard.as_ref().ok_or_else(crashed)?;
                let context = crate::uia::patterns::ActionContext::until(
                    &Cancel::default(),
                    std::time::Instant::now() + DRIVER_DEADLINE,
                );
                clipboard_sequence = Some(clipboard.stage(text, context).await?);
                // Stop may latch while staging awaits clipboard ownership/FFI.
                // Never acknowledge publication after the parent latch closes.
                self.check_delivery(true)?;
            }
            Reply::ClipboardRestore => {
                self.clipboard
                    .as_ref()
                    .ok_or_else(crashed)?
                    .restore()
                    .await?;
            }
            Reply::InputPrepare(packet) => {
                self.check_delivery(Delivery::from(packet.delivery) == Delivery::Foreground)?;
                if !valid(&packet, request) {
                    return Err(crashed());
                }
                let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
                if pending.is_some() {
                    return Err(crashed());
                }
                let previous =
                    self.ledger
                        .reserve(&packet.target, &packet.events, packet.delivery.into());
                *pending = Some((packet, previous));
            }
            Reply::InputComplete(delivered) => {
                let (packet, previous) = self
                    .pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take()
                    .ok_or_else(crashed)?;
                self.ledger.acknowledge(
                    &packet.target,
                    &packet.events,
                    packet.delivery.into(),
                    previous,
                    delivered,
                );
            }
            reply => return Ok(Some(reply)),
        }
        codec::write_frame(
            output,
            &Ack {
                version: super::wire::VERSION,
                id,
                epoch,
                clipboard_sequence,
            },
        )
        .await
        .map_err(|_| crashed())?;
        Ok(None)
    }
    /// Call only once the child is reaped or its action has returned. Never
    /// release before killing an abandoned child which could still send down.
    pub(super) async fn cleanup(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(clipboard) = &self.clipboard {
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_millis(100), clipboard.restore())
                    .await,
                Ok(Ok(()))
            ) {
                tracing::warn!("parent clipboard recovery deadline exceeded");
            }
        }
        if self.ledger.is_empty() {
            return;
        }
        if tokio::time::timeout(
            std::time::Duration::from_millis(100),
            self.release.release(self.ledger.clone()),
        )
        .await
        .is_err()
        {
            tracing::warn!("parent input release deadline exceeded");
        }
    }
}
fn valid(packet: &InputPacket, request: &Request) -> bool {
    let Request::Perform(action) = request else {
        return false;
    };
    let foreground = match action {
        Action::PointerClick { route, .. }
        | Action::PointerScroll { route, .. }
        | Action::PointerDrag { route, .. } => *route == PointerRoute::AllowForeground,
        Action::PressKeys { route, .. } | Action::TypeText { route, .. } => {
            *route == KeyRoute::AllowForeground
        }
        Action::Press { .. }
        | Action::SecondaryAction {
            action: gcu_core::model::NodeAction::Press | gcu_core::model::NodeAction::Confirm,
            ..
        } => {
            // A semantic press can become only a single targeted left click,
            // never a key packet, drag or foreground delivery.
            if !press_packet(&packet.events) {
                return false;
            }
            false
        }
        _ => return false,
    };
    packet.events.len() <= 512
        && !packet.events.is_empty()
        && crate::desktop::HwndStamp::from(&packet.target.window).id() == action.window().id
        && Delivery::from(packet.delivery)
            == if foreground {
                Delivery::Foreground
            } else {
                Delivery::TargetedEvent
            }
}
pub(super) fn foreground_request(request: &Request) -> bool {
    match request {
        Request::Perform(Action::Activate { .. }) => true,
        Request::Perform(
            Action::PointerClick { route, .. }
            | Action::PointerScroll { route, .. }
            | Action::PointerDrag { route, .. },
        ) => *route == PointerRoute::AllowForeground,
        Request::Perform(Action::PressKeys { route, .. } | Action::TypeText { route, .. }) => {
            *route == KeyRoute::AllowForeground
        }
        _ => false,
    }
}
pub(super) fn default_releaser() -> Arc<dyn InputReleaser> {
    #[cfg(windows)]
    {
        Arc::new(crate::input::NativeRelease::default())
    }
    #[cfg(not(windows))]
    {
        Arc::new(crate::input::fake::FakeInput::default())
    }
}

fn press_packet(events: &[Event]) -> bool {
    use gcu_core::driver::Button;
    match events {
        [Event::Move(point, false)] => point.x.is_finite() && point.y.is_finite(),
        [Event::Button(Button::Left, true, down, false), Event::Button(Button::Left, false, up, false)] => {
            down == up && down.x.is_finite() && down.y.is_finite()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_press_packets_cannot_grant_keys_drag_or_unbalanced_downs() {
        let p = gcu_core::model::WindowPoint { x: 20., y: 30. };
        assert!(press_packet(&[Event::Move(p, false)]));
        assert!(press_packet(&[
            Event::Button(Button::Left, true, p, false),
            Event::Button(Button::Left, false, p, false)
        ]));
        for events in [
            vec![Event::Key(65, true, false, false)],
            vec![Event::Move(p, true)],
            vec![Event::Button(Button::Left, true, p, false)],
            vec![
                Event::Button(Button::Right, true, p, false),
                Event::Button(Button::Right, false, p, false),
            ],
        ] {
            assert!(!press_packet(&events));
        }
    }
}
