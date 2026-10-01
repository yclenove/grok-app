//! Independent event-driven SAVE_TARGETS peer, restricted to the owned Xvfb.
//! It uses MULTIPLE, takes ownership before finishing INCR like GTK/GNOME,
//! and serves the bytes it actually received after the original service exits.
use super::*;
use crate::clipboard::HandoffStatus;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Behavior {
    Good,
    Decline,
    Silent,
    EarlyAck,
    Partial,
    WrongReply,
    DelayedAck,
}
enum ManagerCommand {
    Republish,
    Aba,
    Acknowledge,
    TakeAndAcknowledge,
    DestroyRequestor,
    Stop,
}
struct Manager {
    commands: mpsc::Sender<(ManagerCommand, mpsc::Sender<()>)>,
    worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    requests: Arc<AtomicUsize>,
    incremental: Arc<AtomicUsize>,
    received: Arc<AtomicUsize>,
}
impl Manager {
    fn start(behavior: Behavior) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<(ManagerCommand, mpsc::Sender<()>)>();
        let (ready, started) = mpsc::channel();
        let requests = Arc::new(AtomicUsize::new(0));
        let incremental = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(AtomicUsize::new(0));
        let counters = (requests.clone(), incremental.clone(), received.clone());
        let worker = std::thread::spawn(move || {
            let setup = Peer::new(behavior, counters);
            let _ = ready.send(setup.as_ref().map(|_| ()).map_err(Clone::clone));
            let mut peer = setup?;
            loop {
                if let Ok((command, reply)) = rx.try_recv() {
                    if matches!(command, ManagerCommand::Stop) {
                        let _ = reply.send(());
                        return Ok(());
                    }
                    peer.command(command)?;
                    let _ = reply.send(());
                }
                if let Some(event) = peer.conn.poll_for_event().map_err(err)? {
                    peer.event(event)?;
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        });
        started
            .recv_timeout(Duration::from_secs(3))
            .map_err(err)??;
        Ok(Self {
            commands: tx,
            worker: Some(worker),
            requests,
            incremental,
            received,
        })
    }
    fn command(&self, command: ManagerCommand) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.commands.send((command, tx)).map_err(err)?;
        rx.recv_timeout(Duration::from_secs(3)).map_err(err)
    }
    fn stop(mut self) -> Result<(), String> {
        let signal = self.command(ManagerCommand::Stop);
        let joined = self
            .worker
            .take()
            .ok_or("manager worker missing")?
            .join()
            .map_err(|_| "manager worker panicked")?;
        signal?;
        joined
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.command(ManagerCommand::Stop);
            let _ = worker.join();
        }
    }
}
struct Incoming {
    shape: Option<(Atom, u8)>,
    bytes: Vec<u8>,
}
struct Peer {
    conn: RustConnection,
    manager_window: Window,
    clipboard_window: Window,
    manager: Atom,
    clipboard: Atom,
    save: Atom,
    targets: Atom,
    multiple: Atom,
    pairs: Atom,
    property: Atom,
    incr: Atom,
    behavior: Behavior,
    request: Option<SelectionRequestEvent>,
    requested: Vec<Atom>,
    pending: HashMap<Atom, Incoming>,
    formats: Vec<Materialized>,
    counters: (Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>),
}
impl Peer {
    fn new(
        behavior: Behavior,
        counters: (Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>),
    ) -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        let root = conn.setup().roots[screen].root;
        let manager_window = window(&conn, root)?;
        // A different window on the same X client is intentional.
        let clipboard_window = window(&conn, root)?;
        let manager = atom(&conn, b"CLIPBOARD_MANAGER")?;
        let peer = Self {
            manager_window,
            clipboard_window,
            manager,
            clipboard: atom(&conn, b"CLIPBOARD")?,
            save: atom(&conn, b"SAVE_TARGETS")?,
            targets: atom(&conn, b"TARGETS")?,
            multiple: atom(&conn, b"MULTIPLE")?,
            pairs: atom(&conn, b"ATOM_PAIR")?,
            property: atom(&conn, b"_CU_MANAGER_BATCH")?,
            incr: atom(&conn, b"INCR")?,
            conn,
            behavior,
            request: None,
            requested: Vec::new(),
            pending: HashMap::new(),
            formats: Vec::new(),
            counters,
        };
        peer.claim(manager_window, manager, 0)?;
        Ok(peer)
    }
    fn claim(&self, window: Window, selection: Atom, time: u32) -> Result<(), String> {
        self.conn
            .set_selection_owner(window, selection, time)
            .map_err(err)?
            .check()
            .map_err(err)
    }
    fn command(&mut self, command: ManagerCommand) -> Result<(), String> {
        match command {
            ManagerCommand::Republish => self.claim(self.manager_window, self.manager, 0),
            ManagerCommand::Aba => {
                self.claim(self.clipboard_window, self.manager, 0)?;
                self.claim(self.manager_window, self.manager, 0)
            }
            ManagerCommand::Acknowledge => self.ack(),
            ManagerCommand::TakeAndAcknowledge => {
                self.claim(self.clipboard_window, self.clipboard, 0)?;
                self.ack()
            }
            ManagerCommand::DestroyRequestor => self
                .conn
                .destroy_window(self.request.ok_or("missing requestor")?.requestor)
                .map_err(err)?
                .check()
                .map_err(err),
            ManagerCommand::Stop => unreachable!(),
        }
    }
    fn read(
        &self,
        window: Window,
        property: Atom,
        delete: bool,
    ) -> Result<GetPropertyReply, String> {
        let p = self
            .conn
            .get_property(
                delete,
                window,
                property,
                AtomEnum::ANY,
                0,
                (MAX_FORMAT_BYTES / 4 + 1) as u32,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?;
        require(
            p.bytes_after == 0 && p.value.len() <= MAX_FORMAT_BYTES,
            "manager fixture property exceeded bound",
        )?;
        Ok(p)
    }
    fn event(&mut self, event: Event) -> Result<(), String> {
        match event {
            Event::SelectionRequest(r) if r.selection == self.manager && r.target == self.save => {
                self.save(r)
            }
            Event::SelectionRequest(r) if r.selection == self.clipboard => self.serve(r),
            Event::SelectionNotify(n)
                if n.requestor == self.manager_window
                    && n.selection == self.clipboard
                    && n.target == self.multiple =>
            {
                self.converted(n)
            }
            Event::PropertyNotify(n)
                if n.window == self.manager_window
                    && n.state == Property::NEW_VALUE
                    && self.pending.contains_key(&n.atom) =>
            {
                self.chunk(n.atom)
            }
            _ => Ok(()),
        }
    }
    fn save(&mut self, r: SelectionRequestEvent) -> Result<(), String> {
        self.counters.0.fetch_add(1, Ordering::Release);
        require(self.request.is_none(), "SAVE_TARGETS was replayed")?;
        self.request = Some(r);
        if self.behavior == Behavior::Silent {
            return Ok(());
        }
        if self.behavior == Behavior::Decline {
            return notify(&self.conn, &r, 0);
        }
        if self.behavior == Behavior::EarlyAck {
            self.claim(self.clipboard_window, self.clipboard, r.time)?;
            return self.ack();
        }
        let list = self.read(r.requestor, r.property, false)?;
        require(
            list.type_ == u32::from(AtomEnum::ATOM) && list.format == 32,
            "SAVE_TARGETS did not name userdata atoms",
        )?;
        self.requested = list
            .value32()
            .ok_or("manager target words absent")?
            .collect();
        require(
            !self.requested.is_empty()
                && self.requested.len() <= MAX_TARGETS
                && self.requested.iter().copied().collect::<HashSet<_>>().len()
                    == self.requested.len(),
            "manager targets invalid",
        )?;
        if self.behavior == Behavior::Partial {
            self.requested.truncate(1);
        }
        let pairs: Vec<_> = self.requested.iter().flat_map(|t| [*t, *t]).collect();
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.manager_window,
                self.property,
                self.pairs,
                &pairs,
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        self.conn
            .convert_selection(
                self.manager_window,
                self.clipboard,
                self.multiple,
                self.property,
                r.time,
            )
            .map_err(err)?
            .check()
            .map_err(err)
    }
    fn converted(&mut self, n: SelectionNotifyEvent) -> Result<(), String> {
        require(
            n.property == self.property,
            "manager MULTIPLE conversion refused",
        )?;
        let p = self.read(self.manager_window, self.property, true)?;
        require(
            p.type_ == self.pairs && p.format == 32,
            "manager MULTIPLE wrong shape",
        )?;
        let words: Vec<_> = p.value32().ok_or("missing MULTIPLE words")?.collect();
        require(
            words
                == self
                    .requested
                    .iter()
                    .flat_map(|t| [*t, *t])
                    .collect::<Vec<_>>(),
            "manager MULTIPLE dropped format",
        )?;
        for target in &self.requested {
            let value = self.read(self.manager_window, *target, true)?;
            if value.type_ == self.incr {
                require(
                    value.format == 32 && value.value_len == 1,
                    "manager INCR header malformed",
                )?;
                self.pending.insert(
                    *target,
                    Incoming {
                        shape: None,
                        bytes: Vec::new(),
                    },
                );
                self.counters.1.fetch_add(1, Ordering::Release);
            } else {
                self.formats.push(Materialized {
                    target: *target,
                    payload: property_payload(value, MAX_FORMAT_BYTES)?,
                });
            }
        }
        // Real managers may acquire CLIPBOARD before INCR finishes. Losing
        // selection ownership must not truncate the accepted transfers.
        self.claim(
            self.clipboard_window,
            self.clipboard,
            self.request.ok_or("missing SAVE_TARGETS")?.time,
        )?;
        self.maybe_ack()
    }
    fn chunk(&mut self, target: Atom) -> Result<(), String> {
        let p = self.read(self.manager_window, target, true)?;
        // A queued header notification may refer to an already deleted property.
        if p.type_ == 0 {
            return Ok(());
        }
        let incoming = self
            .pending
            .get_mut(&target)
            .ok_or("missing manager INCR")?;
        let shape = (p.type_, p.format);
        require(
            [8, 16, 32].contains(&p.format) && incoming.shape.is_none_or(|s| s == shape),
            "manager INCR changed shape",
        )?;
        incoming.shape = Some(shape);
        require(
            incoming.bytes.len() + p.value.len() <= MAX_FORMAT_BYTES,
            "manager INCR exceeded bound",
        )?;
        if p.value.is_empty() {
            let complete = self
                .pending
                .remove(&target)
                .ok_or("missing final manager INCR")?;
            self.formats.push(Materialized {
                target,
                payload: Payload {
                    kind: shape.0,
                    format: shape.1,
                    bytes: complete.bytes,
                },
            });
        } else {
            incoming.bytes.extend(p.value);
        }
        self.maybe_ack()
    }
    fn maybe_ack(&self) -> Result<(), String> {
        if !self.pending.is_empty() {
            return Ok(());
        }
        self.counters.2.store(self.formats.len(), Ordering::Release);
        if self.behavior != Behavior::DelayedAck {
            self.ack()?;
        }
        Ok(())
    }
    fn ack(&self) -> Result<(), String> {
        let r = self
            .request
            .ok_or("manager has no request to acknowledge")?;
        if self.behavior == Behavior::WrongReply {
            for i in 0..5 {
                let mut n = SelectionNotifyEvent {
                    response_type: xproto::SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: r.time,
                    requestor: r.requestor,
                    selection: r.selection,
                    target: r.target,
                    property: r.property,
                };
                match i {
                    0 => n.time = n.time.wrapping_add(1),
                    1 => n.requestor = self.manager_window,
                    2 => n.selection = self.clipboard,
                    3 => n.target = self.targets,
                    _ => n.property = self.property,
                }
                self.conn
                    .send_event(false, r.requestor, EventMask::NO_EVENT, n)
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
            }
            Ok(())
        } else {
            notify(&self.conn, &r, r.property)
        }
    }
    fn serve(&self, r: SelectionRequestEvent) -> Result<(), String> {
        if r.target == self.targets {
            let offered: Vec<_> = std::iter::once(self.targets)
                .chain(self.formats.iter().map(|f| f.target))
                .collect();
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    r.requestor,
                    r.property,
                    AtomEnum::ATOM,
                    &offered,
                )
                .map_err(err)?
                .check()
                .map_err(err)?;
        } else if let Some(f) = self.formats.iter().find(|f| f.target == r.target) {
            let count = f.payload.bytes.len() / (usize::from(f.payload.format) / 8);
            self.conn
                .change_property(
                    PropMode::REPLACE,
                    r.requestor,
                    r.property,
                    f.payload.kind,
                    f.payload.format,
                    count as u32,
                    &f.payload.bytes,
                )
                .map_err(err)?
                .check()
                .map_err(err)?;
        } else {
            return notify(&self.conn, &r, 0);
        }
        notify(&self.conn, &r, r.property)
    }
}

