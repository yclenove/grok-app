//! Pre-created mailbox slots. Publish renames onto a placeholder.
//!
//! The volume root holds `mailbox/{host-write,guest-write}` and a sibling
//! `host-stage`. Only `mailbox` is exported. Guest `.msg.tmp` names are
//! allowed and are left for the publisher to create.
//!
//! A zero-length file is an empty slot ([`SlotBody::Pending`]), not an error.
//! `ENOENT` and an identity change during the read are transient and counted.
//! Malformed bytes, a torn size, and an over-cap object stick on that slot.

#[cfg(test)]
use std::cell::RefCell;

use super::super::deadline::{HostClock, MonoInstant};
use super::codec::decode_frame;
use super::ops::{Envelope, MailboxReply, Op};
use super::seq::{Observe, SeqTracker};
use super::{FailedRead, MailboxError};

#[cfg(unix)]
use super::super::schedule::POLL_BYTE_BUDGET;
#[cfg(unix)]
use super::ops::FRAME_MAX;
#[cfg(unix)]
use super::Breach;

#[cfg(unix)]
use super::sys::{self, FileId, Kind, SysErr};
#[cfg(unix)]
use std::ffi::{CStr, CString};
#[cfg(unix)]
use std::os::fd::OwnedFd;

/// Export directory under the volume root. The only guest-visible tree.
pub const MAILBOX_DIR: &str = "mailbox";

/// Host message directory inside [`MAILBOX_DIR`].
pub const HOST_WRITE_DIR: &str = "host-write";

/// Guest message directory inside [`MAILBOX_DIR`].
pub const GUEST_WRITE_DIR: &str = "guest-write";

/// Host staging directory. Not part of the exported guest tree.
#[cfg_attr(any(not(test), not(unix)), allow(dead_code))]
pub(super) const HOST_STAGE_DIR: &str = "host-stage";

/// One host-write name. `binding.msg` is published at most once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSlot {
    /// Raw binding bytes. Not an operation envelope.
    Binding,
    /// One of the seven operation slots.
    Op(Op),
}

/// A host or guest message name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRef {
    /// `host-write`.
    Host(HostSlot),
    /// `guest-write` operation slot. The guest tmp names are not read here.
    Guest(Op),
}

/// Recorded directory and host-file identities. Holds no descriptors.
///
/// Counters and a sticky failure are kept here, one row per [`SlotRef`].
/// [`prepare_slots`] starts them at zero. Nothing else replaces or clears them.
#[derive(Debug)]
pub struct Layout {
    #[cfg(unix)]
    inner: Inner,
    watches: [Watch; SLOT_COUNT],
}

const SLOT_COUNT: usize = 15;

#[derive(Debug, Clone, Copy)]
struct Watch {
    retry_absent: u64,
    retry_replaced: u64,
    stuck: bool,
}

impl Watch {
    const fn fresh() -> Self {
        Self {
            retry_absent: 0,
            retry_replaced: 0,
            stuck: false,
        }
    }
}

impl Layout {
    /// Absent retries for `slot` since this attempt was prepared.
    #[must_use]
    pub fn retry_absent(&self, slot: SlotRef) -> u64 {
        self.watches[slot_index(slot)].retry_absent
    }

    /// Replaced retries for `slot` since this attempt was prepared.
    #[must_use]
    pub fn retry_replaced(&self, slot: SlotRef) -> u64 {
        self.watches[slot_index(slot)].retry_replaced
    }
}

fn slot_index(slot: SlotRef) -> usize {
    match slot {
        SlotRef::Host(HostSlot::Binding) => 0,
        SlotRef::Host(HostSlot::Op(op)) => 1 + op.index(),
        SlotRef::Guest(op) => 8 + op.index(),
    }
}

/// Bytes after the structural checks, or an empty slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotBody {
    /// Zero-length, missing, or replaced during the read.
    Pending,
    /// Stable bytes, at most 4096.
    Frame(Vec<u8>),
}

/// One structural read, including the counters after this call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRead {
    /// Pending or a frame.
    pub body: SlotBody,
    /// Absent retries for this slot after the call.
    pub retry_absent: u64,
    /// Replaced retries for this slot after the call.
    pub retry_replaced: u64,
    /// Bytes read by this call, including replaced rereads and the stable frame.
    pub bytes: usize,
}

