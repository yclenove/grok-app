//! Side-panel timeline: throttle, isolation, stop latch, and banners.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;

use gcu_host_protocol::events::Gallery;
use gcu_host_protocol::messages::{Event, Image};
use gcu_host_protocol::panel::{Admit, Notice, Panel, Phase};
use gcu_protocol::result::Outcome;

fn png(payload: &[u8]) -> Image {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(payload);
    Image {
        mime: "image/png".into(),
        width: 2,
        height: 2,
        bytes,
    }
}

fn event(session: &str, target: &str, outcome: &str, duration: u64, image: Option<Image>) -> Event {
    let mut value = serde_json::json!({
        "session_id": session,
        "tool": "click",
        "target": target,
        "outcome": outcome,
        "duration_ms": duration,
        "thumbnail": null,
    });
    if let Some(image) = image {
        value["thumbnail"] = serde_json::json!({
            "mime": image.mime,
            "width": image.width,
            "height": image.height,
            "bytes": image.bytes,
        });
    }
    serde_json::from_value(value).unwrap()
}

fn click(session: &str) -> Event {
    event(session, "Save", "done", 12, None)
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gcu-panel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn push_eleven_in_one_second_keeps_ten_and_notifies_once() {
    let panel = Panel::open();
    let mut notes = panel.subscribe();
    let sample = click("sess-a");
    for index in 0..10 {
        assert_eq!(panel.push(&sample, index), Admit::Kept);
    }
    assert_eq!(panel.push(&sample, 10), Admit::Throttled);
    assert_eq!(panel.push(&sample, 11), Admit::Throttled);
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.steps.len(), 10);
    assert_eq!(view.dropped, 2);
    let mut snapshots = 0;
    while let Ok(notice) = notes.try_recv() {
        if matches!(notice, Notice::Snapshot { .. }) {
            snapshots += 1;
        }
    }
    assert_eq!(snapshots, 11);
}

#[test]
fn push_after_the_window_admits_again() {
    let panel = Panel::open();
    let sample = click("sess-a");
    for index in 0..10 {
        assert_eq!(panel.push(&sample, index), Admit::Kept);
    }
    assert_eq!(panel.push(&sample, 1_000), Admit::Kept);
    assert_eq!(panel.view("sess-a").unwrap().steps.len(), 11);
    assert_eq!(panel.view("sess-a").unwrap().dropped, 0);
}

#[test]
fn push_over_the_cap_keeps_the_newest_two_hundred() {
    let panel = Panel::open();
    for index in 0..205 {
        let sample = event("sess-a", &format!("t{index}"), "done", index, None);
        assert_eq!(panel.push(&sample, index * 100), Admit::Kept);
    }
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.steps.len(), 200);
    assert_eq!(view.steps[0].target, "t5");
    assert_eq!(view.steps[199].target, "t204");
}

#[test]
fn push_keeps_sessions_apart() {
    let panel = Panel::open();
    panel.push(&event("sess-a", "Alpha", "done", 1, None), 0);
    panel.push(&event("sess-b", "Beta", "failed", 2, None), 0);
    let alpha = panel.view("sess-a").unwrap();
    let beta = panel.view("sess-b").unwrap();
    assert_eq!(alpha.steps.len(), 1);
    assert_eq!(alpha.steps[0].target, "Alpha");
    assert_eq!(alpha.phase, Phase::Running);
    assert_eq!(beta.steps[0].target, "Beta");
    assert_eq!(beta.phase, Phase::Failed);
}

#[test]
fn view_without_steps_is_idle() {
    let panel = Panel::open();
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.phase, Phase::Idle);
    assert!(view.steps.is_empty());
    assert!(!view.stopped);
}

#[test]
fn push_failed_outcome_sets_failed_phase() {
    let panel = Panel::open();
    panel.push(&event("sess-a", "Save", "failed", 4, None), 0);
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.phase, Phase::Failed);
    assert_eq!(view.steps[0].outcome, Outcome::Failed);
}

#[test]
fn user_stop_stays_latched_when_a_later_outcome_is_done() {
    let panel = Panel::open();
    assert!(panel.user_stop());
    assert!(panel.user_stop());
    panel.push(&click("sess-a"), 5);
    let view = panel.view("sess-a").unwrap();
    assert!(view.stopped);
    assert_eq!(view.phase, Phase::Stopped);
    assert_eq!(view.steps[0].outcome, Outcome::Done);
}

#[test]
fn user_resume_without_stop_returns_false() {
    let panel = Panel::open();
    let mut notes = panel.subscribe();
    assert!(!panel.user_resume());
    assert!(notes.try_recv().is_err());
    assert!(!panel.view("sess-a").unwrap().stopped);
}

#[test]
fn user_resume_after_stop_shows_the_last_outcome() {
    let panel = Panel::open();
    panel.push(&event("sess-a", "Save", "failed", 4, None), 0);
    assert!(panel.user_stop());
    assert!(panel.user_resume());
    let view = panel.view("sess-a").unwrap();
    assert!(!view.stopped);
    assert_eq!(view.phase, Phase::Failed);
}

