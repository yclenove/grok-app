//! A single Host-owned selection lease. This is NOT an input-completion oracle.
//! Keep pumping this service while a native paste runs and while restored data
//! remain owned. Never call restore merely because Stop/a timer fired, and never
//! drop a live service as a rollback: dropping its X connection loses selection
//! data. The Host must retain it until `close` succeeds or a clipboard manager
//! has accepted the restored formats. App/manager lifecycle integration is separate.
use super::*;
use crate::client::ServerFence;
use grok_computer_use_core::execution::ActionCancellation;
use std::collections::{HashMap, VecDeque};
use x11rb::protocol::xproto::{
    self, ChangeWindowAttributesAux, PropMode, PropertyNotifyEvent, SelectionRequestEvent,
};
use x11rb::wrapper::ConnectionExt as _;

const MAX_PENDING: usize = 256;
const MAX_TRANSFERS: usize = 16;
const TRANSFER_DEADLINE: Duration = Duration::from_secs(5);
const MAX_TASK_TEXT: usize = 32768;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardState {
    Prepared,
    Published,
    Restored,
    Lost,
    Uncertain,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored,
    AlreadyRestored,
    PreservedNewOwner,
}

/// Opaque, non-cloneable identity; another service's lease cannot publish/restore.
pub struct ClipboardLease {
    identity: uuid::Uuid,
}

#[derive(Clone)]
enum Source {
    Saved(Arc<ClipboardSnapshot>),
    Task(Arc<Vec<Materialized>>),
}
impl Source {
    fn formats(&self) -> &[Materialized] {
        match self {
            Self::Saved(s) => &s.formats,
            Self::Task(s) => s,
        }
    }
}

struct Request {
    event: SelectionRequestEvent,
    source: Source,
    acquired: u32,
    allowed: bool,
}
enum Pending {
    Request(Request),
    Property(PropertyNotifyEvent, u64),
    Destroy(Window),
}
struct Transfer {
    source: Source,
    index: usize,
    offset: usize,
    deadline: Instant,
    terminator_sent: bool,
    written_at: u64,
    uncertain: bool,
    target: Atom,
}
struct Change {
    owner: Window,
    epoch: u64,
    timestamp: u32,
    source: Source,
    state: ClipboardState,
}

pub struct ClipboardOwner {
    reader: ClipboardReader,
    saved: Arc<ClipboardSnapshot>,
    window: Window,
    identity: uuid::Uuid,
    state: ClipboardState,
    source: Source,
    acquired: u32,
    clock: u32,
    clock_atom: Atom,
    timestamp_atom: Atom,
    multiple: Atom,
    atom_pair: Atom,
    owned_epoch: Option<u64>,
    change: Option<Change>,
    pending: VecDeque<Pending>,
    transfers: HashMap<(Window, Atom), Transfer>,
    chunk_bytes: usize,
    manager_watch: Option<manager::ManagerWatch>,
    handoff: Option<manager::ManagerTransfer>,
    handoff_state: HandoffStatus,
    keeper: Option<keeper::Transfer>,
}

impl ClipboardOwner {
    /// Consume a snapshot and the exact connection which tracked its identity.
    /// This allocates a private window but does NOT acquire CLIPBOARD ownership.
    pub fn new(
        reader: ClipboardReader,
        snapshot: ClipboardSnapshot,
    ) -> Result<(Self, ClipboardLease), String> {
        if snapshot.connection_id != reader.connection_id {
            return Err("clipboard snapshot belongs to a different reader".into());
        }
        let chunk_bytes = (usize::from(reader.conn.setup().maximum_request_length) * 4)
            .saturating_sub(64)
            .min(16384)
            / 4
            * 4;
        if chunk_bytes < 4 {
            return Err("X11 request limit cannot carry clipboard data".into());
        }
        let window = window(&reader.conn, reader.root)?;
        let clock_atom = atom(&reader.conn, b"_GROK_CU_SELECTION_CLOCK")?;
        let timestamp_atom = atom(&reader.conn, b"TIMESTAMP")?;
        let multiple = atom(&reader.conn, b"MULTIPLE")?;
        let atom_pair = atom(&reader.conn, b"ATOM_PAIR")?;
        let identity = uuid::Uuid::new_v4();
        let saved = Arc::new(snapshot);
        let result = Self {
            reader,
            source: Source::Saved(saved.clone()),
            saved,
            window,
            identity,
            state: ClipboardState::Prepared,
            acquired: 0,
            clock: 0,
            clock_atom,
            timestamp_atom,
            multiple,
            atom_pair,
            owned_epoch: None,
            change: None,
            pending: VecDeque::new(),
            transfers: HashMap::new(),
            chunk_bytes,
            manager_watch: None,
            handoff: None,
            handoff_state: HandoffStatus::NotRequested,
            keeper: None,
        };
        Ok((result, ClipboardLease { identity }))
    }

