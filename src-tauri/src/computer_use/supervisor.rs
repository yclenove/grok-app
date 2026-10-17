//! Locate the sidecar or a developer external engine and admit it.
//!
//! Release mode requires the bundled path and the identity compiled into this
//! App. Developer mode may use another path only with a separate grant, and
//! that path must not present the release identity. Production launches do not
//! pass `--driver fake`.

use std::path::{Path, PathBuf};

use gcu_host_protocol::approval::Office;
use gcu_host_protocol::events::Gallery;
use gcu_host_protocol::supervisor::{
    admit, content_sha256, ensure_engine_file, Admission, Control, EngineChoice, EngineFacts,
    Error, HostBuild, LaunchRequest, OsLaunch, Reject, ReleaseIdentity,
};

use super::host_server;

/// Release when this App is built with `release` assertions off.
pub fn host_build() -> HostBuild {
    if cfg!(debug_assertions) {
        HostBuild::Dev
    } else {
        HostBuild::Release
    }
}

/// Identity recorded when the sidecar was staged.
///
/// A non-empty team, signing id, and cdhash select Developer ID. Otherwise the
/// staged file hash is the identity.
pub fn expected_identity() -> ReleaseIdentity {
    let team = option_env!("GCU_TEAM_ID").unwrap_or("");
    let signing = option_env!("GCU_SIGNING_ID").unwrap_or("");
    let cdhash = option_env!("GCU_CDHASH").unwrap_or("");
    if !team.is_empty() && !signing.is_empty() && !cdhash.is_empty() {
        ReleaseIdentity::MacDeveloperId {
            team_id: team.to_owned(),
            signing_id: signing.to_owned(),
            cdhash: cdhash.to_owned(),
        }
    } else {
        ReleaseIdentity::ContentSha256 {
            sha256: env!("GCU_ENGINE_SHA256").to_owned(),
        }
    }
}

/// Admit `choice` against `facts`. Missing files fail before identity checks.
pub fn decide(
    mode: HostBuild,
    expected: &ReleaseIdentity,
    choice: &EngineChoice,
    facts: &EngineFacts,
) -> Result<PathBuf, Reject> {
    let path = match choice {
        EngineChoice::Bundled { path } | EngineChoice::External { path, .. } => path,
    };
    ensure_engine_file(path)?;
    admit(mode, expected, choice, facts)
}

/// Build a stopped supervisor for the bundled sidecar or a developer path.
pub fn open_control(
    external: Option<PathBuf>,
    dev_grant: bool,
    office: Office,
    gallery: Gallery,
) -> Result<Control<OsLaunch>, Error> {
    let mode = host_build();
    let identity = expected_identity();
    let external_path = external.is_some();
    let chosen = match external {
        Some(path) => canonicalize(&path)?,
        None => bundled_engine()?,
    };
    let choice = if external_path {
        EngineChoice::External {
            path: chosen.clone(),
            dev_grant,
        }
    } else {
        EngineChoice::Bundled {
            path: chosen.clone(),
        }
    };
    let facts = probe(&chosen, &identity, dev_grant && external_path)?;
    let path = decide(mode, &identity, &choice, &facts)?;
    Ok(Control::new(
        OsLaunch::callbacks(host_server::production_callbacks(office, gallery)),
        LaunchRequest::new(path, false),
        Admission {
            mode,
            expected: identity,
            choice,
            facts,
        },
    ))
}

fn bundled_engine() -> Result<PathBuf, Reject> {
    if let Some(path) = std::env::var_os("GCU_ENGINE_PATH") {
        return canonicalize(Path::new(&path));
    }
    for candidate in candidates() {
        if candidate.is_file() {
            return canonicalize(&candidate);
        }
    }
    Err(Reject::MissingBinary)
}

fn candidates() -> Vec<PathBuf> {
    let triple = env!("GCU_TARGET");
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let staged = format!("gcu-{triple}{suffix}");
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(format!("gcu{suffix}")));
            paths.push(dir.join(&staged));
        }
    }
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(staged),
    );
    paths
}

fn canonicalize(path: &Path) -> Result<PathBuf, Reject> {
    ensure_engine_file(path)?;
    std::fs::canonicalize(path).map_err(|_| Reject::MissingBinary)
}

fn probe(
    path: &Path,
    identity: &ReleaseIdentity,
    dev_authorized: bool,
) -> Result<EngineFacts, Error> {
    let sha = content_sha256(path)?;
    let signed = codesign_of(path);
    let mut facts = EngineFacts::content(path, sha);
    facts.dev_authorized = dev_authorized;
    if signed.team_id.is_some() || matches!(identity, ReleaseIdentity::MacDeveloperId { .. }) {
        facts.team_id = signed.team_id;
        facts.signing_id = signed.signing_id;
        if matches!(identity, ReleaseIdentity::MacDeveloperId { .. }) {
            facts.build_id = signed.cdhash;
        }
    }
    Ok(facts)
}

#[cfg(target_os = "macos")]
fn codesign_of(path: &Path) -> gcu_host_protocol::supervisor::CodesignFacts {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["-dvvv"])
        .arg(path)
        .output();
    match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            gcu_host_protocol::supervisor::parse_codesign_display(&text)
        }
        Err(_) => empty_codesign(),
    }
}

#[cfg(not(target_os = "macos"))]
fn codesign_of(_path: &Path) -> gcu_host_protocol::supervisor::CodesignFacts {
    empty_codesign()
}

fn empty_codesign() -> gcu_host_protocol::supervisor::CodesignFacts {
    gcu_host_protocol::supervisor::CodesignFacts {
        signing_id: None,
        team_id: None,
        cdhash: None,
    }
}

#[cfg(test)]
#[path = "tests/supervisor.rs"]
mod tests;
