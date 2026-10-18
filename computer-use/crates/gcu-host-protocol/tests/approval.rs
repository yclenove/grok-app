//! Approval desk: a grant exists only after one real request is consumed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gcu_host_protocol::approval::{
    host_callbacks, sanitize_plain, Act, BindFault, CardView, Claim, Fault, Notice, Office, Outcome,
};
use gcu_host_protocol::auth::ProcessInstance;
use gcu_host_protocol::messages::{
    App, Decision, Identity, Platform, Policy, Request, RequestApp, Risk, Signature,
};
use gcu_host_protocol::supervisor::baseline_policy;

fn engine() -> ProcessInstance {
    ProcessInstance {
        pid: 41,
        started: 9001,
    }
}

fn other_engine() -> ProcessInstance {
    ProcessInstance {
        pid: 42,
        started: 9001,
    }
}

fn digest() -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    bytes[0] = 0xab;
    bytes[31] = 0xcd;
    bytes
}

fn request_for(session: &str, key: &str, path: &str) -> RequestApp {
    RequestApp {
        session_id: session.to_owned(),
        app: App {
            name: "Editor".to_owned(),
            pid: Some(77),
            identity: Identity {
                platform: Platform::Linux,
                key: key.to_owned(),
                team_id: None,
                publisher: Some("Desk".to_owned()),
                sha256: Some(digest()),
                signature: Signature::Valid,
                image_path: path.to_owned(),
                proof: Default::default(),
            },
            icon: None,
        },
        reason: Some("open the file".to_owned()),
        risk: Risk::Low,
    }
}

fn request() -> RequestApp {
    request_for("sess", "linux:editor", "/usr/bin/editor")
}

fn attach(office: &Office) {
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| true));
}

/// Wait until the publish hook has entered, without blocking the test runtime.
async fn entered(rx: std::sync::mpsc::Receiver<()>) {
    tokio::task::spawn_blocking(move || rx.recv())
        .await
        .unwrap()
        .unwrap();
}

fn claim(
    card_image: &gcu_host_protocol::approval::ImageBinding,
    card: &CardView,
    act: Act,
) -> Claim {
    Claim {
        request_id: card.request_id,
        session_id: card.session_id.clone(),
        image: card_image.clone(),
        engine: engine(),
        policy_version: card.policy_version,
        act,
    }
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "gcu-approval-{}-{}-{}",
        std::process::id(),
        name,
        nanos
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_policy(name: &str, policy: &Policy) -> PathBuf {
    let path = scratch(name).join("policy.json");
    gcu_host_protocol::owner_only::atomic_write(
        &path,
        &serde_json::to_vec(policy).unwrap(),
        gcu_host_protocol::owner_only::SyncPolicy::Required,
    )
    .unwrap();
    path
}

fn rpc(id: u64, method: &str, params: serde_json::Value) -> Request {
    Request {
        jsonrpc: "2.0".to_owned(),
        id: Some(id),
        method: method.to_owned(),
        params,
    }
}

#[test]
fn sanitize_plain_bidi_and_controls_become_spaces_or_replacement() {
    let clean = sanitize_plain("a\nb\t\u{202E}c\u{0001}d\u{2066}");
    assert_eq!(clean, "a b \u{FFFD}c\u{FFFD}d\u{FFFD}");
    assert_eq!(sanitize_plain(&"x".repeat(300)).chars().count(), 280);
}

#[tokio::test]
async fn bind_records_session_image_engine_and_policy_version() {
    let office = Office::open(None);
    let mut notes = office.subscribe();
    attach(&office);
    let (card, wait) = office.bind(7, &request(), 1_500).unwrap();
    assert_eq!(card.session_id, "sess");
    assert_eq!(card.image.key, "linux:editor");
    assert_eq!(card.image.image_path, "/usr/bin/editor");
    assert_eq!(card.image.pid, Some(77));
    assert_eq!(card.engine_pid, 41);
    assert_eq!(card.engine_started, 9001);
    assert_eq!(card.policy_version, 1);
    assert!(card.always_enabled);
    assert_eq!(card.opened_ms, 1_500);
    match notes.try_recv().unwrap() {
        Notice::Pending { card: pending } => assert_eq!(pending.request_id, 7),
        other => panic!("pending notice, got {other:?}"),
    }
    drop(wait);
}

#[tokio::test]
async fn decide_without_request_is_absent_and_stores_nothing() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    drop(wait);
    let missing = claim(&card.image, &card, Act::Always);
    let mut missing = missing;
    missing.request_id = 99;
    assert_eq!(office.decide(missing).await, Err(Fault::Absent));
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 1);
}