fn restored(mode: Mode) -> Result<(ClipboardOwner, ClipboardLease, FormatBytes), String> {
    let original = Owner::start(mode)?;
    let mut reader = ClipboardReader::connect()?;
    let snapshot = reader.snapshot(|| Ok(()))?;
    let expected = bytes(&snapshot);
    let (mut owner, lease) = ClipboardOwner::new(reader, snapshot)?;
    owner.publish(
        &lease,
        "temporary task must not enter history",
        &ActionCancellation::default(),
    )?;
    owner.restore(&lease)?;
    drop(original);
    Ok((owner, lease, expected))
}
fn until(owner: &mut ClipboardOwner, wanted: HandoffStatus) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        owner.pump()?;
        if owner.handoff_status() == wanted {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Err(format!(
        "manager handoff expected {wanted:?}, got {:?}",
        owner.handoff_status()
    ))
}
fn spin(owner: &mut ClipboardOwner) -> Result<(), String> {
    for _ in 0..30 {
        owner.pump()?;
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[path = "clipboard_keeper_fixture.rs"]
mod keeper_fixture;
#[path = "clipboard_retention_fixture.rs"]
mod retention_fixture;
pub use keeper_fixture::run_keeper_parent_fixture;
fn sort(mut formats: FormatBytes) -> FormatBytes {
    formats.sort_by_key(|f| f.0);
    formats
}

pub(super) fn run() -> Result<(), String> {
    let user = Requestor::new()?;
    for mode in [Mode::Formats, Mode::LargeFormats] {
        let manager = Manager::start(Behavior::Good)?;
        let (mut owner, lease, expected) = restored(mode)?;
        require(
            owner.begin_handoff(&lease, &ActionCancellation::default())? == HandoffStatus::Pending,
            "handoff did not start",
        )?;
        until(&mut owner, HandoffStatus::Acknowledged)?;
        require(
            owner
                .begin_handoff(&lease, &ActionCancellation::default())
                .is_err()
                && manager.requests.load(Ordering::Acquire) == 1,
            "SAVE_TARGETS replay allowed",
        )?;
        require(
            manager.incremental.load(Ordering::Acquire)
                == if mode == Mode::LargeFormats { 3 } else { 0 },
            "manager did not exercise expected INCR widths",
        )?;
        if mode == Mode::Formats {
            manager.command(ManagerCommand::DestroyRequestor)?;
        }
        owner.close()?;
        owner.close()?;
        drop(owner);
        let saved = ClipboardReader::connect()?.snapshot(|| Ok(()))?;
        require(
            sort(bytes(&saved)) == sort(expected),
            "manager lost original bytes after both providers exited",
        )?;
        manager.stop()?;
        println!("PASS clipboard manager {} handoff survives original and service exit with exact 8/16/32-bit formats", if mode == Mode::Formats { "direct" } else { "INCR" });
    }
    {
        let (mut owner, lease, expected) = restored(Mode::Formats)?;
        require(
            owner.begin_handoff(&lease, &ActionCancellation::default())?
                == HandoffStatus::Unavailable
                && owner.close().is_err(),
            "absent manager released original data",
        )?;
        require(
            sort(bytes(&read_all(&mut owner)?)) == sort(expected),
            "absent manager data not retained",
        )?;
        user.claim(user.window)?;
        owner.close()?;
        user.claim(0)?;
        println!("PASS clipboard absent manager retains restored data and rejects unsafe close");
    }
    {
        let manager = Manager::start(Behavior::Good)?;
        let original = Owner::start(Mode::Formats)?;
        let (mut owner, lease) = prepared()?;
        let (mut foreign, other) = prepared()?;
        require(
            owner
                .begin_handoff(&lease, &ActionCancellation::default())
                .is_err(),
            "prepared original exported",
        )?;
        owner.publish(&lease, "private task", &ActionCancellation::default())?;
        require(
            owner
                .begin_handoff(&lease, &ActionCancellation::default())
                .is_err(),
            "temporary task exported",
        )?;
        owner.restore(&lease)?;
        let cancelled = ActionCancellation::default();
        cancelled.cancel();
        require(
            owner.begin_handoff(&lease, &cancelled).is_err()
                && owner
                    .begin_handoff(&other, &ActionCancellation::default())
                    .is_err()
                && manager.requests.load(Ordering::Acquire) == 0,
            "invalid handoff dispatched SAVE_TARGETS",
        )?;
        foreign.close()?;
        user.claim(user.window)?;
        owner.close()?;
        drop(original);
        manager.stop()?;
        user.claim(0)?;
        println!("PASS clipboard handoff rejects prepared task foreign lease and pre-cancel without manager dispatch");
    }
    for behavior in [Behavior::Decline, Behavior::EarlyAck, Behavior::Partial] {
        let manager = Manager::start(behavior)?;
        let (mut owner, lease, _) = restored(Mode::Formats)?;
        owner.begin_handoff(&lease, &ActionCancellation::default())?;
        let wanted = if behavior == Behavior::Decline {
            HandoffStatus::Declined
        } else {
            HandoffStatus::Unknown
        };
        until(&mut owner, wanted)?;
        require(
            owner.close().is_err(),
            "incomplete manager acknowledgement permitted close",
        )?;
        require(
            manager.requests.load(Ordering::Acquire) == 1,
            "incomplete manager request replayed",
        )?;
        user.claim(user.window)?;
        if wanted == HandoffStatus::Unknown {
            until(&mut owner, HandoffStatus::Interrupted)?;
        }
        owner.close()?;
        require(
            user.selected()? == user.window,
            "manager failure clobbered newer user copy",
        )?;
        manager.stop()?;
        user.claim(0)?;
        println!("PASS clipboard manager {behavior:?} does not prove complete handoff or overwrite newer copy");
    }
    {
        let manager = Manager::start(Behavior::Silent)?;
        let (mut owner, lease, expected) = restored(Mode::Formats)?;
        owner.begin_handoff(&lease, &ActionCancellation::default())?;
        require(
            sort(bytes(&read_all(&mut owner)?)) == sort(expected),
            "unrelated reader lost original bytes",
        )?;
        manager.command(ManagerCommand::TakeAndAcknowledge)?;
        until(&mut owner, HandoffStatus::Unknown)?;
        require(
            owner.close().is_err(),
            "unrelated client read counted as manager delivery",
        )?;
        user.claim(user.window)?;
        until(&mut owner, HandoffStatus::Interrupted)?;
        owner.close()?;
        manager.stop()?;
        user.claim(0)?;
        println!("PASS clipboard unrelated client reads cannot satisfy captured manager delivery evidence");
    }
    for behavior in [Behavior::WrongReply, Behavior::DelayedAck] {
        let manager = Manager::start(behavior)?;
        let (mut owner, lease, expected) = restored(Mode::LargeFormats)?;
        let cancelled = ActionCancellation::default();
        owner.begin_handoff(&lease, &cancelled)?;
        cancelled.cancel();
        let deadline = Instant::now() + Duration::from_secs(3);
        while manager.received.load(Ordering::Acquire) != expected.len()
            && Instant::now() < deadline
        {
            spin(&mut owner)?;
        }
        require(
            manager.received.load(Ordering::Acquire) == expected.len(),
            "manager did not receive all formats",
        )?;
        spin(&mut owner)?;
        require(
            owner.handoff_status() == HandoffStatus::Pending && owner.close().is_err(),
            "mismatched or absent reply released pending service",
        )?;
        require(
            owner.begin_handoff(&lease, &cancelled).is_err()
                && manager.requests.load(Ordering::Acquire) == 1,
            "Stop retried SAVE_TARGETS",
        )?;
        if behavior == Behavior::DelayedAck {
            manager.command(ManagerCommand::Acknowledge)?;
            until(&mut owner, HandoffStatus::Acknowledged)?;
            owner.close()?;
            drop(owner);
            require(
                sort(bytes(&ClipboardReader::connect()?.snapshot(|| Ok(()))?)) == sort(expected),
                "late ack lost bytes",
            )?;
        } else {
            user.claim(user.window)?;
            until(&mut owner, HandoffStatus::Interrupted)?;
            owner.close()?;
        }
        manager.stop()?;
        user.claim(0)?;
        println!("PASS clipboard manager {behavior:?} retains pending service through Stop until exact terminal evidence");
    }
    for command in [
        ManagerCommand::Republish,
        ManagerCommand::Aba,
        ManagerCommand::Stop,
    ] {
        let manager = Manager::start(Behavior::Silent)?;
        let (mut owner, lease, expected) = restored(Mode::Formats)?;
        owner.begin_handoff(&lease, &ActionCancellation::default())?;
        spin(&mut owner)?;
        require(
            owner.handoff_status() == HandoffStatus::Pending
                && owner.close().is_err()
                && manager.requests.load(Ordering::Acquire) == 1,
            "silent manager did not retain pending service",
        )?;
        let label = match command {
            ManagerCommand::Republish => "same-owner replacement",
            ManagerCommand::Aba => "ABA",
            _ => "exit",
        };
        let manager = if matches!(command, ManagerCommand::Stop) {
            manager.stop()?;
            None
        } else {
            manager.command(command)?;
            Some(manager)
        };
        until(&mut owner, HandoffStatus::Interrupted)?;
        require(
            sort(bytes(&read_all(&mut owner)?)) == sort(expected) && owner.close().is_err(),
            "manager lifetime change dropped retained original",
        )?;
        user.claim(user.window)?;
        owner.close()?;
        if let Some(manager) = manager {
            manager.stop()?;
        }
        user.claim(0)?;
        println!(
            "PASS clipboard manager {label} invalidates handoff without destroying restored data"
        );
    }
    retention_fixture::run()?;
    keeper_fixture::run()?;
    Ok(())
}