/// A read that also decoded an operation frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckedRead {
    /// Nothing new to decode.
    Pending {
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// A closed envelope.
    Message {
        /// Decoded frame. Boxed so this enum stays smaller than the envelope.
        envelope: Box<Envelope>,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
    /// Raw `binding.msg` bytes. They are not an envelope.
    Binding {
        /// Published bytes.
        bytes: Vec<u8>,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
    },
}

/// A guest poll joined with the sequence tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestPoll {
    /// Still waiting.
    Pending {
        /// Slot that was read.
        op: Op,
        /// Request still outstanding. Zero if none was sent.
        request_seq: u64,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
        /// Bytes read by this poll, including replaced rereads.
        bytes: usize,
    },
    /// Old lease ACK. The deadline was not moved.
    DiscardedLease {
        /// Discarded lease sequence.
        seq: u64,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
        /// Bytes read by this poll, including replaced rereads.
        bytes: usize,
    },
    /// New accepted or terminal reply.
    Ready {
        /// Normalized reply.
        reply: MailboxReply,
        /// Absent retries so far.
        retry_absent: u64,
        /// Replaced retries so far.
        retry_replaced: u64,
        /// Bytes read by this poll, including replaced rereads and this frame.
        bytes: usize,
    },
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct Inner {
    root_dev: u64,
    root_ino: u64,
    fsid: [u8; 8],
    mailbox: DirId,
    host_write: DirId,
    guest_write: DirId,
    host_stage: DirId,
    /// Placeholder, then the file last published over it.
    ///
    /// The inode number alone is not enough: Linux can reuse it for the next
    /// create in this directory. Size and timestamps catch that substitution.
    host_file: [FileId; 8],
    binding_published: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct DirId {
    dev: u64,
    ino: u64,
}

/// Where a test replaces a file. One hook at a time. Not a channel flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) enum IoPoint {
    /// After the source `fsync`, before the source identity check.
    AfterFsync,
    /// After a non-empty read, before the second `fstat`.
    AfterRead,
    /// After a message directory is listed, before its entries are checked.
    Listed,
}

#[cfg(test)]
type HookFn = Box<dyn FnMut()>;
#[cfg(test)]
type ArmedHook = Option<(IoPoint, HookFn)>;

#[cfg(test)]
thread_local! {
    static HOOK: RefCell<ArmedHook> = const { RefCell::new(None) };
}

/// Clears the hook on drop.
#[cfg(test)]
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) struct Interrupt;

/// Arm `hook` for the next [`IoPoint`]. The hook must not re-enter.
#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(super) fn arm(point: IoPoint, hook: impl FnMut() + 'static) -> Interrupt {
    HOOK.with(|cell| *cell.borrow_mut() = Some((point, Box::new(hook))));
    Interrupt
}

#[cfg(test)]
impl Drop for Interrupt {
    fn drop(&mut self) {
        HOOK.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn hit(point: IoPoint) {
    #[cfg(test)]
    HOOK.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((armed, hook)) = slot.as_mut() {
            if *armed == point {
                hook();
            }
        }
    });
    #[cfg(not(test))]
    {
        let _ = point;
    }
}

/// Create `mailbox`, both message directories, `host-stage`, and the read slots.
///
/// `root` is an already-checked volume-root descriptor. It is not closed.
/// Guest `.msg.tmp` names are not created.
#[cfg_attr(not(unix), allow(clippy::needless_return))]
pub fn prepare_slots(root: i32) -> Result<Layout, MailboxError> {
    #[cfg(not(unix))]
    {
        let _ = root;
        return Err(MailboxError::Unsupported);
    }
    #[cfg(unix)]
    {
        prepare_unix(root)
    }
}

/// Replace one host placeholder with `frame`. The source FD dies with this call.
#[cfg_attr(not(unix), allow(clippy::needless_return))]
pub fn publish(
    root: i32,
    layout: &mut Layout,
    slot: HostSlot,
    frame: &[u8],
) -> Result<(), MailboxError> {
    #[cfg(not(unix))]
    {
        let _ = (root, layout, slot, frame);
        return Err(MailboxError::Unsupported);
    }
    #[cfg(unix)]
    {
        publish_unix(root, &mut layout.inner, slot, frame)
    }
}

/// Read one pre-created slot. Pending is `Ok`. A breach is [`FailedRead`].
///
/// A sticky failure stays on `layout` for this `slot`. The read has no
/// earlier deadline than the byte budget: see [`read_slot_until`].
#[cfg_attr(not(unix), allow(clippy::needless_return))]
pub fn read_slot(root: i32, layout: &mut Layout, slot: SlotRef) -> Result<SlotRead, FailedRead> {
    read_slot_until(root, layout, slot, &OpenClock, open_end())
}

/// [`read_slot`] bounded by `end` on `clock`.
///
/// Replacement retries stop when the next read would pass the remaining byte
/// budget or when `clock.now()` reaches `end`. Both limits are shared by
/// every try in the call, including the stable frame. A deadline miss is not
/// sticky. A byte-budget miss is.
#[cfg_attr(not(unix), allow(clippy::needless_return))]
pub fn read_slot_until<C: HostClock>(
    root: i32,
    layout: &mut Layout,
    slot: SlotRef,
    clock: &C,
    end: MonoInstant,
) -> Result<SlotRead, FailedRead> {
    #[cfg(not(unix))]
    {
        let _ = (root, layout, slot, clock, end);
        return Err(failed(MailboxError::Unsupported, &Watch::fresh(), 0));
    }
    #[cfg(unix)]
    {
        let index = slot_index(slot);
        let (inner, watches) = parts(layout);
        let watch = &mut watches[index];
        if watch.stuck {
            // No IO on this call, so it charges nothing.
            return Err(failed(MailboxError::Sticky, watch, 0));
        }
        match read_body(root, inner, slot, watch, clock, end) {
            Ok((body, bytes)) => Ok(finished(body, watch, bytes)),
            Err((error, bytes)) => Err(failed(error, watch, bytes)),
        }
    }
}

