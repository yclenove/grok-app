//! Softnet start flags from ADR 0018 §5. Fields are addresses, not a free string.

use std::net::Ipv4Addr;

/// Most approved IPv4 `/32` entries on one start (ADR 0018 §5).
pub const MAX_ALLOW_CIDRS: usize = 16;

/// The only block. Callers cannot replace it.
pub const SOFTNET_BLOCK_CIDR: &str = "0.0.0.0/0";

/// Why Softnet flags were rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// Start without an allow list is not the ADR command.
    #[error("softnet allow list is empty")]
    Empty,
    /// More than [`MAX_ALLOW_CIDRS`] addresses.
    #[error("softnet allow list exceeds 16 addresses")]
    TooMany {
        /// Offered length.
        len: usize,
    },
    /// Private, loopback, link-local, metadata, multicast, or other non-global.
    #[error("address {addr} is not a public endpoint")]
    NotPublic {
        /// Rejected address.
        addr: Ipv4Addr,
    },
    /// The same `/32` was listed twice.
    #[error("address {addr} is duplicated")]
    Duplicate {
        /// Repeated address.
        addr: Ipv4Addr,
    },
}

/// Exact Softnet shape: block every IPv4 address, then allow up to 16 public `/32`s.
///
/// There is no prefix-length field, no IPv6 field, and no string field. The
/// block cannot be removed. Building this value does not install a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoftnetFlags {
    allow: [Ipv4Addr; MAX_ALLOW_CIDRS],
    len: u8,
}

impl SoftnetFlags {
    /// Build flags for `allow`, in order.
    ///
    /// Each address is one `/32`. Empty, more than 16, duplicates, and
    /// non-global addresses are rejected. Does not block or touch the network.
    pub fn try_new(allow: &[Ipv4Addr]) -> Result<Self, PolicyError> {
        if allow.is_empty() {
            return Err(PolicyError::Empty);
        }
        if allow.len() > MAX_ALLOW_CIDRS {
            return Err(PolicyError::TooMany { len: allow.len() });
        }
        let Some(len) = u8::try_from(allow.len()).ok() else {
            return Err(PolicyError::TooMany { len: allow.len() });
        };
        let mut stored = [Ipv4Addr::UNSPECIFIED; MAX_ALLOW_CIDRS];
        for (index, addr) in allow.iter().copied().enumerate() {
            if !is_public_endpoint(addr) {
                return Err(PolicyError::NotPublic { addr });
            }
            if stored[..index].contains(&addr) {
                return Err(PolicyError::Duplicate { addr });
            }
            stored[index] = addr;
        }
        Ok(Self { allow: stored, len })
    }

    /// Approved addresses, each meaning one `/32`.
    #[must_use]
    pub fn allow(&self) -> &[Ipv4Addr] {
        let len = usize::from(self.len);
        &self.allow[..len]
    }

    /// Always [`SOFTNET_BLOCK_CIDR`].
    #[must_use]
    pub const fn block_cidr() -> &'static str {
        SOFTNET_BLOCK_CIDR
    }
}

fn is_public_endpoint(addr: Ipv4Addr) -> bool {
    let [first, second, third, _] = addr.octets();
    if first == 0 || first == 10 || first == 127 || first >= 224 {
        return false;
    }
    if first == 172 && (second & 0xf0) == 16 {
        return false;
    }
    if first == 192 && second == 168 {
        return false;
    }
    if first == 169 && second == 254 {
        return false;
    }
    // Carrier-grade NAT. Not a global endpoint.
    if first == 100 && (second & 0xc0) == 64 {
        return false;
    }
    // Documentation and benchmarking ranges.
    if first == 192 && second == 0 && third == 2 {
        return false;
    }
    if first == 198 && second == 51 && third == 100 {
        return false;
    }
    if first == 203 && second == 0 && third == 113 {
        return false;
    }
    if first == 198 && (second == 18 || second == 19) {
        return false;
    }
    true
}
