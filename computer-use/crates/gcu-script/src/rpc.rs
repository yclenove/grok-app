//! Length-prefixed worker frames. No JSON parser and no serde.

use std::io::{self, Read, Write};

/// Decode or encode failure. Malformed input returns this and does not panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecError;

/// One native `cu.*` request. Local `find` and `emit` never become an [`Op`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Authorize and bind an app. This is a driver call.
    App {
        /// App query.
        name: String,
    },
    /// Read state. This is a driver call.
    State {
        /// App query.
        app: String,
        /// When false, the observation is text only.
        screenshot: bool,
    },
    /// Replace an element value. This is a driver call.
    SetValue {
        /// App query.
        app: String,
        /// Snapshot element index.
        index: u32,
        /// Replacement value.
        value: String,
    },
    /// Press a key chord. This is a driver call.
    Press {
        /// App query.
        app: String,
        /// xdotool-style chord.
        key: String,
    },
    /// Click an element. This is a driver call.
    Click {
        /// App query.
        app: String,
        /// Snapshot element index.
        index: u32,
    },
    /// Type text. This is a driver call.
    TypeText {
        /// App query.
        app: String,
        /// Characters to type.
        text: String,
        /// Element to focus first.
        index: Option<u32>,
    },
    /// Wait until an element exists. This is a driver call.
    WaitElement {
        /// App query.
        app: String,
        /// Role, snake_case or display form.
        role: String,
        /// Accessible name, when set.
        name: Option<String>,
        /// Wait horizon in milliseconds.
        timeout_ms: u32,
    },
    /// Wait until text is present or absent. This is a driver call.
    WaitText {
        /// App query.
        app: String,
        /// Observed text.
        text: String,
        /// True waits for presence.
        present: bool,
        /// Wait horizon in milliseconds.
        timeout_ms: u32,
    },
}

/// One indexed element visible to `find`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementView {
    /// Snapshot index.
    pub index: u32,
    /// Display or snake_case role.
    pub role: String,
    /// Accessible name. Empty when the node has none.
    pub name: String,
}

/// State handed back to the script. No handles, paths, or tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateView {
    /// Rendered observation text.
    pub text: String,
    /// Focused element index.
    pub focused: Option<u32>,
    /// Elements `find` can scan without another driver call.
    pub elements: Vec<ElementView>,
}

/// Router reply for one [`Op`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Wire error code. Empty when the call succeeded.
    pub code: String,
    /// Short error text. Empty when the call succeeded.
    pub message: String,
    /// Further driver calls must not be sent.
    pub hard_stop: bool,
    /// Delivery was uncertain. The call must not be replayed.
    pub unknown: bool,
    /// Fresh state, when the router attached one.
    pub state: Option<StateView>,
}

impl Reply {
    /// Successful reply.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn clear(state: Option<StateView>) -> Self {
        Self {
            code: String::new(),
            message: String::new(),
            hard_stop: false,
            unknown: false,
            state,
        }
    }

    /// Failed reply.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn fail(code: &str, message: &str, hard_stop: bool, unknown: bool) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            hard_stop,
            unknown,
            state: None,
        }
    }

    /// User stop. Hard stop, so a later `cu.*` must not reach the router.
    #[must_use]
    pub fn stopped() -> Self {
        Self::fail("stopped_by_user", "stopped", true, false)
    }

    /// Script or native deadline. Hard stop.
    #[must_use]
    pub fn timed_out() -> Self {
        Self::fail("timeout", "timeout", true, false)
    }
}

/// One worker frame. The length prefix lives outside the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Parent to child: code and wall clock.
    Start {
        /// Requested timeout in milliseconds.
        timeout_ms: u32,
        /// User source. Not interpolated into a host script.
        code: String,
    },
    /// Child to parent.
    Request(Op),
    /// Parent to child.
    Reply(Reply),
    /// Child to parent, once.
    Finish {
        /// [`crate::ScriptStatus`] discriminant.
        status: u8,
        /// Capped script output.
        output: String,
        /// Driver calls that left the worker.
        driver_calls: u32,
        /// Wire code, or empty.
        error_code: String,
        /// Short message, or empty.
        error_message: String,
    },
}

/// Encode one frame payload.
///
/// Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn encode_payload(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    match frame {
        Frame::Start { timeout_ms, code } => {
            out.push(1);
            out.extend_from_slice(&timeout_ms.to_le_bytes());
            put_str(&mut out, code);
        }
        Frame::Request(op) => {
            out.push(2);
            put_op(&mut out, op);
        }
        Frame::Reply(reply) => {
            out.push(3);
            put_reply(&mut out, reply);
        }
        Frame::Finish {
            status,
            output,
            driver_calls,
            error_code,
            error_message,
        } => {
            out.push(4);
            out.push(*status);
            put_str(&mut out, output);
            out.extend_from_slice(&driver_calls.to_le_bytes());
            put_str(&mut out, error_code);
            put_str(&mut out, error_message);
        }
    }
    out
}

