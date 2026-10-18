//! App-side checks that call the same admission and lifecycle as the engine crate.
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use gcu_host_protocol::supervisor::{
    EngineChoice, EngineFacts, HostBuild, HostState, Phase, ReleaseIdentity, Signal, Step,
};

use super::super::codesign::{ProbeError, SignatureProbe};

use super::{decide, host_build};

fn file(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gcu-app-{name}-{}", std::process::id()));
    std::fs::write(&path, b"engine").unwrap();
    path
}

#[test]
fn host_build_follows_debug_assertions() {
    if cfg!(debug_assertions) {
        assert_eq!(host_build(), HostBuild::Dev);
    } else {
        assert_eq!(host_build(), HostBuild::Release);
    }
}

#[test]
fn start_twice_while_running_does_not_spawn_again() {
    let mut state = HostState::new();
    assert_eq!(state.apply(Signal::SetEnabled(true), 0), Step::Spawn);
    assert_eq!(state.phase, Phase::Starting);
    assert_eq!(state.apply(Signal::HandshakeOk, 1), Step::None);
    assert_eq!(state.apply(Signal::SetEnabled(true), 2), Step::None);
    assert_eq!(state.phase, Phase::Running);
}

#[test]
fn stop_twice_while_stopped_is_idempotent() {
    let mut state = HostState::new();
    assert_eq!(state.apply(Signal::SetEnabled(false), 0), Step::None);
    assert_eq!(state.apply(Signal::SetEnabled(false), 1), Step::None);
    assert_eq!(state.phase, Phase::Stopped);
}

#[test]
fn handshake_failure_stays_stopped_and_preserves_stop_latch() {
    let mut state = HostState::new();
    assert_eq!(state.apply(Signal::UserStop, 0), Step::None);
    assert_eq!(state.apply(Signal::SetEnabled(true), 1), Step::Spawn);
    assert_eq!(state.apply(Signal::HandshakeFailed, 2), Step::None);
    assert_eq!(state.phase, Phase::Stopped);
    assert!(state.stop_latched);
    assert_eq!(state.apply(Signal::HandshakeFailed, 3), Step::None);
    assert_eq!(state.phase, Phase::Stopped);
}

#[test]
fn third_crash_within_ten_minutes_trips_and_disables() {
    let mut state = HostState::new();
    assert_eq!(state.apply(Signal::SetEnabled(true), 0), Step::Spawn);
    state.apply(Signal::HandshakeOk, 0);
    assert_eq!(
        state.apply(Signal::ChildExited { clean: false }, 1),
        Step::Restart
    );
    state.apply(Signal::HandshakeOk, 1);
    assert_eq!(
        state.apply(Signal::ChildExited { clean: false }, 2),
        Step::Restart
    );
    state.apply(Signal::HandshakeOk, 2);
    assert_eq!(
        state.apply(Signal::ChildExited { clean: false }, 3),
        Step::None
    );
    assert_eq!(state.phase, Phase::Tripped);
    assert!(!state.enabled);
}