#[test]
fn drop_session_leaves_the_other_session() {
    let panel = Panel::open();
    panel.push(&event("sess-a", "Alpha", "done", 1, None), 0);
    panel.push(&event("sess-b", "Beta", "done", 1, None), 0);
    panel.user_stop();
    panel.drop_session("sess-a");
    assert!(panel.view("sess-a").unwrap().steps.is_empty());
    assert!(panel.view("sess-a").unwrap().stopped);
    assert_eq!(panel.view("sess-b").unwrap().steps[0].target, "Beta");
    panel.drop_session("not a key");
    panel.drop_session("missing");
    assert_eq!(panel.view("sess-b").unwrap().steps.len(), 1);
}

#[test]
fn note_restart_clears_on_the_next_kept_step() {
    let panel = Panel::open();
    let sample = click("sess-b");
    for _ in 0..10 {
        panel.push(&sample, 0);
    }
    panel.note_restart();
    panel.note_restart();
    assert_eq!(panel.push(&sample, 1), Admit::Throttled);
    assert_eq!(panel.view("sess-a").unwrap().phase, Phase::Restarted);
    assert_eq!(
        panel.push(&event("sess-a", "Save", "done", 3, None), 1_000),
        Admit::Kept
    );
    assert_eq!(panel.view("sess-a").unwrap().phase, Phase::Running);
    assert!(!panel.view("sess-b").unwrap().restarted);
}

#[test]
fn note_tripped_overrides_a_later_done_step() {
    let panel = Panel::open();
    panel.note_tripped();
    panel.push(&click("sess-a"), 0);
    let view = panel.view("sess-a").unwrap();
    assert!(view.tripped);
    assert_eq!(view.phase, Phase::Tripped);
    assert_eq!(view.steps.len(), 1);
}

#[test]
fn push_rejects_an_unsafe_session_key() {
    let panel = Panel::open();
    assert_eq!(panel.push(&click("../x"), 0), Admit::Rejected);
    assert_eq!(panel.push(&click(""), 0), Admit::Rejected);
    assert!(panel.view("../x").is_none());
}

#[test]
fn push_shot_id_matches_the_gallery_thumbnail() {
    let temp = Temp::new();
    let gallery = Gallery::open(temp.0.clone());
    let panel = Panel::open();
    let sample = event("sess-a", "Save", "done", 12, Some(png(b"pixels")));
    gallery.note(&sample).unwrap();
    assert_eq!(panel.push(&sample, 0), Admit::Kept);
    let id = panel.view("sess-a").unwrap().steps[0]
        .shot_id
        .clone()
        .unwrap();
    assert!(gallery.paths("sess-a", &id).unwrap().thumb.is_some());
}

#[test]
fn push_reuses_the_thumbnail_id_for_the_same_target() {
    let panel = Panel::open();
    let first = event("sess-a", "Save", "done", 12, Some(png(b"pixels")));
    let second = event("sess-b", "Save", "done", 12, Some(png(b"pixels")));
    panel.push(&first, 0);
    panel.push(&second, 0);
    let left = panel.view("sess-a").unwrap().steps[0].shot_id.clone();
    let right = panel.view("sess-b").unwrap().steps[0].shot_id.clone();
    assert_eq!(left, right);
    assert!(left.unwrap().starts_with("e-click-12-"));
}

#[test]
fn forgotten_notice_uses_the_ui_field_names() {
    let panel = Panel::open();
    panel.push(&click("sess-a"), 0);
    let mut notes = panel.subscribe();
    panel.drop_session("sess-a");
    let notice = notes.try_recv().unwrap();
    let value = serde_json::to_value(&notice).unwrap();
    assert_eq!(value["kind"], "forgotten");
    assert_eq!(value["sessionId"], "sess-a");
    assert!(value.get("session_id").is_none());
}

struct Inner;

impl gcu_host_protocol::server::HostCallbacks for Inner {
    fn handle(
        &self,
        _request: gcu_host_protocol::messages::Request,
    ) -> gcu_host_protocol::server::CallbackFuture<'_> {
        Box::pin(async { Ok(serde_json::json!({})) })
    }

    fn disconnected(&self) {}
}

#[tokio::test]
async fn observe_panel_records_host_event_emit() {
    let temp = Temp::new();
    let panel = Panel::open();
    let gallery = Gallery::open(temp.0.clone());
    let wrapped =
        gcu_host_protocol::events::observe_panel(Arc::new(Inner), gallery.clone(), panel.clone());
    let sample = event("sess-a", "Save", "unknown", 9, Some(png(b"pixels")));
    let request = gcu_host_protocol::messages::Request {
        jsonrpc: "2.0".into(),
        id: None,
        method: "host.event.emit".into(),
        params: serde_json::to_value(&sample).unwrap(),
    };
    wrapped.handle(request).await.unwrap();
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.phase, Phase::Unknown);
    assert_eq!(view.steps.len(), 1);
    let id = view.steps[0].shot_id.clone().unwrap();
    assert!(gallery.paths("sess-a", &id).unwrap().thumb.is_some());
}

#[test]
fn user_stop_overrides_the_restart_banner() {
    let panel = Panel::open();
    panel.note_restart();
    panel.user_stop();
    let view = panel.view("sess-a").unwrap();
    assert_eq!(view.phase, Phase::Stopped);
    assert!(view.restarted);
}