    pub fn state(&self) -> ClipboardState {
        self.state
    }

    pub(super) fn saved_was_empty(&self) -> bool {
        self.saved.was_empty()
    }
    pub fn active_transfers(&self) -> usize {
        self.transfers.len()
    }

    /// Publish once, after the caller's authority checks. Cancellation is a
    /// concrete atomic token: no callback can perform cross-client I/O while the
    /// short X server fence is held. An Err may be post-send; retain the service
    /// and lease, never retry or drop it on the assumption nothing happened.
    pub fn publish(
        &mut self,
        lease: &ClipboardLease,
        text: &str,
        cancellation: &ActionCancellation,
    ) -> Result<(), String> {
        self.check_lease(lease)?;
        if self.state != ClipboardState::Prepared {
            return Err("clipboard lease already attempted publication".into());
        }
        cancellation.check()?;
        let formats = task_formats(&self.reader.conn, text)?;
        let timestamp = self.server_time()?;
        self.fenced(|this| {
            let owner = this.barrier()?;
            if owner != this.saved.owner || this.reader.epoch != this.saved.epoch {
                return Err("clipboard changed since snapshot; refusing task publication".into());
            }
            cancellation.check()?;
            this.install(
                this.window,
                timestamp,
                Source::Task(Arc::new(formats)),
                ClipboardState::Published,
            )
        })
    }

    /// Clipboard-only restoration. The caller MUST separately prove it is safe
    /// to end its paste operation; this method does not establish that fact.
    /// A concurrent copy wins, even if it uses the same window or returns via ABA.
    pub fn restore(&mut self, lease: &ClipboardLease) -> Result<RestoreOutcome, String> {
        self.check_lease(lease)?;
        if matches!(
            self.state,
            ClipboardState::Prepared | ClipboardState::Closed
        ) {
            return Err("clipboard lease has no restorable publication".into());
        }
        let timestamp = self.server_time()?;
        self.fenced(|this| {
            let owner = this.barrier()?;
            this.reconcile(owner)?;
            if this.state == ClipboardState::Lost {
                return Ok(RestoreOutcome::PreservedNewOwner);
            }
            if this.state == ClipboardState::Restored {
                return Ok(RestoreOutcome::AlreadyRestored);
            }
            if owner != this.window || this.owned_epoch != Some(this.reader.epoch) {
                this.state = ClipboardState::Lost;
                return Ok(RestoreOutcome::PreservedNewOwner);
            }
            let restored_owner = if this.saved.was_empty() {
                x11rb::NONE
            } else {
                this.window
            };
            this.install(
                restored_owner,
                timestamp,
                Source::Saved(this.saved.clone()),
                ClipboardState::Restored,
            )?;
            Ok(RestoreOutcome::Restored)
        })
    }

    /// Serve a bounded batch, including old INCR transfers after ownership loss.
    /// Payloads are pinned per accepted request: restoring must not change the
    /// bytes of a transfer already in progress. No keyboard/AT-SPI input occurs.
    pub fn pump(&mut self) -> Result<(), String> {
        if self.state == ClipboardState::Closed {
            return Err("clipboard service is closed".into());
        }
        self.server_time()?;
        let mut result = Ok(());
        for _ in 0..MAX_PENDING {
            let Some(event) = self.pending.pop_front() else {
                break;
            };
            let handled = match event {
                Pending::Request(request) => self.serve_request(request),
                Pending::Property(event, sequence) => self.advance(event, sequence),
                Pending::Destroy(window) => {
                    self.remove_window(window);
                    Ok(())
                }
            };
            if let Err(error) = handled {
                result = Err(error);
                break;
            }
        }
        self.expire_transfers();
        if result.is_ok() {
            self.check_handoff()?;
            self.check_keeper()?;
        }
        result
    }

