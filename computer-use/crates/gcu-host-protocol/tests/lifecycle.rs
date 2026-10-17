//! The App uses this same lifecycle owner; no core or driver is linked here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "support/mod.rs"]
mod support;

use gcu_host_protocol::{
    approval::{ConfirmEnd, HandoffEnd, Outcome},
    events::Fault,
    messages::*,
    overlay::safety::{FileLatch, Safety},
};
use std::sync::Arc;
use support::lifecycle::*;

#[tokio::test]
async fn turn_end_duplicate_or_old_generation_never_cleans_a_new_turn() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let old = fixture.lifecycle.begin_turn("a").unwrap();
    fixture.lifecycle.end_turn(old.clone()).await.unwrap();
    fixture.lifecycle.end_turn(old.clone()).await.unwrap();
    assert_eq!(fixture.calls("engine.session.turnEnded"), 1);
    let same_namespace = fixture.lifecycle.begin_turn("a").unwrap();
    fixture.lifecycle.end_turn(old.clone()).await.unwrap();
    assert_eq!(fixture.calls("engine.session.turnEnded"), 1);
    fixture.lifecycle.end_turn(same_namespace).await.unwrap();
    fixture.lifecycle.close("a").await.unwrap();
    fixture.open("a").await;
    let next = fixture.lifecycle.begin_turn("a").unwrap();
    fixture.lifecycle.end_turn(old).await.unwrap();
    assert_eq!(fixture.calls("engine.session.turnEnded"), 2);
    fixture.lifecycle.end_turn(next).await.unwrap();
    assert_eq!(fixture.calls("engine.session.turnEnded"), 3);
}

#[tokio::test]
async fn queued_approval_from_ended_turn_cannot_bind_in_the_next_turn() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let turn = fixture.lifecycle.begin_turn("a").unwrap();
    let queued = fixture.callbacks.handle(Request {
        jsonrpc: "2.0".into(),
        id: Some(7),
        method: "host.approval.requestApp".into(),
        params: serde_json::to_value(fixture.request("a")).unwrap(),
    });
    fixture.lifecycle.end_turn(turn).await.unwrap();
    let next = fixture.lifecycle.begin_turn("a").unwrap();
    assert!(queued.await.is_err());
    fixture.lifecycle.end_turn(next).await.unwrap();
}

#[tokio::test]
async fn close_fork_and_model_change_revoke_credentials_images_and_pending_approvals() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    fixture.open("b").await;
    let first = fixture.lifecycle.current("a").unwrap();
    let wait_a = fixture.pending("a", 10);
    let wait_b = fixture.pending("b", 11);
    let path = fixture.image("a", "shot-a");
    let other = fixture.image("b", "shot-b");
    fixture.lifecycle.close("a").await.unwrap();
    fixture.lifecycle.close("a").await.unwrap();
    assert_eq!(
        wait_a.recv().await,
        gcu_host_protocol::approval::Outcome::Decision(Decision::Deny)
    );
    assert!(fixture.lifecycle.current("a").is_none());
    assert!(fixture.lifecycle.current("b").is_some());
    assert!(!path.exists());
    assert!(other.exists());
    assert_eq!(fixture.gallery.paths("a", "shot-a"), Err(Fault::Missing));
    assert!(fixture
        .gallery
        .admit("a", "late", &png(), gcu_host_protocol::events::Role::Full)
        .is_err());
    assert_eq!(fixture.calls("engine.session.close"), 1);
    fixture.open("a").await;
    assert_ne!(fixture.lifecycle.current("a").unwrap().token, first.token);
    fixture.lifecycle.close("b").await.unwrap();
    assert_eq!(
        wait_b.recv().await,
        gcu_host_protocol::approval::Outcome::Decision(Decision::Deny)
    );
}

#[tokio::test]
async fn disconnected_engine_invalidates_cached_endpoint_and_late_prompt_completion() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let turn = fixture.lifecycle.begin_turn("a").unwrap();
    let pending = fixture.pending("a", 1);
    fixture.disconnect();
    assert!(fixture.lifecycle.current("a").is_none());
    assert_eq!(
        pending.recv().await,
        gcu_host_protocol::approval::Outcome::Decision(Decision::Deny)
    );
    fixture.lifecycle.end_turn(turn).await.unwrap();
    assert_eq!(fixture.calls("engine.session.turnEnded"), 0);
    assert!(fixture.lifecycle.open("a").await.is_err());
    assert!(fixture.lifecycle.needs_reconnect("a"));
}

#[tokio::test]
async fn turn_end_denies_all_pending_lanes_without_retiring_session_credential() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let before = fixture.lifecycle.current("a").unwrap();
    let app = fixture.request("a").app;
    let pending = fixture.pending("a", 10);
    let confirm = fixture
        .office
        .bind_confirm(
            11,
            &ConfirmAction {
                session_id: "a".into(),
                app: app.clone(),
                crop: Some(png()),
                summary: ActionSummary {
                    text: "Save".into(),
                    snapshot_id: "snapshot".into(),
                    action_digest: [1; 32],
                },
            },
            0,
        )
        .unwrap()
        .1;
    let handoff = fixture
        .office
        .bind_handoff(
            12,
            &Handoff {
                session_id: "a".into(),
                app,
                reason: "fixture takeover".into(),
            },
            0,
        )
        .unwrap()
        .1;
    let turn = fixture.lifecycle.begin_turn("a").unwrap();
    fixture.lifecycle.end_turn(turn).await.unwrap();
    assert_eq!(pending.recv().await, Outcome::Decision(Decision::Deny));
    assert_eq!(
        confirm.recv().await,
        Outcome::Confirmed {
            allowed: false,
            end: ConfirmEnd::Denied
        }
    );
    assert_eq!(
        handoff.recv().await,
        Outcome::HandedOff {
            end: HandoffEnd::Closed
        }
    );
    assert_eq!(before.token, fixture.lifecycle.current("a").unwrap().token);
    assert!(!fixture.lifecycle.needs_reconnect("a"));
}

