//! Sensitive confirmation and handoff share the app approval desk.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gcu_host_protocol::approval::{
    action_digest_from_hex, host_callbacks, Act, BindFault, Claim, ConfirmAct, ConfirmClaim,
    ConfirmEnd, ConfirmView, CropView, Fault, HandoffAct, HandoffClaim, HandoffEnd, HandoffView,
    Notice, Office,
};
use gcu_host_protocol::auth::ProcessInstance;
use gcu_host_protocol::messages::{
    ActionSummary, App, ConfirmAction, Handoff, Identity, Image, Platform, Policy, Request,
    RequestApp, Risk, Signature,
};
use gcu_host_protocol::supervisor::baseline_policy;

fn engine() -> ProcessInstance {
    ProcessInstance {
        pid: 41,
        started: 9001,
    }
}

fn digest() -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    bytes[0] = 0xab;
    bytes[31] = 0xcd;
    bytes
}

fn other_digest() -> [u8; 32] {
    let mut bytes = digest();
    bytes[1] = 0x11;
    bytes
}

fn app() -> App {
    App {
        name: "Edit\u{202A}or".to_owned(),
        pid: Some(77),
        identity: Identity {
            platform: Platform::Linux,
            key: "linux:editor".to_owned(),
            team_id: None,
            publisher: Some("Desk".to_owned()),
            sha256: Some(digest()),
            signature: Signature::Valid,
            image_path: "/usr/bin/editor".to_owned(),
        },
        icon: None,
    }
}

fn request_app() -> RequestApp {
    RequestApp {
        session_id: "sess".to_owned(),
        app: app(),
        reason: Some("open the file".to_owned()),
        risk: Risk::Low,
    }
}

fn png() -> Image {
    Image {
        mime: "image/png".to_owned(),
        width: 8,
        height: 8,
        bytes: b"\x89PNG\r\n\x1a\nrest".to_vec(),
    }
}

fn confirm(text: &str, snapshot: &str, bytes: [u8; 32], crop: Option<Image>) -> ConfirmAction {
    ConfirmAction {
        session_id: "sess".to_owned(),
        app: app(),
        summary: ActionSummary {
            text: text.to_owned(),
            snapshot_id: snapshot.to_owned(),
            action_digest: bytes,
        },
        crop,
    }
}

fn handoff(reason: &str) -> Handoff {
    Handoff {
        session_id: "sess".to_owned(),
        app: app(),
        reason: reason.to_owned(),
    }
}

fn attach(office: &Office) {
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
}

fn confirm_claim(card: &ConfirmView, act: ConfirmAct) -> ConfirmClaim {
    ConfirmClaim {
        request_id: card.request_id,
        session_id: card.session_id.clone(),
        image: card.image.clone(),
        engine: engine(),
        policy_version: card.policy_version,
        snapshot_id: card.snapshot_id.clone(),
        action_digest: action_digest_from_hex(&card.action_digest).unwrap(),
        act,
    }
}

fn handoff_claim(card: &HandoffView, act: HandoffAct) -> HandoffClaim {
    HandoffClaim {
        request_id: card.request_id,
        session_id: card.session_id.clone(),
        image: card.image.clone(),
        engine: engine(),
        policy_version: card.policy_version,
        act,
    }
}

fn rpc(id: u64, method: &str, params: serde_json::Value) -> Request {
    Request {
        jsonrpc: "2.0".to_owned(),
        id: Some(id),
        method: method.to_owned(),
        params,
    }
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "gcu-confirm-{}-{}-{}",
        std::process::id(),
        name,
        nanos
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_policy(name: &str, policy: &Policy) -> PathBuf {
    let path = scratch(name).join("policy.json");
    std::fs::write(&path, serde_json::to_vec(policy).unwrap()).unwrap();
    path
}

#[tokio::test]
async fn allow_once_then_replay_does_not_grant_again() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params =
        serde_json::to_value(confirm("Click Run", "snap-1", digest(), Some(png()))).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(9, "host.approval.confirmAction", params))
            .await
    });
    let Notice::ConfirmPending { card } = notes.recv().await.unwrap() else {
        panic!("confirm pending");
    };
    assert_eq!(card.summary, "Click Run");
    assert!(card.reason.is_empty());
    assert!(matches!(card.crop, CropView::Redacted { .. }));
    assert_eq!(
        office
            .decide_confirm(&confirm_claim(&card, ConfirmAct::Allow))
            .unwrap(),
        ConfirmEnd::Allowed
    );
    let value = task.await.unwrap().unwrap();
    assert_eq!(value["confirmed"], true);
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 1);
    assert_eq!(
        office.decide_confirm(&confirm_claim(&card, ConfirmAct::Allow)),
        Err(Fault::Replay)
    );
}

