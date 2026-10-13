//! Scripted approval calls: decisions, timeout, cancel, and recorded requests.
#![cfg(feature = "testing")]

use gcu_host_api::testing::{block_on, ScriptedApprovals};
use gcu_host_api::{
    ActionSummary, AppIdentity, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, HandoffReason,
    ImageRef, ModelReason, Platform, Risk, SessionCtx, SignatureState,
};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn session() -> SessionCtx {
    SessionCtx {
        session_id: "session-1".into(),
    }
}

fn notes() -> AppInfo {
    let icon = ImageRef::from_bytes("image/png", 1, 1, vec![0x89, 0x50, 0x4e, 0x47]);
    AppInfo {
        name: "Notes".into(),
        pid: Some(42),
        identity: AppIdentity {
            platform: Platform::Macos,
            key: "com.example.notes".into(),
            team_id: Some("TEAM123456".into()),
            publisher: None,
            sha256: Some([0x11; 32]),
            signature: SignatureState::Valid,
            image_path: "/Applications/Notes.app/Contents/MacOS/Notes".into(),
        },
        icon: Some(icon),
    }
}

fn finish_within<T: Send + 'static>(handle: thread::JoinHandle<T>, timeout: Duration) -> T {
    let started = Instant::now();
    loop {
        if handle.is_finished() {
            return match handle.join() {
                Ok(value) => value,
                Err(_) => panic!("worker panicked"),
            };
        }
        if started.elapsed() > timeout {
            panic!("worker did not finish within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn request_app_scripted_queue_returns_turn_session_always_and_deny() {
    let approvals = ScriptedApprovals::new();
    let cancel = Cancel::new();
    let session = session();
    let app = notes();
    let reason = ModelReason::new("read the open note");
    let decisions = [
        ApprovalDecision::Turn,
        ApprovalDecision::Session,
        ApprovalDecision::Always,
        ApprovalDecision::Deny,
    ];
    for decision in decisions {
        approvals.push_decision(decision);
    }
    for decision in decisions {
        let got =
            block_on(approvals.request_app(&session, &app, Some(&reason), Risk::Low, &cancel));
        assert_eq!(got, decision);
    }
    assert!(approvals.queued_decisions().is_empty());
    assert_eq!(approvals.app_requests().len(), 4);
}

#[test]
fn request_app_scripted_timeout_returns_timeout() {
    let approvals = ScriptedApprovals::new();
    approvals.push_decision(ApprovalDecision::Timeout);
    let decision =
        block_on(approvals.request_app(&session(), &notes(), None, Risk::Low, &Cancel::new()));
    assert_eq!(decision, ApprovalDecision::Timeout);
}

#[test]
fn request_app_cancel_with_queued_decision_returns_deny_and_keeps_decision() {
    let approvals = ScriptedApprovals::new();
    approvals.push_decision(ApprovalDecision::Turn);
    let cancel = Cancel::new();
    let clone = cancel.clone();
    cancel.cancel();
    cancel.cancel();
    assert!(clone.is_cancelled());
    let decision = block_on(approvals.request_app(&session(), &notes(), None, Risk::High, &cancel));
    assert_eq!(decision, ApprovalDecision::Deny);
    assert_eq!(approvals.queued_decisions(), vec![ApprovalDecision::Turn]);
    assert_eq!(approvals.app_requests().len(), 1);
}

#[test]
fn request_app_cancel_while_pending_returns_deny() {
    let approvals = Arc::new(ScriptedApprovals::new());
    let cancel = Cancel::new();
    let worker = {
        let approvals = Arc::clone(&approvals);
        let cancel = cancel.clone();
        let session = session();
        let app = notes();
        thread::spawn(move || {
            block_on(approvals.request_app(&session, &app, None, Risk::High, &cancel))
        })
    };
    let started = Instant::now();
    while approvals.app_requests().is_empty() {
        if started.elapsed() > Duration::from_secs(2) {
            panic!("request was not recorded");
        }
        thread::yield_now();
    }
    if worker.is_finished() {
        panic!("request returned before cancel");
    }
    cancel.cancel();
    let decision = finish_within(worker, Duration::from_secs(2));
    assert_eq!(decision, ApprovalDecision::Deny);
    assert!(approvals.queued_decisions().is_empty());
}

#[test]
fn request_app_records_identity_reason_risk_and_icon_bytes() {
    let approvals = ScriptedApprovals::new();
    let app = notes();
    let reason = ModelReason::new("read the open note");
    approvals.push_decision(ApprovalDecision::Session);
    let decision = block_on(approvals.request_app(
        &session(),
        &app,
        Some(&reason),
        Risk::High,
        &Cancel::new(),
    ));
    assert_eq!(decision, ApprovalDecision::Session);
    let recorded = approvals.app_requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].session.session_id, "session-1");
    assert_eq!(recorded[0].app.identity, app.identity);
    assert_eq!(recorded[0].app.pid, Some(42));
    assert_eq!(
        recorded[0].reason.as_ref().map(ModelReason::as_str),
        Some("read the open note")
    );
    assert_eq!(recorded[0].risk, Risk::High);
    assert_eq!(
        recorded[0].app.icon.as_ref().map(ImageRef::as_bytes),
        Some(&[0x89, 0x50, 0x4e, 0x47][..])
    );
}

