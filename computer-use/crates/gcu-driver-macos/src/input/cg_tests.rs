//! RealEvents sink with an injected poster. No AppKit, listener, or CGEvent.post.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::cg::{OsPoster, RealEvents};
use super::events::{EventApi, EventKind, PostFault, PostPath, SyntheticEvent};
use super::proof::arm;
use crate::scope_os::{Evidence, JobScope};

struct CountingPoster {
    posts: AtomicUsize,
}

impl CountingPoster {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            posts: AtomicUsize::new(0),
        })
    }

    fn posts(&self) -> usize {
        self.posts.load(Ordering::SeqCst)
    }
}

impl OsPoster for CountingPoster {
    fn appkit_ready(&self) -> bool {
        true
    }

    fn trusted(&self) -> bool {
        true
    }

    fn frontmost_pid(&self) -> Option<i32> {
        Some(4)
    }

    fn post_event(&self, _event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
        self.posts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn mouse_down() -> SyntheticEvent {
    SyntheticEvent {
        path: PostPath::ToPid,
        kind: EventKind::MouseDown,
        pid: 4,
        window_id: 7,
        field_51: Some(7),
        window_local: Some((1.0, 1.0)),
        screen: None,
        button: Some(0),
        click_count: 1,
        scroll: None,
        keycode: None,
        flags: 0,
        down: true,
        unicode: None,
    }
}

#[tokio::test]
async fn unscoped_real_events_post_without_pin_is_sent() {
    let poster = CountingPoster::new();
    let events = RealEvents::with_poster(Arc::clone(&poster) as Arc<dyn OsPoster>);
    let evidence = Evidence::live_for_test();
    let scope = JobScope::admit(&evidence);
    assert!(!evidence.enforcing());
    assert!(!scope.is_scoped());
    let armed = arm(&events, Arc::clone(&evidence), 7, &scope).expect("unscoped gate");
    events
        .post(mouse_down(), Some(armed.gate()))
        .await
        .expect("posted");
    drop(armed);
    assert!(poster.posts() >= 1);
}

#[tokio::test]
async fn real_events_post_without_gate_is_not_sent() {
    let poster = CountingPoster::new();
    let events = RealEvents::with_poster(Arc::clone(&poster) as Arc<dyn OsPoster>);
    let err = events
        .post(mouse_down(), None)
        .await
        .expect_err("missing gate");
    assert_eq!(err, PostFault::NotSent);
    assert_eq!(poster.posts(), 0);
}

#[tokio::test]
async fn scoped_real_events_without_binding_is_not_sent() {
    let poster = CountingPoster::new();
    let events = RealEvents::with_poster(Arc::clone(&poster) as Arc<dyn OsPoster>);
    let evidence = Evidence::live_for_test();
    let scope = JobScope::scoped(None);
    let err = match arm(&events, evidence, 7, &scope) {
        Ok(_) => panic!("scoped without a binding armed a post"),
        Err(err) => err,
    };
    assert_eq!(err, crate::scope_os::not_proved());
    let posted = events.post(mouse_down(), None).await.expect_err("no post");
    assert_eq!(posted, PostFault::NotSent);
    assert_eq!(poster.posts(), 0);
}

#[tokio::test]
async fn overlapping_unscoped_drop_leaves_the_other_post_sent() {
    let poster = CountingPoster::new();
    let events = RealEvents::with_poster(Arc::clone(&poster) as Arc<dyn OsPoster>);
    let evidence = Evidence::live_for_test();
    let scope = JobScope::admit(&evidence);
    assert!(!evidence.enforcing());
    assert!(!scope.is_scoped());
    let first = arm(&events, Arc::clone(&evidence), 7, &scope).expect("first");
    let second = arm(&events, Arc::clone(&evidence), 8, &scope).expect("second");
    events
        .post(mouse_down(), Some(second.gate()))
        .await
        .expect("down");
    drop(first);
    events
        .post(mouse_up(), Some(second.gate()))
        .await
        .expect("up after the other drop");
    assert_eq!(poster.posts(), 2);
    drop(second);
    let missing = JobScope::scoped(None);
    let err = match arm(&events, evidence, 7, &missing) {
        Ok(_) => panic!("scoped without a binding armed a post"),
        Err(err) => err,
    };
    assert_eq!(err, crate::scope_os::not_proved());
    let refused = events
        .post(mouse_down(), None)
        .await
        .expect_err("missing scoped gate");
    assert_eq!(refused, PostFault::NotSent);
    assert_eq!(poster.posts(), 2);
}

fn mouse_up() -> SyntheticEvent {
    let mut event = mouse_down();
    event.kind = EventKind::MouseUp;
    event.down = false;
    event
}
