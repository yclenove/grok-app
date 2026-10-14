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
}
pub(super) struct Notice {
    reply: Reply,
    done: oneshot::Sender<Result<()>>,
}
struct Tracker(mpsc::Sender<Notice>);
impl Tracker {
    async fn notify(&self, reply: Reply) -> Result<()> {
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
    async fn prepare(&self, target: Target, events: Vec<Event>, delivery: Delivery) -> Result<()> {
        self.notify(Reply::InputPrepare(InputPacket {
            target,
            events,
            delivery: delivery.into(),
        }))
        .await
    }
    async fn complete(&self, delivered: bool) -> Result<()> {
        self.notify(Reply::InputComplete(delivered)).await
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
    let _ = notice.done.send(Ok(()));
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
}
impl Recovery {
    pub(super) fn new(release: Arc<dyn InputReleaser>) -> Self {
        Self {
            ledger: Arc::default(),
            release,
            pending: Mutex::default(),
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.ledger.is_empty()
    }
    pub(super) async fn receive<W: AsyncWrite + Unpin>(
        &self,
        reply: Reply,
        request: &Request,
        output: &mut W,
        id: u64,
        epoch: u64,
    ) -> Result<Option<Reply>> {
        match reply {
            Reply::InputPrepare(packet) => {
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
pub(super) fn default_releaser() -> Arc<dyn InputReleaser> {
    #[cfg(windows)]
    {
        Arc::new(crate::input::NativeRelease)
    }
    #[cfg(not(windows))]
    {
        Arc::new(crate::input::fake::FakeInput::default())
    }
}