/// Decode one payload. Extra bytes and truncated frames are errors.
///
/// Blocking: none. Timeout: none. Cancellation: none. Never panics.
///
/// # Errors
/// Returns [`CodecError`] when `bytes` is not one complete frame.
pub fn decode_payload(bytes: &[u8]) -> Result<Frame, CodecError> {
    let mut cur = Cur { buf: bytes, i: 0 };
    let frame = match cur.u8()? {
        1 => Frame::Start {
            timeout_ms: cur.u32()?,
            code: cur.str()?,
        },
        2 => Frame::Request(take_op(&mut cur)?),
        3 => Frame::Reply(take_reply(&mut cur)?),
        4 => Frame::Finish {
            status: cur.u8()?,
            output: cur.str()?,
            driver_calls: cur.u32()?,
            error_code: cur.str()?,
            error_message: cur.str()?,
        },
        _ => return Err(CodecError),
    };
    if cur.i == cur.buf.len() {
        Ok(frame)
    } else {
        Err(CodecError)
    }
}

/// Write a length-prefixed frame and flush it.
///
/// Blocking: writes to `sink`. Timeout: none beyond the writer. Cancellation: none.
///
/// # Errors
/// Returns the writer error, or [`io::ErrorKind::InvalidData`] when the payload
/// does not fit in a `u32` length.
pub fn write_frame(sink: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let payload = encode_payload(frame);
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame"))?;
    sink.write_all(&len.to_le_bytes())?;
    sink.write_all(&payload)?;
    sink.flush()?;
    Ok(())
}

/// Read one length-prefixed frame.
///
/// Blocking: reads from `source` until the frame arrives. Timeout: none beyond
/// the reader. Cancellation: none. A length above 32 MiB is rejected before
/// the buffer is retained.
///
/// # Errors
/// Returns the reader error, or [`io::ErrorKind::InvalidData`] for a bad frame.
pub fn read_frame(source: &mut impl Read) -> io::Result<Frame> {
    let mut len_buf = [0_u8; 4];
    source.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > crate::limits::HOST_BUFFER_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame"));
    }
    let mut buf = vec![0_u8; len];
    source.read_exact(&mut buf)?;
    decode_payload(&buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame"))
}

struct Cur<'a> {
    buf: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn u8(&mut self) -> Result<u8, CodecError> {
        let byte = *self.buf.get(self.i).ok_or(CodecError)?;
        self.i += 1;
        Ok(byte)
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        let end = self.i.checked_add(4).ok_or(CodecError)?;
        let bytes: [u8; 4] = self
            .buf
            .get(self.i..end)
            .ok_or(CodecError)?
            .try_into()
            .map_err(|_| CodecError)?;
        self.i = end;
        Ok(u32::from_le_bytes(bytes))
    }

    fn str(&mut self) -> Result<String, CodecError> {
        let len = self.u32()? as usize;
        let end = self.i.checked_add(len).ok_or(CodecError)?;
        let bytes = self.buf.get(self.i..end).ok_or(CodecError)?;
        self.i = end;
        let text = std::str::from_utf8(bytes).map_err(|_| CodecError)?;
        Ok(text.to_owned())
    }

    fn flag_str(&mut self) -> Result<Option<String>, CodecError> {
        if self.u8()? == 0 {
            Ok(None)
        } else {
            Ok(Some(self.str()?))
        }
    }
}

fn put_str(out: &mut Vec<u8>, text: &str) {
    let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&text.as_bytes()[..text.len().min(len as usize)]);
}

fn put_opt(out: &mut Vec<u8>, text: Option<&str>) {
    match text {
        None => out.push(0),
        Some(text) => {
            out.push(1);
            put_str(out, text);
        }
    }
}

fn put_op(out: &mut Vec<u8>, op: &Op) {
    match op {
        Op::App { name } => {
            out.push(1);
            put_str(out, name);
        }
        Op::State { app, screenshot } => {
            out.push(2);
            put_str(out, app);
            out.push(u8::from(*screenshot));
        }
        Op::SetValue { app, index, value } => {
            out.push(3);
            put_str(out, app);
            out.extend_from_slice(&index.to_le_bytes());
            put_str(out, value);
        }
        Op::Press { app, key } => {
            out.push(4);
            put_str(out, app);
            put_str(out, key);
        }
        Op::Click { app, index } => {
            out.push(5);
            put_str(out, app);
            out.extend_from_slice(&index.to_le_bytes());
        }
        Op::TypeText { app, text, index } => {
            out.push(6);
            put_str(out, app);
            put_str(out, text);
            put_opt_u32(out, *index);
        }
        Op::WaitElement {
            app,
            role,
            name,
            timeout_ms,
        } => {
            out.push(7);
            put_str(out, app);
            put_str(out, role);
            put_opt(out, name.as_deref());
            out.extend_from_slice(&timeout_ms.to_le_bytes());
        }
        Op::WaitText {
            app,
            text,
            present,
            timeout_ms,
        } => {
            out.push(8);
            put_str(out, app);
            put_str(out, text);
            out.push(u8::from(*present));
            out.extend_from_slice(&timeout_ms.to_le_bytes());
        }
    }
}

