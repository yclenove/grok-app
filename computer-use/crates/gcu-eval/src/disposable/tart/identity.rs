//! Attempt and clone identities. A value is an exact token, not a path or a flag.

/// Longest accepted run id or clone id, in bytes.
pub const MAX_IDENTITY_LEN: usize = 64;

/// Why an identity token was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// Empty text is not an identity.
    #[error("identity is empty")]
    Empty,
    /// Longer than [`MAX_IDENTITY_LEN`].
    #[error("identity is longer than the 64-byte cap")]
    TooLong {
        /// Offered length in bytes.
        len: usize,
    },
    /// Path separator, `.`, or `..`.
    #[error("identity contains a path")]
    Path,
    /// `--` would be read as a flag.
    #[error("identity contains --")]
    DoubleDash,
    /// Reserved golden image name or `golden` prefix.
    #[error("identity is a golden name")]
    GoldenName,
    /// Outside `[A-Za-z0-9_-]`, or a leading or trailing separator.
    #[error("identity has an illegal character")]
    Illegal,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity(String);

impl Identity {
    fn parse(raw: &str) -> Result<Self, IdentityError> {
        if raw.is_empty() {
            return Err(IdentityError::Empty);
        }
        if raw.len() > MAX_IDENTITY_LEN {
            return Err(IdentityError::TooLong { len: raw.len() });
        }
        if raw == "." || raw == ".." || raw.bytes().any(|byte| byte == b'/' || byte == b'\\') {
            return Err(IdentityError::Path);
        }
        if raw.contains("--") {
            return Err(IdentityError::DoubleDash);
        }
        if is_golden_name(raw) {
            return Err(IdentityError::GoldenName);
        }
        if !charset_ok(raw) {
            return Err(IdentityError::Illegal);
        }
        Ok(Self(raw.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_golden_name(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.starts_with("golden") || lower.starts_with("macos-s2-eval")
}

fn charset_ok(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
}

/// Eval run id. Match is exact and case-sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunId(Identity);

impl RunId {
    /// Reject empty, overlong, path, `--`, golden, and illegal tokens.
    ///
    /// Does not touch the filesystem.
    pub fn new(raw: &str) -> Result<Self, IdentityError> {
        Ok(Self(Identity::parse(raw)?))
    }

    /// Borrow the exact token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Tart clone id. The same rules as [`RunId`], so it cannot name the golden image.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CloneId(Identity);

impl CloneId {
    /// Reject empty, overlong, path, `--`, golden, and illegal tokens.
    ///
    /// Does not touch the filesystem.
    pub fn new(raw: &str) -> Result<Self, IdentityError> {
        Ok(Self(Identity::parse(raw)?))
    }

    /// Borrow the exact token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The only golden image this interface can name. Not a path and not a clone id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GoldenImage {
    /// ADR 0018 candidate. Naming it does not prove the image exists or is sealed.
    MacosS2Eval,
}

impl GoldenImage {
    /// Fixed Tart name. Not accepted as a [`CloneId`].
    pub const NAME: &'static str = "macos-s2-eval";

    /// The only legal golden name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        Self::NAME
    }
}
