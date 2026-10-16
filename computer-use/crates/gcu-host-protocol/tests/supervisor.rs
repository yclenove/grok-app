//! Supervisor admission, idle exit, handshake failure, and the crash breaker.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use gcu_host_protocol::{
    messages::{AppDecision, Decision, Policy, Request},
    supervisor::{
        admit, baseline_policy, closed_callbacks, content_sha256, ensure_engine_file,
        parse_codesign_display, privilege_allowed, Admission, Control, EngineChoice, EngineFacts,
        EngineProcess, Error, HostBuild, HostState, Launch, LaunchRequest, LivePrivilege, Phase,
        PollExit, ReleaseIdentity, Signal, SpawnFuture, Step, BREAKER_WINDOW_MS, IDLE_MS,
    },
};

struct Script {
    fail: bool,
    privilege: LivePrivilege,
    exit: PollExit,
    spawns: u32,
    last_stopped: bool,
}

struct FakeLaunch {
    script: Arc<Mutex<Script>>,
}

struct FakeEngine {
    script: Arc<Mutex<Script>>,
    shutdown: bool,
    killed: bool,
    privilege: LivePrivilege,
}

impl EngineProcess for FakeEngine {
    fn poll_exit(&mut self) -> PollExit {
        if self.shutdown || self.killed {
            PollExit::Clean
        } else {
            lock(&self.script).exit
        }
    }
    fn begin_shutdown(&mut self) {
        self.shutdown = true;
    }
    fn kill(&mut self) {
        self.killed = true;
    }
    fn privilege(&self) -> LivePrivilege {
        self.privilege
    }
}

impl Launch for FakeLaunch {
    fn spawn<'a>(&'a mut self, request: &'a LaunchRequest) -> SpawnFuture<'a> {
        let script = Arc::clone(&self.script);
        let stopped = request.stopped;
        Box::pin(async move {
            let mut guard = lock(&script);
            guard.spawns += 1;
            guard.last_stopped = stopped;
            let fail = guard.fail;
            let privilege = guard.privilege;
            drop(guard);
            if fail {
                return Err(Error::Handshake(gcu_host_protocol::Error::Timeout));
            }
            Ok(Box::new(FakeEngine {
                script,
                shutdown: false,
                killed: false,
                privilege,
            }) as Box<dyn EngineProcess>)
        })
    }
}

fn lock(script: &Mutex<Script>) -> std::sync::MutexGuard<'_, Script> {
    script.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn script() -> Arc<Mutex<Script>> {
    Arc::new(Mutex::new(Script {
        fail: false,
        privilege: LivePrivilege::normal(),
        exit: PollExit::Running,
        spawns: 0,
        last_stopped: false,
    }))
}

fn control(mode: HostBuild, script: Arc<Mutex<Script>>) -> Control<FakeLaunch> {
    let path = PathBuf::from("/engine");
    let facts = EngineFacts::content(&path, "abc");
    let request = LaunchRequest::new(path.clone(), false);
    Control::new(
        FakeLaunch { script },
        request,
        Admission {
            mode,
            expected: ReleaseIdentity::ContentSha256 {
                sha256: "abc".into(),
            },
            choice: EngineChoice::Bundled { path },
            facts,
        },
    )
}

fn spawns(script: &Mutex<Script>) -> u32 {
    lock(script).spawns
}

fn bundled(path: &str) -> (ReleaseIdentity, EngineChoice, EngineFacts) {
    let path = PathBuf::from(path);
    (
        ReleaseIdentity::ContentSha256 {
            sha256: "shipped".into(),
        },
        EngineChoice::Bundled { path: path.clone() },
        EngineFacts::content(path, "shipped"),
    )
}

#[tokio::test]
async fn start_twice_while_running_does_not_spawn_again() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::SetEnabled(true), 1).await.unwrap();
    assert_eq!(spawns(&script), 1);
    assert_eq!(host.status().phase, Phase::Running);
}

#[tokio::test]
async fn stop_twice_while_stopped_is_idempotent() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(false), 0).await.unwrap();
    host.handle(Signal::SetEnabled(false), 1).await.unwrap();
    assert_eq!(spawns(&script), 0);
    assert_eq!(host.status().phase, Phase::Stopped);
    host.handle(Signal::SetEnabled(true), 2).await.unwrap();
    host.handle(Signal::SetEnabled(false), 3).await.unwrap();
    host.poll(4).await.unwrap();
    host.handle(Signal::SetEnabled(false), 5).await.unwrap();
    assert_eq!(host.status().phase, Phase::Stopped);
    assert_eq!(spawns(&script), 1);
}