#[tokio::test]
async fn engine_replacement_old_callbacks_and_old_image_response_cannot_revive_authority() {
    let first = Fixture::new().await;
    first.open("a").await;
    let credential = first.lifecycle.current("a").unwrap();
    let queued = first.callbacks.handle(Request {
        jsonrpc: "2.0".into(),
        id: Some(7),
        method: "host.approval.requestApp".into(),
        params: serde_json::to_value(first.request("a")).unwrap(),
    });
    let replacement = first.replacement().await;
    replacement.open("a").await;
    // A late disconnect from the previous process must not close the new peer.
    first.callbacks.disconnected();
    assert!(replacement.lifecycle.current("a").is_some());
    assert!(queued.await.is_err());
    assert_eq!(
        replacement
            .lifecycle
            .admit_full("a", &credential, "late", &png()),
        Err(Fault::Missing)
    );
    assert_ne!(
        credential.token,
        replacement.lifecycle.current("a").unwrap().token
    );
}

#[tokio::test]
async fn concurrent_open_close_and_late_gallery_cleanup_are_idempotent_and_isolated() {
    let fixture = Fixture::new().await;
    let (a, again) = tokio::join!(fixture.lifecycle.open("a"), fixture.lifecycle.open("a"));
    assert_eq!(a.unwrap().token, again.unwrap().token);
    assert_eq!(fixture.calls("engine.session.open"), 1);
    let mut notices = fixture.lifecycle.subscribe_closed();
    let (one, two) = tokio::join!(fixture.lifecycle.close("a"), fixture.lifecycle.close("a"));
    one.unwrap();
    two.unwrap();
    assert_eq!(fixture.calls("engine.session.close"), 1);
    assert_eq!(notices.recv().await.unwrap(), "a");
    assert!(notices.try_recv().is_err());
    fixture.open("a").await;
    let path = fixture.image("a", "new");
    fixture.gallery.purge_closed("a");
    assert!(path.exists());
    assert!(!fixture.lifecycle.needs_reconnect("a"));
}

#[tokio::test]
async fn catalog_cleared_disabled_feature_stops_reconnect_without_reusing_old_header() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    fixture.open("b").await;
    fixture.lifecycle.catalog_cleared("a");
    assert!(fixture.lifecycle.current("a").is_some());
    fixture.lifecycle.close("a").await.unwrap();
    fixture.lifecycle.close("b").await.unwrap();
    assert!(fixture.lifecycle.needs_reconnect("a"));
    fixture.lifecycle.catalog_cleared("a");
    assert!(!fixture.lifecycle.needs_reconnect("a"));
    assert!(fixture.lifecycle.current("a").is_none());
    assert!(fixture.lifecycle.needs_reconnect("b"));
}

#[tokio::test]
async fn lifecycle_boundaries_restart_and_model_requests_never_clear_persisted_stop() {
    let fixture = Fixture::new().await;
    let latch = Arc::new(FileLatch(std::env::temp_dir().join(format!(
        "gcu-life-stop-{}",
        gcu_host_protocol::auth::Secret::random().hex()
    ))));
    let safety = Safety::new(latch.clone());
    safety.latch();
    safety.persist().await.unwrap();
    fixture.open("a").await;
    let turn = fixture.lifecycle.begin_turn("a").unwrap();
    fixture.lifecycle.end_turn(turn).await.unwrap();
    fixture.lifecycle.close("a").await.unwrap();
    fixture.open("a").await;
    let replacement = fixture.replacement().await;
    replacement.open("a").await;
    assert!(safety.stopped());
    assert!(Safety::new(latch.clone()).stopped());
    assert!(safety.resume_by_user().await.is_err()); // No trusted control peer.
    assert!(safety.stopped());
    assert!(gcu_protocol::ToolName::ALL
        .iter()
        .all(|tool| !tool.as_str().contains("resume")));
    assert_eq!(fixture.calls("engine.session.resume"), 0);
    assert_eq!(replacement.calls("engine.session.resume"), 0);
    let _ = std::fs::remove_file(&latch.0);
}

#[tokio::test]
async fn cancelled_close_revokes_before_waiting_for_engine() {
    use std::future::Future;
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let pending = fixture.pending("a", 1);
    let mut closing = Box::pin(fixture.lifecycle.close("a"));
    std::future::poll_fn(|cx| {
        assert!(closing.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(fixture.lifecycle.current("a").is_none());
    assert_eq!(pending.recv().await, Outcome::Decision(Decision::Deny));
    drop(closing);
    assert!(!fixture.peer.connected());
    fixture.disconnect();
    assert!(fixture.lifecycle.current("a").is_none());
}

#[tokio::test]
async fn failed_close_keeps_token_revoked_and_denies_approvals_and_images() {
    let fixture = Fixture::new().await;
    fixture.open("a").await;
    let pending = fixture.pending("a", 1);
    let path = fixture.image("a", "old");
    // The App observes a lost transport before its disconnect callback runs.
    fixture.peer.close();
    assert!(fixture.lifecycle.close("a").await.is_err());
    assert_eq!(pending.recv().await, Outcome::Decision(Decision::Deny));
    assert!(fixture.lifecycle.current("a").is_none());
    assert!(fixture.lifecycle.needs_reconnect("a"));
    assert!(!path.exists());
    assert_eq!(fixture.gallery.paths("a", "old"), Err(Fault::Missing));
    fixture.disconnect();
}