/// Clock that never reaches [`open_end`]. Tests and callers without a
/// deadline still share the byte budget.
#[derive(Debug, Clone, Copy)]
struct OpenClock;

impl HostClock for OpenClock {
    fn now(&self) -> MonoInstant {
        MonoInstant::EPOCH
    }
}

fn open_end() -> MonoInstant {
    MonoInstant::from_nanos(u64::MAX)
}

#[cfg(unix)]
fn parts(layout: &mut Layout) -> (&Inner, &mut [Watch; SLOT_COUNT]) {
    (&layout.inner, &mut layout.watches)
}

/// [`read_slot`] plus a decode. A codec failure sticks.
pub fn read_checked(
    root: i32,
    layout: &mut Layout,
    slot: SlotRef,
) -> Result<CheckedRead, FailedRead> {
    let read = read_slot(root, layout, slot)?;
    match read.body {
        SlotBody::Pending => Ok(CheckedRead::Pending {
            retry_absent: read.retry_absent,
            retry_replaced: read.retry_replaced,
        }),
        SlotBody::Frame(bytes) if matches!(slot, SlotRef::Host(HostSlot::Binding)) => {
            Ok(CheckedRead::Binding {
                bytes,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            })
        }
        SlotBody::Frame(bytes) => match decode_frame(&bytes) {
            Ok(envelope) => Ok(CheckedRead::Message {
                envelope: Box::new(envelope),
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
            }),
            Err(error) => {
                let watch = &mut layout.watches[slot_index(slot)];
                watch.stuck = true;
                Err(failed(error, watch, read.bytes))
            }
        },
    }
}

/// Read a guest slot and apply [`SeqTracker::observe`]. Codec and sequence errors stick.
pub fn poll_guest(
    root: i32,
    layout: &mut Layout,
    op: Op,
    tracker: &mut SeqTracker,
) -> Result<GuestPoll, FailedRead> {
    poll_guest_until(root, layout, op, tracker, &OpenClock, open_end())
}

/// [`poll_guest`] bounded by `end` on `clock`. Replacement retries share that end.
pub fn poll_guest_until<C: HostClock>(
    root: i32,
    layout: &mut Layout,
    op: Op,
    tracker: &mut SeqTracker,
    clock: &C,
    end: MonoInstant,
) -> Result<GuestPoll, FailedRead> {
    let read = read_slot_until(root, layout, SlotRef::Guest(op), clock, end)?;
    let bytes = read.bytes;
    match read.body {
        SlotBody::Pending => Ok(GuestPoll::Pending {
            op,
            request_seq: tracker.outstanding(op),
            retry_absent: read.retry_absent,
            retry_replaced: read.retry_replaced,
            bytes,
        }),
        SlotBody::Frame(frame) => match tracker.observe(op, &frame) {
            Ok(Observe::Pending { op, request_seq }) => Ok(GuestPoll::Pending {
                op,
                request_seq,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
                bytes,
            }),
            Ok(Observe::DiscardedLease { seq }) => Ok(GuestPoll::DiscardedLease {
                seq,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
                bytes,
            }),
            Ok(Observe::Reply(reply)) => Ok(GuestPoll::Ready {
                reply,
                retry_absent: read.retry_absent,
                retry_replaced: read.retry_replaced,
                bytes,
            }),
            Err(error) => {
                let watch = &mut layout.watches[slot_index(SlotRef::Guest(op))];
                watch.stuck = true;
                Err(failed(error, watch, bytes))
            }
        },
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn finished(body: SlotBody, watch: &Watch, bytes: usize) -> SlotRead {
    SlotRead {
        body,
        retry_absent: watch.retry_absent,
        retry_replaced: watch.retry_replaced,
        bytes,
    }
}

fn failed(error: MailboxError, watch: &Watch, bytes: usize) -> FailedRead {
    FailedRead {
        error,
        retry_absent: watch.retry_absent,
        retry_replaced: watch.retry_replaced,
        bytes,
    }
}

#[cfg(unix)]
const EMPTY_FILE: FileId = FileId {
    dev: 0,
    ino: 0,
    nlink: 0,
    size: 0,
    mtime_sec: 0,
    mtime_nsec: 0,
    ctime_sec: 0,
    ctime_nsec: 0,
    kind: Kind::Other,
};

#[cfg(unix)]
fn prepare_unix(root: i32) -> Result<Layout, MailboxError> {
    let root_id = sys::fstat(root).map_err(map_sys)?;
    if root_id.kind != Kind::Dir {
        return Err(Breach::NotDirectory.into());
    }
    let fsid = sys::fsid(root).map_err(map_sys)?;
    let (mailbox, mailbox_fd) = make_dir(root, c"mailbox", root_id.dev, fsid)?;
    let (host_write, host_fd) =
        make_dir(sys::raw_fd(&mailbox_fd), c"host-write", root_id.dev, fsid)?;
    let (guest_write, guest_fd) =
        make_dir(sys::raw_fd(&mailbox_fd), c"guest-write", root_id.dev, fsid)?;
    let (host_stage, stage_fd) = make_dir(root, c"host-stage", root_id.dev, fsid)?;
    let mut host_file = [EMPTY_FILE; 8];
    for (index, name) in host_names().iter().enumerate() {
        let id = make_file(sys::raw_fd(&host_fd), name, root_id.dev, fsid)?;
        *host_file
            .get_mut(index)
            .ok_or(MailboxError::Breach(Breach::Identity))? = id;
    }
    for name in guest_msgs() {
        let _id = make_file(sys::raw_fd(&guest_fd), name, root_id.dev, fsid)?;
    }
    sys::fsync_fd(sys::raw_fd(&host_fd)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&guest_fd)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&mailbox_fd)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&stage_fd)).map_err(map_sys)?;
    sys::fsync_fd(root).map_err(map_sys)?;
    Ok(Layout {
        inner: Inner {
            root_dev: root_id.dev,
            root_ino: root_id.ino,
            fsid,
            mailbox,
            host_write,
            guest_write,
            host_stage,
            host_file,
            binding_published: false,
        },
        watches: [Watch::fresh(); SLOT_COUNT],
    })
}