#[tokio::test]
async fn handshake_failure_stays_stopped_and_preserves_stop_latch() {
    let script = script();
    lock(&script).fail = true;
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::UserStop, 0).await.unwrap();
    let error = host.handle(Signal::SetEnabled(true), 1).await.unwrap_err();
    assert!(matches!(
        error,
        Error::Handshake(gcu_host_protocol::Error::Timeout)
    ));
    host.poll(2).await.unwrap();
    assert_eq!(spawns(&script), 1);
    assert_eq!(host.status().phase, Phase::Stopped);
    assert!(host.status().stop_latched);
    assert!(host.status().enabled);
}

#[tokio::test]
async fn third_crash_within_ten_minutes_trips_and_disables() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    lock(&script).exit = PollExit::Crash;
    host.poll(1_000).await.unwrap();
    host.poll(2_000).await.unwrap();
    host.poll(3_000).await.unwrap();
    assert_eq!(spawns(&script), 3);
    assert_eq!(host.status().phase, Phase::Tripped);
    assert!(!host.status().enabled);
    let error = host
        .handle(Signal::SetEnabled(true), 4_000)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Tripped));
    host.handle(Signal::SetEnabled(false), 5_000).await.unwrap();
    assert_eq!(host.status().phase, Phase::Stopped);
    host.handle(Signal::SetEnabled(true), 6_000).await.unwrap();
    assert_eq!(host.status().phase, Phase::Running);
}

#[tokio::test]
async fn crash_outside_window_does_not_trip() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    lock(&script).exit = PollExit::Crash;
    host.poll(0).await.unwrap();
    host.poll(0).await.unwrap();
    host.poll(BREAKER_WINDOW_MS).await.unwrap();
    assert_eq!(host.status().phase, Phase::Running);
    assert!(host.status().enabled);
    assert_eq!(spawns(&script), 4);
}

#[tokio::test]
async fn idle_sixty_seconds_without_sessions_requests_shutdown() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.poll(IDLE_MS).await.unwrap();
    assert_eq!(host.status().phase, Phase::Stopping);
    host.poll(IDLE_MS).await.unwrap();
    assert_eq!(host.status().phase, Phase::Stopped);
    assert!(host.status().enabled);
    host.poll(IDLE_MS * 2).await.unwrap();
    assert_eq!(spawns(&script), 1);
}

#[tokio::test]
async fn active_session_does_not_idle_exit() {
    let script = script();
    let mut host = control(HostBuild::Dev, script);
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::Sessions(1), 1).await.unwrap();
    host.poll(IDLE_MS + 1).await.unwrap();
    assert_eq!(host.status().phase, Phase::Running);
    assert_eq!(host.status().sessions, 1);
}

#[tokio::test]
async fn restart_after_crash_keeps_stop_latch() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::UserStop, 1).await.unwrap();
    lock(&script).exit = PollExit::Crash;
    host.poll(2).await.unwrap();
    assert!(host.status().stop_latched);
    assert!(lock(&script).last_stopped);
    assert_eq!(host.status().phase, Phase::Running);
}

#[tokio::test]
async fn user_resume_clears_latch_and_restart_does_not() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::UserStop, 1).await.unwrap();
    host.handle(Signal::UserResume, 2).await.unwrap();
    lock(&script).exit = PollExit::Crash;
    host.poll(3).await.unwrap();
    assert!(!host.status().stop_latched);
    assert!(!lock(&script).last_stopped);
}

#[tokio::test]
async fn clean_shutdown_is_not_a_crash() {
    let script = script();
    let mut host = control(HostBuild::Dev, script);
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::SetEnabled(false), 1).await.unwrap();
    host.poll(2).await.unwrap();
    assert_eq!(host.status().phase, Phase::Stopped);
    host.handle(Signal::SetEnabled(true), 3).await.unwrap();
    assert_eq!(host.status().phase, Phase::Running);
}

#[tokio::test]
async fn reenable_during_stopping_spawns_after_clean_exit() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    host.handle(Signal::SetEnabled(false), 1).await.unwrap();
    host.handle(Signal::SetEnabled(true), 2).await.unwrap();
    assert_eq!(spawns(&script), 1);
    assert_eq!(host.status().phase, Phase::Stopping);
    host.poll(3).await.unwrap();
    assert_eq!(spawns(&script), 2);
    assert_eq!(host.status().phase, Phase::Running);
}