#[tokio::test]
async fn bind_without_engine_or_image_key_does_not_open_a_card() {
    let office = Office::open(None);
    assert_eq!(
        office.bind(1, &request(), 0).err(),
        Some(BindFault::NoEngine)
    );
    attach(&office);
    assert_eq!(
        office.bind(2, &request_for("sess", "", "/bin/ed"), 0).err(),
        Some(BindFault::Unverified)
    );
    assert_eq!(
        office
            .bind(3, &request_for("sess", "linux:editor", ""), 0)
            .err(),
        Some(BindFault::Unverified)
    );
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn decide_cross_identity_stays_pending_until_the_real_claim() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(4, &request(), 0).unwrap();
    let mut wrong_path = claim(&card.image, &card, Act::Always);
    wrong_path.image.image_path = "/tmp/other".to_owned();
    assert_eq!(office.decide(wrong_path).await, Err(Fault::Identity));
    let mut wrong_sha = claim(&card.image, &card, Act::Turn);
    wrong_sha.image.sha256_hex = Some("ab".to_owned());
    assert_eq!(office.decide(wrong_sha).await, Err(Fault::Identity));
    let mut wrong_engine = claim(&card.image, &card, Act::Session);
    wrong_engine.engine = other_engine();
    assert_eq!(office.decide(wrong_engine).await, Err(Fault::Identity));
    let mut wrong_version = claim(&card.image, &card, Act::Deny);
    wrong_version.policy_version = 9;
    assert_eq!(office.decide(wrong_version).await, Err(Fault::Identity));
    let mut wrong_session = claim(&card.image, &card, Act::Turn);
    wrong_session.session_id = "other".to_owned();
    assert_eq!(office.decide(wrong_session).await, Err(Fault::Identity));
    assert!(office.policy().always.is_empty());
    office
        .decide(claim(&card.image, &card, Act::Deny))
        .await
        .unwrap();
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Deny)).await,
        Err(Fault::Replay)
    );
    drop(wait);
}

#[tokio::test]
async fn decide_turn_session_and_deny_do_not_persist() {
    let office = Office::open(None);
    attach(&office);
    for (id, act) in [(1_u64, Act::Turn), (2, Act::Session), (3, Act::Deny)] {
        let (card, wait) = office.bind(id, &request(), 0).unwrap();
        office.decide(claim(&card.image, &card, act)).await.unwrap();
        drop(wait);
    }
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 1);
}

#[tokio::test]
async fn decide_always_persists_once_then_publishes_the_new_version() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let office = Office::open(None);
    office.attach(engine(), None);
    let calls2 = Arc::clone(&calls);
    let seen2 = Arc::clone(&seen);
    office.set_publish_hook(Arc::new(move |version| {
        calls2.fetch_add(1, Ordering::SeqCst);
        seen2.lock().unwrap().push(version);
        true
    }));
    let (card, wait) = office.bind(8, &request(), 0).unwrap();
    let task = tokio::spawn({
        let office = office.clone();
        let made = claim(&card.image, &card, Act::Always);
        async move { office.decide(made).await }
    });
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Always));
    task.await.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().unwrap().as_slice(), &[2]);
    let policy = office.policy();
    assert_eq!(policy.version, 2);
    assert_eq!(policy.always.len(), 1);
    assert_eq!(policy.always[0].key, "linux:editor");
    assert_eq!(policy.always[0].image_path, "/usr/bin/editor");
    assert_eq!(policy.always[0].sha256, Some(digest()));
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Always)).await,
        Err(Fault::Replay)
    );
}