#[cfg(unix)]
fn publish_unix(
    root: i32,
    layout: &mut Inner,
    slot: HostSlot,
    frame: &[u8],
) -> Result<(), MailboxError> {
    let len = u64::try_from(frame.len()).map_err(|_| MailboxError::Breach(Breach::FrameSize))?;
    if len == 0 {
        return Err(Breach::Empty.into());
    }
    if frame.len() > FRAME_MAX {
        return Err(Breach::FrameSize.into());
    }
    let index = host_index(slot);
    if matches!(slot, HostSlot::Binding) && layout.binding_published {
        return Err(Breach::BindingFrozen.into());
    }
    check_root(root, layout)?;
    let mailbox = open_recorded(root, c"mailbox", layout.mailbox, layout)?;
    let stage = open_recorded(root, c"host-stage", layout.host_stage, layout)?;
    let dest = open_recorded(
        sys::raw_fd(&mailbox),
        c"host-write",
        layout.host_write,
        layout,
    )?;
    let dirs = PublishDirs {
        root,
        mailbox: &mailbox,
        stage: &stage,
        dest: &dest,
    };
    let file_name = host_names()
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    let tmp_name = host_tmps()
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    let existing = stat_name(sys::raw_fd(&dest), file_name)?;
    accept_target(&existing, layout, index)?;
    let tmp = create_tmp(sys::raw_fd(&stage), tmp_name)?;
    let born = require_fresh(&tmp, layout)?;
    sys::write_all(sys::raw_fd(&tmp), frame).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&tmp)).map_err(map_sys)?;
    hit(IoPoint::AfterFsync);
    if !source_intact(&dirs, &tmp, tmp_name, &born, len, layout)? {
        return Err(Breach::Identity.into());
    }
    // The write and fsync window is closed only by this check, immediately
    // before the rename. An absent or substituted target is still a breach.
    confirm_publish(&dirs, layout)?;
    let again = stat_name(sys::raw_fd(&dest), file_name)?;
    accept_target(&again, layout, index)?;
    sys::rename_at(sys::raw_fd(&stage), tmp_name, sys::raw_fd(&dest), file_name)
        .map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&dest)).map_err(map_sys)?;
    sys::fsync_fd(sys::raw_fd(&stage)).map_err(map_sys)?;
    confirm_publish(&dirs, layout)?;
    if !published_intact(&dirs, &tmp, file_name, &born, len, layout)? {
        return Err(Breach::Identity.into());
    }
    let published = sys::fstat(sys::raw_fd(&tmp)).map_err(map_sys)?;
    let recorded = layout
        .host_file
        .get_mut(index)
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    *recorded = published;
    if matches!(slot, HostSlot::Binding) {
        layout.binding_published = true;
    }
    Ok(())
}

#[cfg(unix)]
fn with_bytes<T>(
    result: Result<T, MailboxError>,
    charged: usize,
) -> Result<T, (MailboxError, usize)> {
    result.map_err(|err| (err, charged))
}

