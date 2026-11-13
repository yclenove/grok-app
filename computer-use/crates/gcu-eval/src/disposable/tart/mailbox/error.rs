//! Failures of the offline mailbox. Pending is not one of them.

/// Why a frame, slot, or sequence check failed.
///
/// [`Self::Sticky`] means the slot already failed on bytes or binding. The
/// caller stops. A later read of that slot does not clear it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MailboxError {
    /// This platform has no `openat` mailbox.
    #[error("mailbox is unsupported on this platform")]
    Unsupported,
    /// A filesystem call failed for a reason that is not a protocol breach.
    #[error("mailbox filesystem operation failed")]
    Io,
    /// The slot was not a single regular file the publisher may replace.
    #[error("mailbox slot breached the contract")]
    Breach(Breach),
    /// The frame bytes are not a closed mailbox message.
    #[error("mailbox frame was rejected")]
    Malformed(#[from] CodecFault),
    /// Sequence, boot, or binding rules failed.
    #[error("mailbox sequence or binding failed")]
    Sequence(SeqFault),
    /// This slot already failed. Stop the attempt.
    #[error("mailbox slot failed sticky; stop")]
    Sticky,
}

/// Why a directory entry was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Breach {
    /// The descriptor was not a directory.
    #[error("not a directory")]
    NotDirectory,
    /// `prepare_slots` found a name that already exists.
    #[error("mailbox name already exists")]
    Exists,
    /// The stage temporary name was already present.
    #[error("stage temporary already exists")]
    TmpExists,
    /// The name is a symlink. It was not followed.
    #[error("refusing a symlink")]
    Link,
    /// FIFO, directory, socket, or device. Not a regular file.
    #[error("refusing a non-regular file")]
    NotRegular,
    /// `nlink` was not 1.
    #[error("refusing a hard link")]
    Nlink,
    /// The file is larger than the 4096-byte frame cap.
    #[error("mailbox object exceeds 4096 bytes")]
    Size,
    /// Metadata stayed put, but the byte count did not match that size.
    #[error("mailbox object byte count does not match its stable size")]
    Torn,
    /// The object is not on the prepared device.
    #[error("mailbox object is on an unexpected device")]
    Dev,
    /// `f_fsid` does not match the prepared directory.
    #[error("mailbox object is on an unexpected filesystem")]
    Fsid,
    /// The name does not refer to the recorded placeholder or last published file.
    #[error("mailbox object identity does not match")]
    Identity,
    /// `binding.msg` was already published.
    #[error("binding was already published")]
    BindingFrozen,
    /// A publication has no bytes. An empty file is only a placeholder.
    #[error("refusing an empty publication")]
    Empty,
    /// The publication is larger than one frame.
    #[error("publication exceeds one frame")]
    FrameSize,
}

/// Why frame bytes were rejected before they became a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodecFault {
    /// The length prefix does not equal the following byte count.
    #[error("frame length does not match")]
    Length,
    /// The frame is larger than 4096 bytes.
    #[error("frame exceeds 4096 bytes")]
    TooLong,
    /// The JSON is not UTF-8.
    #[error("frame is not utf-8")]
    Utf8,
    /// Bytes follow the one JSON value.
    #[error("frame has trailing bytes")]
    Trailing,
    /// A key is outside the closed object.
    #[error("frame has an unknown field")]
    Unknown,
    /// An object key appears twice.
    #[error("frame repeats a field")]
    Duplicate,
    /// More than three nested containers.
    #[error("frame nests more than three containers")]
    Depth,
    /// An array has more than 16 elements.
    #[error("frame array has more than 16 elements")]
    Array,
    /// A number is negative.
    #[error("frame number is negative")]
    Negative,
    /// A number has a fractional part.
    #[error("frame number is fractional")]
    Fraction,
    /// A number uses an exponent.
    #[error("frame number uses an exponent")]
    Exponent,
    /// A number does not fit in `u64`.
    #[error("frame number overflows u64")]
    Overflow,
    /// A string is not a closed token or a 1..=64 byte ASCII identity.
    #[error("frame identity is not allowed")]
    Identity,
    /// A digest encoding is not 64 lowercase hex characters.
    #[error("frame digest has the wrong length")]
    DigestLen,
    /// The hex is not a [`super::super::manifest::Digest`].
    #[error("frame digest is not a digest")]
    Digest,
    /// The attempt nonce is not 32 lowercase hex characters.
    #[error("frame nonce is not a 128-bit nonce")]
    Nonce,
    /// `v` is not the guest channel version.
    #[error("frame version is not 1")]
    Version,
    /// The status and payload are not a closed pair for this op.
    #[error("frame status and payload do not match")]
    Shape,
    /// The JSON is not the closed schema.
    #[error("frame schema was rejected")]
    Schema,
}

/// Why a request or reply failed the sequence rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SeqFault {
    /// The op sequence was not the next integer starting at 1.
    #[error("sequence is not the next value")]
    NotConsecutive,
    /// The next sequence would wrap past `u64::MAX`.
    #[error("sequence would wrap")]
    Wrapped,
    /// `request_seq` does not echo `seq`.
    #[error("reply does not echo the request sequence")]
    Echo,
    /// Run, attempt nonce, or VM digest does not match.
    #[error("binding does not match")]
    Binding,
    /// Boot was missing, early, or different.
    #[error("boot binding does not match")]
    Boot,
    /// The same sequence was published again with different bytes.
    #[error("sequence was reused with different bytes")]
    Changed,
    /// The frame's op is not the slot that was read.
    #[error("reply op does not match the slot")]
    WrongOp,
    /// The sequence has not been sent yet.
    #[error("sequence is in the future")]
    Future,
    /// Sequence 0 was not issued.
    #[error("sequence was not sent")]
    Unsent,
    /// A non-lease reply names an older request.
    #[error("reply names a request that is not current")]
    WrongRequest,
    /// A lease acknowledgement is not a stored grant.
    #[error("lease acknowledgement does not match a sent grant")]
    LeaseMismatch,
    /// Inventory or lease exceeded 2048 requests in this attempt.
    #[error("too many inventory or lease requests")]
    TooMany,
    /// `sample_ms + 2000` overflowed.
    #[error("lease deadline overflowed")]
    Overflow,
    /// A request landed in a reply slot, or the status is not legal here.
    #[error("reply status is not legal")]
    Status,
}

impl From<Breach> for MailboxError {
    fn from(kind: Breach) -> Self {
        Self::Breach(kind)
    }
}

/// A failed slot read. The counters are the totals for this slot so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{error} (absent {retry_absent}, replaced {retry_replaced})")]
pub struct FailedRead {
    /// Why this read failed.
    pub error: MailboxError,
    /// Transient `ENOENT` results so far, including this read when it was one.
    pub retry_absent: u64,
    /// Reads discarded because the object changed, including this one.
    pub retry_replaced: u64,
}
