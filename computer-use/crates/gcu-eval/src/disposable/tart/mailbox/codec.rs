//! Length-prefixed JSON frames. A bounded scan runs before `serde`.

use super::super::identity::MAX_IDENTITY_LEN;
use super::super::output::BoundedOutput;
use super::ops::{Envelope, MailboxReply, FRAME_MAX, MAX_JSON_ARRAY, MAX_JSON_DEPTH};
use super::wire;
use super::{CodecFault, MailboxError};

/// Encode one envelope. The same value encodes to the same bytes.
pub fn encode_frame(env: &Envelope) -> Result<Vec<u8>, MailboxError> {
    let json = wire::encode_envelope(env)?;
    let text = std::str::from_utf8(&json).map_err(|_| CodecFault::Utf8)?;
    scan_json(text)?;
    Ok(prefix(&json)?)
}

/// Decode one envelope. Rejects a frame this module would not encode.
pub fn decode_frame(bytes: &[u8]) -> Result<Envelope, MailboxError> {
    let json = split(bytes)?;
    let text = std::str::from_utf8(json).map_err(|_| CodecFault::Utf8)?;
    scan_json(text)?;
    Ok(wire::decode_envelope(text)?)
}

/// Encode a normalized reply into a [`BoundedOutput`] frame.
pub fn encode_reply(reply: &MailboxReply) -> Result<BoundedOutput, MailboxError> {
    let json = wire::encode_reply(reply)?;
    let text = std::str::from_utf8(&json).map_err(|_| CodecFault::Utf8)?;
    scan_json(text)?;
    let frame = prefix(&json)?;
    BoundedOutput::try_new(frame).map_err(|_| MailboxError::Malformed(CodecFault::TooLong))
}

/// Decode a normalized reply frame.
pub fn decode_reply(bytes: &[u8]) -> Result<MailboxReply, MailboxError> {
    let json = split(bytes)?;
    let text = std::str::from_utf8(json).map_err(|_| CodecFault::Utf8)?;
    scan_json(text)?;
    Ok(wire::decode_reply(text)?)
}

fn prefix(json: &[u8]) -> Result<Vec<u8>, CodecFault> {
    if json.len().saturating_add(4) > FRAME_MAX {
        return Err(CodecFault::TooLong);
    }
    let len = u32::try_from(json.len()).map_err(|_| CodecFault::TooLong)?;
    let mut out = Vec::with_capacity(json.len().saturating_add(4));
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(json);
    Ok(out)
}

fn split(bytes: &[u8]) -> Result<&[u8], CodecFault> {
    if bytes.len() > FRAME_MAX {
        return Err(CodecFault::TooLong);
    }
    let Some((head, body)) = bytes.split_first_chunk::<4>() else {
        return Err(CodecFault::Length);
    };
    let declared = usize::try_from(u32::from_be_bytes(*head)).map_err(|_| CodecFault::Length)?;
    if declared != body.len() {
        return Err(CodecFault::Length);
    }
    Ok(body)
}