#[tokio::test]
async fn decide_always_when_disabled_is_deny_without_a_version_bump() {
    let mut policy = baseline_policy();
    policy.always_allow_enabled = false;
    let path = write_policy("disabled", &policy);
    let office = Office::open(Some(path));
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::clone(&calls);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(move |_| {
        calls2.fetch_add(1, Ordering::SeqCst);
        true
    }));
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    assert!(!card.always_enabled);
    office
        .decide(claim(&card.image, &card, Act::Always))
        .await
        .unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Deny));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(office.policy().version, 1);
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn decide_always_publish_failure_rolls_back_and_stays_pending() {
    let office = Office::open(None);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(|_| false));
    let (card, wait) = office.bind(3, &request(), 0).unwrap();
    let made = claim(&card.image, &card, Act::Always);
    assert_eq!(office.decide(made).await, Err(Fault::Failed));
    assert_eq!(office.policy().version, 1);
    assert!(office.policy().always.is_empty());
    office.set_publish_hook(Arc::new(|_| true));
    office
        .decide(claim(&card.image, &card, Act::Turn))
        .await
        .unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Turn));
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn decide_always_disk_failure_does_not_consume_or_publish() {
    let dir = scratch("disk-fail");
    let blocker = dir.join("not-a-directory");
    std::fs::write(&blocker, b"x").unwrap();
    let office = Office::open(Some(blocker.join("policy.json")));
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::clone(&calls);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(move |_| {
        calls2.fetch_add(1, Ordering::SeqCst);
        true
    }));
    let (card, _wait) = office.bind(1, &request(), 0).unwrap();
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Always)).await,
        Err(Fault::Failed)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(office.policy().version, 1);
    office
        .decide(claim(&card.image, &card, Act::Deny))
        .await
        .unwrap();
}

#[tokio::test]
async fn decide_always_writes_0600_policy_and_reloads_it() {
    let dir = scratch("disk-ok");
    let path = dir.join("nested").join("policy.json");
    let office = Office::open(Some(path.clone()));
    attach(&office);
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    office
        .decide(claim(&card.image, &card, Act::Always))
        .await
        .unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Always));
    let raw = std::fs::read(&path).unwrap();
    let loaded: Policy = serde_json::from_value(
        serde_json::from_slice::<serde_json::Value>(&raw).unwrap()["policy"].clone(),
    )
    .unwrap();
    assert_eq!(loaded.version, 2);
    assert_eq!(loaded.always.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }
    let reloaded = Office::open(Some(path));
    assert_eq!(reloaded.policy().version, 2);
    assert_eq!(reloaded.policy().always[0].key, "linux:editor");
}

#[tokio::test]
async fn decide_always_bumps_version_when_the_identity_is_already_stored() {
    let mut policy = baseline_policy();
    policy.version = 4;
    policy.always.push(Identity {
        platform: Platform::Linux,
        key: "linux:editor".to_owned(),
        team_id: None,
        publisher: Some("Desk".to_owned()),
        sha256: Some(digest()),
        signature: Signature::Valid,
        image_path: "/usr/bin/editor".to_owned(),
        proof: Default::default(),
    });
    let path = write_policy("already", &policy);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    let office = Office::open(Some(path));
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(move |version| {
        seen2.lock().unwrap().push(version);
        true
    }));
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    assert_eq!(card.policy_version, 4);
    office
        .decide(claim(&card.image, &card, Act::Always))
        .await
        .unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Always));
    assert_eq!(office.policy().version, 5);
    assert_eq!(office.policy().always.len(), 1);
    assert_eq!(seen.lock().unwrap().as_slice(), &[5]);
}

#[tokio::test]
async fn decide_always_version_overflow_fails_closed() {
    let mut policy = baseline_policy();
    policy.version = u64::MAX;
    let path = write_policy("overflow", &policy);
    let office = Office::open(Some(path));
    attach(&office);
    let (card, _wait) = office.bind(1, &request(), 0).unwrap();
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Always)).await,
        Err(Fault::Failed)
    );
    assert_eq!(office.policy().version, u64::MAX);
    assert!(office.policy().always.is_empty());
    office
        .decide(claim(&card.image, &card, Act::Deny))
        .await
        .unwrap();
}

