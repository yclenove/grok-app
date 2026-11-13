//! Short ASCII identities, the public attempt nonce, and digest text.

use super::super::identity::MAX_IDENTITY_LEN;
use super::super::manifest::{Digest, DIGEST_LEN};
use super::CodecFault;

/// One ASCII identity token, 1..=64 bytes, `[A-Za-z0-9_-]`.
///
/// The first and last byte are alphanumeric. This is not a [`super::super::identity::RunId`]:
/// boot and desktop tokens are not golden-image names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AsciiId(String);

impl AsciiId {
    /// Reject empty, overlong, and non-token text.
    pub fn new(raw: &str) -> Result<Self, CodecFault> {
        if raw.is_empty() || raw.len() > MAX_IDENTITY_LEN {
            return Err(CodecFault::Identity);
        }
        let bytes = raw.as_bytes();
        if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
            return Err(CodecFault::Identity);
        }
        if !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
        {
            return Err(CodecFault::Identity);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Borrow the token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Public 128-bit attempt binding. Not a key. Zero is a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttemptNonce([u8; 16]);

impl AttemptNonce {
    /// Take 16 bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Decode 32 lowercase hex characters.
    pub fn from_hex(text: &str) -> Result<Self, CodecFault> {
        if text.len() != 32 {
            return Err(CodecFault::Nonce);
        }
        Ok(Self(parse_fixed(text).map_err(|()| CodecFault::Nonce)?))
    }

    /// Canonical lowercase hex.
    #[must_use]
    pub fn to_hex(self) -> String {
        encode_hex(&self.0)
    }

    /// Borrow the 16 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

pub(super) fn parse_digest(text: &str) -> Result<Digest, CodecFault> {
    if text.len() != DIGEST_LEN * 2 {
        return Err(CodecFault::DigestLen);
    }
    let bytes = parse_fixed::<DIGEST_LEN>(text).map_err(|()| CodecFault::Digest)?;
    Digest::try_from_bytes(bytes).map_err(|_| CodecFault::Digest)
}

pub(super) fn digest_hex(digest: Digest) -> String {
    encode_hex(digest.as_bytes())
}

fn parse_fixed<const N: usize>(text: &str) -> Result<[u8; N], ()> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return Err(());
    }
    let mut out = [0u8; N];
    for (index, slot) in out.iter_mut().enumerate() {
        let hi = hex_val(bytes[index * 2])?;
        let lo = hex_val(bytes[index * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
