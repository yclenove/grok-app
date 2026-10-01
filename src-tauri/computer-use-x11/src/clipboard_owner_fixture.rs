//! Tests only: all selection operations stay on the explicitly owned Xvfb.
use super::super::{ClipboardLease, ClipboardOwner, ClipboardState, RestoreOutcome};
use super::*;
use grok_computer_use_core::execution::ActionCancellation;

type FormatBytes = Vec<(Atom, Atom, u8, Vec<u8>)>;
fn bytes(snapshot: &ClipboardSnapshot) -> FormatBytes {
    snapshot
        .formats
        .iter()
        .map(|f| {
            (
                f.target,
                f.payload.kind,
                f.payload.format,
                f.payload.bytes.clone(),
            )
        })
        .collect()
}
fn prepared() -> Result<(ClipboardOwner, ClipboardLease), String> {
    let mut reader = ClipboardReader::connect()?;
    let snapshot = reader.snapshot(|| Ok(()))?;
    ClipboardOwner::new(reader, snapshot)
}
fn read_all(owner: &mut ClipboardOwner) -> Result<ClipboardSnapshot, String> {
    let worker = std::thread::spawn(|| ClipboardReader::connect()?.snapshot(|| Ok(())));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.is_finished() {
        owner.pump()?;
        if Instant::now() >= deadline {
            return Err("owned reader thread exceeded deadline".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = worker.join().map_err(|_| "owned reader thread panic")?;
    owner.pump()?;
    result
}
fn require(value: bool, message: &str) -> Result<(), String> {
    if value {
        Ok(())
    } else {
        Err(message.into())
    }
}

struct Requestor {
    conn: RustConnection,
    window: Window,
    clipboard: Atom,
    property: Atom,
    utf8: Atom,
    incr: Atom,
}
impl Requestor {
    fn new() -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        let window = window(&conn, conn.setup().roots[screen].root)?;
        Ok(Self {
            window,
            clipboard: atom(&conn, b"CLIPBOARD")?,
            property: atom(&conn, b"_CU_OWNER_TEST_DATA")?,
            utf8: atom(&conn, b"UTF8_STRING")?,
            incr: atom(&conn, b"INCR")?,
            conn,
        })
    }
    fn begin(&self, target: Atom, property: Atom, time: u32) -> Result<(), String> {
        self.conn
            .convert_selection(self.window, self.clipboard, target, property, time)
            .map_err(err)?
            .check()
            .map_err(err)
    }
    fn notify(&self, owner: &mut ClipboardOwner) -> Result<SelectionNotifyEvent, String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            owner.pump()?;
            while let Some(event) = self.conn.poll_for_event().map_err(err)? {
                if let Event::SelectionNotify(n) = event {
                    return Ok(n);
                }
            }
            if Instant::now() >= deadline {
                return Err("owned selection notify deadline".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn property(&self, property: Atom, delete: bool) -> Result<GetPropertyReply, String> {
        self.conn
            .get_property(
                delete,
                self.window,
                property,
                AtomEnum::ANY,
                0,
                (MAX_FORMAT_BYTES / 4 + 1) as u32,
            )
            .map_err(err)?
            .reply()
            .map_err(err)
    }
    fn receive(
        &self,
        owner: &mut ClipboardOwner,
        property: Atom,
    ) -> Result<(Atom, u8, Vec<u8>), String> {
        let first = self.property(property, false)?;
        if first.type_ != self.incr {
            return Ok((first.type_, first.format, first.value));
        }
        require(
            first.format == 32 && first.value_len == 1,
            "outbound INCR header is malformed",
        )?;
        self.conn
            .delete_property(self.window, property)
            .map_err(err)?
            .check()
            .map_err(err)?;
        let mut result = Vec::new();
        let mut shape = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            owner.pump()?;
            let chunk = self.property(property, true)?;
            if chunk.type_ != x11rb::NONE {
                let current = (chunk.type_, chunk.format);
                require(
                    shape.is_none_or(|s| s == current),
                    "INCR output changed type or width",
                )?;
                shape = Some(current);
                require(
                    chunk.bytes_after == 0 && result.len() + chunk.value.len() <= MAX_FORMAT_BYTES,
                    "INCR fixture output exceeded bounds",
                )?;
                if chunk.value.is_empty() {
                    owner.pump()?;
                    return Ok((current.0, current.1, result));
                }
                result.extend(chunk.value);
            }
            if Instant::now() >= deadline {
                return Err("outbound INCR fixture deadline".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn claim(&self, owner: Window) -> Result<(), String> {
        self.conn
            .set_selection_owner(owner, self.clipboard, x11rb::CURRENT_TIME)
            .map_err(err)?
            .check()
            .map_err(err)
    }
    fn selected(&self) -> Result<Window, String> {
        Ok(self
            .conn
            .get_selection_owner(self.clipboard)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .owner)
    }
}

pub(super) fn run() -> Result<(), String> {
    let client = Requestor::new()?;
    require(
        client.selected()? == x11rb::NONE,
        "publisher fixture requires empty owned display",
    )?;
    {
        let (mut owner, lease) = prepared()?;
        let (mut foreign, other) = prepared()?;
        require(
            owner
                .publish(&other, "wrong", &ActionCancellation::default())
                .is_err(),
            "foreign lease published",
        )?;
        let cancelled = ActionCancellation::default();
        cancelled.cancel();
        require(
            owner.publish(&lease, "cancelled", &cancelled).is_err(),
            "cancelled publication succeeded",
        )?;
        for text in [String::new(), "bad\0text".into(), "x".repeat(32769)] {
            require(
                owner
                    .publish(&lease, &text, &ActionCancellation::default())
                    .is_err(),
                "invalid task text published",
            )?;
        }
        require(
            client.selected()? == x11rb::NONE,
            "rejected publication changed clipboard",
        )?;
        foreign.close()?;
        println!("PASS clipboard publication rejects foreign lease cancelled and invalid text without ownership write");
        owner.publish(&lease, "task 中文🙂", &ActionCancellation::default())?;
        require(
            owner.state() == ClipboardState::Published && client.selected()? != x11rb::NONE,
            "task publication missing",
        )?;
        require(
            owner
                .publish(&lease, "replay", &ActionCancellation::default())
                .is_err(),
            "task publication replayed",
        )?;
        require(owner.restore(&other).is_err(), "foreign lease restored")?;
        let snapshot = read_all(&mut owner)?;
        require(
            snapshot.formats.len() == 4
                && snapshot.formats.iter().all(|f| {
                    f.payload.bytes == "task 中文🙂".as_bytes()
                        && f.payload.format == 8
                        && f.payload.kind == client.utf8
                }),
            "Unicode task formats not independently readable",
        )?;
        require(owner.close().is_err(), "live task clipboard service closed")?;
        gtk_read(&mut owner)?;
        println!("PASS clipboard published Unicode formats readable and lease cannot replay or prematurely close");
        require(
            owner.restore(&lease)? == RestoreOutcome::Restored && client.selected()? == x11rb::NONE,
            "initial empty clipboard was not restored",
        )?;
        require(
            owner.restore(&lease)? == RestoreOutcome::AlreadyRestored,
            "empty restore not idempotent",
        )?;
        owner.close()?;
        println!("PASS clipboard originally empty selection restored and service closes only after release");
    }
    for mode in [Mode::Formats, Mode::LargeFormats] {
        let original = Owner::start(mode)?;
        let mut reader = ClipboardReader::connect()?;
        let snapshot = reader.snapshot(|| Ok(()))?;
        let expected = bytes(&snapshot);
        let (mut owner, lease) = ClipboardOwner::new(reader, snapshot)?;
        owner.publish(&lease, "temporary text", &ActionCancellation::default())?;
        drop(original);
        // Queue a conversion before restoring; the original request must retain
        // the task bytes even though it is serviced after the restoration fence.
        client.begin(client.utf8, client.property, 0)?;
        require(
            owner.restore(&lease)? == RestoreOutcome::Restored,
            "saved clipboard restoration failed",
        )?;
        require(
            client.notify(&mut owner)?.property == client.property,
            "queued task request declined",
        )?;
        require(
            client.receive(&mut owner, client.property)?.2 == b"temporary text",
            "queued task request silently switched to saved bytes",
        )?;
        let restored = read_all(&mut owner)?;
        require(
            bytes(&restored) == expected && owner.state() == ClipboardState::Restored,
            "restored multi-format bytes/types differ",
        )?;
        require(
            owner.close().is_err(),
            "restored selection provider destroyed live data",
        )?;
        require(
            owner.restore(&lease)? == RestoreOutcome::AlreadyRestored,
            "multi-format restore not idempotent",
        )?;
        client.claim(client.window)?;
        require(
            owner.restore(&lease)? == RestoreOutcome::PreservedNewOwner
                && client.selected()? == client.window,
            "restoration overwrote subsequent user copy",
        )?;
        owner.close()?;
        client.claim(x11rb::NONE)?;
        println!("PASS clipboard {} 8/16/32-bit restoration survives original provider exit with queued-request isolation", if mode == Mode::LargeFormats { "large INCR" } else { "direct" });
    }
    for aba in [false, true] {
        let original = Owner::start(Mode::Formats)?;
        let (mut owner, lease) = prepared()?;
        original.command(if aba {
            Control::Aba
        } else {
            Control::Republish
        })?;
        require(
            owner
                .publish(&lease, "stale", &ActionCancellation::default())
                .is_err()
                && client.selected()? == original.window,
            "stale snapshot publication overwrote newer copy",
        )?;
        owner.close()?;
        println!(
            "PASS clipboard {} prevents stale snapshot publication",
            if aba { "ABA" } else { "same-owner copy" }
        );
    }
    {
        let (mut owner, lease) = prepared()?;
        owner.publish(&lease, "retained", &ActionCancellation::default())?;
        let own_window = client.selected()?;
        client.claim(client.window)?;
        client.claim(own_window)?;
        require(
            owner.restore(&lease)? == RestoreOutcome::PreservedNewOwner
                && client.selected()? == own_window,
            "return-to-same-window ABA restored stale data",
        )?;
        require(
            owner.close().is_err(),
            "ABA close destroyed externally republished owner window",
        )?;
        client.claim(x11rb::NONE)?;
        owner.close()?;
        println!("PASS clipboard ownership return to same service window preserves new epoch and refuses unsafe close");
    }
    protocol(&client)?;
    transfers(&client)?;
    manager_fixture::run()?;
    Ok(())
}

#[path = "clipboard_manager_fixture.rs"]
mod manager_fixture;
pub use manager_fixture::run_keeper_parent_fixture;

fn protocol(client: &Requestor) -> Result<(), String> {
    let (mut owner, lease) = prepared()?;
    owner.publish(&lease, "café", &ActionCancellation::default())?;
    client.begin(u32::from(AtomEnum::STRING), client.property, 0)?;
    require(
        client.notify(&mut owner)?.property == client.property,
        "Latin1 request declined",
    )?;
    require(
        client.receive(&mut owner, client.property)?
            == (u32::from(AtomEnum::STRING), 8, vec![b'c', b'a', b'f', 0xe9]),
        "Latin1 STRING not encoded correctly",
    )?;
    client.begin(client.utf8, x11rb::NONE, 0)?;
    require(
        client.notify(&mut owner)?.property == client.utf8,
        "legacy None property target fallback failed",
    )?;
    let timestamp = atom(&client.conn, b"TIMESTAMP")?;
    client.begin(timestamp, client.property, 0)?;
    client.notify(&mut owner)?;
    let time = client.property(client.property, false)?;
    require(
        time.type_ == u32::from(AtomEnum::INTEGER) && time.format == 32 && time.value_len == 1,
        "TIMESTAMP shape incorrect",
    )?;
    let acquired = time
        .value32()
        .ok_or("no timestamp values")?
        .next()
        .ok_or("empty timestamp")?;
    for when in [acquired.wrapping_sub(1), acquired.wrapping_add(1_000_000)] {
        client.begin(client.utf8, client.property, when)?;
        require(
            client.notify(&mut owner)?.property == x11rb::NONE,
            "out-of-ownership-time request succeeded",
        )?;
    }
    println!("PASS clipboard STRING Latin1 legacy property and exact timestamp interval validated");
    let multiple = atom(&client.conn, b"MULTIPLE")?;
    let pair_type = atom(&client.conn, b"ATOM_PAIR")?;
    let p1 = atom(&client.conn, b"_CU_OWNER_TEST_MULTI1")?;
    let p2 = atom(&client.conn, b"_CU_OWNER_TEST_MULTI2")?;
    let bad = atom(&client.conn, b"DELETE")?;
    let pairs = vec![
        client.utf8,
        p1,
        timestamp,
        p2,
        bad,
        client.utf8,
        client.utf8,
        p1,
        multiple,
        timestamp,
        client.utf8,
        client.property,
    ];
    client
        .conn
        .change_property32(
            PropMode::REPLACE,
            client.window,
            client.property,
            pair_type,
            &pairs,
        )
        .map_err(err)?
        .check()
        .map_err(err)?;
    client.begin(multiple, client.property, 0)?;
    require(
        client.notify(&mut owner)?.property == client.property,
        "MULTIPLE rejected whole valid list",
    )?;
    let returned: Vec<_> = client
        .property(client.property, false)?
        .value32()
        .ok_or("bad MULTIPLE response type")?
        .collect();
    require(
        returned
            == vec![
                client.utf8,
                p1,
                timestamp,
                p2,
                bad,
                0,
                client.utf8,
                0,
                multiple,
                0,
                client.utf8,
                0,
            ],
        "MULTIPLE failed pairs or collision semantics wrong",
    )?;
    require(
        client.receive(&mut owner, p1)?.2 == "café".as_bytes(),
        "MULTIPLE successful item lost",
    )?;
    for pairs in [vec![client.utf8], vec![client.utf8; 66]] {
        client
            .conn
            .change_property32(
                PropMode::REPLACE,
                client.window,
                client.property,
                pair_type,
                &pairs,
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        client.begin(multiple, client.property, 0)?;
        require(
            client.notify(&mut owner)?.property == 0,
            "malformed MULTIPLE list accepted",
        )?;
    }
    client.begin(multiple, 0, 0)?;
    require(
        client.notify(&mut owner)?.property == 0,
        "MULTIPLE without property accepted",
    )?;
    owner.restore(&lease)?;
    owner.close()?;
    println!("PASS clipboard MULTIPLE ordered success failure duplicate recursion collision and bounds enforced");
    Ok(())
}

fn gtk_read(owner: &mut ClipboardOwner) -> Result<(), String> {
    // Independent toolkit, not the production reader paired with its publisher.
    let mut child = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/gtk_clipboard_reader.py"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(err)?;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            owner.pump()?;
            if let Some(status) = child.try_wait().map_err(err)? {
                return require(
                    status.success(),
                    "independent GTK could not read the published Unicode selection",
                );
            }
            if Instant::now() >= deadline {
                return Err("independent GTK clipboard read timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result?;
    println!("PASS clipboard independent GTK reads published Unicode text without AX or keyboard substitution");
    Ok(())
}

fn transfers(client: &Requestor) -> Result<(), String> {
    let (mut owner, lease) = prepared()?;
    let text = "x".repeat(24000);
    let token = ActionCancellation::default();
    owner.publish(&lease, &text, &token)?;
    token.cancel();
    owner.pump()?;
    require(
        owner.state() == ClipboardState::Published,
        "Stop implicitly rolled back an unproven paste",
    )?;
    let multiple = atom(&client.conn, b"MULTIPLE")?;
    let pair_type = atom(&client.conn, b"ATOM_PAIR")?;
    let timestamp = atom(&client.conn, b"TIMESTAMP")?;
    let p1 = atom(&client.conn, b"_CU_OWNER_TEST_MULTI1")?;
    let p2 = atom(&client.conn, b"_CU_OWNER_TEST_MULTI2")?;
    client
        .conn
        .change_property32(
            PropMode::REPLACE,
            client.window,
            client.property,
            pair_type,
            &[client.utf8, p1, timestamp, p2],
        )
        .map_err(err)?
        .check()
        .map_err(err)?;
    client.begin(multiple, client.property, 0)?;
    require(
        client.notify(&mut owner)?.property == client.property && owner.active_transfers() == 1,
        "MULTIPLE mixed INCR/direct conversion failed",
    )?;
    require(
        client.property(p2, false)?.value_len == 1
            && client.receive(&mut owner, p1)?.2 == text.as_bytes(),
        "MULTIPLE mixed transfer values wrong",
    )?;
    let doomed = Requestor::new()?;
    doomed.begin(doomed.utf8, doomed.property, 0)?;
    doomed
        .conn
        .destroy_window(doomed.window)
        .map_err(err)?
        .check()
        .map_err(err)?;
    owner.pump()?;
    require(
        owner.state() == ClipboardState::Published && owner.active_transfers() == 0,
        "destroyed queued requestor broke healthy service",
    )?;
    println!("PASS clipboard mixed MULTIPLE INCR and dead queued requestor survive without Stop-triggered restoration");
    client.begin(client.utf8, client.property, 0)?;
    require(
        client.notify(&mut owner)?.property == client.property && owner.active_transfers() == 1,
        "large task did not enter INCR",
    )?;
    require(
        client.property(client.property, false)?.type_ == client.incr,
        "large task header missing",
    )?;
    require(
        owner.restore(&lease)? == RestoreOutcome::Restored && client.selected()? == 0,
        "mid-INCR empty restoration failed",
    )?;
    require(
        owner.close().is_err(),
        "service closed with outstanding INCR",
    )?;
    require(
        client.receive(&mut owner, client.property)?.2 == text.as_bytes()
            && owner.active_transfers() == 0,
        "INCR did not pin original task through restore and zero-ack",
    )?;
    owner.close()?;
    println!("PASS clipboard mid-INCR restore preserves accepted task bytes until typed terminator acknowledgement");
    let (mut owner, lease) = prepared()?;
    owner.publish(&lease, &text, &ActionCancellation::default())?;
    let mut clients = Vec::new();
    for _ in 0..16 {
        let q = Requestor::new()?;
        q.begin(q.utf8, q.property, 0)?;
        require(
            q.notify(&mut owner)?.property == q.property,
            "bounded concurrent INCR refused too early",
        )?;
        clients.push(q);
    }
    client.begin(client.utf8, client.property, 0)?;
    require(
        client.notify(&mut owner)?.property == 0 && owner.active_transfers() == 16,
        "INCR capacity bound ignored",
    )?;
    let first = &clients[0];
    first.begin(first.utf8, first.property, 0)?;
    require(
        first.notify(&mut owner)?.property == 0
            && first.property(first.property, false)?.type_ == first.incr,
        "duplicate transfer overwrote active property",
    )?;
    drop(clients.pop());
    for _ in 0..10 {
        owner.pump()?;
        if owner.active_transfers() == 15 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    require(
        owner.active_transfers() == 15,
        "destroyed requestor transfer not retired",
    )?;
    client.claim(client.window)?;
    require(
        owner.restore(&lease)? == RestoreOutcome::PreservedNewOwner,
        "active transfer stole external copy",
    )?;
    require(
        clients[0].receive(&mut owner, clients[0].property)?.2 == text.as_bytes(),
        "ownership loss interrupted accepted INCR",
    )?;
    let deadline = Instant::now() + Duration::from_secs(6);
    while owner.active_transfers() > 0 && Instant::now() < deadline {
        owner.pump()?;
        std::thread::sleep(Duration::from_millis(5));
    }
    require(
        owner.active_transfers() == 0 && client.selected()? == client.window,
        "stalled transfer expiry changed newer clipboard",
    )?;
    require(
        clients[1].property(clients[1].property, false)?.type_ == clients[1].incr,
        "expiry deleted foreign property",
    )?;
    owner.close()?;
    client.claim(0)?;
    println!("PASS clipboard INCR concurrency duplicate destroy expiry and ownership loss remain isolated");
    Ok(())
}