    /// Refuse to close while restored data are still provided by this service.
    /// Clipboard-manager/exit handoff must happen BEFORE this returns success.
    pub fn close(&mut self) -> Result<(), String> {
        if self.try_close()? {
            Ok(())
        } else {
            Err("clipboard service still owns data or unfinished transfers; retain it".into())
        }
    }

    /// Distinguish expected retention from a native transport/cleanup error.
    pub fn try_close(&mut self) -> Result<bool, String> {
        if self.state == ClipboardState::Closed {
            return Ok(true);
        }
        self.pump()?;
        self.fenced(|this| {
            let owner = this.barrier()?;
            if owner == this.window
                || this.change.is_some()
                || !this.pending.is_empty()
                || !this.transfers.is_empty()
                || this.handoff_pending()
                || this.keeper_pending()
            {
                return Ok(false);
            }
            if let Some(handoff) = this.handoff.as_mut() {
                if handoff.requestor != x11rb::NONE {
                    // The peer may already have retired this private requestor.
                    // Close it first so a transport error cannot leave the live
                    // service window destroyed with a still-unfinished close.
                    destroy_private_window(&this.reader.conn, handoff.requestor)?;
                    handoff.requestor = x11rb::NONE;
                }
            }
            destroy_private_window(&this.reader.conn, this.window)?;
            this.state = ClipboardState::Closed;
            Ok(true)
        })
    }

    pub(super) fn check_lease(&self, lease: &ClipboardLease) -> Result<(), String> {
        if lease.identity != self.identity {
            return Err("clipboard lease belongs to another owner service".into());
        }
        Ok(())
    }
    fn fenced<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let mut fence = ServerFence::new(self.reader.conn.clone())?;
        let result = f(self);
        if let Err(error) = fence.close() {
            self.state = ClipboardState::Uncertain;
            return Err(format!(
                "clipboard server fence cleanup is uncertain: {error}"
            ));
        }
        result
    }
    fn install(
        &mut self,
        owner: Window,
        timestamp: u32,
        source: Source,
        state: ClipboardState,
    ) -> Result<(), String> {
        let epoch = self
            .reader
            .epoch
            .checked_add(1)
            .ok_or("clipboard epoch exhausted")?;
        self.change = Some(Change {
            owner,
            epoch,
            timestamp,
            source,
            state,
        });
        self.state = ClipboardState::Uncertain;
        self.reader
            .conn
            .set_selection_owner(owner, self.reader.clipboard, timestamp)
            .map_err(err)?
            .check()
            .map_err(err)?;
        let observed = self.barrier()?;
        self.reconcile(observed)?;
        if self.state != state {
            return Err("clipboard publication did not retain exact native ownership".into());
        }
        Ok(())
    }
    fn reconcile(&mut self, owner: Window) -> Result<(), String> {
        if let Some(change) = self.change.take() {
            if owner == change.owner && self.reader.epoch == change.epoch {
                self.source = change.source;
                self.acquired = change.timestamp;
                self.owned_epoch = (owner == self.window).then_some(change.epoch);
                self.state = change.state;
            } else {
                // The native barrier proves all prior SetSelectionOwner requests
                // on this connection were processed. Never re-send a failed swap.
                self.state = ClipboardState::Lost;
                self.owned_epoch = None;
            }
        } else if self
            .owned_epoch
            .is_some_and(|epoch| epoch != self.reader.epoch || owner != self.window)
        {
            self.state = ClipboardState::Lost;
            self.owned_epoch = None;
        }
        Ok(())
    }
    fn barrier(&mut self) -> Result<Window, String> {
        let owner = self
            .reader
            .conn
            .get_selection_owner(self.reader.clipboard)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .owner;
        self.collect()?;
        Ok(owner)
    }
    fn collect(&mut self) -> Result<(), String> {
        for _ in 0..EVENT_BUDGET {
            let Some((event, sequence)) = self
                .reader
                .conn
                .poll_for_event_with_sequence()
                .map_err(err)?
            else {
                return Ok(());
            };
            self.capture(event, sequence)?;
        }
        Err("clipboard owner event budget exceeded".into())
    }
    fn capture(&mut self, event: Event, sequence: u64) -> Result<(), String> {
        self.reader.track(&event)?;
        self.track_handoff(&event)?;
        if self.change.is_none()
            && self
                .owned_epoch
                .is_some_and(|epoch| epoch != self.reader.epoch)
        {
            self.state = ClipboardState::Lost;
            self.owned_epoch = None;
        }
        let pending = match event {
            Event::SelectionRequest(request)
                if request.owner == self.window && request.selection == self.reader.clipboard =>
            {
                // Even already-queued requests retain the source from their
                // acceptance epoch, never whichever payload happens to be current.
                let allowed = request.response_type & 0x80 == 0
                    && matches!(
                        self.state,
                        ClipboardState::Published | ClipboardState::Restored
                    )
                    && self.owned_epoch == Some(self.reader.epoch);
                Some(Pending::Request(Request {
                    event: request,
                    source: self.source.clone(),
                    acquired: self.acquired,
                    allowed,
                }))
            }
            Event::PropertyNotify(event)
                if event.response_type & 0x80 == 0
                    && event.window != self.window
                    && event.window != self.reader.watch =>
            {
                Some(Pending::Property(event, sequence))
            }
            Event::DestroyNotify(event) if event.response_type & 0x80 == 0 => {
                Some(Pending::Destroy(event.window))
            }
            _ => None,
        };
        if let Some(pending) = pending {
            if self.pending.len() == MAX_PENDING {
                return Err("clipboard request queue is full".into());
            }
            self.pending.push_back(pending);
        }
        Ok(())
    }
    fn server_time(&mut self) -> Result<u32, String> {
        self.collect()?;
        self.reader
            .conn
            .change_property8(
                PropMode::REPLACE,
                self.window,
                self.clock_atom,
                AtomEnum::INTEGER,
                &[1],
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        let mut found = None;
        for _ in 0..EVENT_BUDGET {
            let Some((event, sequence)) = self
                .reader
                .conn
                .poll_for_event_with_sequence()
                .map_err(err)?
            else {
                break;
            };
            if let Event::PropertyNotify(change) = &event {
                if change.window == self.window
                    && change.atom == self.clock_atom
                    && change.state == Property::NEW_VALUE
                {
                    found = Some(change.time);
                }
            }
            self.capture(event, sequence)?;
        }
        let time = found
            .filter(|t| *t != x11rb::CURRENT_TIME)
            .ok_or("clipboard server timestamp unavailable")?;
        self.clock = time;
        Ok(time)
    }
}