fn put_opt_u32(out: &mut Vec<u8>, value: Option<u32>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn put_reply(out: &mut Vec<u8>, reply: &Reply) {
    let mut flags = 0_u8;
    if reply.hard_stop {
        flags |= 1;
    }
    if reply.unknown {
        flags |= 2;
    }
    out.push(flags);
    put_str(out, &reply.code);
    put_str(out, &reply.message);
    match &reply.state {
        None => out.push(0),
        Some(state) => {
            out.push(1);
            put_str(out, &state.text);
            put_opt_u32(out, state.focused);
            let count = u32::try_from(state.elements.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&count.to_le_bytes());
            for element in state.elements.iter().take(count as usize) {
                out.extend_from_slice(&element.index.to_le_bytes());
                put_str(out, &element.role);
                put_str(out, &element.name);
            }
        }
    }
}

fn take_op(cur: &mut Cur<'_>) -> Result<Op, CodecError> {
    Ok(match cur.u8()? {
        1 => Op::App { name: cur.str()? },
        2 => Op::State {
            app: cur.str()?,
            screenshot: cur.u8()? != 0,
        },
        3 => Op::SetValue {
            app: cur.str()?,
            index: cur.u32()?,
            value: cur.str()?,
        },
        4 => Op::Press {
            app: cur.str()?,
            key: cur.str()?,
        },
        5 => Op::Click {
            app: cur.str()?,
            index: cur.u32()?,
        },
        6 => Op::TypeText {
            app: cur.str()?,
            text: cur.str()?,
            index: take_opt_u32(cur)?,
        },
        7 => Op::WaitElement {
            app: cur.str()?,
            role: cur.str()?,
            name: cur.flag_str()?,
            timeout_ms: cur.u32()?,
        },
        8 => Op::WaitText {
            app: cur.str()?,
            text: cur.str()?,
            present: cur.u8()? != 0,
            timeout_ms: cur.u32()?,
        },
        _ => return Err(CodecError),
    })
}

fn take_opt_u32(cur: &mut Cur<'_>) -> Result<Option<u32>, CodecError> {
    if cur.u8()? == 0 {
        Ok(None)
    } else {
        Ok(Some(cur.u32()?))
    }
}

fn take_reply(cur: &mut Cur<'_>) -> Result<Reply, CodecError> {
    let flags = cur.u8()?;
    let code = cur.str()?;
    let message = cur.str()?;
    let state = if cur.u8()? == 0 {
        None
    } else {
        let text = cur.str()?;
        let focused = take_opt_u32(cur)?;
        let count = cur.u32()? as usize;
        if count > cur.buf.len().saturating_sub(cur.i) {
            return Err(CodecError);
        }
        let mut elements = Vec::with_capacity(count);
        for _ in 0..count {
            elements.push(ElementView {
                index: cur.u32()?,
                role: cur.str()?,
                name: cur.str()?,
            });
        }
        Some(StateView {
            text,
            focused,
            elements,
        })
    };
    Ok(Reply {
        code,
        message,
        hard_stop: flags & 1 != 0,
        unknown: flags & 2 != 0,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_keeps_every_op() {
        let frames = [
            Frame::Start {
                timeout_ms: 30,
                code: "return 1".into(),
            },
            Frame::Request(Op::TypeText {
                app: "Ed".into(),
                text: "季度".into(),
                index: Some(3),
            }),
            Frame::Request(Op::WaitElement {
                app: "Ed".into(),
                role: "sheet".into(),
                name: None,
                timeout_ms: 5,
            }),
            Frame::Reply(Reply {
                code: String::new(),
                message: String::new(),
                hard_stop: false,
                unknown: false,
                state: Some(StateView {
                    text: "1 button Save".into(),
                    focused: None,
                    elements: vec![ElementView {
                        index: 1,
                        role: "button".into(),
                        name: "Save".into(),
                    }],
                }),
            }),
            Frame::Finish {
                status: 3,
                output: "x".into(),
                driver_calls: 2,
                error_code: "stale_element".into(),
                error_message: "fresh".into(),
            },
        ];
        for frame in frames {
            let bytes = encode_payload(&frame);
            assert_eq!(decode_payload(&bytes).unwrap(), frame);
        }
    }

    #[test]
    fn truncated_and_trailing_bytes_are_errors() {
        assert!(decode_payload(&[]).is_err());
        assert!(decode_payload(&[1, 0]).is_err());
        let mut bytes = encode_payload(&Frame::Start {
            timeout_ms: 1,
            code: "a".into(),
        });
        bytes.push(9);
        assert!(decode_payload(&bytes).is_err());
        assert!(decode_payload(&[0xff, 0xff, 0xff, 0xff]).is_err());
    }
}
