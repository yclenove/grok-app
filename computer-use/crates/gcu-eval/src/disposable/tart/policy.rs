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
    /// Private, loopback, link-local, documentation, benchmarking, multicast,
    /// or another non-global or reserved address.
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
/// Public means not in the special-use table in this module. `192.0.0.0/24`
/// is rejected as a whole.
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
    /// non-global or reserved addresses are rejected. Does not block or touch
    /// the network.
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

/// Non-global and reserved IPv4 prefixes, as `(network, prefix length)`.
///
/// `192.0.0.0/24` is rejected as a whole. The few addresses in that block
/// that IANA marks globally reachable are not treated as public endpoints.
const NOT_PUBLIC: &[(u32, u8)] = &[
    net(0, 0, 0, 0, 8),
    net(10, 0, 0, 0, 8),
    net(100, 64, 0, 0, 10),
    net(127, 0, 0, 0, 8),
    net(169, 254, 0, 0, 16),
    net(172, 16, 0, 0, 12),
    net(192, 0, 0, 0, 24),
    net(192, 0, 2, 0, 24),
    net(192, 88, 99, 0, 24),
    net(192, 168, 0, 0, 16),
    net(198, 18, 0, 0, 15),
    net(198, 51, 100, 0, 24),
    net(203, 0, 113, 0, 24),
    net(224, 0, 0, 0, 4),
    net(240, 0, 0, 0, 4),
    net(255, 255, 255, 255, 32),
];

const fn net(a: u8, b: u8, c: u8, d: u8, bits: u8) -> (u32, u8) {
    (u32::from_be_bytes([a, b, c, d]), bits)
}

fn is_public_endpoint(addr: Ipv4Addr) -> bool {
    let value = u32::from(addr);
    !NOT_PUBLIC.iter().any(|&(network, bits)| {
        let mask = prefix_mask(bits);
        (value & mask) == (network & mask)
    })
}

fn prefix_mask(bits: u8) -> u32 {
    match bits {
        // Table widths are 4..=32. Shift amount is then 28..=0.
        1..=32 => u32::MAX << (32 - u32::from(bits)),
        // A zero mask matches every address, so the caller rejects it.
        _ => 0,
    }
}
