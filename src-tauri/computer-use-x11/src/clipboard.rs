//! Host-only X11 clipboard preservation. Clipboard bytes never enter an
//! Observation, model tool result, Debug implementation, or error message.
//!
//! This is the materialization half of a clipboard transaction, NOT a paste
//! implementation. The original owner remains untouched. A publisher must
//! check `unchanged` again inside its short server-side ownership fence, retain
//! every materialized format, and keep serving restored data until ownership
//! changes or a clipboard manager has acknowledged receiving it. This is not
//! disk durability or App lifetime integration. PasteText's reply
//! alone does not prove completion of an asynchronous toolkit paste callback.
use crate::client::err;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, GetPropertyReply, Property,
    Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

#[path = "clipboard_retention.rs"]
mod retention;
pub use retention::{
    ClipboardMonitor, ClipboardRetention, RetainedClipboard, RetentionState, RetentionStatus,
};

const MAX_TARGETS: usize = 64;
const MAX_FORMAT_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const SNAPSHOT_DEADLINE: Duration = Duration::from_secs(3);
const EVENT_BUDGET: usize = 256;

// Deliberately not Debug/Serialize: these may contain arbitrary private data.
struct Payload {
    kind: Atom,
    format: u8,
    bytes: Vec<u8>,
}
struct Materialized {
    target: Atom,
    payload: Payload,
}

/// Opaque, bounded preservation data. Not an authorization or a paste result.
pub struct ClipboardSnapshot {
    connection_id: uuid::Uuid,
    owner: Window,
    epoch: u64,
    formats: Vec<Materialized>,
}

/// Format metadata only; no clipboard payload or provider-controlled names.
pub struct ClipboardFormat {
    pub target_atom: Atom,
    pub type_atom: Atom,
    pub format_bits: u8,
    pub byte_count: usize,
}

impl ClipboardSnapshot {
    pub fn format_count(&self) -> usize {
        self.formats.len()
    }
    pub fn byte_count(&self) -> usize {
        self.formats.iter().map(|f| f.payload.bytes.len()).sum()
    }
    pub fn was_empty(&self) -> bool {
        self.owner == x11rb::NONE
    }
    pub fn formats(&self) -> impl Iterator<Item = ClipboardFormat> + '_ {
        self.formats.iter().map(|entry| ClipboardFormat {
            target_atom: entry.target,
            type_atom: entry.payload.kind,
            format_bits: entry.payload.format,
            byte_count: entry.payload.bytes.len(),
        })
    }
}

/// Dedicated connection: never drains the desktop input/capture connection.
/// Only private requestor windows/properties are written. No clipboard claiming,
/// key synthesis, clipboard-manager requests, or fallback to another selection.
pub struct ClipboardReader {
    conn: Arc<RustConnection>,
    root: Window,
    watch: Window,
    clipboard: Atom,
    targets: Atom,
    incr: Atom,
    transfer: Atom,
    connection_id: uuid::Uuid,
    epoch: u64,
}

