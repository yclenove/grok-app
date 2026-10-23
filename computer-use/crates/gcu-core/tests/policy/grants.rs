use super::support::*;

#[tokio::test]
async fn grant_tiers_expire_on_turn_and_session_close() {
    let app = notes();
    let config = config();
    let activity = FakeUserActivity::new();
    let session = Session::open("s", Arc::new(activity), Arc::new(NopRelease));
    let activity = FakeUserActivity::new();
    session.begin_turn().unwrap();
    session.record_decision(ApprovalDecision::Turn, &app);
    let state = ActionIntent::new(Tool::GetAppState);
    let decision = eval(
        &app,
        &config,
        &session.grants(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    session.turn_ended().await;
    let decision = eval(
        &app,
        &config,
        &session.grants(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    session.record_decision(ApprovalDecision::Session, &app);
    session.begin_turn().unwrap();
    session.turn_ended().await;
    let decision = eval(
        &app,
        &config,
        &session.grants(),
        &activity,
        None,
        None,
        &state,
        session.is_closed(),
    );
    assert!(matches!(decision, PolicyDecision::Allow { .. }));
    session.session_closed().await;
    let decision = eval(
        &app,
        &config,
        &session.grants(),
        &activity,
        None,
        None,
        &state,
        session.is_closed(),
    );
    assert_eq!(
        refused(&decision),
        Some(gcu_protocol::ErrorCode::ApprovalDenied)
    );
    assert!(session.grants().covers(&config, &app).is_none());
}

#[test]
fn always_grant_follows_platform_identity_and_can_be_disabled() {
    let activity = FakeUserActivity::new();
    let linux = notes();
    let mut config = config();
    config.always = vec![linux.identity.clone()];
    let state = ActionIntent::new(Tool::GetAppState);
    let allow = eval(
        &linux,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(allow, PolicyDecision::Allow { .. }));
    let mut replaced = linux.clone();
    replaced.identity.sha256 = Some([0x99; 32]);
    let decision = eval(
        &replaced,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    let mut unsigned = linux.clone();
    unsigned.identity.sha256 = None;
    config.always = vec![unsigned.identity.clone()];
    let decision = eval(
        &unsigned,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));

    // A moved path used to stay Allow because a valid team matched the bundle
    // alone. §7.2 requires the path, hash, cdhash, and certificate too.
    let mac = macos_notes();
    config.always = vec![mac.identity.clone()];
    let same = eval(
        &mac,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(same, PolicyDecision::Allow { .. }));
    let mut moved = mac.clone();
    moved.identity.image_path = "/tmp/Notes".into();
    moved.identity.sha256 = Some([0x55; 32]);
    let decision = eval(
        &moved,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    let mut rebuilt = mac.clone();
    rebuilt.identity.proof.cdhash = Some("ff".repeat(20));
    let decision = eval(
        &rebuilt,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    rebuilt = mac.clone();
    rebuilt.identity.proof.certificate_sha256 = Some("99".repeat(32));
    let decision = eval(
        &rebuilt,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    moved.identity.team_id = Some("OTHERTEAM".into());
    let decision = eval(
        &moved,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));

    // A narrow record cannot prove the missing facts, so it is not upgraded.
    let mut legacy = mac.identity.clone();
    legacy.proof = GrantProof::default();
    config.always = vec![legacy.clone()];
    let decision = eval(
        &mac,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    let decision = eval(
        &app("Legacy", legacy),
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));

    let windows = windows_notes();
    config.always = vec![windows.identity.clone()];
    let same = eval(
        &windows,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(same, PolicyDecision::Allow { .. }));
    let mut updated = windows.clone();
    updated.identity.sha256 = Some([0x66; 32]);
    updated.identity.image_path = "D:\\Notes\\Notes.exe".into();
    updated.identity.key = updated.identity.image_path.clone();
    let decision = eval(
        &updated,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
    updated.identity.publisher = Some("CN=Other".into());
    let decision = eval(
        &updated,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));

    config.always_allow_enabled = false;
    config.always = vec![mac.identity.clone()];
    let decision = eval(
        &mac,
        &config,
        &GrantTable::default(),
        &activity,
        None,
        None,
        &state,
        false,
    );
    assert!(matches!(decision, PolicyDecision::NeedApproval(_)));
}

#[test]
fn approval_binding_change_is_invalidated_field_by_field() {
    let app = notes();
    let binding = ApprovalBinding {
        app: app.identity.clone(),
        pid: app.pid,
        window_id: "w".into(),
        parameters: "to=ada".into(),
        action_digest: [1; 32],
        snapshot_id: "snap".into(),
        policy_version: 3,
        scope_run: None,
        scope_generation: None,
    };
    assert!(revalidate(&binding, &binding).is_ok());
    let mut current = binding.clone();
    current.app.image_path = "/other".into();
    assert_eq!(
        revalidate(&binding, &current),
        Err(Invalidated::AppIdentity)
    );
    current = binding.clone();
    current.pid = Some(99);
    assert_eq!(
        revalidate(&binding, &current),
        Err(Invalidated::ProcessInstance)
    );
    current = binding.clone();
    current.window_id = "other".into();
    assert_eq!(revalidate(&binding, &current), Err(Invalidated::Window));
    current = binding.clone();
    current.parameters = "to=grace".into();
    assert_eq!(
        revalidate(&binding, &current),
        Err(Invalidated::ActionParams)
    );
    current = binding.clone();
    current.action_digest = [2; 32];
    assert_eq!(
        revalidate(&binding, &current),
        Err(Invalidated::ActionParams)
    );
    current = binding.clone();
    current.snapshot_id = "next".into();
    assert_eq!(revalidate(&binding, &current), Err(Invalidated::Snapshot));
    current = binding.clone();
    current.policy_version = 4;
    assert_eq!(
        revalidate(&binding, &current),
        Err(Invalidated::PolicyVersion)
    );
    let rendered = format!("{binding:?}");
    assert!(!rendered.contains("to=ada"));
    assert!(rendered.contains("<redacted>"));
}

#[tokio::test(flavor = "current_thread")]
async fn approval_wait_session_close_denies_without_recording() {
    let approvals = Arc::new(ScriptedApprovals::new());
    let session = Arc::new(Session::open(
        "s",
        Arc::new(FakeUserActivity::new()),
        Arc::new(NopRelease),
    ));
    session.begin_turn().unwrap();
    let app = notes();
    let running = session.clone();
    let provider = approvals.clone();
    let target = app.clone();
    let task = tokio::spawn(async move {
        running
            .request_app(
                provider.as_ref(),
                &target,
                None,
                Risk::Low,
                Duration::from_secs(30),
            )
            .await
    });
    for _ in 0..50 {
        if !approvals.app_requests().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(!approvals.app_requests().is_empty());
    session.session_closed().await;
    assert_eq!(task.await.unwrap(), ApprovalDecision::Deny);
    assert!(session.grants().covers(&config(), &app).is_none());
    let summary = ActionSummary {
        text: "click \"Save draft\"".into(),
        snapshot_id: "snap".into(),
        action_digest: [1; 32],
    };
    let again = Arc::new(Session::open(
        "s2",
        Arc::new(FakeUserActivity::new()),
        Arc::new(NopRelease),
    ));
    let confirm_provider = approvals.clone();
    let confirm_session = again.clone();
    let confirm_app = app.clone();
    let confirm = tokio::spawn(async move {
        confirm_session
            .request_confirm(
                confirm_provider.as_ref(),
                &confirm_app,
                &summary,
                None,
                Duration::from_secs(30),
            )
            .await
    });
    for _ in 0..50 {
        if !approvals.confirm_requests().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(!approvals.confirm_requests().is_empty());
    approvals.push_confirm(true);
    again.session_closed().await;
    assert!(!confirm.await.unwrap());
    assert_eq!(approvals.queued_confirms(), vec![true]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn approval_wait_timeout_does_not_grant() {
    let approvals = ScriptedApprovals::new();
    let session = Session::open("s", Arc::new(FakeUserActivity::new()), Arc::new(NopRelease));
    session.begin_turn().unwrap();
    let app = notes();
    let decision = session
        .request_app(&approvals, &app, None, Risk::High, Duration::from_secs(30))
        .await;
    assert_eq!(decision, ApprovalDecision::Timeout);
    assert!(session.grants().covers(&config(), &app).is_none());
    assert_eq!(approvals.app_requests().len(), 1);
}