fn destroy_private_window(conn: &RustConnection, window: Window) -> Result<(), String> {
    match conn.destroy_window(window).map_err(err)?.check() {
        Ok(()) => Ok(()),
        Err(x11rb::errors::ReplyError::X11Error(error))
            if error.error_kind == x11rb::protocol::ErrorKind::Window =>
        {
            Ok(())
        }
        Err(error) => Err(err(error)),
    }
}

fn task_formats(conn: &RustConnection, text: &str) -> Result<Vec<Materialized>, String> {
    if text.is_empty() || text.len() > MAX_TASK_TEXT || text.contains('\0') {
        return Err("clipboard task text is empty, contains NUL, or exceeds bounds".into());
    }
    let utf8 = atom(conn, b"UTF8_STRING")?;
    let mut result = vec![];
    for name in [
        b"UTF8_STRING".as_slice(),
        b"TEXT",
        b"text/plain;charset=utf-8",
        b"text/plain",
    ] {
        result.push(Materialized {
            target: atom(conn, name)?,
            payload: Payload {
                kind: utf8,
                format: 8,
                bytes: text.as_bytes().to_vec(),
            },
        });
    }
    if text.chars().all(|c| u32::from(c) <= 255) {
        result.push(Materialized {
            target: u32::from(AtomEnum::STRING),
            payload: Payload {
                kind: u32::from(AtomEnum::STRING),
                format: 8,
                bytes: text.chars().map(|c| c as u8).collect(),
            },
        });
    }
    Ok(result)
}
fn valid_time(time: u32, acquired: u32, now: u32) -> bool {
    time == x11rb::CURRENT_TIME
        || ((time.wrapping_sub(acquired) as i32) >= 0 && (now.wrapping_sub(time) as i32) >= 0)
}

#[path = "clipboard_send.rs"]
mod send;

#[path = "clipboard_manager.rs"]
mod manager;
pub use manager::HandoffStatus;

#[path = "clipboard_keeper.rs"]
mod keeper;
pub use keeper::{keeper_entrypoint, KeeperProcess, KeeperStatus};
