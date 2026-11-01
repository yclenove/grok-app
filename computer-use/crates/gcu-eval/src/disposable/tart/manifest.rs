//! Install manifest. Three digests, and nothing that can load a file.

/// SHA-256-sized digest. This module does not hash anything.
pub const DIGEST_LEN: usize = 32;

/// Why a digest was rejected. A manifest does not compare its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// A digest of all zeros is not a measurement.
    #[error("digest is all zeros")]
    ZeroDigest,
}

/// 32 data bytes. Not a path and not a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest([u8; DIGEST_LEN]);

impl Digest {
    /// Reject an all-zero digest.
    ///
    /// Does not read a file and does not hash.
    pub fn try_from_bytes(bytes: [u8; DIGEST_LEN]) -> Result<Self, ManifestError> {
        if bytes == [0; DIGEST_LEN] {
            return Err(ManifestError::ZeroDigest);
        }
        Ok(Self(bytes))
    }

    /// Borrow the 32 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

/// Program, golden, and policy digests. Data only.
///
/// Holding a manifest does not install Tart, Softnet, or a provider, and it
/// does not satisfy admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallManifest {
    program: Digest,
    golden: Digest,
    policy: Digest,
}

impl InstallManifest {
    /// Three digests. Equal values are still only data.
    ///
    /// Does not block, does not read the filesystem, and does not decide
    /// whether the program, image, and policy were measured or installed.
    pub const fn new(program: Digest, golden: Digest, policy: Digest) -> Self {
        Self {
            program,
            golden,
            policy,
        }
    }

    /// Program digest.
    #[must_use]
    pub fn program(&self) -> Digest {
        self.program
    }

    /// Golden image digest.
    #[must_use]
    pub fn golden(&self) -> Digest {
        self.golden
    }

    /// Policy digest.
    #[must_use]
    pub fn policy(&self) -> Digest {
        self.policy
    }
}