#[tokio::test]
async fn deny_returns_confirmed_false_and_stores_nothing() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(confirm("Click Run", "snap-1", digest(), None)).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(10, "host.approval.confirmAction", params))
            .await
    });
    let Notice::ConfirmPending { card } = notes.recv().await.unwrap() else {
        panic!("confirm pending");
    };
    assert!(matches!(card.crop, CropView::Missing));
    assert_eq!(
        office
            .decide_confirm(&confirm_claim(&card, ConfirmAct::Deny))
            .unwrap(),
        ConfirmEnd::Denied
    );
    let value = task.await.unwrap().unwrap();
    assert_eq!(value["confirmed"], false);
    assert!(office.policy().always.is_empty());
    assert_eq!(
        office.decide_confirm(&confirm_claim(&card, ConfirmAct::Deny)),
        Err(Fault::Replay)
    );
}

#[tokio::test]
async fn wrong_digest_allow_is_stale_and_then_replay() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params =
        serde_json::to_value(confirm("Click Run", "snap-1", digest(), Some(png()))).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(11, "host.approval.confirmAction", params))
            .await
    });
    let Notice::ConfirmPending { card } = notes.recv().await.unwrap() else {
        panic!("confirm pending");
    };
    let mut claim = confirm_claim(&card, ConfirmAct::Allow);
    claim.action_digest = other_digest();
    assert_eq!(office.decide_confirm(&claim).unwrap(), ConfirmEnd::Stale);
    let value = task.await.unwrap().unwrap();
    assert_eq!(value["confirmed"], false);
    match notes.recv().await.unwrap() {
        Notice::ConfirmSettled { end, .. } => assert_eq!(end, ConfirmEnd::Stale),
        other => panic!("settled, got {other:?}"),
    }
    assert_eq!(office.decide_confirm(&claim), Err(Fault::Replay));
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn policy_version_drift_voids_an_allow() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(confirm("Click Run", "snap-1", digest(), None)).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(12, "host.approval.confirmAction", params))
            .await
    });
    let Notice::ConfirmPending { card } = notes.recv().await.unwrap() else {
        panic!("confirm pending");
    };
    let (app_card, app_wait) = office.bind(40, &request_app(), 0).unwrap();
    let app_claim = Claim {
        request_id: app_card.request_id,
        session_id: app_card.session_id.clone(),
        image: app_card.image.clone(),
        engine: engine(),
        policy_version: app_card.policy_version,
        act: Act::Always,
    };
    office.decide(app_claim).await.unwrap();
    drop(app_wait);
    assert_eq!(office.policy().version, 2);
    assert_eq!(
        office
            .decide_confirm(&confirm_claim(&card, ConfirmAct::Allow))
            .unwrap(),
        ConfirmEnd::Stale
    );
    assert_eq!(task.await.unwrap().unwrap()["confirmed"], false);
}

#[tokio::test]
async fn session_close_denies_a_confirmation() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(confirm("Click Run", "snap-1", digest(), None)).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(13, "host.approval.confirmAction", params))
            .await
    });
    let Notice::ConfirmPending { card } = notes.recv().await.unwrap() else {
        panic!("confirm pending");
    };
    office.close_session(&card.session_id);
    assert_eq!(task.await.unwrap().unwrap()["confirmed"], false);
    match notes.recv().await.unwrap() {
        Notice::ConfirmSettled { end, .. } => assert_eq!(end, ConfirmEnd::Denied),
        other => panic!("settled, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn confirm_timeout_is_not_a_grant() {
    let mut policy = baseline_policy();
    policy.timeout_secs = 1;
    let office = Office::open(Some(write_policy("confirm-timeout", &policy)));
    attach(&office);
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(confirm("Click Run", "snap-1", digest(), None)).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(14, "host.approval.confirmAction", params))
            .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(task.await.unwrap().unwrap()["confirmed"], false);
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn identity_mismatch_does_not_consume_a_confirmation() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let (card, wait) = office
        .bind_confirm(15, &confirm("Click Run", "snap-1", digest(), None), 0)
        .unwrap();
    let mut claim = confirm_claim(&card, ConfirmAct::Allow);
    claim.session_id = "other".to_owned();
    assert_eq!(office.decide_confirm(&claim), Err(Fault::Identity));
    claim.session_id = card.session_id.clone();
    assert_eq!(office.decide_confirm(&claim).unwrap(), ConfirmEnd::Allowed);
    assert!(matches!(
        wait.recv().await,
        gcu_host_protocol::approval::Outcome::Confirmed { allowed: true, .. }
    ));
    let _ = notes.try_recv();
}

