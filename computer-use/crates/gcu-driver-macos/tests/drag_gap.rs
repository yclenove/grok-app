//! Drag samples wait one egui frame on the injected clock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use gcu_core::driver::{Cancel, Delivery, DriverAction, PointerRoute, WindowRef};
use gcu_core::model::WindowPoint;
use gcu_driver_macos::input::activity::ActivityClock;
use gcu_driver_macos::input::clock::FakeClock;
use gcu_driver_macos::input::hotkey::{Guards, SharedStopLatch};
use gcu_driver_macos::input::{
    deliver, plan_drag, Call, EventApi, EventKind, FakeEvents, InputLedger, InputTarget, PostPath,
    DRAG_SAMPLE_GAP,
};

struct Idle;

impl ActivityClock for Idle {
    fn ms_since_physical_input(&self) -> Option<u64> {
        Some(3000)
    }
    fn stop_available(&self) -> bool {
        true
    }
}

fn target() -> InputTarget {
    InputTarget {
        pid: 4,
        window_id: 7,
        bundle_id: "com.example.Editor".into(),
        app_name: "Editor".into(),
        origin_x: 10.0,
        origin_y: 20.0,
        width_px: 160.0,
        height_px: 80.0,
        scale: 2.0,
    }
}

fn window() -> WindowRef {
    WindowRef {
        app: gcu_core::driver::AppRef {
            info: gcu_protocol::AppInfo {
                id: "com.example.Editor".into(),
                name: "Editor".into(),
                pid: Some(4),
                identity: None,
            },
        },
        id: "cg:7:4".into(),
    }
}

fn points() -> [WindowPoint; 2] {
    [
        WindowPoint { x: 20.0, y: 20.0 },
        WindowPoint { x: 80.0, y: 40.0 },
    ]
}

fn posted(fake: &FakeEvents) -> Vec<EventKind> {
    fake.calls()
        .iter()
        .filter_map(|call| match call {
            Call::Post(event) => Some(event.kind),
            _ => None,
        })
        .collect()
}

#[test]
fn two_point_drag_includes_a_move_before_release() {
    let [start, end] = points();
    let events = plan_drag(&target(), &[start, end], PostPath::ToPid).unwrap();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            EventKind::MouseDown,
            EventKind::MouseDrag,
            EventKind::MouseUp
        ]
    );
    assert!(events[1].down);
    assert!(!events[2].down);
    assert_eq!(events[1].window_local, events[2].window_local);
    assert_eq!(events[1].window_local, Some((40.0, 20.0)));
}

#[tokio::test]
async fn two_point_drag_posts_move_after_injected_frame_gap() {
    let clock = Arc::new(FakeClock::new());
    let fake = Arc::new(FakeEvents::ready());
    fake.set_guards(Arc::new(Guards::new(
        clock.clone(),
        Arc::new(Idle),
        Arc::new(SharedStopLatch::new(false)),
        3000,
    )));
    let [start, end] = points();
    let action = DriverAction::PointerDrag {
        win: window(),
        path: vec![start, end],
        route: PointerRoute::TargetedOnly,
    };
    let events = Arc::clone(&fake) as Arc<dyn EventApi>;
    let task = tokio::spawn(async move {
        deliver(
            events,
            Arc::new(InputLedger::new()),
            &target(),
            &action,
            &Cancel::default(),
            Duration::from_secs(2),
        )
        .await
    });
    // Deadline wait, Stop's clock poll, and the drag gap are three waiters.
    armed(&clock, &fake, 1).await;
    clock.advance(DRAG_SAMPLE_GAP - Duration::from_millis(1));
    still(&fake, 1).await;
    clock.advance(Duration::from_millis(1));
    armed(&clock, &fake, 2).await;
    clock.advance(DRAG_SAMPLE_GAP - Duration::from_millis(1));
    still(&fake, 2).await;
    clock.advance(Duration::from_millis(1));
    let delivery = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("drag delivery hung")
        .expect("drag task panicked")
        .expect("drag delivery failed");
    assert_eq!(delivery, Delivery::TargetedEvent);
    assert_eq!(
        posted(&fake),
        [
            EventKind::MouseDown,
            EventKind::MouseDrag,
            EventKind::MouseUp
        ]
    );
}

async fn armed(clock: &FakeClock, fake: &FakeEvents, posts: usize) {
    let start = std::time::Instant::now();
    loop {
        if posted(fake).len() >= posts && clock.waiters() >= 3 {
            return;
        }
        if start.elapsed() > Duration::from_secs(2) {
            panic!(
                "drag gap was not armed (posts {}, waiters {})",
                posted(fake).len(),
                clock.waiters()
            );
        }
        tokio::task::yield_now().await;
    }
}

async fn still(fake: &FakeEvents, posts: usize) {
    for _ in 0..80 {
        assert_eq!(posted(fake).len(), posts, "{:?}", posted(fake));
        tokio::task::yield_now().await;
    }
}
