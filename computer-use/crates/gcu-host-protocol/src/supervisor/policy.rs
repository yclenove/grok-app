//! Pure launch admission, crash breaker, idle exit, and host lifecycle.
//!
//! No process is spawned here. Callers supply canonical paths and OS facts.
//! A release engine must match the shipped build identity. An external binary
//! is refused in release mode. Developer mode allows it only with a separate
//! grant, and never when it presents the release identity. Identifier-only
//! macOS designated requirements are not a release identity (ADR 0006).
//! Privileged Windows tokens are refused before any restart is attempted.

use crate::messages::Policy;
use std::path::{Path, PathBuf};

/// Ten-minute crash window from architecture §15.2.
pub const BREAKER_WINDOW_MS: u64 = 10 * 60 * 1000;
/// Third crash inside the window opens the breaker and disables the feature.
pub const BREAKER_LIMIT: usize = 3;
/// Exit a running engine after this long with zero sessions.
pub const IDLE_MS: u64 = 60_000;

/// How the App binary itself was built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostBuild {
    /// Signed/packaged App. External engines and privileged tokens are refused.
    Release,
    /// Local developer build. A separate grant may name an external engine.
    Dev,
}

/// Identity the packaged engine must present. Identifier-only is not a variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseIdentity {
    /// macOS Developer ID. Team id is required; a signing id alone is refused.
    MacDeveloperId {
        /// Apple team identifier.
        team_id: String,
        /// Designated signing identifier.
        signing_id: String,
        /// CDHash of the shipped binary.
        cdhash: String,
    },
    /// Windows Authenticode publisher plus the signing certificate hash.
    WindowsAuthenticode {
        /// Publisher subject.
        publisher: String,
        /// SHA-256 of the signing certificate.
        certificate_sha256: String,
    },
    /// Exact bytes shipped with this App build (Linux, or a build without a team).
    ContentSha256 {
        /// Lowercase hex SHA-256 of the engine file.
        sha256: String,
    },
}

/// Which binary the user asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineChoice {
    /// Sidecar shipped inside the App bundle.
    Bundled {
        /// Canonical path of that sidecar.
        path: PathBuf,
    },
    /// Developer-selected path. `dev_grant` is a separate authorization, not the path itself.
    External {
        /// Canonical path.
        path: PathBuf,
        /// Explicit developer grant. Release mode ignores this.
        dev_grant: bool,
    },
}

/// OS facts about a candidate engine. Model text never fills these fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineFacts {
    /// Canonical executable path the probe opened.
    pub canonical_path: PathBuf,
    /// Apple team or Authenticode publisher. Empty is treated as absent.
    pub team_id: Option<String>,
    /// Signing identifier. Present without a team is identifier-only.
    pub signing_id: Option<String>,
    /// CDHash, certificate hash, or content SHA-256.
    pub build_id: Option<String>,
    /// Separate developer authorization for this exact external binary.
    pub dev_authorized: bool,
    /// Windows high/system integrity or an elevated token.
    pub elevated: bool,
    /// Windows TokenUIAccess.
    pub ui_access: bool,
    /// The probe could not read the token. Release mode fails closed.
    pub privilege_unknown: bool,
}

impl EngineFacts {
    /// Facts for a content-addressed bundled binary with no extra privileges.
    pub fn content(path: impl Into<PathBuf>, sha256: impl Into<String>) -> Self {
        Self {
            canonical_path: path.into(),
            team_id: None,
            signing_id: None,
            build_id: Some(sha256.into()),
            dev_authorized: false,
            elevated: false,
            ui_access: false,
            privilege_unknown: false,
        }
    }
}

/// Why a launch is refused before it counts as a crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    /// Team, certificate, cdhash, or content hash does not match the package.
    #[error("engine build identity does not match")]
    IdentityMismatch,
    /// macOS designated requirement matches only the signing identifier.
    #[error("identifier-only signature is not a release identity")]
    IdentifierOnly,
    /// Release mode never accepts a path outside the bundle.
    #[error("external engine is refused in release mode")]
    ExternalNotAllowed,
    /// Developer external path without the separate grant.
    #[error("external engine requires a separate developer grant")]
    DevGrantMissing,
    /// External path presented the packaged identity.
    #[error("external engine reuses the release identity")]
    ReleaseIdentityReused,
    /// The candidate path is not the chosen bundled or external path.
    #[error("engine path does not match the selection")]
    PathMismatch,
    /// Elevated, high integrity, or an unreadable token in release mode.
    #[error("elevated engine is refused")]
    Elevation,
    /// TokenUIAccess is set.
    #[error("UIAccess engine is refused")]
    UiAccess,
    /// The selected file is not present.
    #[error("engine binary is missing")]
    MissingBinary,
}

/// Parsed `codesign -dvvv` fields. Missing and `not set` values are absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodesignFacts {
    /// `Identifier=`.
    pub signing_id: Option<String>,
    /// `TeamIdentifier=`.
    pub team_id: Option<String>,
    /// `CDHash=`.
    pub cdhash: Option<String>,
}

