//! Bounded command output and scripted replies. Overlong bodies are not stored.

use std::collections::VecDeque;

use super::deadline::MonoInstant;

/// Largest retained reply, in bytes (ADR 0018 §2, one command ≤ 1MiB).
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Reply bytes that fit in [`MAX_OUTPUT_BYTES`].
#[derive(Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    bytes: Vec<u8>,
}

impl BoundedOutput {
    /// Keep `bytes` only when the length is inside the cap.
    ///
    /// The oversized buffer is dropped. Does not block.
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, OutputError> {
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(OutputError::TooLong { len: bytes.len() });
        }
        Ok(Self { bytes })
    }

    /// Borrow the retained bytes. They are not parsed.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Retained length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// No retained bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl std::fmt::Debug for BoundedOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = self.bytes.len().min(8);
        write!(
            formatter,
            "BoundedOutput {{ len: {}, prefix: {:?} }}",
            self.bytes.len(),
            &self.bytes[..prefix]
        )
    }
}

/// Why output was not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    /// Longer than [`MAX_OUTPUT_BYTES`]. The bytes were not kept.
    #[error("command output exceeds 1MiB")]
    TooLong {
        /// Offered length.
        len: usize,
    },
}

/// Result of one scripted or rejected call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutcome {
    /// The script returned a bounded body.
    Success,
    /// The script returned a bounded failure body.
    Failed,
    /// The script said the call timed out. The fake did not sleep.
    TimedOut,
    /// The script returned a bounded body that the caller must not parse as success.
    Malformed,
    /// The script said the reply was over the cap. No body is stored.
    Overlong,
    /// The deadline was already due. The script was not consumed.
    Expired,
    /// No scripted reply remained. The fake fails closed.
    ScriptExhausted,
}

/// One recorded call. The body lives in the [`CallError`] or the `Ok` value, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry<C> {
    /// Structured command. Not an argv vector.
    pub command: C,
    /// Deadline instant the call was given.
    pub deadline: MonoInstant,
    /// How the fake classified the call.
    pub outcome: ScriptOutcome,
}

/// Failure of a fake Tart or guest call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// `now` was already at or past the deadline. Nothing ran.
    #[error("deadline already expired")]
    Expired,
    /// Scripted failure with a bounded body.
    #[error("command failed")]
    Failed(BoundedOutput),
    /// Scripted timeout. The fake did not sleep.
    #[error("command timed out")]
    TimedOut,
    /// Scripted unusable reply with a bounded body.
    #[error("malformed reply")]
    Malformed(BoundedOutput),
    /// Scripted overlong reply. The body was not stored.
    #[error("reply exceeded 1MiB")]
    OutputTooLong,
    /// The script queue was empty.
    #[error("no scripted reply; failing closed")]
    ScriptExhausted,
}

/// Prefabricated reply. Success, failure, and malformed bodies are bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedReply {
    /// Bounded success body.
    Success(BoundedOutput),
    /// Bounded failure body.
    Failed(BoundedOutput),
    /// Timeout without sleeping.
    TimedOut,
    /// Bounded body that is not a success.
    Malformed(BoundedOutput),
    /// Overlong reply. No bytes are attached.
    Overlong,
}

impl ScriptedReply {
    /// Bounded success. An overlong body is rejected and not stored.
    pub fn success(body: Vec<u8>) -> Result<Self, OutputError> {
        Ok(Self::Success(BoundedOutput::try_new(body)?))
    }

    /// Bounded failure. An overlong body is rejected and not stored.
    pub fn failed(body: Vec<u8>) -> Result<Self, OutputError> {
        Ok(Self::Failed(BoundedOutput::try_new(body)?))
    }

    /// Bounded malformed body. An overlong body is rejected and not stored.
    pub fn malformed(body: Vec<u8>) -> Result<Self, OutputError> {
        Ok(Self::Malformed(BoundedOutput::try_new(body)?))
    }

    /// Scripted timeout.
    #[must_use]
    pub const fn timed_out() -> Self {
        Self::TimedOut
    }

    /// Scripted overlong reply with nowhere to put the bytes.
    #[must_use]
    pub const fn overlong() -> Self {
        Self::Overlong
    }

    fn into_result(self) -> Result<BoundedOutput, CallError> {
        match self {
            Self::Success(body) => Ok(body),
            Self::Failed(body) => Err(CallError::Failed(body)),
            Self::TimedOut => Err(CallError::TimedOut),
            Self::Malformed(body) => Err(CallError::Malformed(body)),
            Self::Overlong => Err(CallError::OutputTooLong),
        }
    }
}

pub(super) fn dispatch_script(
    script: &mut VecDeque<ScriptedReply>,
    now: MonoInstant,
    deadline: MonoInstant,
) -> Result<BoundedOutput, CallError> {
    if now >= deadline {
        return Err(CallError::Expired);
    }
    match script.pop_front() {
        Some(reply) => reply.into_result(),
        None => Err(CallError::ScriptExhausted),
    }
}

pub(super) fn script_outcome(result: &Result<BoundedOutput, CallError>) -> ScriptOutcome {
    match result {
        Ok(_) => ScriptOutcome::Success,
        Err(CallError::Expired) => ScriptOutcome::Expired,
        Err(CallError::Failed(_)) => ScriptOutcome::Failed,
        Err(CallError::TimedOut) => ScriptOutcome::TimedOut,
        Err(CallError::Malformed(_)) => ScriptOutcome::Malformed,
        Err(CallError::OutputTooLong) => ScriptOutcome::Overlong,
        Err(CallError::ScriptExhausted) => ScriptOutcome::ScriptExhausted,
    }
}
