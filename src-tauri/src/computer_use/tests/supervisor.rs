//! App-side checks that call the same admission and lifecycle as the engine crate.
use std::path::PathBuf;

use gcu_host_protocol::supervisor::{
    EngineChoice, EngineFacts, HostBuild, HostState, Phase, ReleaseIdentity, Signal, Step,
};

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