/// One bounded read. Every try, including the stable frame, shares `end` and
/// the bytes still inside [`POLL_BYTE_BUDGET`]. The next try is refused before
/// the read when the object's size would pass that remainder. `ENOENT` stays
/// one pending result. A deadline miss is not sticky.
#[cfg(unix)]
fn read_body<C: HostClock>(
    root: i32,
    layout: &Inner,
    slot: SlotRef,
    watch: &mut Watch,
    clock: &C,
    end: MonoInstant,
) -> Result<(SlotBody, usize), (MailboxError, usize)> {
    let mut charged = 0usize;
    let mut turns = 0usize;
    loop {
        if clock.now() >= end {
            return Err((MailboxError::Deadline, charged));
        }
        turns = turns.saturating_add(1);
        if turns > POLL_BYTE_BUDGET {
            watch.stuck = true;
            return Err((MailboxError::PollBudget, charged));
        }
        let remaining = POLL_BYTE_BUDGET.saturating_sub(charged);
        if remaining == 0 {
            watch.stuck = true;
            return Err((MailboxError::PollBudget, charged));
        }
        with_bytes(check_root(root, layout), charged)?;
        let mailbox = with_bytes(
            open_recorded(root, c"mailbox", layout.mailbox, layout),
            charged,
        )?;
        with_bytes(
            preflight(sys::raw_fd(&mailbox), &mailbox_allow(), Kind::Dir),
            charged,
        )?;
        let (dir_name, expect, file_name) = with_bytes(locate(slot, layout), charged)?;
        let dir = with_bytes(
            open_recorded(sys::raw_fd(&mailbox), dir_name, expect, layout),
            charged,
        )?;
        match slot {
            SlotRef::Host(_) => with_bytes(
                preflight(sys::raw_fd(&dir), &host_names(), Kind::File),
                charged,
            )?,
            SlotRef::Guest(_) => with_bytes(
                preflight(sys::raw_fd(&dir), &guest_allow(), Kind::File),
                charged,
            )?,
        }
        let chain = ReadChain {
            root,
            mailbox: &mailbox,
            dir: &dir,
            dir_name,
            dir_id: expect,
        };
        let pre = match sys::fstat_at(sys::raw_fd(&dir), file_name) {
            Ok(id) => id,
            Err(SysErr::NoEnt) => {
                return match absent(&chain, layout, watch) {
                    Ok(body) => Ok((body, charged)),
                    Err(err) => Err((err, charged)),
                };
            }
            Err(SysErr::Loop) => return Err((Breach::Link.into(), charged)),
            Err(err) => return Err((map_sys(err), charged)),
        };
        if pre.kind == Kind::Link {
            return Err((Breach::Link.into(), charged));
        }
        let fd = match sys::open_read(sys::raw_fd(&dir), file_name) {
            Ok(fd) => fd,
            Err(SysErr::NoEnt) => {
                return match absent(&chain, layout, watch) {
                    Ok(body) => Ok((body, charged)),
                    Err(err) => Err((err, charged)),
                };
            }
            Err(SysErr::Loop) => return Err((Breach::Link.into(), charged)),
            Err(SysErr::Special) => return Err((Breach::NotRegular.into(), charged)),
            Err(err) => return Err((map_sys(err), charged)),
        };
        let opened = with_bytes(checked_file(&fd, layout), charged)?;
        if opened.size == 0 {
            let held = with_bytes(stable(&chain, &fd, file_name, &opened, layout), charged)?;
            if held {
                return Ok((SlotBody::Pending, charged));
            }
            with_bytes(replaced(watch), charged)?;
            continue;
        }
        let cap = with_bytes(
            u64::try_from(FRAME_MAX).map_err(|_| MailboxError::Breach(Breach::FrameSize)),
            charged,
        )?;
        if opened.size > cap {
            let held = with_bytes(stable(&chain, &fd, file_name, &opened, layout), charged)?;
            if !held {
                with_bytes(replaced(watch), charged)?;
                continue;
            }
            watch.stuck = true;
            return Err((Breach::Size.into(), charged));
        }
        let remaining_u = u64::try_from(remaining).unwrap_or(u64::MAX);
        if opened.size > remaining_u {
            watch.stuck = true;
            return Err((MailboxError::PollBudget, charged));
        }
        let read_cap = remaining.min(FRAME_MAX.saturating_add(1));
        if clock.now() >= end {
            return Err((MailboxError::Deadline, charged));
        }
        let mut bytes = match sys::read_capped(sys::raw_fd(&fd), read_cap) {
            Ok(bytes) => bytes,
            Err((err, partial)) => {
                return Err((map_sys(err), charged.saturating_add(partial.min(read_cap))));
            }
        };
        if bytes.len() > read_cap {
            bytes.truncate(read_cap);
        }
        if !bytes.is_empty() {
            hit(IoPoint::AfterRead);
        }
        let total = charged.saturating_add(bytes.len());
        let held = with_bytes(stable(&chain, &fd, file_name, &opened, layout), total)?;
        if !held {
            with_bytes(replaced(watch), total)?;
            charged = total;
            continue;
        }
        // The stable try can itself run past the shared end; do not hand back
        // a late frame. Not sticky: a later call with a fresh end can read it.
        if clock.now() >= end {
            return Err((MailboxError::Deadline, total));
        }
        let got = with_bytes(
            u64::try_from(bytes.len()).map_err(|_| MailboxError::Io),
            total,
        )?;
        if got != opened.size {
            watch.stuck = true;
            return Err((Breach::Torn.into(), total));
        }
        return Ok((SlotBody::Frame(bytes), total));
    }
}