/// Decide whether this candidate may be executed.
///
/// Release mode requires the bundled path and the full packaged identity.
/// Developer mode may run a rebuilt bundled binary, or an external binary that
/// has its own grant and does not wear the release identity. Elevated and
/// UIAccess tokens are refused in every mode. An unreadable token is refused
/// only in release mode.
pub fn admit(
    mode: HostBuild,
    expected: &ReleaseIdentity,
    choice: &EngineChoice,
    facts: &EngineFacts,
) -> Result<PathBuf, Reject> {
    if facts.ui_access {
        return Err(Reject::UiAccess);
    }
    if facts.elevated || (facts.privilege_unknown && mode == HostBuild::Release) {
        return Err(Reject::Elevation);
    }
    let (path, external, grant) = match choice {
        EngineChoice::Bundled { path } => (path, false, false),
        EngineChoice::External { path, dev_grant } => (path, true, *dev_grant),
    };
    if external && mode == HostBuild::Release {
        return Err(Reject::ExternalNotAllowed);
    }
    if facts.canonical_path != *path {
        return Err(Reject::PathMismatch);
    }
    if external && (!grant || !facts.dev_authorized) {
        return Err(Reject::DevGrantMissing);
    }
    match expected {
        ReleaseIdentity::MacDeveloperId {
            team_id,
            signing_id,
            cdhash,
        } => admit_mac(
            mode,
            external,
            team_id,
            signing_id,
            cdhash,
            facts,
            path.clone(),
        ),
        ReleaseIdentity::WindowsAuthenticode {
            publisher,
            certificate_sha256,
        } => admit_material(
            mode,
            external,
            facts.team_id.as_deref(),
            facts.build_id.as_deref(),
            publisher,
            certificate_sha256,
            path.clone(),
        ),
        ReleaseIdentity::ContentSha256 { sha256 } => admit_material(
            mode,
            external,
            None,
            facts.build_id.as_deref(),
            "",
            sha256,
            path.clone(),
        ),
    }
}

fn admit_mac(
    mode: HostBuild,
    external: bool,
    team_id: &str,
    signing_id: &str,
    cdhash: &str,
    facts: &EngineFacts,
    path: PathBuf,
) -> Result<PathBuf, Reject> {
    let team = nonempty(facts.team_id.as_deref());
    let signing = nonempty(facts.signing_id.as_deref());
    let hash = nonempty(facts.build_id.as_deref());
    if signing.is_some() && team.is_none() {
        if mode == HostBuild::Dev && !external {
            return Ok(path);
        }
        return Err(Reject::IdentifierOnly);
    }
    let full = team == Some(team_id) && signing == Some(signing_id) && hash == Some(cdhash);
    if external {
        return if full {
            Err(Reject::ReleaseIdentityReused)
        } else {
            Ok(path)
        };
    }
    if mode == HostBuild::Dev || full {
        Ok(path)
    } else {
        Err(Reject::IdentityMismatch)
    }
}

fn admit_material(
    mode: HostBuild,
    external: bool,
    team: Option<&str>,
    hash: Option<&str>,
    expected_team: &str,
    expected_hash: &str,
    path: PathBuf,
) -> Result<PathBuf, Reject> {
    let hash = nonempty(hash);
    let team_ok = expected_team.is_empty() || nonempty(team) == Some(expected_team);
    let full = team_ok && hash == Some(expected_hash);
    if external {
        return if full {
            Err(Reject::ReleaseIdentityReused)
        } else {
            Ok(path)
        };
    }
    if mode == HostBuild::Dev || full {
        Ok(path)
    } else {
        Err(Reject::IdentityMismatch)
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

/// Refuse a live process token. Release mode treats an unreadable token as elevated.
pub fn privilege_allowed(
    mode: HostBuild,
    elevated: bool,
    ui_access: bool,
    unknown: bool,
) -> Result<(), Reject> {
    if ui_access {
        return Err(Reject::UiAccess);
    }
    if elevated || (unknown && mode == HostBuild::Release) {
        return Err(Reject::Elevation);
    }
    Ok(())
}

/// The selected path must be a regular file. Missing sidecars fail closed.
pub fn ensure_engine_file(path: &Path) -> Result<(), Reject> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Reject::MissingBinary)
    }
}

/// Lowercase hex SHA-256 of a file, streamed in 64 KiB chunks.
pub fn content_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Read Identifier, TeamIdentifier, and CDHash from `codesign -dvvv` text.
pub fn parse_codesign_display(text: &str) -> CodesignFacts {
    let mut facts = CodesignFacts {
        signing_id: None,
        team_id: None,
        cdhash: None,
    };
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("not set") {
            continue;
        }
        match key.trim() {
            "Identifier" => facts.signing_id = Some(value.to_owned()),
            "TeamIdentifier" => facts.team_id = Some(value.to_owned()),
            "CDHash" => facts.cdhash = Some(value.to_owned()),
            _ => {}
        }
    }
    facts
}

/// Conservative policy: no persistent grants, foreground stays ask, standard confirmation.
pub fn baseline_policy() -> Policy {
    Policy {
        version: 1,
        always_allow_enabled: true,
        sensitive_action_confirmation: crate::messages::ConfirmationLevel::Standard,
        timeout_secs: 120,
        allow_foreground: crate::messages::ForegroundPolicy::Ask,
        macos_private_spi: true,
        user_idle_ms: 3000,
        stop_hotkey: "default".into(),
        max_tree_nodes: 1200,
        max_image_edge: 1568,
        image_format: "png".into(),
        extra_denied_apps: Vec::new(),
        always: Vec::new(),
    }
}