#[tokio::test]
async fn release_unknown_privilege_is_refused() {
    let script = script();
    lock(&script).privilege = LivePrivilege::unknown();
    let mut host = control(HostBuild::Release, script);
    let error = host.handle(Signal::SetEnabled(true), 0).await.unwrap_err();
    assert!(matches!(
        error,
        Error::Refused(gcu_host_protocol::supervisor::Reject::Elevation)
    ));
    assert_eq!(host.status().phase, Phase::Stopped);
}

#[tokio::test]
async fn dev_unknown_privilege_is_allowed() {
    let script = script();
    lock(&script).privilege = LivePrivilege::unknown();
    let mut host = control(HostBuild::Dev, script);
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    assert_eq!(host.status().phase, Phase::Running);
}

#[test]
fn release_bundled_identity_mismatch_is_refused() {
    let (identity, choice, mut facts) = bundled("/engine");
    facts.build_id = Some("other".into());
    let error = admit(HostBuild::Release, &identity, &choice, &facts).unwrap_err();
    assert_eq!(
        error,
        gcu_host_protocol::supervisor::Reject::IdentityMismatch
    );
}

#[test]
fn release_identifier_only_is_refused() {
    let path = PathBuf::from("/engine");
    let identity = ReleaseIdentity::MacDeveloperId {
        team_id: "TEAM".into(),
        signing_id: "app".into(),
        cdhash: "cd".into(),
    };
    let facts = EngineFacts {
        canonical_path: path.clone(),
        team_id: None,
        signing_id: Some("app".into()),
        build_id: Some("cd".into()),
        dev_authorized: false,
        elevated: false,
        ui_access: false,
        privilege_unknown: false,
    };
    let choice = EngineChoice::Bundled { path };
    assert_eq!(
        admit(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::IdentifierOnly
    );
    assert!(admit(HostBuild::Dev, &identity, &choice, &facts).is_ok());
}

#[test]
fn release_external_path_is_refused() {
    let path = PathBuf::from("/outside");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let facts = EngineFacts::content(&path, "other");
    let choice = EngineChoice::External {
        path,
        dev_grant: true,
    };
    assert_eq!(
        admit(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::ExternalNotAllowed
    );
}

#[test]
fn dev_external_without_grant_is_refused() {
    let path = PathBuf::from("/outside");
    let identity = ReleaseIdentity::ContentSha256 {
        sha256: "shipped".into(),
    };
    let mut facts = EngineFacts::content(&path, "other");
    facts.dev_authorized = false;
    let choice = EngineChoice::External {
        path,
        dev_grant: false,
    };
    assert_eq!(
        admit(HostBuild::Dev, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::DevGrantMissing
    );
}

#[test]
fn dev_external_with_separate_grant_is_allowed() {
    let path = PathBuf::from("/outside");
    let identity = ReleaseIdentity::MacDeveloperId {
        team_id: "TEAM".into(),
        signing_id: "app".into(),
        cdhash: "cd".into(),
    };
    let facts = EngineFacts {
        canonical_path: path.clone(),
        team_id: Some("OTHER".into()),
        signing_id: Some("ext".into()),
        build_id: Some("nope".into()),
        dev_authorized: true,
        elevated: false,
        ui_access: false,
        privilege_unknown: false,
    };
    let choice = EngineChoice::External {
        path,
        dev_grant: true,
    };
    assert!(admit(HostBuild::Dev, &identity, &choice, &facts).is_ok());
}

#[test]
fn dev_external_reusing_release_identity_is_refused() {
    let path = PathBuf::from("/outside");
    let identity = ReleaseIdentity::MacDeveloperId {
        team_id: "TEAM".into(),
        signing_id: "app".into(),
        cdhash: "cd".into(),
    };
    let facts = EngineFacts {
        canonical_path: path.clone(),
        team_id: Some("TEAM".into()),
        signing_id: Some("app".into()),
        build_id: Some("cd".into()),
        dev_authorized: true,
        elevated: false,
        ui_access: false,
        privilege_unknown: false,
    };
    let choice = EngineChoice::External {
        path,
        dev_grant: true,
    };
    assert_eq!(
        admit(HostBuild::Dev, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::ReleaseIdentityReused
    );
}

#[test]
fn release_elevated_or_uiaccess_is_refused() {
    let (identity, choice, mut facts) = bundled("/engine");
    facts.elevated = true;
    assert_eq!(
        admit(HostBuild::Release, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::Elevation
    );
    facts.elevated = false;
    facts.ui_access = true;
    assert_eq!(
        admit(HostBuild::Dev, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::UiAccess
    );
    assert_eq!(
        privilege_allowed(HostBuild::Release, false, false, true).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::Elevation
    );
    assert!(privilege_allowed(HostBuild::Dev, false, false, true).is_ok());
}

#[test]
fn path_mismatch_and_authenticode_follow_the_same_rule() {
    let (identity, choice, mut facts) = bundled("/engine");
    facts.canonical_path = PathBuf::from("/other");
    assert_eq!(
        admit(HostBuild::Dev, &identity, &choice, &facts).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::PathMismatch
    );
    let path = PathBuf::from("/engine");
    let identity = ReleaseIdentity::WindowsAuthenticode {
        publisher: "CN=Grok".into(),
        certificate_sha256: "cert".into(),
    };
    let mut facts = EngineFacts::content(&path, "cert");
    facts.team_id = Some("CN=Grok".into());
    assert!(admit(
        HostBuild::Release,
        &identity,
        &EngineChoice::Bundled { path },
        &facts
    )
    .is_ok());
}

#[test]
fn codesign_display_ignores_not_set_and_content_hash_is_stable() {
    let facts = parse_codesign_display(
        "Identifier=app.id\nTeamIdentifier=not set\nCDHash=ABCDEF\nInfo.plist entries=12\n",
    );
    assert_eq!(facts.signing_id.as_deref(), Some("app.id"));
    assert_eq!(facts.team_id, None);
    assert_eq!(facts.cdhash.as_deref(), Some("ABCDEF"));
    let file = std::env::temp_dir().join(format!("gcu-sha-{}", std::process::id()));
    std::fs::write(&file, b"a").unwrap();
    let hash = content_sha256(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    assert_eq!(
        hash,
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
    );
    assert_eq!(
        ensure_engine_file(PathBuf::from("/no/such/gcu").as_path()).unwrap_err(),
        gcu_host_protocol::supervisor::Reject::MissingBinary
    );
}

#[test]
fn disable_while_starting_kills_without_a_crash_step() {
    let mut state = HostState::new();
    assert_eq!(state.apply(Signal::SetEnabled(true), 0), Step::Spawn);
    assert_eq!(state.apply(Signal::SetEnabled(false), 1), Step::Kill);
    assert_eq!(
        state.apply(Signal::ChildExited { clean: true }, 2),
        Step::None
    );
    assert_eq!(state.phase, Phase::Stopped);
    assert!(!state.enabled);
}

#[tokio::test]
async fn closed_host_loads_policy_and_denies_approval() {
    let callbacks = closed_callbacks(baseline_policy());
    let policy = callbacks
        .handle(Request {
            jsonrpc: "2.0".into(),
            id: Some(1),
            method: "host.policy.load".into(),
            params: serde_json::json!({}),
        })
        .await
        .unwrap();
    let policy: Policy = serde_json::from_value(policy).unwrap();
    assert_eq!(policy.version, 1);
    assert!(policy.always.is_empty());
    let decision = callbacks
        .handle(Request {
            jsonrpc: "2.0".into(),
            id: Some(2),
            method: "host.approval.requestApp".into(),
            params: serde_json::json!({}),
        })
        .await
        .unwrap();
    let decision: AppDecision = serde_json::from_value(decision).unwrap();
    assert_eq!(decision.decision, Decision::Deny);
    let confirmed = callbacks
        .handle(Request {
            jsonrpc: "2.0".into(),
            id: Some(3),
            method: "host.approval.confirmAction".into(),
            params: serde_json::json!({}),
        })
        .await
        .unwrap();
    assert_eq!(confirmed["confirmed"], false);
}

#[tokio::test]
async fn open_local_endpoint_without_peer_is_disconnected() {
    let script = script();
    let mut host = control(HostBuild::Dev, Arc::clone(&script));
    let missing = host.open_local_endpoint("chat").await;
    assert!(matches!(missing, Err(Error::Disconnected)));
    host.handle(Signal::SetEnabled(true), 0).await.unwrap();
    let running = host.open_local_endpoint("chat").await;
    assert!(matches!(running, Err(Error::Disconnected)));
    let closed = host.close_endpoint("chat").await;
    assert!(matches!(closed, Err(Error::Disconnected)));
}