#[cfg(unix)]
fn make_dir(
    parent: i32,
    name: &CStr,
    dev: u64,
    fsid: [u8; 8],
) -> Result<(DirId, OwnedFd), MailboxError> {
    match sys::mkdir_excl(parent, name) {
        Ok(()) => {}
        Err(SysErr::Exist) => return Err(Breach::Exists.into()),
        Err(err) => return Err(map_sys(err)),
    }
    let fd = sys::open_dir(parent, name).map_err(map_sys)?;
    let id = sys::fstat(sys::raw_fd(&fd)).map_err(map_sys)?;
    if id.kind != Kind::Dir || id.dev != dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(&fd), fsid)?;
    Ok((
        DirId {
            dev: id.dev,
            ino: id.ino,
        },
        fd,
    ))
}

#[cfg(unix)]
fn make_file(dir: i32, name: &CStr, dev: u64, fsid: [u8; 8]) -> Result<FileId, MailboxError> {
    let fd = match sys::open_create(dir, name) {
        Ok(fd) => fd,
        Err(SysErr::Exist) => return Err(Breach::Exists.into()),
        Err(err) => return Err(map_sys(err)),
    };
    let id = sys::fstat(sys::raw_fd(&fd)).map_err(map_sys)?;
    if id.kind != Kind::File || id.nlink != 1 || id.size != 0 || id.dev != dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(&fd), fsid)?;
    Ok(id)
}

#[cfg(unix)]
fn check_root(root: i32, layout: &Inner) -> Result<(), MailboxError> {
    let id = sys::fstat(root).map_err(map_sys)?;
    if id.kind != Kind::Dir || id.dev != layout.root_dev || id.ino != layout.root_ino {
        return Err(Breach::Identity.into());
    }
    same_fs(root, layout.fsid)
}

#[cfg(unix)]
fn open_recorded(
    parent: i32,
    name: &CStr,
    expect: DirId,
    layout: &Inner,
) -> Result<OwnedFd, MailboxError> {
    let fd = match sys::open_dir(parent, name) {
        Ok(fd) => fd,
        Err(SysErr::Loop) => return Err(Breach::Link.into()),
        Err(SysErr::NotDir) => return Err(Breach::NotDirectory.into()),
        Err(err) => return Err(map_sys(err)),
    };
    confirm_named(parent, name, &fd, expect, layout)?;
    Ok(fd)
}

#[cfg(unix)]
struct PublishDirs<'a> {
    root: i32,
    mailbox: &'a OwnedFd,
    stage: &'a OwnedFd,
    dest: &'a OwnedFd,
}

#[cfg(unix)]
struct ReadChain<'a> {
    root: i32,
    mailbox: &'a OwnedFd,
    dir: &'a OwnedFd,
    dir_name: &'static CStr,
    dir_id: DirId,
}

/// The name in `parent` must still be the recorded directory and this FD.
#[cfg(unix)]
fn confirm_named(
    parent: i32,
    name: &CStr,
    fd: &OwnedFd,
    expect: DirId,
    layout: &Inner,
) -> Result<(), MailboxError> {
    let named = match sys::fstat_at(parent, name) {
        Ok(id) => id,
        Err(SysErr::NoEnt) => return Err(Breach::Identity.into()),
        Err(SysErr::Loop) => return Err(Breach::Link.into()),
        Err(err) => return Err(map_sys(err)),
    };
    if named.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    if named.kind != Kind::Dir {
        return Err(Breach::NotDirectory.into());
    }
    let opened = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if opened.kind != Kind::Dir
        || opened.dev != expect.dev
        || opened.ino != expect.ino
        || named.dev != opened.dev
        || named.ino != opened.ino
    {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(fd), layout.fsid)
}

#[cfg(unix)]
fn confirm_publish(dirs: &PublishDirs<'_>, layout: &Inner) -> Result<(), MailboxError> {
    check_root(dirs.root, layout)?;
    confirm_named(dirs.root, c"mailbox", dirs.mailbox, layout.mailbox, layout)?;
    confirm_named(
        dirs.root,
        c"host-stage",
        dirs.stage,
        layout.host_stage,
        layout,
    )?;
    confirm_named(
        sys::raw_fd(dirs.mailbox),
        c"host-write",
        dirs.dest,
        layout.host_write,
        layout,
    )
}

#[cfg(unix)]
fn confirm_read(chain: &ReadChain<'_>, layout: &Inner) -> Result<(), MailboxError> {
    check_root(chain.root, layout)?;
    confirm_named(
        chain.root,
        c"mailbox",
        chain.mailbox,
        layout.mailbox,
        layout,
    )?;
    confirm_named(
        sys::raw_fd(chain.mailbox),
        chain.dir_name,
        chain.dir,
        chain.dir_id,
        layout,
    )
}

#[cfg(unix)]
fn same_fs(fd: i32, expect: [u8; 8]) -> Result<(), MailboxError> {
    let got = sys::fsid(fd).map_err(map_sys)?;
    if got == expect {
        Ok(())
    } else {
        Err(Breach::Fsid.into())
    }
}

#[cfg(unix)]
fn stat_name(dir: i32, name: &CStr) -> Result<FileId, MailboxError> {
    match sys::fstat_at(dir, name) {
        Ok(id) => Ok(id),
        Err(SysErr::NoEnt) => Err(Breach::Identity.into()),
        Err(SysErr::Loop) => Err(Breach::Link.into()),
        Err(err) => Err(map_sys(err)),
    }
}