fn scan_json(text: &str) -> Result<(), CodecFault> {
    let mut scan = Scan {
        bytes: text.as_bytes(),
        index: 0,
    };
    scan.skip_ws();
    scan.value(1)?;
    if scan.index != scan.bytes.len() {
        return Err(CodecFault::Trailing);
    }
    Ok(())
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl Scan<'_> {
    fn value(&mut self, depth: usize) -> Result<(), CodecFault> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => {
                let _text = self.string()?;
                Ok(())
            }
            b'-' => Err(CodecFault::Negative),
            b'0'..=b'9' => self.number(),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => Err(CodecFault::Schema),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), CodecFault> {
        if depth > MAX_JSON_DEPTH {
            return Err(CodecFault::Depth);
        }
        self.expect(b'{')?;
        self.skip_ws();
        if self.eat(b'}') {
            return Ok(());
        }
        let mut keys = Vec::new();
        loop {
            let key = self.string()?;
            if keys.iter().any(|seen: &String| seen == &key) {
                return Err(CodecFault::Duplicate);
            }
            keys.push(key);
            self.skip_ws();
            self.expect(b':')?;
            self.value(depth.saturating_add(1))?;
            self.skip_ws();
            if self.eat(b',') {
                self.skip_ws();
                continue;
            }
            break;
        }
        self.expect(b'}')
    }

    fn array(&mut self, depth: usize) -> Result<(), CodecFault> {
        if depth > MAX_JSON_DEPTH {
            return Err(CodecFault::Depth);
        }
        self.expect(b'[')?;
        self.skip_ws();
        if self.eat(b']') {
            return Ok(());
        }
        let mut count = 0usize;
        loop {
            count = count.saturating_add(1);
            if count > MAX_JSON_ARRAY {
                return Err(CodecFault::Array);
            }
            self.value(depth.saturating_add(1))?;
            self.skip_ws();
            if self.eat(b',') {
                self.skip_ws();
                continue;
            }
            break;
        }
        self.expect(b']')
    }

    fn number(&mut self) -> Result<(), CodecFault> {
        let byte = self.peek()?;
        if byte == b'+' {
            return Err(CodecFault::Schema);
        }
        if byte == b'0' {
            self.index = self.index.saturating_add(1);
            return match self.bytes.get(self.index) {
                Some(b'0'..=b'9') => Err(CodecFault::Schema),
                Some(b'.') => Err(CodecFault::Fraction),
                Some(b'e' | b'E') => Err(CodecFault::Exponent),
                _ => Ok(()),
            };
        }
        if !byte.is_ascii_digit() {
            return Err(CodecFault::Schema);
        }
        let mut value = 0u64;
        while let Some(digit) = self.bytes.get(self.index) {
            match digit {
                b'0'..=b'9' => {
                    let next = u64::from(*digit - b'0');
                    value = value
                        .checked_mul(10)
                        .and_then(|wide| wide.checked_add(next))
                        .ok_or(CodecFault::Overflow)?;
                    self.index = self.index.saturating_add(1);
                }
                b'.' => return Err(CodecFault::Fraction),
                b'e' | b'E' => return Err(CodecFault::Exponent),
                _ => break,
            }
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, CodecFault> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self.bump()?;
            match byte {
                b'"' => break,
                b'\\' => self.escape(&mut out)?,
                0x20..=0x7e => self.push_ascii(&mut out, byte)?,
                _ => return Err(CodecFault::Identity),
            }
        }
        Ok(out)
    }

    fn escape(&mut self, out: &mut String) -> Result<(), CodecFault> {
        let byte = match self.bump()? {
            b'"' => b'"',
            b'\\' => b'\\',
            b'/' => b'/',
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'u' => return self.unicode(out),
            _ => return Err(CodecFault::Schema),
        };
        self.push_ascii(out, byte)
    }

    fn unicode(&mut self, out: &mut String) -> Result<(), CodecFault> {
        let mut code = 0u32;
        for _ in 0..4 {
            let byte = self.bump()?;
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => return Err(CodecFault::Schema),
            };
            code = (code << 4) | digit;
        }
        let ch = char::from_u32(code).ok_or(CodecFault::Identity)?;
        let unit = u32::from(ch);
        if !(0x20..=0x7e).contains(&unit) {
            return Err(CodecFault::Identity);
        }
        let byte = u8::try_from(unit).map_err(|_| CodecFault::Identity)?;
        self.push_ascii(out, byte)
    }

    fn push_ascii(&self, out: &mut String, byte: u8) -> Result<(), CodecFault> {
        if !(0x20..=0x7e).contains(&byte) || out.len() >= MAX_IDENTITY_LEN {
            return Err(CodecFault::Identity);
        }
        out.push(char::from(byte));
        Ok(())
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), CodecFault> {
        if self
            .bytes
            .get(self.index..)
            .is_some_and(|rest| rest.starts_with(word))
        {
            self.index = self.index.saturating_add(word.len());
            Ok(())
        } else {
            Err(CodecFault::Schema)
        }
    }

    fn skip_ws(&mut self) {
        while matches!(
            self.bytes.get(self.index),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.index = self.index.saturating_add(1);
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), CodecFault> {
        if self.eat(byte) {
            Ok(())
        } else {
            Err(CodecFault::Schema)
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.index) == Some(&byte) {
            self.index = self.index.saturating_add(1);
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Result<u8, CodecFault> {
        self.bytes
            .get(self.index)
            .copied()
            .ok_or(CodecFault::Schema)
    }

    fn bump(&mut self) -> Result<u8, CodecFault> {
        let byte = self.peek()?;
        self.index = self.index.saturating_add(1);
        Ok(byte)
    }
}
