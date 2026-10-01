//! One-shot transfer of a restored original to a private process, not a global
//! CLIPBOARD_MANAGER. Neither control-channel EOF nor Stop means paste completed.
use super::*;

#[path = "clipboard_keeper_child.rs"]
mod child;
#[path = "clipboard_keeper_transport.rs"]
mod transport;
pub use transport::ProcessWitness as KeeperProcess;
use transport::{frame, number, COMMIT, FAILED, OWNED, READY, SEED};

const PROOF: &[u8] = b"_GROK_CU_KEEPER_PROOF_V1";
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeeperStatus {
    NotRequested,
    Starting,
    Committing,
    Transferred,
    Superseded,
    Failed,
    Unknown,
}
pub(super) struct Transfer {
    transport: transport::Transport,
    nonce: [u8; 16],
    epoch: u64,
    window: Window,
    deadline: Instant,
    status: KeeperStatus,
}

pub fn keeper_entrypoint() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|a| a != transport::FLAG) {
        return None;
    }
    if args.len() != 2 {
        return Some(2);
    }
    // This runs before App logging, singleton setup, directories or GUI startup.
    Some(if child::run().is_ok() { 0 } else { 1 })
}

impl ClipboardOwner {
    pub fn keeper_status(&self) -> KeeperStatus {
        self.keeper
            .as_ref()
            .map_or(KeeperStatus::NotRequested, |k| k.status)
    }
    pub fn keeper_process(&self) -> Option<KeeperProcess> {
        self.keeper.as_ref().map(|k| k.transport.process.clone())
    }
    /// Explicit one-shot exit preparation. Caller must retain/pump the original
    /// until safe close; spawning the child is NOT successful transfer evidence.
    pub fn begin_keeper(
        &mut self,
        lease: &ClipboardLease,
        cancellation: &ActionCancellation,
    ) -> Result<(), String> {
        self.check_lease(lease)?;
        cancellation.check()?;
        if self.state != ClipboardState::Restored
            || self.saved.was_empty()
            || self.keeper.is_some()
            || self.handoff_pending()
        {
            return Err(
                "keeper requires an unattempted restored nonempty original with no pending manager"
                    .into(),
            );
        }
        let nonce = *uuid::Uuid::new_v4().as_bytes();
        self.server_time()?;
        let epoch = self.fenced(|this| {
            let current = this.barrier()?;
            if current != this.window || this.owned_epoch != Some(this.reader.epoch) {
                return Err("clipboard changed before keeper preparation".into());
            }
            cancellation.check()?;
            write_proof(&this.reader.conn, this.window, &nonce)?;
            Ok(this.reader.epoch)
        })?;
        let transport =
            transport::Transport::spawn(frame(SEED, self.window, self.acquired, &nonce))?;
        self.keeper = Some(Transfer {
            transport,
            nonce,
            epoch,
            window: 0,
            deadline: Instant::now() + DEADLINE,
            status: KeeperStatus::Starting,
        });
        Ok(())
    }
    pub(in crate::clipboard) fn keeper_pending(&self) -> bool {
        matches!(
            self.keeper_status(),
            KeeperStatus::Starting | KeeperStatus::Committing | KeeperStatus::Unknown
        )
    }
    pub(super) fn check_keeper(&mut self) -> Result<(), String> {
        let Some(mut keeper) = self.keeper.take() else {
            return Ok(());
        };
        let result = self.poll_keeper(&mut keeper);
        self.keeper = Some(keeper);
        result
    }

