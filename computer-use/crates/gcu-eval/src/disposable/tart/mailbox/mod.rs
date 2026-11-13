//! Offline mailbox codec, slot publication, and sequence checks.
//!
//! This is not a [`super::guest::GuestChannel`] adapter. It does not start a
//! VM, mount a volume, or read the environment. A zero-length placeholder is
//! an empty slot, not an error: the guest only looks up names the host created
//! before boot. `ENOENT` and a replacement during a read are transient.
//! Malformed bytes stick.

mod codec;
mod error;
mod ids;
mod ops;
mod seq;
mod slots;
#[cfg(unix)]
#[path = "sys.rs"]
mod sys;
mod wire;

#[cfg(test)]
mod tests;

pub use codec::{decode_frame, decode_reply, encode_frame, encode_reply};
pub use error::{Breach, CodecFault, FailedRead, MailboxError, SeqFault};
pub use ids::{AsciiId, AttemptNonce};
pub use ops::{
    Activity, Envelope, InventoryConsume, InventorySnapshot, LeaseRequest, LeaseTerminal,
    LoginState, MailboxReply, MsgStatus, Op, Outcome, Payload, ReplyResult, ReplyState,
    RevokeOutcome, RevokeSlot, RunScore, SideOp, SideResult, SnapStatus, ValueOutcome, FRAME_MAX,
    LEASE_WINDOW_MS, MAX_JSON_ARRAY, MAX_JSON_DEPTH, MAX_POLLS,
};
pub use seq::{AttemptBinding, Observe, SeqTracker};
pub use slots::{
    poll_guest, prepare_slots, publish, read_checked, read_slot, CheckedRead, GuestPoll, HostSlot,
    Layout, SlotBody, SlotRead, SlotRef, GUEST_WRITE_DIR, HOST_WRITE_DIR, MAILBOX_DIR,
};