#[test]
fn release_bundled_identity_mismatch_is_refused() {
    let path = file("mismatch");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let facts = EngineFacts::content(&path, "other");
    let choice = EngineChoice::Bundled { path: path.clone() };
    let error = decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert_eq!(
        error,
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
}

#[test]
fn dev_external_with_separate_grant_is_allowed() {
    let path = file("external");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let mut facts = EngineFacts::content(&path, "other");
    facts.dev_authorized = true;
    let choice = EngineChoice::External {
        path: path.clone(),
        dev_grant: true,
    };
    assert!(decide(HostBuild::Dev, &identity, &choice, &facts).is_ok());
    let _ = std::fs::remove_file(path);
}

#[test]
fn release_external_path_is_refused() {
    let path = file("release-external");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let facts = EngineFacts::content(&path, "other");
    let choice = EngineChoice::External {
        path: path.clone(),
        dev_grant: true,
    };
    let error = decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert_eq!(
        error,
        gcu_host_protocol::supervisor::Reject::ExternalNotAllowed
    );
}

#[test]
fn release_elevated_or_uiaccess_is_refused() {
    let path = file("elevated");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let mut facts = EngineFacts::content(&path, "shipped");
    facts.ui_access = true;
    let choice = EngineChoice::Bundled { path: path.clone() };
    let error = decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err();
    let _ = std::fs::remove_file(path);
    assert_eq!(error, gcu_host_protocol::supervisor::Reject::UiAccess);
}

struct FakeProbe {
    verify: Result<(), ProbeError>,
    requirement: Result<(), ProbeError>,
    display: Result<String, ProbeError>,
    requirement_calls: Cell<u32>,
    display_calls: Cell<u32>,
    seen_requirement: RefCell<Option<String>>,
}

impl SignatureProbe for FakeProbe {
    fn verify(&self, _path: &Path) -> Result<(), ProbeError> {
        self.verify
    }

    fn test_requirement(&self, _path: &Path, requirement: &str) -> Result<(), ProbeError> {
        self.requirement_calls
            .set(self.requirement_calls.get().saturating_add(1));
        *self.seen_requirement.borrow_mut() = Some(requirement.to_owned());
        self.requirement
    }

    fn display(&self, _path: &Path) -> Result<String, ProbeError> {
        self.display_calls
            .set(self.display_calls.get().saturating_add(1));
        self.display.clone()
    }
}

fn fake(
    verify: Result<(), ProbeError>,
    requirement: Result<(), ProbeError>,
    display: Result<String, ProbeError>,
) -> FakeProbe {
    FakeProbe {
        verify,
        requirement,
        display,
        requirement_calls: Cell::new(0),
        display_calls: Cell::new(0),
        seen_requirement: RefCell::new(None),
    }
}

fn developer_id() -> ReleaseIdentity {
    ReleaseIdentity::MacDeveloperId {
        team_id: "TEAMID1234".into(),
        signing_id: "ai.grok.computer-use".into(),
        cdhash: "abcdef0123456789".into(),
    }
}

fn matching_display() -> String {
    "\
Identifier=ai.grok.computer-use
TeamIdentifier=TEAMID1234
CDHash=abcdef0123456789
"
    .to_owned()
}

fn bundled(
    name: &str,
    mode: HostBuild,
    probe: &FakeProbe,
) -> (
    EngineFacts,
    Result<PathBuf, gcu_host_protocol::supervisor::Reject>,
) {
    let path = file(name);
    let identity = developer_id();
    let facts = super::facts_with_probe(&path, "sha", false, &identity, mode, probe);
    let choice = EngineChoice::Bundled { path: path.clone() };
    let decision = decide(mode, &identity, &choice, &facts);
    let _ = std::fs::remove_file(path);
    (facts, decision)
}

#[test]
fn release_matching_developer_id_is_admitted_when_signature_and_requirement_hold() {
    let probe = fake(Ok(()), Ok(()), Ok(matching_display()));
    let (facts, decision) = bundled("valid-release", HostBuild::Release, &probe);
    let expected =
        super::super::codesign::developer_id_requirement("TEAMID1234").expect("alphanumeric team");
    assert_eq!(
        probe.seen_requirement.borrow().as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(facts.team_id.as_deref(), Some("TEAMID1234"));
    assert_eq!(facts.signing_id.as_deref(), Some("ai.grok.computer-use"));
    assert_eq!(facts.build_id.as_deref(), Some("abcdef0123456789"));
    assert!(decision.is_ok());
}

#[test]
fn release_verify_failure_timeout_or_missing_codesign_is_refused() {
    for (name, error) in [
        ("verify-failed", ProbeError::Failed),
        ("verify-timeout", ProbeError::TimedOut),
        ("verify-missing", ProbeError::Unavailable),
    ] {
        let probe = fake(Err(error), Ok(()), Ok(matching_display()));
        let (facts, decision) = bundled(name, HostBuild::Release, &probe);
        assert_eq!(probe.display_calls.get(), 0, "{name}");
        assert_eq!(probe.requirement_calls.get(), 0, "{name}");
        assert!(facts.team_id.is_none(), "{name}");
        assert!(facts.signing_id.is_none(), "{name}");
        assert!(facts.build_id.is_none(), "{name}");
        assert_eq!(
            decision.unwrap_err(),
            gcu_host_protocol::supervisor::Reject::IdentityMismatch,
            "{name}"
        );
    }
    let probe = fake(Ok(()), Ok(()), Err(ProbeError::TimedOut));
    let (facts, decision) = bundled("display-timeout", HostBuild::Release, &probe);
    assert_eq!(probe.requirement_calls.get(), 1);
    assert!(facts.team_id.is_none());
    assert_eq!(
        decision.unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
}

#[test]
fn release_self_signed_same_name_failing_requirement_is_refused() {
    let probe = fake(Ok(()), Err(ProbeError::Failed), Ok(matching_display()));
    let (facts, decision) = bundled("self-signed", HostBuild::Release, &probe);
    assert_eq!(probe.display_calls.get(), 0);
    assert_eq!(
        probe.seen_requirement.borrow().as_deref(),
        super::super::codesign::developer_id_requirement("TEAMID1234").as_deref()
    );
    assert!(facts.team_id.is_none());
    assert!(facts.signing_id.is_none());
    assert!(facts.build_id.is_none());
    assert_eq!(
        decision.unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
}

#[test]
fn release_display_match_with_failed_verification_is_refused() {
    let probe = fake(Err(ProbeError::Failed), Ok(()), Ok(matching_display()));
    let (facts, decision) = bundled("tampered-display", HostBuild::Release, &probe);
    assert_eq!(probe.display_calls.get(), 0);
    assert!(facts.team_id.is_none());
    assert_ne!(facts.build_id.as_deref(), Some("abcdef0123456789"));
    assert_eq!(
        decision.unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
}

#[test]
fn release_identifier_only_display_is_still_refused() {
    let probe = fake(
        Ok(()),
        Ok(()),
        Ok("Identifier=ai.grok.computer-use\nTeamIdentifier=not set\n".into()),
    );
    let (facts, decision) = bundled("identifier-only", HostBuild::Release, &probe);
    assert_eq!(facts.signing_id.as_deref(), Some("ai.grok.computer-use"));
    assert!(facts.team_id.is_none());
    assert_eq!(
        decision.unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentifierOnly
    );
}

#[test]
fn release_unsafe_team_id_is_refused_without_building_a_requirement() {
    let path = file("unsafe-team");
    let identity = ReleaseIdentity::MacDeveloperId {
        team_id: "TEAM\"ID".into(),
        signing_id: "ai.grok.computer-use".into(),
        cdhash: "abcdef0123456789".into(),
    };
    let probe = fake(Ok(()), Ok(()), Ok(matching_display()));
    let facts = super::facts_with_probe(&path, "sha", false, &identity, HostBuild::Release, &probe);
    assert_eq!(probe.requirement_calls.get(), 0);
    assert!(super::super::codesign::developer_id_requirement("TEAM\"ID").is_none());
    assert!(facts.team_id.is_none());
    let choice = EngineChoice::Bundled { path: path.clone() };
    assert_eq!(
        decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn dev_identifier_only_bundle_stays_allowed_without_apple_requirement() {
    let probe = fake(
        Ok(()),
        Err(ProbeError::Failed),
        Ok("Identifier=dev.gcu\nTeamIdentifier=not set\nCDHash=local\n".into()),
    );
    let (facts, decision) = bundled("dev-identifier", HostBuild::Dev, &probe);
    assert_eq!(probe.requirement_calls.get(), 0);
    assert_eq!(facts.signing_id.as_deref(), Some("dev.gcu"));
    assert!(facts.team_id.is_none());
    assert!(decision.is_ok());
}

#[test]
fn dev_bundled_unverified_binary_stays_allowed() {
    let probe = fake(Err(ProbeError::Failed), Ok(()), Ok(matching_display()));
    let (facts, decision) = bundled("dev-unsigned", HostBuild::Dev, &probe);
    assert_eq!(probe.display_calls.get(), 0);
    assert!(facts.team_id.is_none());
    assert!(facts.signing_id.is_none());
    assert!(decision.is_ok());
}

#[test]
fn dev_external_release_identity_is_still_refused() {
    let path = file("dev-external-release");
    let identity = developer_id();
    let probe = fake(Ok(()), Err(ProbeError::Failed), Ok(matching_display()));
    let facts = super::facts_with_probe(&path, "sha", true, &identity, HostBuild::Dev, &probe);
    assert_eq!(probe.requirement_calls.get(), 0);
    assert_eq!(facts.team_id.as_deref(), Some("TEAMID1234"));
    assert_eq!(facts.build_id.as_deref(), Some("abcdef0123456789"));
    let choice = EngineChoice::External {
        path: path.clone(),
        dev_grant: true,
    };
    assert_eq!(
        decide(HostBuild::Dev, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::ReleaseIdentityReused
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn codesign_argv_is_strict_for_a_file_and_deep_for_a_bundle() {
    assert_eq!(super::super::codesign::CHECK_TIMEOUT.as_secs(), 5);
    let path = file("argv-file");
    let args = super::super::codesign::verify_argv(&path);
    assert_eq!(args.len(), 3);
    assert_eq!(args[0], "--verify");
    assert_eq!(args[1], "--strict");
    assert_eq!(args[2], path.as_os_str());
    assert!(!args.iter().any(|arg| arg == "--deep"));
    let dir = std::env::temp_dir().join(format!("gcu-app-argv-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let deep = super::super::codesign::verify_argv(&dir);
    assert!(deep.iter().any(|arg| arg == "--deep"));
    assert_eq!(
        deep.last().map(std::ffi::OsString::as_os_str),
        Some(dir.as_os_str())
    );
    let requirement = super::super::codesign::developer_id_requirement("TEAMID1234").expect("team");
    assert!(
        super::super::codesign::developer_id_requirement("OTHERTEAM99")
            .expect("other")
            .contains("OTHERTEAM99")
    );
    let req_args = super::super::codesign::requirement_argv(&path, &requirement);
    assert_eq!(req_args.len(), 4);
    let flag = req_args[2].to_string_lossy();
    assert_eq!(
        flag,
        "-R=anchor apple generic and certificate leaf[subject.OU] = \"TEAMID1234\""
    );
    assert_eq!(
        req_args.last().map(std::ffi::OsString::as_os_str),
        Some(path.as_os_str())
    );
    assert!(!flag.contains(&path.display().to_string()));
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn content_identity_keeps_its_hash_when_codesign_is_unavailable() {
    let path = file("content-only");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let probe = fake(
        Err(ProbeError::Unavailable),
        Err(ProbeError::Unavailable),
        Err(ProbeError::Unavailable),
    );
    let facts = super::facts_with_probe(&path, "abc", false, &identity, HostBuild::Release, &probe);
    assert_eq!(facts.build_id.as_deref(), Some("abc"));
    assert!(facts.team_id.is_none());
    assert!(facts.signing_id.is_none());
    let choice = EngineChoice::Bundled { path: path.clone() };
    assert_eq!(
        decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
    let mut matched = facts;
    matched.build_id = Some("shipped".into());
    assert!(decide(HostBuild::Release, &identity, &choice, &matched).is_ok());
    let _ = std::fs::remove_file(path);
}

#[test]
fn non_macos_probe_keeps_content_facts_and_does_not_invent_a_team() {
    if cfg!(target_os = "macos") {
        return;
    }
    let path = file("plain");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let facts = super::probe(&path, &identity, false).expect("hash");
    let hash = gcu_host_protocol::supervisor::content_sha256(&path).expect("hash");
    assert_eq!(facts.build_id.as_deref(), Some(hash.as_str()));
    assert!(facts.team_id.is_none());
    assert!(facts.signing_id.is_none());
    let cleared = super::probe(&path, &developer_id(), false).expect("hash");
    assert!(cleared.team_id.is_none());
    assert!(cleared.signing_id.is_none());
    assert!(cleared.build_id.is_none());
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "macos")]
#[test]
fn bounded_command_kills_a_child_that_exceeds_its_timeout() {
    let started = std::time::Instant::now();
    let error = super::super::codesign::run_bounded(
        Path::new("/bin/sleep"),
        &[std::ffi::OsString::from("30")],
        std::time::Duration::from_millis(200),
        false,
    )
    .expect_err("sleep must hit the deadline");
    assert_eq!(error, ProbeError::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let missing = std::env::temp_dir().join(format!("gcu-no-codesign-{}", std::process::id()));
    let error = super::super::codesign::run_bounded(
        &missing,
        &[],
        std::time::Duration::from_secs(1),
        false,
    )
    .expect_err("missing program");
    assert_eq!(error, ProbeError::Unavailable);
}

#[cfg(target_os = "macos")]
#[test]
fn requirement_syntax_is_accepted_by_real_codesign() {
    // The release path builds `-R=<expr>`. A syntax error would also reject, so
    // prove with a satisfiable expression that the form itself is understood.
    let dir = std::env::temp_dir().join(format!("gcu-codesign-req-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("tiny");
    std::fs::copy("/usr/bin/true", &bin).expect("copy a Mach-O to sign");
    let signed = std::process::Command::new("/usr/bin/codesign")
        .args(["-s", "-", "--force", "-i", "com.example.gcu.req"])
        .arg(&bin)
        .status()
        .expect("ad-hoc codesign");
    assert!(signed.success(), "codesign -s - failed");
    let live = super::super::codesign::OsProbe;
    assert_eq!(
        live.test_requirement(&bin, "identifier \"com.example.gcu.req\""),
        Ok(())
    );
    assert_eq!(
        live.test_requirement(&bin, "identifier \"com.example.other\""),
        Err(ProbeError::Failed)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "macos")]
#[test]
fn ad_hoc_codesign_verifies_and_a_tampered_copy_does_not() {
    let dir = std::env::temp_dir().join(format!("gcu-codesign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("tiny");
    // A copy of a system Mach-O, then an ad-hoc signature. No compiler, keychain, or user file.
    std::fs::copy("/usr/bin/true", &bin).expect("copy a Mach-O to sign");
    let signed = std::process::Command::new("/usr/bin/codesign")
        .args(["-s", "-", "--force"])
        .arg(&bin)
        .status()
        .expect("ad-hoc codesign");
    assert!(signed.success(), "codesign -s - failed");
    let live = super::super::codesign::OsProbe;
    assert_eq!(live.verify(&bin), Ok(()));
    let identity = developer_id();
    let requirement = super::super::codesign::developer_id_requirement("TEAMID1234").expect("team");
    assert!(live.test_requirement(&bin, &requirement).is_err());
    let facts = super::facts_with_probe(&bin, "sha", false, &identity, HostBuild::Release, &live);
    assert!(facts.team_id.is_none());
    assert!(facts.signing_id.is_none());
    let choice = EngineChoice::Bundled { path: bin.clone() };
    assert_eq!(
        decide(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
    let dev_facts = super::facts_with_probe(&bin, "sha", false, &identity, HostBuild::Dev, &live);
    assert!(dev_facts.signing_id.is_some());
    assert!(decide(HostBuild::Dev, &identity, &choice, &dev_facts).is_ok());

    let mut bytes = std::fs::read(&bin).unwrap();
    let index = bytes.len() / 2;
    bytes[index] ^= 0xff;
    std::fs::write(&bin, &bytes).unwrap();
    assert_eq!(live.verify(&bin), Err(ProbeError::Failed));
    let tampered = super::facts_with_probe(&bin, "sha", false, &identity, HostBuild::Dev, &live);
    assert!(tampered.signing_id.is_none());
    assert!(tampered.team_id.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
