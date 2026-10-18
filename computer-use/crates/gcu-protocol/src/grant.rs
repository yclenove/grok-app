//! Persistent-grant comparison for architecture §7.2.
//!
//! Route A and route B both call [`persistent_eq`]. A missing path, content
//! hash, code hash, certificate, publisher, team, product, or build does not
//! widen the match. Legacy records that omit evidence do not match.

/// Platform of a persistent grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantPlatform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

/// Signature state of a persistent grant. `Invalid` and `Unknown` never match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantSignature {
    /// The OS accepted the signature.
    Valid,
    /// The OS rejected the signature.
    Invalid,
    /// The image has no signature.
    Unsigned,
    /// The OS check did not produce a result.
    Unknown,
}

/// Borrowed facts for one persistent-grant comparison.
#[derive(Debug, Clone, Copy)]
pub struct GrantFacts<'a> {
    /// Verifying platform.
    pub platform: GrantPlatform,
    /// OS signature result.
    pub signature: GrantSignature,
    /// Bundle id, normalized path, or Linux key.
    pub key: &'a str,
    /// Canonical executable path.
    pub image_path: &'a str,
    /// SHA-256 of the image. Windows uses this as the build digest.
    pub sha256: Option<[u8; 32]>,
    /// macOS Team ID.
    pub team_id: Option<&'a str>,
    /// Publisher subject. Not a certificate fingerprint.
    pub publisher: Option<&'a str>,
    /// macOS code-directory hash.
    pub cdhash: Option<&'a str>,
    /// SHA-256 of the signing certificate.
    pub certificate_sha256: Option<&'a str>,
    /// OS product name.
    pub product: Option<&'a str>,
    /// macOS bundle version. Absent on Windows unless a string was stored.
    pub build: Option<&'a str>,
    /// Linux device id.
    pub file_dev: Option<u64>,
    /// Linux inode.
    pub file_inode: Option<u64>,
}

/// Whether two OS identities are the same persistent grant.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn persistent_eq(granted: GrantFacts<'_>, current: GrantFacts<'_>) -> bool {
    if granted.platform != current.platform || granted.signature != current.signature {
        return false;
    }
    if !matches!(
        granted.signature,
        GrantSignature::Valid | GrantSignature::Unsigned
    ) {
        return false;
    }
    if granted.key.is_empty() || granted.key != current.key {
        return false;
    }
    if granted.image_path.is_empty() || granted.image_path != current.image_path {
        return false;
    }
    if granted.sha256.is_none() || granted.sha256 != current.sha256 {
        return false;
    }
    match granted.platform {
        GrantPlatform::Macos => macos(granted, current),
        GrantPlatform::Windows => windows(granted, current),
        GrantPlatform::Linux => linux(granted, current),
    }
}

fn macos(granted: GrantFacts<'_>, current: GrantFacts<'_>) -> bool {
    if !same_hex(granted.cdhash, current.cdhash, &[40, 64]) {
        return false;
    }
    if granted.signature == GrantSignature::Unsigned {
        return agree_text(granted.team_id, current.team_id)
            && agree_text(granted.publisher, current.publisher)
            && agree_hex(granted.certificate_sha256, current.certificate_sha256)
            && agree_text(granted.product, current.product)
            && agree_text(granted.build, current.build);
    }
    same_text(granted.team_id, current.team_id)
        && same_text(granted.publisher, current.publisher)
        && same_hex(
            granted.certificate_sha256,
            current.certificate_sha256,
            &[64],
        )
        && same_text(granted.product, current.product)
        && same_text(granted.build, current.build)
}

fn windows(granted: GrantFacts<'_>, current: GrantFacts<'_>) -> bool {
    if granted.key != granted.image_path || current.key != current.image_path {
        return false;
    }
    if granted.signature == GrantSignature::Unsigned {
        return agree_text(granted.publisher, current.publisher)
            && agree_hex(granted.certificate_sha256, current.certificate_sha256)
            && agree_text(granted.product, current.product)
            && agree_text(granted.build, current.build);
    }
    same_text(granted.publisher, current.publisher)
        && same_hex(
            granted.certificate_sha256,
            current.certificate_sha256,
            &[64],
        )
        && same_text(granted.product, current.product)
        && agree_text(granted.build, current.build)
}

fn linux(granted: GrantFacts<'_>, current: GrantFacts<'_>) -> bool {
    match (
        granted.file_dev,
        granted.file_inode,
        current.file_dev,
        current.file_inode,
    ) {
        (Some(dev), Some(inode), Some(other_dev), Some(other_inode))
            if dev == other_dev && inode == other_inode => {}
        _ => return false,
    }
    agree_text(granted.publisher, current.publisher)
        && agree_text(granted.team_id, current.team_id)
        && agree_hex(granted.cdhash, current.cdhash)
        && agree_hex(granted.certificate_sha256, current.certificate_sha256)
        && agree_text(granted.product, current.product)
        && agree_text(granted.build, current.build)
}

fn same_text(left: Option<&str>, right: Option<&str>) -> bool {
    match (present(left), present(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn agree_text(left: Option<&str>, right: Option<&str>) -> bool {
    present(left) == present(right)
}

fn agree_hex(left: Option<&str>, right: Option<&str>) -> bool {
    norm_hex(left, &[40, 64]) == norm_hex(right, &[40, 64])
}

fn same_hex(left: Option<&str>, right: Option<&str>, lengths: &[usize]) -> bool {
    match (norm_hex(left, lengths), norm_hex(right, lengths)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

fn norm_hex(value: Option<&str>, lengths: &[usize]) -> Option<String> {
    let text = present(value)?;
    if !lengths.contains(&text.len()) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(text.to_ascii_lowercase())
}