#[test]
fn confirm_action_records_snapshot_digest_and_crop_bytes() {
    let approvals = ScriptedApprovals::new();
    let summary = ActionSummary {
        text: "click Send".into(),
        snapshot_id: "s_7f3a".into(),
        action_digest: [0xab; 32],
    };
    let crop = ImageRef::from_bytes("image/png", 2, 2, vec![9, 8, 7, 6]);
    approvals.push_confirm(true);
    let confirmed = block_on(approvals.confirm_action(
        &session(),
        &notes(),
        &summary,
        Some(&crop),
        &Cancel::new(),
    ));
    assert!(confirmed);
    let recorded = approvals.confirm_requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].summary, summary);
    assert_eq!(
        recorded[0].crop.as_ref().map(ImageRef::as_bytes),
        Some(crop.as_bytes())
    );
    assert!(approvals.queued_confirms().is_empty());
}

#[test]
fn confirm_action_cancel_returns_false_and_keeps_answer() {
    let approvals = ScriptedApprovals::new();
    approvals.push_confirm(true);
    let cancel = Cancel::new();
    cancel.cancel();
    let confirmed = block_on(approvals.confirm_action(
        &session(),
        &notes(),
        &ActionSummary {
            text: "click Send".into(),
            snapshot_id: "s_7f3a".into(),
            action_digest: [0xab; 32],
        },
        None,
        &cancel,
    ));
    assert!(!confirmed);
    assert_eq!(approvals.queued_confirms(), vec![true]);
}

#[test]
fn handoff_records_engine_reason() {
    let approvals = ScriptedApprovals::new();
    block_on(approvals.handoff(&session(), &notes(), HandoffReason::Captcha, &Cancel::new()));
    let recorded = approvals.handoffs();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].reason, HandoffReason::Captcha);
    assert_eq!(recorded[0].app.name, "Notes");
}

#[test]
fn cancel_already_signalled_wakes_without_parking_forever() {
    let cancel = Cancel::new();
    cancel.cancel();
    block_on(cancel.cancelled());
    assert!(cancel.is_cancelled());
}

#[test]
fn approval_provider_is_object_safe() {
    let approvals = ScriptedApprovals::new();
    approvals.push_decision(ApprovalDecision::Turn);
    let provider: &dyn ApprovalProvider = &approvals;
    let decision =
        block_on(provider.request_app(&session(), &notes(), None, Risk::Low, &Cancel::new()));
    assert_eq!(decision, ApprovalDecision::Turn);
}
