//! Portable control-channel tests; no native event loop or wall-clock waits.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};
#[derive(Default)]
struct Host {
    resume_fails: AtomicBool,
    stopped: AtomicBool,
    stops: AtomicUsize,
    resumes: AtomicUsize,
    approvals: Mutex<Vec<String>>,
}
impl Controls for Host {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.stops.fetch_add(1, Ordering::Relaxed);
    }
    fn resume(&self) -> Result<(), Error> {
        if self.resume_fails.load(Ordering::Acquire) {
            return Err(Error::Host("fixture persistence failure".into()));
        }
        self.stopped.store(false, Ordering::Release);
        self.resumes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    fn healthy(&self) -> bool {
        true
    }
    fn open_approval(&self, request: &str) -> Result<(), Error> {
        crate::lock(&self.approvals).push(request.into());
        Ok(())
    }
}
struct Copy;
impl Catalog for Copy {
    fn text(&self, key: CopyKey) -> String {
        format!("translated {key:?}")
    }
}
#[test]
fn stop_independent_of_blocked_action_and_approval_debounces_then_explicit_resume() {
    let host = Host::default();
    // Locks simulate queues held indefinitely. Dispatch never touches either.
    let action = Mutex::new(());
    let approval = Mutex::new(());
    let _a = action.lock().unwrap();
    let _p = approval.lock().unwrap();
    let overlay = Overlay::new(
        Arc::new(crate::MonotonicClock::default()),
        Arc::new(Copy),
        Config::default(),
    );
    use gcu_host_api::OverlayProvider;
    overlay.status(
        &gcu_host_api::SessionCtx {
            session_id: "s".into(),
        },
        &gcu_host_api::StatusText {
            message: String::new(),
            foreground: false,
        },
    );
    let monitor = Rect {
        x: 0.,
        y: 0.,
        w: 1000.,
        h: 800.,
    };
    let mut debounce = Debounce::default();
    let scene = overlay.scene(monitor, false, true);
    for ms in [0, 20, 200] {
        dispatch(
            &host,
            &mut debounce,
            Duration::from_millis(ms),
            &scene,
            UserEvent {
                action: ButtonAction::Stop,
                revision: scene.revision,
            },
        )
        .unwrap();
    }
    assert_eq!(host.stops.load(Ordering::Relaxed), 1);
    assert!(host.stopped());
    let stopped = overlay.scene(monitor, true, true);
    dispatch(
        &host,
        &mut debounce,
        Duration::from_millis(210),
        &stopped,
        UserEvent {
            action: ButtonAction::Resume,
            revision: stopped.revision,
        },
    )
    .unwrap();
    assert!(host.stopped()); // A double click on Stop must not become Resume.
    dispatch(
        &host,
        &mut debounce,
        Duration::from_millis(300),
        &stopped,
        UserEvent {
            action: ButtonAction::Resume,
            revision: stopped.revision,
        },
    )
    .unwrap();
    assert!(!host.stopped());
    dispatch(
        &host,
        &mut debounce,
        Duration::from_millis(310),
        &scene,
        UserEvent {
            action: ButtonAction::Stop,
            revision: scene.revision,
        },
    )
    .unwrap();
    assert!(host.stopped());
}
#[test]
fn stale_approval_and_unhealthy_resume_cannot_change_host() {
    let host = Host::default();
    host.stop();
    let overlay = Overlay::new(
        Arc::new(crate::MonotonicClock::default()),
        Arc::new(Copy),
        Config::default(),
    );
    let scene = overlay.scene(
        Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        },
        true,
        false,
    );
    dispatch(
        &host,
        &mut Debounce::default(),
        Duration::ZERO,
        &scene,
        UserEvent {
            action: ButtonAction::Resume,
            revision: 0,
        },
    )
    .unwrap();
    assert_eq!(host.resumes.load(Ordering::Relaxed), 0);
    assert!(host.stopped());
}
#[test]
fn failed_resume_retains_latch_and_external_resume_never_debounces_next_stop() {
    let host = Host::default();
    let overlay = Overlay::new(
        Arc::new(crate::MonotonicClock::default()),
        Arc::new(Copy),
        Config::default(),
    );
    use gcu_host_api::OverlayProvider;
    let session = gcu_host_api::SessionCtx {
        session_id: "s".into(),
    };
    overlay.status(
        &session,
        &gcu_host_api::StatusText {
            message: String::new(),
            foreground: false,
        },
    );
    let monitor = Rect {
        x: 0.,
        y: 0.,
        w: 1000.,
        h: 800.,
    };
    let mut debounce = Debounce::default();
    let running = overlay.scene(monitor, false, true);
    dispatch(
        &host,
        &mut debounce,
        Duration::ZERO,
        &running,
        UserEvent {
            action: ButtonAction::Stop,
            revision: running.revision,
        },
    )
    .unwrap();
    host.resume().unwrap(); // Other trusted UI/CLI, without touching debounce state.
    dispatch(
        &host,
        &mut debounce,
        Duration::from_millis(10),
        &running,
        UserEvent {
            action: ButtonAction::Stop,
            revision: running.revision,
        },
    )
    .unwrap();
    assert!(host.stopped());
    assert_eq!(host.stops.load(Ordering::Relaxed), 2);
    host.resume_fails.store(true, Ordering::Release);
    let stopped = overlay.scene(monitor, true, true);
    assert!(dispatch(
        &host,
        &mut debounce,
        Duration::from_millis(300),
        &stopped,
        UserEvent {
            action: ButtonAction::Resume,
            revision: stopped.revision
        }
    )
    .is_err());
    assert!(host.stopped());
}
#[test]
fn approval_entry_uses_exact_host_request_and_rejects_stale_frame() {
    let host = Host::default();
    let overlay = Overlay::new(
        Arc::new(crate::MonotonicClock::default()),
        Arc::new(Copy),
        Config::default(),
    );
    let session = gcu_host_api::SessionCtx {
        session_id: "s".into(),
    };
    overlay.approval_entry(&session, Some("request-a".into()));
    let monitor = Rect {
        x: 0.,
        y: 0.,
        w: 1000.,
        h: 800.,
    };
    let old = overlay.scene(monitor, false, true);
    overlay.approval_entry(&session, Some("request-b".into()));
    let current = overlay.scene(monitor, false, true);
    let mut debounce = Debounce::default();
    dispatch(
        &host,
        &mut debounce,
        Duration::ZERO,
        &current,
        UserEvent {
            action: ButtonAction::Approval,
            revision: old.revision,
        },
    )
    .unwrap();
    assert!(crate::lock(&host.approvals).is_empty());
    dispatch(
        &host,
        &mut debounce,
        Duration::ZERO,
        &current,
        UserEvent {
            action: ButtonAction::Approval,
            revision: current.revision,
        },
    )
    .unwrap();
    assert_eq!(*crate::lock(&host.approvals), ["request-b"]);
    assert!(!host.stopped());
}
