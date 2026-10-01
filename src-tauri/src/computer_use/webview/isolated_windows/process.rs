//! Native process failure is different from a logical Stop or an unresponsive
//! renderer. Running scripts require a signaled handle for their exact renderer.

use super::*;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
};
use webview2_com::ProcessFailedEventHandler;

pub(super) trait PhysicalExit: Send + Sync {
    fn exited(&self) -> bool;

    #[cfg(feature = "computer-use-probe")]
    fn diagnostic(&self) -> String {
        format!("exited={}", self.exited())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    MainRendererExited,
    BrowserExited,
    NativeViewClosed,
}

fn classify(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Option<Failure> {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Some(Failure::MainRendererExited),
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Some(Failure::BrowserExited),
        _ => None,
    }
}

/// Install once for this captured native view, never by its reusable label.
/// CoreWebView2 owns the handler until it closes. The callback contains no COM
/// object, so it cannot form a WebView/handler reference cycle.
pub(crate) fn install(
    webview: &ICoreWebView2,
    world: &Arc<NativeWorld>,
    retire: impl Fn(Failure) + 'static,
) -> Result<(), String> {
    if world.process_observed() {
        return Ok(());
    }
    let observed = world.clone();
    let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        unsafe { args.ProcessFailedKind(&mut kind)? };
        if let Some(failure) = classify(kind) {
            #[cfg(feature = "computer-use-probe")]
            eprintln!(
                "renderer-witness: native_failure={failure:?} {}",
                observed.diagnostic()
            );
            // Retire document authority before releasing any native occupancy.
            retire(failure);
            if let Some(ticket) = observed.process_failed(failure) {
                watch_exit(&observed, ticket, failure);
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_ProcessFailed(&handler, &mut token) }
        .map_err(|_| "WebView process observer unavailable")?;
    world.mark_process_observed();
    Ok(())
}

fn watch_exit(world: &Arc<NativeWorld>, ticket: uuid::Uuid, failure: Failure) {
    let pending = world.clone();
    // A failure notification can precede the Windows process handle becoming
    // signaled. This rare failure-only watcher uses no COM and survives closure
    // of the native STA. Normal completion ends it; it never acts on a new ticket.
    if std::thread::Builder::new()
        .name("grok-cu-renderer-exit".into())
        .spawn(move || {
            #[cfg(feature = "computer-use-probe")]
            let started = std::time::Instant::now();
            while pending.poll_exit(ticket, failure) {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            #[cfg(feature = "computer-use-probe")]
            eprintln!(
                "renderer-witness: watch_finished elapsed_ms={} {}",
                started.elapsed().as_millis(),
                pending.diagnostic()
            );
        })
        .is_err()
    {
        world.exit_watch_failed(ticket);
        tracing::warn!("WebView exit witness watcher unavailable; occupancy retained");
    }
}

/// The owner has fenced the exact native view and is about to close it. Retain
/// physical ownership independently of the STA/COM event handlers being torn
/// down. A shared/live renderer still cannot be acknowledged as stopped.
pub(crate) fn closing(world: &Arc<NativeWorld>) {
    if let Some(ticket) = world.close() {
        watch_exit(world, ticket, Failure::NativeViewClosed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer_use::webview::execution::ScriptTracker;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };

    struct Witness(bool);
    impl PhysicalExit for Witness {
        fn exited(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn unresponsive_gpu_subframe_and_unknown_events_are_not_main_renderer_exit() {
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND(999),
        ] {
            assert_eq!(classify(kind), None);
        }
        assert_eq!(
            classify(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some(Failure::MainRendererExited)
        );
    }

    #[test]
    fn renderer_exit_settles_a_lost_callback_without_reusing_context_or_replaying() {
        let world = NativeWorld::default();
        let tracker = ScriptTracker::default();
        let operation = tracker
            .begin("old", "native-a", &Default::default(), &Default::default())
            .unwrap();
        let ticket = uuid::Uuid::new_v4();
        let (tx, rx) = std::sync::mpsc::channel();
        world.admit(ticket, 1, operation.clone(), tx).unwrap();
        world.reserve(1).unwrap();
        assert!(world.publish(1, "old-context".into()));
        assert!(world.attach(ticket, Arc::new(Witness(true))));
        tracker.cancel("old");
        assert!(!tracker.is_idle("old"));
        assert!(world.process_failed(Failure::MainRendererExited).is_none());
        assert!(tracker.is_idle("old"));
        assert!(rx
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("renderer exited"));
        assert!(world.reserve(1).is_err());
        assert!(!world.publish(1, "late-context".into()));
        assert_eq!(world.reserve(2).unwrap(), None);
        let next = tracker
            .begin("new", "native-a", &Default::default(), &Default::default())
            .unwrap();
        let (tx, _) = std::sync::mpsc::channel();
        let next_ticket = uuid::Uuid::new_v4();
        world.admit(next_ticket, 2, next.clone(), tx).unwrap();
        assert!(world.attach(next_ticket, Arc::new(Witness(false))));
        assert!(!world.attach(ticket, Arc::new(Witness(true))));
        world.completed(ticket);
        operation.finished();
        assert!(!tracker.is_idle("new"));
        assert_eq!(world.pending_ticket(), Some(next_ticket));
        assert_eq!(
            world.process_failed(Failure::MainRendererExited),
            Some(next_ticket)
        );
        assert!(world.poll_exit(next_ticket, Failure::MainRendererExited));
        assert!(
            !tracker.is_idle("new"),
            "old witness must not settle new renderer"
        );
        world.completed(next_ticket);
        next.finished();
    }

    #[test]
    fn browser_failure_fences_context_but_is_not_all_children_exit_evidence() {
        let world = NativeWorld::default();
        let tracker = ScriptTracker::default();
        let operation = tracker
            .begin("run", "native-a", &Default::default(), &Default::default())
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 1, operation.clone(), tx).unwrap();
        world.reserve(1).unwrap();
        assert!(world.attach(ticket, Arc::new(Witness(false))));
        assert_eq!(world.process_failed(Failure::BrowserExited), Some(ticket));
        assert!(!tracker.is_idle("run"));
        assert!(rx.try_recv().is_err());
        assert!(world.reserve(1).is_err());
        world.completed(ticket);
        operation.finished();
    }

    #[test]
    fn delayed_old_renderer_failure_cannot_settle_a_live_replacement_process() {
        let world = NativeWorld::default();
        let tracker = ScriptTracker::default();
        let next = tracker
            .begin(
                "replacement",
                "view",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 2, next.clone(), tx).unwrap();
        world.reserve(2).unwrap();
        assert!(world.attach(ticket, Arc::new(Witness(false))));
        assert_eq!(
            world.process_failed(Failure::MainRendererExited),
            Some(ticket)
        );
        assert!(!tracker.is_idle("replacement"));
        assert!(rx.try_recv().is_err());
        assert!(
            world.reserve(2).is_err(),
            "authority is fenced, but execution is not falsely idle"
        );
        world.completed(ticket);
        next.finished();
    }

    #[test]
    fn late_physical_exit_settles_once_without_requiring_another_native_event() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Later(AtomicBool);
        impl PhysicalExit for Later {
            fn exited(&self) -> bool {
                self.0.load(Ordering::SeqCst)
            }
        }
        let world = NativeWorld::default();
        let tracker = ScriptTracker::default();
        let old = tracker
            .begin("old", "view", &Default::default(), &Default::default())
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 1, old.clone(), tx).unwrap();
        world.reserve(1).unwrap();
        let witness = Arc::new(Later(AtomicBool::new(false)));
        assert!(world.attach(ticket, witness.clone()));
        assert_eq!(
            world.process_failed(Failure::MainRendererExited),
            Some(ticket)
        );
        assert_eq!(
            world.process_failed(Failure::MainRendererExited),
            None,
            "one watcher per ticket"
        );
        assert!(world.poll_exit(ticket, Failure::MainRendererExited));
        assert!(!tracker.is_idle("old"));
        witness.0.store(true, Ordering::SeqCst);
        assert!(!world.poll_exit(ticket, Failure::MainRendererExited));
        assert!(tracker.is_idle("old"));
        assert!(rx.try_recv().unwrap().is_err());
        assert!(!world.poll_exit(ticket, Failure::MainRendererExited));
        assert!(rx.try_recv().is_err());
        let next = tracker
            .begin("new", "view", &Default::default(), &Default::default())
            .unwrap();
        let (tx, _) = std::sync::mpsc::channel();
        world
            .admit(uuid::Uuid::new_v4(), 2, next.clone(), tx)
            .unwrap();
        assert!(!world.poll_exit(ticket, Failure::MainRendererExited));
        assert!(!tracker.is_idle("new"));
        next.finished();
    }

    #[test]
    fn native_world_admission_is_exclusive_even_across_adapter_trackers() {
        let world = NativeWorld::default();
        let trackers = [ScriptTracker::default(), ScriptTracker::default()];
        let first = trackers[0]
            .begin("a", "view", &Default::default(), &Default::default())
            .unwrap();
        let second = trackers[1]
            .begin("b", "view", &Default::default(), &Default::default())
            .unwrap();
        let (tx, _) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 1, first.clone(), tx.clone()).unwrap();
        assert!(world
            .admit(uuid::Uuid::new_v4(), 1, second.clone(), tx)
            .is_err());
        world.completed(uuid::Uuid::new_v4());
        assert!(world.pending_ticket().is_some());
        world.completed(ticket);
        first.finished();
        second.finished();
    }

    #[test]
    fn normal_close_without_failure_event_settles_proven_exit_and_preserves_replacement() {
        use crate::computer_use::webview::lifecycle::Registry;
        let registry = Registry::default();
        let old_view = registry.reserve().unwrap();
        registry.publish("same-label", &old_view).unwrap();
        let old_doc = old_view.document().unwrap();
        let world = old_view.script_world().unwrap();
        let tracker = ScriptTracker::default();
        let old = tracker
            .begin(
                "old",
                "old-native",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world
            .admit(ticket, old_doc.generation, old.clone(), tx)
            .unwrap();
        world.reserve(old_doc.generation).unwrap();
        assert!(world.attach(ticket, Arc::new(Witness(true))));
        let replacement = registry.reserve().unwrap();
        registry.publish("same-label", &replacement).unwrap();
        let next_doc = replacement.document().unwrap();
        let next = tracker
            .begin(
                "new",
                "new-native",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();

        registry.closed("same-label", &old_view);
        let reply = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("normal close must not need a ProcessFailed event after proven exit");
        assert!(reply.unwrap_err().contains("outcome unknown"));
        assert!(tracker.is_idle("old"));
        assert!(!tracker.is_idle("new"));
        assert!(registry.current("same-label").unwrap().matches(next_doc));
        assert!(
            world.reserve(old_doc.generation + 1).is_err(),
            "a closed native world cannot reopen by generation alone"
        );
        old.finished();
        assert!(!tracker.is_idle("new"));
        next.finished();
    }

    #[test]
    fn normal_close_keeps_a_live_or_shared_renderer_pending_until_exact_exit() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Later(AtomicBool);
        impl PhysicalExit for Later {
            fn exited(&self) -> bool {
                self.0.load(Ordering::SeqCst)
            }
        }
        let world = Arc::new(NativeWorld::default());
        let tracker = ScriptTracker::default();
        let old = tracker
            .begin("old", "native", &Default::default(), &Default::default())
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 1, old.clone(), tx).unwrap();
        world.reserve(1).unwrap();
        let witness = Arc::new(Later(AtomicBool::new(false)));
        assert!(world.attach(ticket, witness.clone()));
        closing(&world);
        closing(&world); // repeated close must reuse the same pending witness
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(80))
            .is_err());
        assert!(!tracker.is_idle("old"));
        assert!(world.reserve(2).is_err());
        assert!(!world.attach(ticket, Arc::new(Witness(true))));
        witness.0.store(true, Ordering::SeqCst);
        let outcome = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(outcome.unwrap_err().contains("closed; outcome unknown"));
        assert!(tracker.is_idle("old"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn normal_close_rejects_unstarted_work_and_cannot_reopen_by_generation() {
        let world = Arc::new(NativeWorld::default());
        let tracker = ScriptTracker::default();
        let operation = tracker
            .begin("setup", "native", &Default::default(), &Default::default())
            .unwrap();
        let ticket = uuid::Uuid::new_v4();
        let (tx, rx) = std::sync::mpsc::channel();
        world.admit(ticket, 1, operation, tx.clone()).unwrap();
        world.reserve(1).unwrap();
        closing(&world);
        assert!(rx.try_recv().unwrap().unwrap_err().contains("closed"));
        assert!(tracker.is_idle("setup"));
        assert!(!world.publish(2, "late-context".into()));
        assert!(world.reserve(2).is_err());
        assert!(!world.attach(ticket, Arc::new(Witness(true))));
        let next = tracker
            .begin("next", "native", &Default::default(), &Default::default())
            .unwrap();
        assert!(world
            .admit(uuid::Uuid::new_v4(), 2, next.clone(), tx)
            .is_err());
        next.finished(); // rejected before dispatch, not a running script
    }

    #[test]
    fn watcher_start_failure_keeps_ownership_and_can_be_rearmed() {
        let world = NativeWorld::default();
        let tracker = ScriptTracker::default();
        let pending = tracker
            .begin("run", "view", &Default::default(), &Default::default())
            .unwrap();
        let (tx, _) = std::sync::mpsc::channel();
        let ticket = uuid::Uuid::new_v4();
        world.admit(ticket, 1, pending.clone(), tx).unwrap();
        world.reserve(1).unwrap();
        assert!(world.attach(ticket, Arc::new(Witness(false))));
        assert_eq!(world.process_failed(Failure::BrowserExited), Some(ticket));
        world.exit_watch_failed(uuid::Uuid::new_v4());
        assert_eq!(world.process_failed(Failure::BrowserExited), None);
        world.exit_watch_failed(ticket);
        assert!(!tracker.is_idle("run"));
        assert_eq!(world.process_failed(Failure::BrowserExited), Some(ticket));
        world.completed(ticket);
        pending.finished();
        assert!(!world.poll_exit(ticket, Failure::BrowserExited));
    }
}