#[cfg(unix)]
fn accept_target(id: &FileId, layout: &Inner, index: usize) -> Result<(), MailboxError> {
    if id.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    if id.kind != Kind::File {
        return Err(Breach::NotRegular.into());
    }
    if id.nlink != 1 {
        return Err(Breach::Nlink.into());
    }
    if id.dev != layout.root_dev {
        return Err(Breach::Dev.into());
    }
    let expect = layout
        .host_file
        .get(index)
        .copied()
        .ok_or(MailboxError::Breach(Breach::Identity))?;
    if same_file(id, &expect) {
        Ok(())
    } else {
        Err(Breach::Identity.into())
    }
}

#[cfg(unix)]
fn create_tmp(dir: i32, name: &CStr) -> Result<OwnedFd, MailboxError> {
    match sys::open_create(dir, name) {
        Ok(fd) => Ok(fd),
        Err(SysErr::Exist) => Err(Breach::TmpExists.into()),
        Err(SysErr::Loop) => Err(Breach::Link.into()),
        Err(err) => Err(map_sys(err)),
    }
}

#[cfg(unix)]
fn require_fresh(fd: &OwnedFd, layout: &Inner) -> Result<FileId, MailboxError> {
    let id = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if id.kind != Kind::File || id.nlink != 1 || id.size != 0 || id.dev != layout.root_dev {
        return Err(Breach::Identity.into());
    }
    same_fs(sys::raw_fd(fd), layout.fsid)?;
    Ok(id)
}

#[cfg(unix)]
fn source_intact(
    dirs: &PublishDirs<'_>,
    tmp: &OwnedFd,
    name: &CStr,
    born: &FileId,
    len: u64,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    confirm_publish(dirs, layout)?;
    let after = sys::fstat(sys::raw_fd(tmp)).map_err(map_sys)?;
    let named = match sys::fstat_at(sys::raw_fd(dirs.stage), name) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    same_fs(sys::raw_fd(tmp), layout.fsid)?;
    Ok(same_object(&after, born)
        && after.nlink == 1
        && after.kind == Kind::File
        && after.size == len
        && same_file(&named, &after))
}

#[cfg(unix)]
fn published_intact(
    dirs: &PublishDirs<'_>,
    tmp: &OwnedFd,
    name: &CStr,
    born: &FileId,
    len: u64,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    confirm_publish(dirs, layout)?;
    let named = match sys::fstat_at(sys::raw_fd(dirs.dest), name) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    let still = sys::fstat(sys::raw_fd(tmp)).map_err(map_sys)?;
    Ok(same_object(&still, born)
        && still.nlink == 1
        && still.size == len
        && same_file(&named, &still))
}

#[cfg(unix)]
fn checked_file(fd: &OwnedFd, layout: &Inner) -> Result<FileId, MailboxError> {
    let id = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    if id.kind == Kind::Link {
        return Err(Breach::Link.into());
    }
    if id.kind != Kind::File {
        return Err(Breach::NotRegular.into());
    }
    if id.nlink != 1 {
        return Err(Breach::Nlink.into());
    }
    if id.dev != layout.root_dev {
        return Err(Breach::Dev.into());
    }
    same_fs(sys::raw_fd(fd), layout.fsid)?;
    Ok(id)
}

#[cfg(unix)]
fn stable(
    chain: &ReadChain<'_>,
    fd: &OwnedFd,
    name: &CStr,
    before: &FileId,
    layout: &Inner,
) -> Result<bool, MailboxError> {
    confirm_read(chain, layout)?;
    let after = sys::fstat(sys::raw_fd(fd)).map_err(map_sys)?;
    let named = match sys::fstat_at(sys::raw_fd(chain.dir), name) {
        Ok(id) => id,
        Err(SysErr::NoEnt) => return Ok(false),
        Err(err) => return Err(map_sys(err)),
    };
    Ok(same_file(before, &after) && named.dev == after.dev && named.ino == after.ino)
}

#[cfg(unix)]
fn absent(
    chain: &ReadChain<'_>,
    layout: &Inner,
    watch: &mut Watch,
) -> Result<SlotBody, MailboxError> {
    confirm_read(chain, layout)?;
    bump(&mut watch.retry_absent)?;
    Ok(SlotBody::Pending)
}

#[cfg(unix)]
fn replaced(watch: &mut Watch) -> Result<SlotBody, MailboxError> {
    bump(&mut watch.retry_replaced)?;
    Ok(SlotBody::Pending)
}