#[tokio::test]
async fn close_session_denies_that_session_and_leaves_the_other() {
    let office = Office::open(None);
    attach(&office);
    let (first, first_wait) = office
        .bind(1, &request_for("one", "linux:editor", "/bin/ed"), 0)
        .unwrap();
    let (second, second_wait) = office
        .bind(2, &request_for("two", "linux:other", "/bin/other"), 0)
        .unwrap();
    let task = tokio::spawn(async move { first_wait.recv().await });
    office.close_session("one");
    assert_eq!(task.await.unwrap(), Outcome::Decision(Decision::Deny));
    assert_eq!(
        office.decide(claim(&first.image, &first, Act::Turn)).await,
        Err(Fault::Replay)
    );
    office
        .decide(claim(&second.image, &second, Act::Turn))
        .await
        .unwrap();
    assert_eq!(second_wait.recv().await, Outcome::Decision(Decision::Turn));
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn close_session_during_publish_denies_and_drops_the_grant() {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let calls = Arc::new(AtomicUsize::new(0));
    let office = Office::open(None);
    office.attach(engine(), None);
    let calls2 = Arc::clone(&calls);
    office.set_publish_hook(Arc::new(move |_| {
        let n = calls2.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let _ = entered_tx.send(());
            let _ = release_rx.lock().unwrap().recv();
        }
        true
    }));
    let (card, wait) = office.bind(5, &request(), 0).unwrap();
    let made = claim(&card.image, &card, Act::Always);
    let office_decide = office.clone();
    let decide_task = tokio::spawn(async move { office_decide.decide(made).await });
    let wait_task = tokio::spawn(async move { wait.recv().await });
    entered(entered_rx).await;
    office.close_session("sess");
    release_tx.send(()).unwrap();
    assert_eq!(wait_task.await.unwrap(), Outcome::Decision(Decision::Deny));
    assert_eq!(decide_task.await.unwrap(), Err(Fault::Failed));
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 3);
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Always)).await,
        Err(Fault::Replay)
    );
    assert!(calls.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn decide_while_publishing_is_replay() {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let office = Office::open(None);
    office.attach(engine(), None);
    office.set_publish_hook(Arc::new(move |_| {
        let _ = entered_tx.send(());
        let _ = release_rx.lock().unwrap().recv();
        true
    }));
    let (card, wait) = office.bind(6, &request(), 0).unwrap();
    let first = claim(&card.image, &card, Act::Always);
    let second = first.clone();
    let office_decide = office.clone();
    let task = tokio::spawn(async move { office_decide.decide(first).await });
    entered(entered_rx).await;
    assert_eq!(office.decide(second).await, Err(Fault::Replay));
    release_tx.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Always));
    assert_eq!(office.policy().version, 2);
}

#[tokio::test(start_paused = true)]
async fn countdown_elapsed_is_timeout_not_a_grant() {
    let mut policy = baseline_policy();
    policy.timeout_secs = 1;
    let path = write_policy("timeout", &policy);
    let office = Office::open(Some(path));
    attach(&office);
    let (card, wait) = office.bind(9, &request(), 0).unwrap();
    assert_eq!(card.timeout_secs, 1);
    let task = tokio::spawn(async move { wait.recv().await });
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(task.await.unwrap(), Outcome::TimedOut);
    assert!(office.policy().always.is_empty());
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Timeout)).await,
        Err(Fault::Replay)
    );
}

#[tokio::test]
async fn act_timeout_consumes_without_a_grant() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(11, &request(), 0).unwrap();
    office
        .decide(claim(&card.image, &card, Act::Timeout))
        .await
        .unwrap();
    assert_eq!(wait.recv().await, Outcome::TimedOut);
    assert!(office.policy().always.is_empty());
    assert_eq!(office.policy().version, 1);
}

#[tokio::test]
async fn bind_reason_strips_bidi_and_keeps_identity_raw() {
    let mut asked = request();
    asked.reason = Some("go\u{202E}west\nnow".to_owned());
    asked.app.name = "Name\u{202A}x".to_owned();
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(1, &asked, 0).unwrap();
    assert_eq!(card.reason, "go\u{FFFD}west now");
    assert_eq!(card.name, "Name\u{FFFD}x");
    assert_eq!(card.image.image_path, "/usr/bin/editor");
    office
        .decide(claim(&card.image, &card, Act::Turn))
        .await
        .unwrap();
    drop(wait);
}

