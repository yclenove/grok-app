//! Portable overlay contract; native desktop checks use a separate opt-in probe.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_api::{CursorKind, OverlayProvider, ScreenPoint, SessionCtx, StatusText, WindowInfo};
use gcu_overlay::{animation::Spring, Catalog, Clock, Config, CopyKey, Overlay, Rect, State};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Default)]
struct Time(AtomicU64);
impl Clock for Time {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::Relaxed))
    }
}
struct Copy;
impl Catalog for Copy {
    fn text(&self, key: CopyKey) -> String {
        key.as_str().into()
    }
}
fn setup() -> (Overlay, Arc<Time>, SessionCtx, WindowInfo) {
    let time = Arc::new(Time::default());
    let overlay = Overlay::new(time.clone(), Arc::new(Copy), Config::default());
    (
        overlay,
        time,
        SessionCtx {
            session_id: "a".into(),
        },
        WindowInfo {
            id: "w".into(),
            title: "fixture".into(),
            frame: Rect {
                x: -1200.,
                y: 20.,
                w: 1000.,
                h: 800.,
            },
            scale: 2.,
        },
    )
}
fn monitor() -> Rect {
    Rect {
        x: -1440.,
        y: 0.,
        w: 1440.,
        h: 900.,
    }
}

#[test]
fn cursor_idle_and_turn_end_injected_clock_hide_without_clearing_stop() {
    let (o, t, s, w) = setup();
    o.show_target(
        &s,
        &w,
        ScreenPoint { x: -400., y: 100. },
        CursorKind::Pointer,
    );
    assert!(o.scene(monitor(), false, true).cursor.is_some());
    t.0.store(30_001, Ordering::Relaxed);
    assert!(o.scene(monitor(), false, true).cursor.is_none());
    o.hide(&s);
    assert_eq!(o.scene(monitor(), true, true).state, State::Stopped);
    assert!(o.scene(monitor(), true, true).capsule.is_some());
}

#[test]
fn foreground_approval_and_hidden_states_use_catalog_and_exact_request() {
    let (o, _, s, _) = setup();
    o.status(
        &s,
        &StatusText {
            message: "fixture".into(),
            foreground: true,
        },
    );
    assert_eq!(o.scene(monitor(), false, true).state, State::Foreground);
    o.approval_entry(&s, Some("request-7".into()));
    let scene = o.scene(monitor(), false, true);
    assert_eq!(scene.state, State::Approval);
    assert_eq!(scene.approval.as_deref(), Some("request-7"));
    assert!(scene
        .buttons
        .iter()
        .any(|b| b.label == CopyKey::Approval.as_str()));
    assert!(scene.capsule.unwrap().x < 0.);
    o.hide(&s);
    assert!(o.scene(monitor(), false, true).capsule.is_none());
}

#[test]
fn disabled_visuals_leave_stopped_and_unhealthy_facts_intact() {
    let o = Overlay::new(
        Arc::new(Time::default()),
        Arc::new(Copy),
        Config {
            visible: false,
            ..Default::default()
        },
    );
    let scene = o.scene(monitor(), true, false);
    assert_eq!(scene.state, State::Unavailable);
    assert!(scene.buttons.is_empty());
    assert!(scene.cursor.is_none());
}

#[test]
fn spring_partitioned_frames_match_and_retarget_preserves_position() {
    let mut spring = Spring::new(ScreenPoint { x: 0., y: 0. }, Duration::ZERO, 0.35);
    spring.retarget(ScreenPoint { x: 100., y: 50. }, Duration::ZERO);
    let halfway = spring.sample(Duration::from_millis(175));
    assert!(halfway.x > 0. && halfway.x < 100.);
    spring.retarget(ScreenPoint { x: -100., y: 10. }, Duration::from_millis(175));
    assert_eq!(spring.sample(Duration::from_millis(175)), halfway);
    assert!((spring.sample(Duration::from_secs(3)).x + 100.).abs() < 0.001);
}

#[test]
fn invalid_geometry_and_excess_sessions_are_bounded() {
    let (o, _, s, mut w) = setup();
    w.scale = f64::NAN;
    o.show_target(&s, &w, ScreenPoint { x: 10., y: 10. }, CursorKind::Pointer);
    assert!(o.scene(monitor(), false, true).cursor.is_none());
    for i in 0..1000 {
        o.status(
            &SessionCtx {
                session_id: i.to_string(),
            },
            &StatusText {
                message: "x".repeat(1000),
                foreground: false,
            },
        );
    }
    assert!(o.session_count() <= 64);
    assert!(o.scene(monitor(), false, true).message.chars().count() <= 160);
}

#[test]
fn wayland_and_xwayland_never_claim_overlay_safety() {
    use gcu_overlay::availability::{Availability, DisplaySession};
    for session in [DisplaySession::Wayland, DisplaySession::XWayland] {
        let a = Availability::for_session(session);
        assert!(!a.available);
        assert!(!a.excluded_from_capture);
        assert_eq!(a.reason, Some(CopyKey::WaylandUnavailable));
    }
    assert!(Availability::for_session(DisplaySession::X11).available);
}