#[tokio::test]
async fn app_decide_on_a_confirm_id_does_not_grant() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office
        .bind_confirm(16, &confirm("Click Run", "snap-1", digest(), None), 0)
        .unwrap();
    let app_claim = Claim {
        request_id: card.request_id,
        session_id: card.session_id.clone(),
        image: card.image.clone(),
        engine: engine(),
        policy_version: card.policy_version,
        act: Act::Always,
    };
    assert_eq!(office.decide(app_claim).await, Err(Fault::Identity));
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 1);
    assert_eq!(
        office
            .decide_confirm(&confirm_claim(&card, ConfirmAct::Deny))
            .unwrap(),
        ConfirmEnd::Denied
    );
    assert!(matches!(
        wait.recv().await,
        gcu_host_protocol::approval::Outcome::Confirmed {
            allowed: false,
            end: ConfirmEnd::Denied,
        }
    ));
}

#[tokio::test]
async fn hostile_crop_bytes_are_not_stored() {
    let office = Office::open(None);
    attach(&office);
    let hostile = b"GIF89a-not-a-png".to_vec();
    let (card, wait) = office
        .bind_confirm(
            17,
            &confirm(
                "go\u{202E}west",
                "snap-1",
                digest(),
                Some(Image {
                    mime: "image/png".to_owned(),
                    width: 8,
                    height: 8,
                    bytes: hostile.clone(),
                }),
            ),
            0,
        )
        .unwrap();
    assert_eq!(card.summary, "go\u{FFFD}west");
    assert!(matches!(card.crop, CropView::Unlocated));
    assert_eq!(card.name, "Edit\u{FFFD}or");
    let encoded = serde_json::to_vec(&card).unwrap();
    assert!(!encoded
        .windows(hostile.len())
        .any(|window| window == hostile));
    drop(wait);
    assert_eq!(
        office
            .bind_confirm(18, &confirm("x", "", digest(), None), 0)
            .err(),
        Some(BindFault::Unverified)
    );
}

#[tokio::test]
async fn handoff_handled_is_empty_and_not_a_grant() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let before = office.policy();
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(handoff("login\u{202E}x")).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(19, "host.approval.handoff", params))
            .await
    });
    let Notice::HandoffPending { card } = notes.recv().await.unwrap() else {
        panic!("handoff pending");
    };
    assert_eq!(card.code, "other");
    assert_eq!(card.detail, "login\u{FFFD}x");
    assert_eq!(
        office
            .decide_handoff(&handoff_claim(&card, HandoffAct::Handled))
            .unwrap(),
        HandoffEnd::Handled
    );
    let value = task.await.unwrap().unwrap();
    assert!(value.as_object().unwrap().is_empty());
    assert!(value.get("confirmed").is_none());
    assert_eq!(
        serde_json::to_value(office.policy()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let callbacks = host_callbacks(office.clone());
    let again = serde_json::to_value(handoff("login")).unwrap();
    let replay = callbacks
        .handle(rpc(19, "host.approval.handoff", again))
        .await
        .unwrap();
    assert!(replay.as_object().unwrap().is_empty());
    assert_eq!(
        office.decide_handoff(&handoff_claim(&card, HandoffAct::Handled)),
        Err(Fault::Replay)
    );
}

#[tokio::test]
async fn handoff_session_close_does_not_confirm() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let (card, wait) = office.bind_handoff(20, &handoff("captcha"), 0).unwrap();
    assert_eq!(card.code, "captcha");
    assert!(card.detail.is_empty());
    office.close_session(&card.session_id);
    assert!(matches!(
        wait.recv().await,
        gcu_host_protocol::approval::Outcome::HandedOff {
            end: HandoffEnd::Closed,
        }
    ));
    match notes.recv().await.unwrap() {
        Notice::HandoffPending { .. } => {}
        other => panic!("pending first, got {other:?}"),
    }
    match notes.recv().await.unwrap() {
        Notice::HandoffSettled { end, .. } => assert_eq!(end, HandoffEnd::Closed),
        other => panic!("settled, got {other:?}"),
    }
}

#[tokio::test]
async fn bad_digest_hex_is_stale() {
    assert_eq!(action_digest_from_hex("zz"), Err(Fault::Stale));
    assert_eq!(action_digest_from_hex(&"ab".repeat(32)).unwrap()[0], 0xab);
}
