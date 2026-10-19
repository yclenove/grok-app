//! Settings mutations exercise the same office used by production Host callbacks.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{Arc, Mutex};

use gcu_host_protocol::{
    approval::{host_callbacks, Act, Claim, Fault, Office, Outcome},
    auth::ProcessInstance,
    messages::{
        App, ConfirmationLevel, Decision, ForegroundPolicy, Identity, Platform, Policy, Request,
        RequestApp, Risk, Signature,
    },
};

fn engine() -> ProcessInstance {
    ProcessInstance {
        pid: 41,
        started: 9001,
    }
}

fn request() -> RequestApp {
    RequestApp {
        session_id: "local".into(),
        app: App {
            name: "Editor".into(),
            pid: Some(77),
            identity: Identity {
                platform: Platform::Linux,
                key: "editor".into(),
                team_id: None,
                publisher: None,
                sha256: Some([7; 32]),
                signature: Signature::Unsigned,
                image_path: "/fixture/editor".into(),
                proof: Default::default(),
            },
            icon: None,
        },
        reason: None,
        risk: Risk::Low,
    }
}

fn claim(card: &gcu_host_protocol::approval::CardView, act: Act) -> Claim {
    Claim {
        request_id: card.request_id,
        session_id: card.session_id.clone(),
        image: card.image.clone(),
        engine: engine(),
        policy_version: card.policy_version,
        act,
    }
}

async fn grant(office: &Office, id: u64) {
    let (card, wait) = office.bind(id, &request(), 1234).unwrap();
    office.decide(claim(&card, Act::Always)).await.unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Always));
}

fn path() -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!(
            "gcu-settings-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ))
        .join("policy.json")
}

#[tokio::test]
async fn revoke_always_next_call_requests_a_new_card_and_persists_removal() {
    let path = path();
    let office = Office::open(Some(path.clone()));
    office.attach(engine(), None);
    // Stand in for engine.policy.changed: cache only what host.policy.load owns.
    let cached = Arc::new(Mutex::new(office.policy()));
    let loaded = cached.clone();
    let host_policy = office.clone();
    office.set_publish_hook(Arc::new(move |version| {
        let policy = host_policy.policy();
        assert_eq!(policy.version, version);
        *loaded.lock().unwrap() = policy;
        true
    }));
    grant(&office, 1).await;
    assert_eq!(cached.lock().unwrap().always.len(), 1);
    let rows = office.approvals();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Editor");
    assert!(rows[0].granted_ms.is_some());
    office.revoke(&rows[0].id).await.unwrap();
    assert!(cached.lock().unwrap().always.is_empty());
    assert!(Office::open(Some(path.clone())).approvals().is_empty());

    let mut notices = office.subscribe();
    let callbacks = host_callbacks(office.clone());
    let next = tokio::spawn(async move {
        callbacks
            .handle(Request {
                jsonrpc: "2.0".into(),
                id: Some(2),
                method: "host.approval.requestApp".into(),
                params: serde_json::to_value(request()).unwrap(),
            })
            .await
            .unwrap()
    });
    let gcu_host_protocol::approval::Notice::Pending { card } = notices.recv().await.unwrap()
    else {
        panic!("next call must show a fresh approval card");
    };
    assert_eq!(card.policy_version, 3);
    office.decide(claim(&card, Act::Deny)).await.unwrap();
    assert_eq!(next.await.unwrap()["decision"], "deny");
    office.set_publish_hook(Arc::new(|_| true));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn change_preferences_offline_persists_and_preserves_grants() {
    let path = path();
    let office = Office::open(Some(path.clone()));
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
    grant(&office, 1).await;
    let granted_ms = office.approvals()[0].granted_ms;
    office.detach();
    office
        .set_preferences(ConfirmationLevel::Strict, ForegroundPolicy::Never)
        .await
        .unwrap();
    office.save_enabled(true).unwrap();
    let loaded = Office::open(Some(path.clone()));
    assert_eq!(
        loaded.policy().sensitive_action_confirmation,
        ConfirmationLevel::Strict
    );
    assert_eq!(loaded.policy().allow_foreground, ForegroundPolicy::Never);
    assert_eq!(loaded.approvals().len(), 1);
    assert_eq!(loaded.approvals()[0].name, "Editor");
    assert_eq!(loaded.approvals()[0].granted_ms, granted_ms);
    assert!(loaded.enabled());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn revoke_publish_failure_retains_revocation_and_disconnects_cached_authority() {
    let office = Office::open(None);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
    grant(&office, 1).await;
    office.set_publish_hook(Arc::new(|_| false));
    assert!(office.revoke(&office.approvals()[0].id).await.is_err());
    assert!(office.policy().always.is_empty());
    assert!(office.bind(2, &request(), 0).is_err());
}

#[tokio::test]
async fn write_failure_keeps_the_previous_policy_and_reports_error() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"blocker").unwrap();
    let office = Office::open(Some(path.join("policy.json")));
    assert!(office
        .set_preferences(ConfirmationLevel::Off, ForegroundPolicy::Always)
        .await
        .is_err());
    assert_eq!(office.policy().version, 1);
    assert_eq!(
        office.policy().sensitive_action_confirmation,
        ConfirmationLevel::Standard
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn legacy_policy_retains_identity_without_invented_name_or_time() {
    let path = path();
    let mut policy = gcu_host_protocol::supervisor::baseline_policy();
    policy.always.push(request().app.identity);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    gcu_host_protocol::owner_only::atomic_write(
        &path,
        &serde_json::to_vec(&policy).unwrap(),
        gcu_host_protocol::owner_only::SyncPolicy::Required,
    )
    .unwrap();
    let office = Office::open(Some(path.clone()));
    assert_eq!(office.approvals()[0].name, "editor");
    assert_eq!(office.approvals()[0].granted_ms, None);
    assert!(!office.enabled());
    office.revoke(&office.approvals()[0].id).await.unwrap();
    assert!(Office::open(Some(path.clone())).policy().always.is_empty());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn revoke_unknown_identity_leaves_revision_and_grants_unchanged() {
    let office = Office::open(None);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
    grant(&office, 1).await;
    assert_eq!(office.revoke("another-build").await, Err(Fault::Absent));
    assert_eq!(office.policy().version, 2);
    assert_eq!(office.approvals().len(), 1);
    assert!(office.bind(2, &request(), 0).is_ok());
}

#[tokio::test]
async fn policy_edit_invalidates_pending_card_before_it_can_restore_revoked_authority() {
    let office = Office::open(None);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
    grant(&office, 1).await;
    let (card, wait) = office.bind(2, &request(), 0).unwrap();
    office.revoke(&office.approvals()[0].id).await.unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Deny));
    assert_eq!(
        office.decide(claim(&card, Act::Always)).await,
        Err(Fault::Replay)
    );
    assert!(office.policy().always.is_empty());
}

#[test]
fn corrupt_file_defaults_off_and_grants_nothing() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    gcu_host_protocol::owner_only::atomic_write(
        &path,
        b"invalid",
        gcu_host_protocol::owner_only::SyncPolicy::Required,
    )
    .unwrap();
    let office = Office::open(Some(path.clone()));
    assert!(!office.enabled());
    assert!(office.approvals().is_empty());
    let _: Policy = office.policy();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