impl ClipboardReader {
    pub fn connect() -> Result<Self, String> {
        crate::require_native_x11()?;
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        let root = conn.setup().roots[screen].root;
        // Selection-change tracking is required, not a best-effort fallback.
        let version = conn
            .xfixes_query_version(5, 0)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if version.major_version < 1 {
            return Err("X11 clipboard preservation requires XFixes selection tracking".into());
        }
        let clipboard = atom(&conn, b"CLIPBOARD")?;
        let targets = atom(&conn, b"TARGETS")?;
        let incr = atom(&conn, b"INCR")?;
        let transfer = atom(&conn, b"_GROK_CU_SELECTION_TRANSFER")?;
        let watch = window(&conn, root)?;
        conn.xfixes_select_selection_input(
            watch,
            clipboard,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(err)?
        .check()
        .map_err(err)?;
        Ok(Self {
            conn: Arc::new(conn),
            root,
            watch,
            clipboard,
            targets,
            incr,
            transfer,
            connection_id: uuid::Uuid::new_v4(),
            epoch: 0,
        })
    }

    /// Materialize all advertised supported formats before any future mutation.
    /// Unsupported/private targets fail closed instead of silently losing them.
    /// `check` is called throughout polling; cancellation never claims ownership.
    pub fn snapshot(
        &mut self,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<ClipboardSnapshot, String> {
        let deadline = Instant::now() + SNAPSHOT_DEADLINE;
        check()?;
        let (owner, epoch) = self.identity()?;
        let mut snapshot = ClipboardSnapshot {
            connection_id: self.connection_id,
            owner,
            epoch,
            formats: vec![],
        };
        if owner == x11rb::NONE {
            check()?;
            return Ok(snapshot);
        }
        let offered = self.convert(self.targets, MAX_TARGETS * 4, deadline, &mut check)?;
        let offered = target_atoms(&offered)?;
        let mut targets = vec![];
        for target in offered {
            check_deadline(deadline, &mut check)?;
            let name = self
                .conn
                .get_atom_name(target)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .name;
            match target_policy(&name) {
                TargetPolicy::Data => targets.push(target),
                TargetPolicy::Protocol => (),
                TargetPolicy::Unsupported => return Err(
                    "X11 clipboard contains an unsupported preservation target; owner unchanged"
                        .into(),
                ),
            }
        }
        if targets.is_empty() {
            return Err("X11 clipboard owner exposes no preservable data formats".into());
        }
        let mut total = 0usize;
        for target in targets {
            self.require_identity(owner, epoch)?;
            let cap = MAX_FORMAT_BYTES.min(MAX_TOTAL_BYTES - total);
            let payload = self.convert(target, cap, deadline, &mut check)?;
            total = checked_total(total, payload.bytes.len())?;
            snapshot.formats.push(Materialized { target, payload });
        }
        check_deadline(deadline, &mut check)?;
        self.require_identity(owner, epoch)?;
        Ok(snapshot)
    }

    /// Detect even same-window re-publication/ABA ownership changes. A successful
    /// check is only an ownership observation, not an immutable-content proof:
    /// X11 cannot detect a provider changing bytes without re-claiming ownership.
    /// A future publisher must fence its own swap; this never authorizes input.
    pub fn unchanged(&mut self, snapshot: &ClipboardSnapshot) -> Result<bool, String> {
        if snapshot.connection_id != self.connection_id {
            return Ok(false);
        }
        let (owner, epoch) = self.identity()?;
        Ok(owner == snapshot.owner && epoch == snapshot.epoch)
    }

    fn track(&mut self, event: &Event) -> Result<(), String> {
        if let Event::XfixesSelectionNotify(change) = event {
            if change.window == self.watch && change.selection == self.clipboard {
                self.epoch = self
                    .epoch
                    .checked_add(1)
                    .ok_or("X11 selection epoch exhausted")?;
            }
        }
        Ok(())
    }

    fn identity(&mut self) -> Result<(Window, u64), String> {
        // The reply is a server barrier for preceding owner-change events.
        let owner = self
            .conn
            .get_selection_owner(self.clipboard)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .owner;
        for _ in 0..EVENT_BUDGET {
            let Some(event) = self.conn.poll_for_event().map_err(err)? else {
                return Ok((owner, self.epoch));
            };
            self.track(&event)?;
        }
        Err("X11 clipboard event budget exceeded".into())
    }

    fn require_identity(&mut self, owner: Window, epoch: u64) -> Result<(), String> {
        if self.identity()? != (owner, epoch) {
            return Err("X11 clipboard ownership changed during preservation".into());
        }
        Ok(())
    }

    fn convert(
        &mut self,
        target: Atom,
        cap: usize,
        deadline: Instant,
        check: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Payload, String> {
        check_deadline(deadline, check)?;
        // Fresh native resource per conversion: late replies after timeout can
        // never satisfy a later conversion, even for the same target/property.
        let requestor = window(&self.conn, self.root)?;
        let result = self.convert_on(requestor, target, cap, deadline, check);
        // Destroying this owned window also terminates a stalled INCR transfer.
        // Never destroy an owner/requestor supplied by another application.
        let cleanup = self
            .conn
            .destroy_window(requestor)
            .map_err(err)?
            .check()
            .map_err(err);
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    fn convert_on(
        &mut self,
        requestor: Window,
        target: Atom,
        cap: usize,
        deadline: Instant,
        check: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Payload, String> {
        self.conn
            .convert_selection(
                requestor,
                self.clipboard,
                target,
                self.transfer,
                x11rb::CURRENT_TIME,
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        loop {
            let event = self.next(deadline, check)?;
            if let Event::SelectionNotify(reply) = event {
                if reply.requestor != requestor
                    || reply.selection != self.clipboard
                    || reply.target != target
                    || reply.time != x11rb::CURRENT_TIME
                {
                    continue;
                }
                if reply.property == x11rb::NONE {
                    return Err("X11 clipboard owner declined a required format".into());
                }
                if reply.property == self.transfer {
                    break;
                }
            }
        }
        let initial = self.property(requestor, cap)?;
        if initial.kind != self.incr {
            return Ok(initial);
        }
        let estimate = incr_estimate(&initial)?;
        if estimate > cap {
            return Err("X11 clipboard INCR exceeds preservation bounds".into());
        }
        // Deleting the complete INCR header above acknowledged readiness. Each
        // complete chunk is deleted only after bounded retrieval; zero ends it.
        let mut assembled: Option<Payload> = None;
        loop {
            let event = self.next(deadline, check)?;
            if let Event::PropertyNotify(change) = event {
                if change.window != requestor
                    || change.atom != self.transfer
                    || change.state != Property::NEW_VALUE
                {
                    continue;
                }
                let chunk = self.property(requestor, cap)?;
                if chunk.kind == self.incr {
                    return Err("X11 clipboard nested INCR is invalid".into());
                }
                let finished = chunk.bytes.is_empty();
                append_chunk(&mut assembled, chunk, cap)?;
                if finished {
                    let complete = assembled.ok_or("X11 clipboard INCR missing type")?;
                    if complete.bytes.len() < estimate {
                        return Err(
                            "X11 clipboard INCR shorter than its declared lower bound".into()
                        );
                    }
                    return Ok(complete);
                }
            }
        }
    }

    fn property(&self, requestor: Window, cap: usize) -> Result<Payload, String> {
        let reply = self
            .conn
            .get_property(
                true,
                requestor,
                self.transfer,
                AtomEnum::ANY,
                0,
                cap.div_ceil(4) as u32,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?;
        property_payload(reply, cap)
    }

    fn next(
        &mut self,
        deadline: Instant,
        check: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Event, String> {
        loop {
            check_deadline(deadline, check)?;
            if let Some(event) = self.conn.poll_for_event().map_err(err)? {
                self.track(&event)?;
                return Ok(event);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn atom(conn: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    Ok(conn
        .intern_atom(false, name)
        .map_err(err)?
        .reply()
        .map_err(err)?
        .atom)
}
fn window(conn: &RustConnection, root: Window) -> Result<Window, String> {
    let window = conn.generate_id().map_err(err)?;
    conn.create_window(
        0,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(err)?
    .check()
    .map_err(err)?;
    Ok(window)
}
fn check_deadline(
    deadline: Instant,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    check()?;
    if Instant::now() >= deadline {
        return Err("X11 clipboard preservation deadline exceeded".into());
    }
    Ok(())
}

#[derive(PartialEq, Eq, Debug)]
enum TargetPolicy {
    Data,
    Protocol,
    Unsupported,
}
fn target_policy(name: &[u8]) -> TargetPolicy {
    match name {
        b"TARGETS" | b"TIMESTAMP" | b"MULTIPLE" | b"SAVE_TARGETS" => TargetPolicy::Protocol,
        b"UTF8_STRING"
        | b"STRING"
        | b"TEXT"
        | b"COMPOUND_TEXT"
        | b"text/plain"
        | b"text/plain;charset=utf-8"
        | b"text/plain;charset=UTF-8"
        | b"text/html"
        | b"text/uri-list"
        | b"text/rtf"
        | b"application/rtf"
        | b"image/png"
        | b"image/jpeg"
        | b"image/bmp"
        | b"image/tiff"
        | b"application/octet-stream" => TargetPolicy::Data,
        _ => TargetPolicy::Unsupported,
    }
}
fn property_payload(reply: GetPropertyReply, cap: usize) -> Result<Payload, String> {
    let width = match reply.format {
        8 => 1,
        16 => 2,
        32 => 4,
        _ => return Err("X11 clipboard property has an invalid format".into()),
    };
    if reply.type_ == x11rb::NONE
        || reply.bytes_after != 0
        || reply.value.len() > cap
        || (reply.value_len as usize).checked_mul(width) != Some(reply.value.len())
    {
        return Err("X11 clipboard property is incomplete or exceeds bounds".into());
    }
    Ok(Payload {
        kind: reply.type_,
        format: reply.format,
        bytes: reply.value,
    })
}
fn target_atoms(payload: &Payload) -> Result<Vec<Atom>, String> {
    if payload.kind != u32::from(AtomEnum::ATOM)
        || payload.format != 32
        || payload.bytes.is_empty()
        || !payload.bytes.len().is_multiple_of(4)
        || payload.bytes.len() / 4 > MAX_TARGETS
    {
        return Err("X11 clipboard TARGETS is not a bounded ATOM array".into());
    }
    let mut seen = HashSet::new();
    let mut result = vec![];
    for bytes in payload.bytes.as_chunks::<4>().0 {
        let atom = u32::from_ne_bytes(*bytes);
        if atom == x11rb::NONE {
            return Err("X11 clipboard TARGETS contains None".into());
        }
        if seen.insert(atom) {
            result.push(atom);
        }
    }
    Ok(result)
}
fn incr_estimate(payload: &Payload) -> Result<usize, String> {
    if payload.format != 32 || payload.bytes.len() != 4 {
        return Err("X11 clipboard INCR header is invalid".into());
    }
    Ok(u32::from_ne_bytes(payload.bytes.as_slice().try_into().map_err(err)?) as usize)
}
fn append_chunk(assembled: &mut Option<Payload>, chunk: Payload, cap: usize) -> Result<(), String> {
    if let Some(old) = assembled {
        if old.kind != chunk.kind || old.format != chunk.format {
            return Err("X11 clipboard INCR changed type or format".into());
        }
        if chunk.bytes.len() > cap.saturating_sub(old.bytes.len()) {
            return Err("X11 clipboard INCR aggregate exceeds bounds".into());
        }
        old.bytes.extend_from_slice(&chunk.bytes);
    } else {
        if chunk.bytes.len() > cap {
            return Err("X11 clipboard INCR exceeds bounds".into());
        }
        *assembled = Some(chunk);
    }
    Ok(())
}
fn checked_total(total: usize, next: usize) -> Result<usize, String> {
    total
        .checked_add(next)
        .filter(|n| *n <= MAX_TOTAL_BYTES)
        .ok_or_else(|| "X11 clipboard total preservation bound exceeded".into())
}

#[cfg(feature = "native-probe")]
#[path = "clipboard_fixture.rs"]
mod fixture;
#[cfg(feature = "native-probe")]
pub use fixture::run_clipboard_selftest;
#[cfg(feature = "native-probe")]
pub use fixture::run_keeper_parent_fixture;

#[path = "clipboard_owner.rs"]
mod owner;
pub use owner::{
    keeper_entrypoint, ClipboardLease, ClipboardOwner, ClipboardState, HandoffStatus,
    KeeperProcess, KeeperStatus, RestoreOutcome,
};

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(kind: u32, format: u8, bytes: &[u8]) -> Payload {
        Payload {
            kind,
            format,
            bytes: bytes.to_vec(),
        }
    }
    fn property(format: u8, bytes: &[u8]) -> GetPropertyReply {
        GetPropertyReply {
            format,
            type_: 11,
            value_len: (bytes.len() / (format as usize / 8).max(1)) as u32,
            value: bytes.to_vec(),
            ..Default::default()
        }
    }
    #[test]
    fn protocol_targets_are_not_converted_and_side_effects_are_rejected() {
        for n in [
            b"TARGETS".as_slice(),
            b"TIMESTAMP",
            b"MULTIPLE",
            b"SAVE_TARGETS",
        ] {
            assert_eq!(target_policy(n), TargetPolicy::Protocol);
        }
        for n in [
            b"DELETE".as_slice(),
            b"INSERT_SELECTION",
            b"INSERT_PROPERTY",
            b"INCR",
            b"application/x-private",
            b"UTF8_STRING\0",
        ] {
            assert_eq!(target_policy(n), TargetPolicy::Unsupported);
        }
        for n in [
            b"UTF8_STRING".as_slice(),
            b"COMPOUND_TEXT",
            b"text/html",
            b"image/png",
            b"text/plain;charset=UTF-8",
        ] {
            assert_eq!(target_policy(n), TargetPolicy::Data);
        }
    }
    #[test]
    fn preserves_native_8_16_32_properties_and_empty_typed_data() {
        for format in [8, 16, 32] {
            let bytes = [0, 255, 0, 127, 3, 4, 5, 6];
            let p = property_payload(property(format, &bytes), bytes.len()).unwrap();
            assert_eq!(p.format, format);
            assert_eq!(p.kind, 11);
            assert_eq!(p.bytes, bytes);
            assert!(property_payload(property(format, &[]), 0).is_ok());
        }
    }
    #[test]
    fn rejects_partial_oversized_missing_and_invalid_properties() {
        let mut partial = property(8, &[1, 2]);
        partial.bytes_after = 1;
        let mut missing = property(8, &[]);
        missing.type_ = 0;
        let mut misaligned = property(16, &[1, 2]);
        misaligned.value.push(3);
        let mut wrong_count = property(32, &[0; 4]);
        wrong_count.value_len = u32::MAX;
        for p in [partial, missing, misaligned, wrong_count, property(7, &[1])] {
            assert!(property_payload(p, MAX_FORMAT_BYTES).is_err());
        }
        assert!(property_payload(property(8, &[1, 2]), 1).is_err());
    }
    #[test]
    fn targets_are_bounded_typed_deduplicated_nonzero_atoms() {
        let bytes = [9u32, 10, 9]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            target_atoms(&payload(AtomEnum::ATOM.into(), 32, &bytes)).unwrap(),
            [9, 10]
        );
        for p in [
            payload(9, 32, &bytes),
            payload(AtomEnum::ATOM.into(), 8, &bytes),
            payload(AtomEnum::ATOM.into(), 32, &[]),
            payload(AtomEnum::ATOM.into(), 32, &[0; 4]),
            payload(AtomEnum::ATOM.into(), 32, &[1; (MAX_TARGETS + 1) * 4]),
        ] {
            assert!(target_atoms(&p).is_err());
        }
    }
    #[test]
    fn incr_header_requires_exact_single_32_bit_lower_bound() {
        assert_eq!(
            incr_estimate(&payload(9, 32, &17u32.to_ne_bytes())).unwrap(),
            17
        );
        assert!(incr_estimate(&payload(9, 8, &[1; 4])).is_err());
        assert!(incr_estimate(&payload(9, 32, &[1; 8])).is_err());
    }
    #[test]
    fn incr_never_changes_type_format_or_exceeds_total() {
        let mut p = None;
        append_chunk(&mut p, payload(11, 8, b"abc"), 6).unwrap();
        assert!(append_chunk(&mut p, payload(12, 8, b"x"), 6).is_err());
        assert!(append_chunk(&mut p, payload(11, 16, b"xy"), 6).is_err());
        assert!(append_chunk(&mut p, payload(11, 8, b"defg"), 6).is_err());
        append_chunk(&mut p, payload(11, 8, b"def"), 6).unwrap();
        append_chunk(&mut p, payload(11, 8, b""), 6).unwrap();
        assert_eq!(p.unwrap().bytes, b"abcdef");
        assert!(append_chunk(&mut None, payload(11, 8, b"x"), 0).is_err());
    }
    #[test]
    fn aggregate_limit_and_integer_overflow_fail_closed() {
        assert_eq!(
            checked_total(MAX_TOTAL_BYTES - 1, 1).unwrap(),
            MAX_TOTAL_BYTES
        );
        assert!(checked_total(MAX_TOTAL_BYTES, 1).is_err());
        assert!(checked_total(usize::MAX, 1).is_err());
    }
    #[test]
    fn cancellation_and_deadline_are_checked_without_native_input() {
        assert!(
            check_deadline(Instant::now() + Duration::from_secs(1), &mut || Err(
                "cancelled".into()
            ))
            .is_err()
        );
        assert!(check_deadline(Instant::now() - Duration::from_millis(1), &mut || Ok(())).is_err());
    }
}