#[tokio::test]
async fn corrupt_policy_file_opens_as_baseline_with_no_grants() {
    let path = scratch("corrupt").join("policy.json");
    gcu_host_protocol::owner_only::atomic_write(
        &path,
        b"not-json",
        gcu_host_protocol::owner_only::SyncPolicy::Required,
    )
    .unwrap();
    let office = Office::open(Some(path));
    assert_eq!(office.policy().version, 1);
    assert!(office.policy().always.is_empty());
    assert!(office.policy().always_allow_enabled);
}

#[tokio::test]
async fn detach_denies_pending_and_a_later_bind_needs_a_new_engine() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    let task = tokio::spawn(async move { wait.recv().await });
    office.detach();
    assert_eq!(task.await.unwrap(), Outcome::Decision(Decision::Deny));
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Turn)).await,
        Err(Fault::Replay)
    );
    assert_eq!(
        office.bind(2, &request(), 0).err(),
        Some(BindFault::NoEngine)
    );
}

#[tokio::test]
async fn deny_pending_keeps_the_engine_for_a_later_request() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(1, &request(), 0).unwrap();
    office.deny_pending();
    assert_eq!(wait.recv().await, Outcome::Decision(Decision::Deny));
    let (next, next_wait) = office.bind(2, &request(), 0).unwrap();
    office
        .decide(claim(&next.image, &next, Act::Deny))
        .await
        .unwrap();
    drop(next_wait);
    let _ = card;
}

#[tokio::test]
async fn request_app_session_close_returns_deny() {
    let office = Office::open(None);
    office.attach(engine(), None);
    let mut notes = office.subscribe();
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(request()).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(15, "host.approval.requestApp", params))
            .await
    });
    let Notice::Pending { card } = notes.recv().await.unwrap() else {
        panic!("pending");
    };
    office.close_session(&card.session_id);
    let value = task.await.unwrap().unwrap();
    assert_eq!(value["decision"], "deny");
    assert!(office.policy().always.is_empty());
}

#[tokio::test(start_paused = true)]
async fn request_app_timeout_is_rpc_error_not_a_grant() {
    let mut policy = baseline_policy();
    policy.timeout_secs = 1;
    let office = Office::open(Some(write_policy("rpc-timeout", &policy)));
    office.attach(engine(), None);
    let callbacks = host_callbacks(office.clone());
    let params = serde_json::to_value(request()).unwrap();
    let task = tokio::spawn(async move {
        callbacks
            .handle(rpc(16, "host.approval.requestApp", params))
            .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.code, -32001);
    assert_eq!(error.message, "approval_timeout");
    assert!(office.policy().always.is_empty());
}

#[tokio::test]
async fn callbacks_deny_confirm_and_load_the_office_policy() {
    let office = Office::open(None);
    let callbacks = host_callbacks(office.clone());
    let confirmed = callbacks
        .handle(rpc(1, "host.approval.confirmAction", serde_json::json!({})))
        .await
        .unwrap();
    assert_eq!(confirmed["confirmed"], false);
    let loaded = callbacks
        .handle(rpc(2, "host.policy.load", serde_json::json!({})))
        .await
        .unwrap();
    assert_eq!(loaded["version"], 1);
    let denied = callbacks
        .handle(rpc(
            3,
            "host.approval.requestApp",
            serde_json::to_value(request()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(denied["decision"], "deny");
    let empty = callbacks
        .handle(rpc(4, "host.approval.handoff", serde_json::json!({})))
        .await
        .unwrap();
    assert!(empty.as_object().unwrap().is_empty());
}

#[tokio::test]
async fn dropping_the_wait_times_the_request_out() {
    let office = Office::open(None);
    attach(&office);
    let (card, wait) = office.bind(21, &request(), 0).unwrap();
    drop(wait);
    assert_eq!(
        office.decide(claim(&card.image, &card, Act::Always)).await,
        Err(Fault::Replay)
    );
    assert!(office.policy().always.is_empty());
}