    // Probe-only scheduling control: deliver the already queued, production
    // COMMIT without consuming its reply. The fixture waits the REAL deadline;
    // it does not forge an ACK, modify a clock, or change the child executable.
    #[cfg(feature = "native-probe")]
    pub(in crate::clipboard) fn fixture_flush_keeper_commit(&mut self) -> Result<Instant, String> {
        if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
            return Err("keeper scheduling fixture requires owned Xvfb".into());
        }
        let keeper = self.keeper.as_mut().ok_or("missing fixture keeper")?;
        if keeper.status != KeeperStatus::Committing {
            return Err("fixture must retain the original queued commit".into());
        }
        for _ in 0..32 {
            if keeper.transport.flush_pending()? {
                return Ok(keeper.deadline);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err("fixture commit socket remained busy".into())
    }
    fn poll_keeper(&mut self, keeper: &mut Transfer) -> Result<(), String> {
        if !matches!(
            keeper.status,
            KeeperStatus::Starting | KeeperStatus::Committing | KeeperStatus::Unknown
        ) {
            return Ok(());
        }
        let current = self.barrier()?;
        if current != self.window && (keeper.window == 0 || current != keeper.window) {
            keeper.status = KeeperStatus::Superseded;
            keeper.transport.disconnect();
            return Ok(());
        }
        if keeper.transport.process.finished() {
            keeper.status = KeeperStatus::Failed;
            keeper.transport.disconnect();
            return Err("keeper process exited before verified transfer".into());
        }
        let recovering = keeper.status == KeeperStatus::Unknown;
        let result = (|| {
            if !recovering && Instant::now() > keeper.deadline {
                return Err("keeper handshake deadline; do not retry".into());
            }
            // A deadline cannot fabricate success, but neither does it erase
            // the original channel and exact native ownership evidence. Read
            // late replies without flushing any remaining COMMIT bytes.
            let reply = if recovering {
                keeper.transport.read_reply()?
            } else {
                keeper.transport.poll()?
            };
            let Some(message) = reply else {
                return Ok(());
            };
            if message[16..] != keeper.nonce {
                return Err("keeper control identity mismatch".into());
            }
            let window = number(&message[8..12]);
            if keeper.status == KeeperStatus::Starting
                && &message[..8] == READY
                && window != 0
                && number(&message[12..16]) == 0
            {
                self.fenced(|this| {
                    let current = this.barrier()?;
                    if current != this.window
                        || this.reader.epoch != keeper.epoch
                        || this.state != ClipboardState::Restored
                    {
                        return Err("clipboard changed before keeper commit".into());
                    }
                    require_proof(&this.reader.conn, window, &keeper.nonce)?;
                    Ok(())
                })?;
                keeper.window = window;
                // Record before queuing: a later channel error may be post-send.
                keeper.status = KeeperStatus::Committing;
                keeper
                    .transport
                    .queue(frame(COMMIT, window, 0, &keeper.nonce))?;
                keeper.deadline = Instant::now() + DEADLINE;
            } else if matches!(
                keeper.status,
                KeeperStatus::Committing | KeeperStatus::Unknown
            ) && &message[..8] == OWNED
                && window == keeper.window
                && number(&message[12..16]) != 0
            {
                self.fenced(|this| {
                    let current = this.barrier()?;
                    if current != window || keeper.epoch.checked_add(1) != Some(this.reader.epoch) {
                        return Err("keeper ownership changed before acknowledgement".into());
                    }
                    require_proof(&this.reader.conn, window, &keeper.nonce)?;
                    Ok(())
                })?;
                keeper.status = KeeperStatus::Transferred;
                keeper.transport.disconnect();
            } else if &message[..8] == FAILED {
                return Err("keeper rejected native ownership transfer".into());
            } else {
                return Err("keeper control phase mismatch".into());
            }
            Ok(())
        })();
        if result.is_err() {
            keeper.status = if keeper.status == KeeperStatus::Starting {
                KeeperStatus::Failed
            } else {
                KeeperStatus::Unknown
            };
            if keeper.status == KeeperStatus::Failed {
                keeper.transport.disconnect();
            }
        }
        result
    }
}

fn write_proof(conn: &RustConnection, window: Window, nonce: &[u8; 16]) -> Result<(), String> {
    conn.change_property8(
        PropMode::REPLACE,
        window,
        atom(conn, PROOF)?,
        AtomEnum::INTEGER,
        nonce,
    )
    .map_err(err)?
    .check()
    .map_err(err)
}
fn require_proof(conn: &RustConnection, window: Window, nonce: &[u8; 16]) -> Result<(), String> {
    let value = conn
        .get_property(false, window, atom(conn, PROOF)?, AtomEnum::INTEGER, 0, 4)
        .map_err(err)?
        .reply()
        .map_err(err)?;
    if value.type_ != u32::from(AtomEnum::INTEGER)
        || value.format != 8
        || value.bytes_after != 0
        || value.value != nonce
    {
        return Err("keeper private window proof mismatch".into());
    }
    Ok(())
}