#[cfg(unix)]
fn bump(counter: &mut u64) -> Result<(), MailboxError> {
    *counter = counter.checked_add(1).ok_or(MailboxError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn locate(
    slot: SlotRef,
    layout: &Inner,
) -> Result<(&'static CStr, DirId, &'static CStr), MailboxError> {
    match slot {
        SlotRef::Host(host) => {
            let index = host_index(host);
            let name = host_names()
                .get(index)
                .copied()
                .ok_or(MailboxError::Breach(Breach::Identity))?;
            Ok((c"host-write", layout.host_write, name))
        }
        SlotRef::Guest(op) => {
            let name = guest_msgs()
                .get(op.index())
                .copied()
                .ok_or(MailboxError::Breach(Breach::Identity))?;
            Ok((c"guest-write", layout.guest_write, name))
        }
    }
}

#[cfg(unix)]
fn same_object(id: &FileId, born: &FileId) -> bool {
    id.dev == born.dev && id.ino == born.ino
}

#[cfg(unix)]
fn same_file(left: &FileId, right: &FileId) -> bool {
    left.dev == right.dev
        && left.ino == right.ino
        && left.nlink == right.nlink
        && left.size == right.size
        && left.mtime_sec == right.mtime_sec
        && left.mtime_nsec == right.mtime_nsec
        && left.ctime_sec == right.ctime_sec
        && left.ctime_nsec == right.ctime_nsec
        && left.kind == right.kind
}

#[cfg(unix)]
fn host_index(slot: HostSlot) -> usize {
    match slot {
        HostSlot::Binding => 0,
        HostSlot::Op(op) => op.index().saturating_add(1),
    }
}

#[cfg(unix)]
fn map_sys(err: SysErr) -> MailboxError {
    match err {
        SysErr::NoEnt => Breach::Identity.into(),
        SysErr::Exist => Breach::Exists.into(),
        SysErr::Loop => Breach::Link.into(),
        SysErr::NotDir => Breach::NotDirectory.into(),
        SysErr::Special => Breach::NotRegular.into(),
        SysErr::Io => MailboxError::Io,
    }
}

#[cfg(unix)]
fn host_names() -> [&'static CStr; 8] {
    [
        c"binding.msg",
        c"inventory.msg",
        c"probe.msg",
        c"bootstrap.msg",
        c"login.msg",
        c"run.msg",
        c"lease.msg",
        c"revoke.msg",
    ]
}

#[cfg(unix)]
fn host_tmps() -> [&'static CStr; 8] {
    [
        c"binding.msg.tmp",
        c"inventory.msg.tmp",
        c"probe.msg.tmp",
        c"bootstrap.msg.tmp",
        c"login.msg.tmp",
        c"run.msg.tmp",
        c"lease.msg.tmp",
        c"revoke.msg.tmp",
    ]
}

#[cfg(unix)]
fn guest_msgs() -> [&'static CStr; 7] {
    [
        c"inventory.msg",
        c"probe.msg",
        c"bootstrap.msg",
        c"login.msg",
        c"run.msg",
        c"lease.msg",
        c"revoke.msg",
    ]
}

#[cfg(unix)]
fn mailbox_allow() -> [&'static CStr; 2] {
    [c"host-write", c"guest-write"]
}

#[cfg(unix)]
fn guest_allow() -> [&'static CStr; 14] {
    let msg = guest_msgs();
    let tmp = guest_tmps();
    [
        msg[0], msg[1], msg[2], msg[3], msg[4], msg[5], msg[6], tmp[0], tmp[1], tmp[2], tmp[3],
        tmp[4], tmp[5], tmp[6],
    ]
}

#[cfg(unix)]
fn preflight(dir: i32, allow: &[&CStr], expect_kind: Kind) -> Result<(), MailboxError> {
    let limit = allow.len().saturating_add(1);
    let names = sys::list_limited(dir, limit).map_err(map_sys)?;
    if names.len() > allow.len() {
        return Err(Breach::Identity.into());
    }
    if expect_kind == Kind::File {
        hit(IoPoint::Listed);
    }
    for name in names {
        if !allow.iter().any(|cand| cand.to_bytes() == name.as_slice()) {
            return Err(Breach::Identity.into());
        }
        let owned = CString::new(name).map_err(|_| MailboxError::Io)?;
        let id = match sys::fstat_at(dir, &owned) {
            Ok(id) => id,
            // A listed message entry may go away before this lookup: a guest
            // tmp renamed over its slot, or a transient virtiofs ENOENT. The
            // selected slot's own lookup counts absence as Pending. A missing
            // message directory stays a breach.
            Err(SysErr::NoEnt) if expect_kind == Kind::File => continue,
            Err(SysErr::NoEnt) => return Err(Breach::Identity.into()),
            Err(SysErr::Loop) => return Err(Breach::Link.into()),
            Err(err) => return Err(map_sys(err)),
        };
        if id.kind == Kind::Link {
            return Err(Breach::Link.into());
        }
        if id.kind != expect_kind {
            return Err(if expect_kind == Kind::Dir {
                Breach::NotDirectory.into()
            } else {
                Breach::NotRegular.into()
            });
        }
    }
    Ok(())
}

#[cfg(unix)]
fn guest_tmps() -> [&'static CStr; 7] {
    [
        c"inventory.msg.tmp",
        c"probe.msg.tmp",
        c"bootstrap.msg.tmp",
        c"login.msg.tmp",
        c"run.msg.tmp",
        c"lease.msg.tmp",
        c"revoke.msg.tmp",
    ]
}
