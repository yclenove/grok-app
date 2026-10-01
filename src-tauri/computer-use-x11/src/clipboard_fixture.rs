//! Independent owner on another native X11 connection. Fixture only, owned Xvfb.
use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU32, AtomicUsize, Ordering},
    mpsc, Arc,
};
use x11rb::protocol::xproto::{
    self, ChangeWindowAttributesAux, PropMode, SelectionNotifyEvent, SelectionRequestEvent,
};
use x11rb::wrapper::ConnectionExt as _;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Formats,
    LargeFormats,
    Incr,
    ChangedType,
    ChangedFormat,
    ShortIncr,
    HugeIncr,
    BadTargets,
    ManyTargets,
    Unsupported,
    Decline,
    Stall,
    Takeover,
}
enum Control {
    Republish,
    Aba,
    Stop,
}
struct Owner {
    commands: mpsc::Sender<(Control, mpsc::Sender<()>)>,
    worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    window: Window,
    reads: Arc<AtomicUsize>,
    last_requestor: Arc<AtomicU32>,
}
impl Owner {
    fn start(mode: Mode) -> Result<Self, String> {
        let (commands, incoming) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let reads = Arc::new(AtomicUsize::new(0));
        let last_requestor = Arc::new(AtomicU32::new(0));
        let r = reads.clone();
        let q = last_requestor.clone();
        let worker = std::thread::spawn(move || serve(mode, incoming, ready, r, q));
        let window = started
            .recv_timeout(Duration::from_secs(3))
            .map_err(err)??;
        Ok(Self {
            commands,
            worker: Some(worker),
            window,
            reads,
            last_requestor,
        })
    }
    fn command(&self, command: Control) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.commands.send((command, tx)).map_err(err)?;
        rx.recv_timeout(Duration::from_secs(3)).map_err(err)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.command(Control::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Transfer {
    property: Atom,
    target: Atom,
    offset: usize,
    format: u8,
}
fn serve(
    mode: Mode,
    commands: mpsc::Receiver<(Control, mpsc::Sender<()>)>,
    ready: mpsc::Sender<Result<Window, String>>,
    reads: Arc<AtomicUsize>,
    last: Arc<AtomicU32>,
) -> Result<(), String> {
    let setup = (|| {
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        let root = conn.setup().roots[screen].root;
        let owner = window(&conn, root)?;
        let clipboard = atom(&conn, b"CLIPBOARD")?;
        conn.set_selection_owner(owner, clipboard, x11rb::CURRENT_TIME)
            .map_err(err)?
            .check()
            .map_err(err)?;
        Ok::<_, String>((conn, root, owner, clipboard))
    })();
    let (conn, root, owner, clipboard) = match setup {
        Ok(s) => s,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let targets = atom(&conn, b"TARGETS")?;
    let utf8 = atom(&conn, b"UTF8_STRING")?;
    let incr = atom(&conn, b"INCR")?;
    let binary = atom(&conn, b"application/octet-stream")?;
    let rtf = atom(&conn, b"text/rtf")?;
    let html = atom(&conn, b"text/html")?;
    let png = atom(&conn, b"image/png")?;
    let delete = atom(&conn, b"DELETE")?;
    let mut transfers: HashMap<Window, Transfer> = HashMap::new();
    ready.send(Ok(owner)).map_err(err)?;
    loop {
        if let Ok((command, reply)) = commands.try_recv() {
            match command {
                Control::Stop => {
                    let _ = reply.send(());
                    return Ok(());
                }
                Control::Republish => {
                    conn.set_selection_owner(owner, clipboard, x11rb::CURRENT_TIME)
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                }
                Control::Aba => {
                    let other = window(&conn, root)?;
                    conn.set_selection_owner(other, clipboard, x11rb::CURRENT_TIME)
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                    conn.set_selection_owner(owner, clipboard, x11rb::CURRENT_TIME)
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                    conn.destroy_window(other)
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                }
            }
            let _ = reply.send(());
        }
        match conn.poll_for_event().map_err(err)? {
            Some(Event::SelectionRequest(request)) if request.selection == clipboard => {
                last.store(request.requestor, Ordering::Release);
                if request.target == targets {
                    if mode == Mode::BadTargets {
                        conn.change_property8(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            AtomEnum::STRING,
                            b"wrong target type",
                        )
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                    } else {
                        let mut offered = vec![targets, utf8];
                        if matches!(mode, Mode::Formats | Mode::LargeFormats) {
                            offered.extend([binary, rtf, html, png]);
                        }
                        if mode == Mode::Unsupported {
                            offered.push(delete);
                        }
                        if mode == Mode::ManyTargets {
                            offered = vec![utf8; MAX_TARGETS + 1];
                        }
                        conn.change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            AtomEnum::ATOM,
                            &offered,
                        )
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                    }
                    notify(&conn, &request, request.property)?;
                    continue;
                }
                reads.fetch_add(1, Ordering::Relaxed);
                if mode == Mode::Stall {
                    continue;
                }
                if mode == Mode::Decline {
                    notify(&conn, &request, x11rb::NONE)?;
                    continue;
                }
                if matches!(
                    mode,
                    Mode::Incr
                        | Mode::ChangedType
                        | Mode::ChangedFormat
                        | Mode::ShortIncr
                        | Mode::HugeIncr
                ) {
                    conn.change_window_attributes(
                        request.requestor,
                        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                    )
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
                    let size = if mode == Mode::HugeIncr {
                        MAX_FORMAT_BYTES + 1
                    } else if mode == Mode::ShortIncr {
                        32769
                    } else {
                        32768
                    };
                    conn.change_property32(
                        PropMode::REPLACE,
                        request.requestor,
                        request.property,
                        incr,
                        &[size as u32],
                    )
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
                    transfers.insert(
                        request.requestor,
                        Transfer {
                            property: request.property,
                            target: utf8,
                            offset: 0,
                            format: 8,
                        },
                    );
                    notify(&conn, &request, request.property)?;
                    continue;
                }
                if request.target == binary {
                    conn.change_property16(
                        PropMode::REPLACE,
                        request.requestor,
                        request.property,
                        AtomEnum::INTEGER,
                        &if mode == Mode::LargeFormats {
                            [0, 0xff00, 0x8081, u16::MAX].repeat(8192)
                        } else {
                            vec![0, 0xff00, 0x8081, u16::MAX]
                        },
                    )
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
                } else if request.target == rtf {
                    conn.change_property32(
                        PropMode::REPLACE,
                        request.requestor,
                        request.property,
                        AtomEnum::CARDINAL,
                        &if mode == Mode::LargeFormats {
                            [0, 0xff00ff00, u32::MAX].repeat(8192)
                        } else {
                            vec![0, 0xff00ff00, u32::MAX]
                        },
                    )
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
                } else {
                    let large = vec![0x61; 65536];
                    let bytes: &[u8] = if mode == Mode::LargeFormats && request.target == utf8 {
                        &large
                    } else if request.target == utf8 {
                        "fixture 中文🙂".as_bytes()
                    } else if request.target == png {
                        &[0, 255, 137, 0, 10, 13, 254]
                    } else {
                        b"<b>owned HTML</b>"
                    };
                    conn.change_property8(
                        PropMode::REPLACE,
                        request.requestor,
                        request.property,
                        request.target,
                        bytes,
                    )
                    .map_err(err)?
                    .check()
                    .map_err(err)?;
                }
                if mode == Mode::Takeover {
                    let other = window(&conn, root)?;
                    conn.set_selection_owner(other, clipboard, x11rb::CURRENT_TIME)
                        .map_err(err)?
                        .check()
                        .map_err(err)?;
                }
                notify(&conn, &request, request.property)?;
            }
            Some(Event::PropertyNotify(event)) if event.state == Property::DELETE => {
                if let Some(transfer) = transfers.get_mut(&event.window) {
                    if event.atom != transfer.property {
                        continue;
                    }
                    let chunk = if transfer.offset < 32768 {
                        vec![0x5a; 4096]
                    } else {
                        vec![]
                    };
                    if transfer.offset > 0 && mode == Mode::ChangedType {
                        transfer.target = AtomEnum::STRING.into();
                    }
                    if transfer.offset > 0 && mode == Mode::ChangedFormat {
                        transfer.format = 16;
                    }
                    // A rejected/expired reader destroys only its own requestor.
                    // BadWindow is expected here and ends this fixture transfer.
                    let written = if transfer.format == 16 {
                        conn.change_property16(
                            PropMode::REPLACE,
                            event.window,
                            transfer.property,
                            transfer.target,
                            &vec![0x5a5a; chunk.len() / 2],
                        )
                        .map_err(err)?
                        .check()
                    } else {
                        conn.change_property8(
                            PropMode::REPLACE,
                            event.window,
                            transfer.property,
                            transfer.target,
                            &chunk,
                        )
                        .map_err(err)?
                        .check()
                    };
                    transfer.offset += chunk.len();
                    if chunk.is_empty() || written.is_err() {
                        transfers.remove(&event.window);
                    }
                }
            }
            Some(_) => (),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
fn notify(
    conn: &RustConnection,
    request: &SelectionRequestEvent,
    property: Atom,
) -> Result<(), String> {
    let event = SelectionNotifyEvent {
        response_type: xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property,
    };
    conn.send_event(false, request.requestor, EventMask::NO_EVENT, event)
        .map_err(err)?
        .check()
        .map_err(err)
}
fn current(reader: &ClipboardReader) -> Result<Window, String> {
    Ok(reader
        .conn
        .get_selection_owner(reader.clipboard)
        .map_err(err)?
        .reply()
        .map_err(err)?
        .owner)
}

pub fn run_clipboard_selftest() -> Result<(), String> {
    if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
        return Err("owned Xvfb required; refusing ordinary desktop clipboard access".into());
    }
    crate::require_native_x11()?;
    let mut reader = ClipboardReader::connect()?;
    if current(&reader)? != x11rb::NONE {
        return Err("private fixture display must start without a clipboard owner".into());
    }
    let empty = reader.snapshot(|| Ok(()))?;
    if !empty.was_empty() || empty.format_count() != 0 || empty.byte_count() != 0 {
        return Err("empty clipboard snapshot is not empty".into());
    }
    println!("PASS clipboard empty owner remains empty without claiming a selection");
    {
        let owner = Owner::start(Mode::Formats)?;
        let snapshot = reader.snapshot(|| Ok(()))?;
        if snapshot.format_count() != 5
            || !reader.unchanged(&snapshot)?
            || current(&reader)? != owner.window
        {
            return Err("materialization changed owner or dropped a format".into());
        }
        let b = atom(&reader.conn, b"application/octet-stream")?;
        let r = atom(&reader.conn, b"text/rtf")?;
        let u = atom(&reader.conn, b"UTF8_STRING")?;
        let h = atom(&reader.conn, b"text/html")?;
        let p = atom(&reader.conn, b"image/png")?;
        for entry in &snapshot.formats {
            let expected_8: Option<&[u8]> = if entry.target == u {
                Some("fixture 中文🙂".as_bytes())
            } else if entry.target == h {
                Some(b"<b>owned HTML</b>")
            } else if entry.target == p {
                Some(&[0, 255, 137, 0, 10, 13, 254])
            } else {
                None
            };
            if expected_8.is_some_and(|bytes| {
                entry.payload.format != 8
                    || entry.payload.kind != entry.target
                    || entry.payload.bytes != bytes
            }) {
                return Err("8-bit Unicode/HTML/binary preservation mismatch".into());
            }
            if entry.target == b
                && (entry.payload.format != 16
                    || entry.payload.kind != u32::from(AtomEnum::INTEGER)
                    || entry.payload.bytes
                        != [0u16, 0xff00, 0x8081, u16::MAX]
                            .into_iter()
                            .flat_map(u16::to_ne_bytes)
                            .collect::<Vec<_>>())
            {
                return Err("16-bit binary preservation mismatch".into());
            }
            if entry.target == r
                && (entry.payload.format != 32
                    || entry.payload.kind != u32::from(AtomEnum::CARDINAL)
                    || entry.payload.bytes
                        != [0u32, 0xff00ff00, u32::MAX]
                            .into_iter()
                            .flat_map(u32::to_ne_bytes)
                            .collect::<Vec<_>>())
            {
                return Err("32-bit binary preservation mismatch".into());
            }
        }
        let expected_bytes = "fixture 中文🙂".len() + b"<b>owned HTML</b>".len() + 7 + 8 + 12;
        if snapshot.byte_count() != expected_bytes
            || snapshot.formats().map(|f| f.byte_count).sum::<usize>() != expected_bytes
        {
            return Err("clipboard metadata disagrees with independent byte count".into());
        }
        println!("PASS clipboard native 8/16/32-bit multi-format bytes and types preserved");
        let mut foreign = ClipboardReader::connect()?;
        if foreign.unchanged(&snapshot)? {
            return Err("foreign connection accepted a snapshot identity".into());
        }
        println!("PASS clipboard snapshot identity cannot cross reader connections");
        owner.command(Control::Republish)?;
        if reader.unchanged(&snapshot)? || current(&reader)? != owner.window {
            return Err("same-window new copy was mistaken for unchanged ownership".into());
        }
        println!("PASS clipboard same-window republication invalidates the old snapshot");
        let next = reader.snapshot(|| Ok(()))?;
        owner.command(Control::Aba)?;
        if reader.unchanged(&next)? || current(&reader)? != owner.window {
            return Err("ownership ABA was not detected".into());
        }
        println!("PASS clipboard ownership ABA invalidates the old snapshot");
    }
    {
        let owner = Owner::start(Mode::Incr)?;
        let snapshot = reader.snapshot(|| Ok(()))?;
        if snapshot.formats.len() != 1
            || snapshot.formats[0].payload.bytes != vec![0x5a; 32768]
            || current(&reader)? != owner.window
        {
            return Err("INCR materialization did not preserve exact native bytes".into());
        }
        println!("PASS clipboard multi-chunk INCR completes with exact bytes and unchanged owner");
    }
    for (mode, message) in [
        (Mode::ChangedType, "INCR changed type"),
        (Mode::ChangedFormat, "INCR changed format"),
        (Mode::ShortIncr, "INCR truncated lower bound"),
        (Mode::HugeIncr, "INCR oversized lower bound"),
        (Mode::BadTargets, "malformed TARGETS"),
        (Mode::ManyTargets, "oversized TARGETS"),
        (Mode::Unsupported, "unsupported side-effect target"),
        (Mode::Decline, "declined required format"),
    ] {
        let owner = Owner::start(mode)?;
        if reader.snapshot(|| Ok(())).is_ok() || current(&reader)? != owner.window {
            return Err(format!(
                "{message} was accepted or changed original ownership"
            ));
        }
        if matches!(
            mode,
            Mode::BadTargets | Mode::ManyTargets | Mode::Unsupported
        ) && owner.reads.load(Ordering::Relaxed) != 0
        {
            return Err("unsafe TARGETS triggered a data conversion".into());
        }
        println!("PASS clipboard {message} rejected without claiming ownership");
    }
    {
        let owner = Owner::start(Mode::Stall)?;
        let started = Instant::now();
        let result = reader.snapshot(|| {
            if started.elapsed() > Duration::from_millis(100) {
                Err("owned fixture cancellation".into())
            } else {
                Ok(())
            }
        });
        if !matches!(result, Err(ref error) if error == "owned fixture cancellation")
            || started.elapsed() > Duration::from_secs(1)
            || current(&reader)? != owner.window
        {
            return Err("cancelled clipboard conversion did not return without mutation".into());
        }
        let requestor = owner.last_requestor.load(Ordering::Acquire);
        if reader
            .conn
            .get_window_attributes(requestor)
            .map_err(err)?
            .reply()
            .is_ok()
        {
            return Err("cancelled conversion left its requestor window alive".into());
        }
        println!(
            "PASS clipboard cancellation destroys the exact requestor and retains original owner"
        );
        let started = Instant::now();
        let result = reader.snapshot(|| Ok(()));
        if !matches!(result, Err(ref error) if error.contains("deadline exceeded"))
            || started.elapsed() > Duration::from_secs(5)
            || current(&reader)? != owner.window
        {
            return Err("silent owner exceeded bounded preservation deadline".into());
        }
        println!(
            "PASS clipboard silent owner expires without retrying or replacing clipboard data"
        );
    }
    {
        let owner = Owner::start(Mode::Takeover)?;
        if reader.snapshot(|| Ok(())).is_ok() || current(&reader)? == owner.window {
            return Err("clipboard takeover during conversion was not preserved/rejected".into());
        }
        println!(
            "PASS clipboard concurrent new owner is preserved and invalidates materialization"
        );
    }
    {
        let owner = Owner::start(Mode::Formats)?;
        let snapshot = reader.snapshot(|| Ok(()))?;
        drop(owner);
        if reader.unchanged(&snapshot)? || current(&reader)? != x11rb::NONE {
            return Err("dead clipboard provider was treated as an unchanged owner".into());
        }
        println!(
            "PASS clipboard provider exit invalidates snapshot without resurrecting old owner"
        );
    }
    gtk_acceptance(&mut reader)?;
    owner_fixture::run()?;
    Ok(())
}

#[path = "clipboard_owner_fixture.rs"]
mod owner_fixture;
pub use owner_fixture::run_keeper_parent_fixture;

fn gtk_acceptance(reader: &mut ClipboardReader) -> Result<(), String> {
    let mut child = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/gtk_clipboard_owner.py"
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(err)?;
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().ok_or("missing GTK clipboard stdout")?;
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let result = (|| {
        let ready = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(err)?
            .map_err(err)?;
        if ready.trim() != "ready" {
            return Err("GTK clipboard fixture did not become ready".into());
        }
        let original = current(reader)?;
        let snapshot = reader.snapshot(|| Ok(()))?;
        let utf8 = atom(&reader.conn, b"UTF8_STRING")?;
        let value = snapshot
            .formats
            .iter()
            .find(|f| f.target == utf8)
            .ok_or("GTK UTF8_STRING was dropped")?;
        if value.payload.bytes != "CU clipboard fixture 中文🙂".as_bytes()
            || current(reader)? != original
            || !reader.unchanged(&snapshot)?
        {
            return Err("independent GTK clipboard preservation mismatch".into());
        }
        println!("PASS clipboard independent GTK Unicode owner remains intact across full materialization");
        Ok(())
    })();
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "quit");
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}
