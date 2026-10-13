//! Bounded NDJSON framing shared by stdio and the standalone fuzz target.
use serde_json::Value;

/// Maximum request line, excluding its newline. Larger lines are discarded.
pub const MAX_LINE_BYTES: usize = 1_048_576;
/// A parsed JSON value or a payload-free JSON-RPC error code.
#[derive(Debug, PartialEq)]
pub enum Decoded {
    /// One complete JSON value; the transport validates its RPC envelope.
    Message(Value),
    /// Parse error (-32700), or oversized/invalid request (-32600).
    Error(i32),
}
/// Incremental, bounded NDJSON decoder. Never guesses Content-Length framing.
/// Pure memory work; no IO, blocking waits, timeout, or cancellation.
#[derive(Default)]
pub struct NdjsonCodec {
    line: Vec<u8>,
    discarding: bool,
}
impl NdjsonCodec {
    /// Consume bytes, yielding at most one event per completed line. An oversized
    /// line yields one error after its delimiter; retained memory is bounded.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Decoded> {
        let mut output = Vec::new();
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            let complete = part.last() == Some(&b'\n');
            let data = if complete {
                &part[..part.len() - 1]
            } else {
                part
            };
            if !self.discarding {
                if self.line.len().saturating_add(data.len()) > MAX_LINE_BYTES {
                    self.line.clear();
                    self.discarding = true;
                } else {
                    self.line.extend_from_slice(data);
                }
            }
            if complete {
                output.push(if self.discarding {
                    Decoded::Error(-32600)
                } else {
                    match serde_json::from_slice(&self.line) {
                        Ok(value) => Decoded::Message(value),
                        Err(_) => Decoded::Error(-32700),
                    }
                });
                self.line.clear();
                self.discarding = false;
            }
        }
        output
    }
    /// Finish at EOF. A partial line is invalid even if its JSON is complete;
    /// NDJSON requires its delimiter. Repeated finish calls are empty.
    pub fn finish(&mut self) -> Option<Decoded> {
        let error = if self.discarding {
            Some(Decoded::Error(-32600))
        } else if !self.line.is_empty() {
            Some(Decoded::Error(-32700))
        } else {
            None
        };
        self.line.clear();
        self.discarding = false;
        error
    }
}
