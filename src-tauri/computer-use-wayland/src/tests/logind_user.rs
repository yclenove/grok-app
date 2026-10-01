#[path = "logind_user_fixture.rs"]
mod fixture;
#[path = "logind_user_lifetime.rs"]
mod lifetime;
use super::logind::{ready, PATH};
use fixture::*;
use std::{collections::HashMap, sync::atomic::Ordering, time::Duration};
use zbus::zvariant::{OwnedValue, Value};

async fn lost(owner: tokio::task::JoinHandle<Result<(), String>>) -> String {
    tokio::time::timeout(Duration::from_secs(2), owner)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err()
}

#[tokio::test]
async fn user_manager_and_shell_peer_bind_without_session_environment() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let mut watch = f.watch().await.unwrap();
    watch
        .verify_peer(1234, unsafe { libc::geteuid() })
        .await
        .unwrap();
    assert_eq!(f.state.pid_user_reads.load(Ordering::SeqCst), 2);
    let policy = watch.input_policy();
    assert!(!policy.input_available());
    let (send, recv) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(recv));
    ready(&policy).await;
    f.host.deny.store(true, Ordering::SeqCst);
    assert!(!policy.input_available());
    f.host.deny.store(false, Ordering::SeqCst);
    assert!(policy.input_available());
    send.send(()).unwrap();
    owner.await.unwrap().unwrap();
    assert!(!policy.input_available());
}

#[tokio::test]
async fn direct_app_with_user_service_peer_retains_inventory_fence() {
    let state = State::default();
    state
        .direct
        .lock()
        .unwrap()
        .insert(std::process::id(), PATH.try_into().unwrap());
    let f = Fixture::new(state, Session::default()).await;
    let mut watch = f.watch().await.unwrap();
    assert_eq!(f.state.pid_user_reads.load(Ordering::SeqCst), 0);
    watch
        .verify_peer(1234, unsafe { libc::geteuid() })
        .await
        .unwrap();
    let policy = watch.input_policy();
    let (_send, recv) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(recv));
    ready(&policy).await;
    f.manager_signal("SessionNew", session_ref("other", OTHER))
        .await;
    assert!(lost(owner).await.contains("graphical"));
    assert!(!policy.input_available());
}

#[tokio::test]
async fn only_exact_no_session_error_can_use_user_manager() {
    let state = State::default();
    state.generic_error.store(true, Ordering::SeqCst);
    let f = Fixture::new(state, Session::default()).await;
    assert!(f.watch().await.err().unwrap().contains("Failed"));
    assert_eq!(f.state.pid_user_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn actual_ssh_session_cannot_escape_to_graphical_display() {
    let state = State::default();
    state
        .direct
        .lock()
        .unwrap()
        .insert(std::process::id(), SSH.try_into().unwrap());
    let f = Fixture::new(state, Session::default()).await;
    assert!(f.watch().await.err().unwrap().contains("local Wayland"));
    assert_eq!(f.state.pid_user_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn user_manager_rejects_foreign_pid_owner_and_uid() {
    let state = State::default();
    state.foreign_user.store(true, Ordering::SeqCst);
    let f = Fixture::new(state, Session::default()).await;
    assert!(f.watch().await.err().unwrap().contains("authenticated UID"));
    let state = State {
        uid: unsafe { libc::geteuid() }.wrapping_add(1),
        ..State::default()
    };
    let f = Fixture::new(state, Session::default()).await;
    assert!(f.watch().await.err().unwrap().contains("UID mismatch"));
}

#[tokio::test]
async fn user_manager_rejects_unsafe_graphical_session_snapshots() {
    for session in [
        Session {
            locked: true,
            ..Session::default()
        },
        Session {
            active: false,
            ..Session::default()
        },
        Session {
            remote: true,
            ..Session::default()
        },
        Session {
            kind: "x11",
            ..Session::default()
        },
        Session {
            class: "greeter",
            ..Session::default()
        },
        Session {
            uid: unsafe { libc::geteuid() }.wrapping_add(1),
            ..Session::default()
        },
    ] {
        let f = Fixture::new(State::default(), session).await;
        assert!(f.watch().await.is_err());
    }
}

#[tokio::test]
async fn user_manager_rejects_missing_duplicate_ambiguous_or_wrong_display_inventory() {
    for refs in [
        vec![],
        vec![session_ref("c42", PATH), session_ref("c42", PATH)],
        vec![session_ref("c42", PATH), session_ref("other", OTHER)],
        vec![session_ref("ssh", SSH)],
        vec![session_ref("wrong-id", PATH)],
        vec![session_ref("c42", PATH); 65],
    ] {
        let state = State::default();
        *state.sessions.lock().unwrap() = refs;
        let f = Fixture::new(state, Session::default()).await;
        assert!(f.watch().await.is_err());
    }
    for display in [
        session_ref("", "/"),
        session_ref("self", "/org/freedesktop/login1/session/self"),
        session_ref("other", OTHER),
    ] {
        let state = State::default();
        *state.display.lock().unwrap() = display;
        let f = Fixture::new(state, Session::default()).await;
        assert!(f.watch().await.is_err());
    }
}

#[tokio::test]
async fn ssh_inventory_churn_preserves_same_original_watch() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, stop, owner) = f.started().await;
    let before = f.state.user_reads.load(Ordering::SeqCst);
    f.state
        .sessions
        .lock()
        .unwrap()
        .push(session_ref("ssh", SSH));
    f.manager_signal("SessionNew", session_ref("ssh", SSH))
        .await;
    f.wait_reads(before + 2).await;
    ready(&policy).await;
    assert!(!owner.is_finished());
    f.state
        .sessions
        .lock()
        .unwrap()
        .retain(|r| r.1.as_str() != SSH);
    f.manager_signal("SessionRemoved", session_ref("ssh", SSH))
        .await;
    f.wait_reads(before + 4).await;
    ready(&policy).await;
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    assert!(!policy.input_available());
}

#[tokio::test]
async fn second_graphical_signal_cannot_hide_behind_positive_inventory() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, _stop, owner) = f.started().await;
    // Current User.Sessions deliberately stays unchanged: inspect the signal.
    f.manager_signal("SessionNew", session_ref("other", OTHER))
        .await;
    assert!(lost(owner).await.contains("graphical"));
    assert!(!policy.input_available());
}

#[tokio::test]
async fn display_change_pulse_cannot_resurrect_old_watch() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, _stop, owner) = f.started().await;
    for display in [session_ref("other", OTHER), session_ref("c42", PATH)] {
        f.changed(
            HashMap::from([(
                "Display".into(),
                OwnedValue::try_from(Value::from(display)).unwrap(),
            )]),
            vec![],
        )
        .await;
    }
    assert!(lost(owner).await.contains("Display changed"));
    assert!(!policy.input_available());
}

#[tokio::test]
async fn invalidated_user_inventory_is_permanent_loss() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, _stop, owner) = f.started().await;
    f.changed(HashMap::new(), vec!["Sessions".into()]).await;
    assert!(lost(owner).await.contains("invalidated"));
    assert!(!policy.input_available());
}

#[tokio::test]
async fn cancel_during_original_refresh_closes_and_joins() {
    let f = Fixture::new(State::default(), Session::default()).await;
    let (policy, stop, owner) = f.started().await;
    f.state.hold_user();
    f.manager_signal("SessionNew", session_ref("ssh", SSH))
        .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while policy.input_available() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_millis(300), owner)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!policy.input_available());
    f.state.release_user();
}
