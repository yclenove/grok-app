use super::*;
use crate::input::api::Target;
use gcu_core::{
    driver::{Button, Cancel, DriverError},
    model::WindowPoint,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

struct Heavy {
    calls: AtomicUsize,
    enum_calls: AtomicUsize,
    com_calls: AtomicUsize,
    violations: AtomicUsize,
    sealed: AtomicBool,
}
impl Heavy {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            enum_calls: AtomicUsize::new(0),
            com_calls: AtomicUsize::new(0),
            violations: AtomicUsize::new(0),
            sealed: AtomicBool::new(false),
        }
    }
    fn prepare(&self) {
        if self.sealed.load(Ordering::SeqCst) {
            self.violations.fetch_add(1, Ordering::SeqCst);
        }
        self.enum_calls.fetch_add(1, Ordering::SeqCst);
        self.com_calls.fetch_add(1, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
    fn seal(&self) {
        self.sealed.store(true, Ordering::SeqCst);
    }
}

struct Live {
    foreground: AtomicU64,
    alive: AtomicBool,
    root: u64,
    pid: u32,
    tid: u32,
    class_ok: AtomicBool,
    left: AtomicI32,
    top: AtomicI32,
    right: AtomicI32,
    bottom: AtomicI32,
    iconic: AtomicBool,
    created: AtomicU64,
    dpi: AtomicU32,
}
impl Live {
    fn matching(expected: &ExpectedIdentity) -> Self {
        Self {
            foreground: AtomicU64::new(expected.root),
            alive: AtomicBool::new(true),
            root: expected.root,
            pid: expected.pid,
            tid: expected.tid,
            class_ok: AtomicBool::new(true),
            left: AtomicI32::new(expected.frame.left),
            top: AtomicI32::new(expected.frame.top),
            right: AtomicI32::new(expected.frame.right),
            bottom: AtomicI32::new(expected.frame.bottom),
            iconic: AtomicBool::new(expected.minimized),
            created: AtomicU64::new(expected.created),
            dpi: AtomicU32::new(expected.dpi),
        }
    }
}
impl ScalarAdmission for Live {
    fn foreground(&self) -> u64 {
        self.foreground.load(Ordering::SeqCst)
    }
    fn is_window(&self, hwnd: u64) -> bool {
        self.alive.load(Ordering::SeqCst) && hwnd == 9
    }
    fn root(&self, hwnd: u64) -> u64 {
        let _ = hwnd;
        self.root
    }
    fn thread_process(&self, _hwnd: u64) -> (u32, u32) {
        (self.tid, self.pid)
    }
    fn class_matches(&self, hwnd: u64, class: &[u16]) -> bool {
        if !self.class_ok.load(Ordering::SeqCst) {
            return false;
        }
        let text = if hwnd == 9 {
            "Edit"
        } else if hwnd == 8 {
            "FixtureWindow"
        } else {
            return false;
        };
        class.iter().copied().eq(text.encode_utf16())
    }
    fn frame(&self, hwnd: u64) -> Option<FramePx> {
        if hwnd != 8 {
            return None;
        }
        Some(FramePx {
            left: self.left.load(Ordering::SeqCst),
            top: self.top.load(Ordering::SeqCst),
            right: self.right.load(Ordering::SeqCst),
            bottom: self.bottom.load(Ordering::SeqCst),
        })
    }
    fn iconic(&self, hwnd: u64) -> bool {
        hwnd == 8 && self.iconic.load(Ordering::SeqCst)
    }
    fn generation(&self, hwnd: u64, property: &[u16]) -> Option<u64> {
        if hwnd != 8 || property.is_empty() {
            return None;
        }
        Some(self.created.load(Ordering::SeqCst))
    }
    fn dpi(&self, _hwnd: u64) -> u32 {
        self.dpi.load(Ordering::SeqCst)
    }
}

fn target() -> Target {
    Target {
        window: crate::testing::window(8, 7),
        hwnd: 9,
        class: "Edit".into(),
        message_capable: true,
    }
}
fn identity(target: &Target) -> ExpectedIdentity {
    let mut generation_property: Vec<u16> = "GrokComputerUse.WindowGeneration.test"
        .encode_utf16()
        .collect();
    generation_property.push(0);
    ExpectedIdentity {
        root: target.window.hwnd,
        hwnd: target.hwnd,
        class_units: target.class.encode_utf16().collect(),
        pid: target.window.pid,
        tid: 42,
        // Same integers `testing::window` stores in the frame enumeration would read back.
        frame: FramePx {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        },
        minimized: false,
        root_pid: target.window.pid,
        root_class_units: target.window.class.encode_utf16().collect(),
        created: target.window.created,
        generation_property,
        dpi: 96,
    }
}
fn button() -> Event {
    Event::Button(Button::Left, true, WindowPoint { x: 1.0, y: 1.0 }, false)
}
#[derive(Clone, Copy, Debug)]
enum Flip {
    None,
    Cancel,
    Deadline,
    Foreground,
    Moved,
    Resized,
    Minimized,
    Stamp,
}

struct Ran {
    error: Result<()>,
    inserts: usize,
    heavy_calls: usize,
    violations: usize,
}

fn run(pointer: bool, flip: Flip) -> Ran {
    let target = target();
    let expected = identity(&target);
    let stamp = expected.created;
    let heavy = Arc::new(Heavy::new());
    let live = Arc::new(Live::matching(&expected));
    let cancel = Cancel::default();
    let deadline = Instant::now() + Duration::from_millis(80);
    let context = if matches!(flip, Flip::Deadline) {
        ActionContext::until(&cancel, deadline)
    } else {
        ActionContext::new(&cancel)
    };
    let ledger = InputLedger::default();
    let events = [button()];
    let inserts = Arc::new(AtomicUsize::new(0));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let heavy_worker = Arc::clone(&heavy);
    let live_worker = Arc::clone(&live);
    let inserts_worker = Arc::clone(&inserts);
    let worker = std::thread::spawn(move || {
        let heavy_hook = Arc::clone(&heavy_worker);
        set_after_prepare(move || {
            assert_eq!(
                heavy_hook.calls.load(Ordering::SeqCst),
                1,
                "blocking validation must finish before the post-prepare barrier"
            );
            assert!(heavy_hook.enum_calls.load(Ordering::SeqCst) >= 1);
            assert!(heavy_hook.com_calls.load(Ordering::SeqCst) >= 1);
            heavy_hook.seal();
            ready_tx.send(()).expect("ready");
            go_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("test did not release the post-validation barrier");
        });
        let heavy_prepare = heavy_worker;
        let live_admit = live_worker;
        let inserts_body = inserts_worker;
        let error = prepare_then_dispatch(
            || {
                heavy_prepare.prepare();
                Ok(expected.clone())
            },
            Dispatch {
                ledger: &ledger,
                target: &target,
                events: &events,
                delivery: Delivery::Foreground,
                context: &context,
                pointer,
            },
            |prepared| bounded_admit(|| context.check(), prepared, live_admit.as_ref()),
            |_prepared| {
                inserts_body.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        );
        let empty = ledger.is_empty();
        (error, empty)
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("validation did not reach the post-prepare barrier");
    match flip {
        Flip::None => {}
        Flip::Cancel => cancel.cancel(),
        Flip::Deadline => {
            while Instant::now() < deadline {
                std::thread::yield_now();
            }
        }
        Flip::Foreground => {
            live.foreground
                .store(expected_root_other(), Ordering::SeqCst);
        }
        Flip::Moved => live.left.store(40, Ordering::SeqCst),
        Flip::Resized => live.right.store(900, Ordering::SeqCst),
        Flip::Minimized => live.iconic.store(true, Ordering::SeqCst),
        Flip::Stamp => live.created.store(stamp.wrapping_add(1), Ordering::SeqCst),
    }
    go_tx.send(()).expect("release");
    let (error, empty) = worker.join().expect("worker");
    if !matches!(flip, Flip::None) {
        assert!(
            empty,
            "a refused packet must not leave a new down in the ledger"
        );
    }
    Ran {
        error,
        inserts: inserts.load(Ordering::SeqCst),
        heavy_calls: heavy.calls.load(Ordering::SeqCst),
        violations: heavy.violations.load(Ordering::SeqCst),
    }
}

fn expected_root_other() -> u64 {
    8 + 1
}

fn assert_clean(ran: &Ran) {
    assert_eq!(ran.heavy_calls, 1, "validate runs once");
    assert_eq!(
        ran.violations, 0,
        "admit must not enumerate or call COM after validation"
    );
}

#[test]
fn pointer_packets_are_mouse_events_only() {
    let point = WindowPoint { x: 1.0, y: 1.0 };
    assert!(packet_is_pointer(&[Event::Move(point, false)]));
    assert!(packet_is_pointer(&[Event::Button(
        Button::Left,
        true,
        point,
        false
    )]));
    assert!(packet_is_pointer(&[Event::Wheel(0, 1, point)]));
    assert!(!packet_is_pointer(&[Event::Key(1, true, false, false)]));
    assert!(!packet_is_pointer(&[Event::Unicode(10, true)]));
}

#[test]
fn unchanged_admission_inserts_once_for_pointer_and_keyboard() {
    for pointer in [false, true] {
        let ran = run(pointer, Flip::None);
        assert!(ran.error.is_ok(), "{pointer}");
        assert_eq!(ran.inserts, 1, "{pointer}");
        assert_clean(&ran);
    }
}

#[test]
fn cancel_after_validation_refuses_before_insert() {
    for pointer in [false, true] {
        let ran = run(pointer, Flip::Cancel);
        assert_eq!(ran.inserts, 0, "{pointer}");
        assert_clean(&ran);
        assert_eq!(
            ran.error,
            Err(DriverError::Cancelled {
                maybe_executed: false
            }),
            "{pointer}"
        );
    }
}

#[test]
fn deadline_after_validation_refuses_before_insert() {
    for pointer in [false, true] {
        let ran = run(pointer, Flip::Deadline);
        assert_eq!(ran.inserts, 0, "{pointer}");
        assert_clean(&ran);
        assert_eq!(
            ran.error,
            Err(DriverError::Timeout {
                maybe_executed: false
            }),
            "{pointer}"
        );
    }
}

#[test]
fn foreground_change_after_validation_refuses_before_insert() {
    for pointer in [false, true] {
        let ran = run(pointer, Flip::Foreground);
        assert_eq!(ran.inserts, 0, "{pointer}");
        assert_clean(&ran);
        assert_eq!(
            ran.error,
            Err(DriverError::CapabilityUnavailable),
            "{pointer}"
        );
    }
}

fn point() -> WindowPoint {
    WindowPoint { x: 3.0, y: 4.0 }
}

#[test]
fn refusal_after_a_successful_move_keeps_earlier_uncertainty_and_rolls_back() {
    let target = target();
    let expected = identity(&target);
    let cancel = Cancel::default();
    let context = ActionContext::new(&cancel);
    let ledger = InputLedger::default();
    ledger
        .dispatch_packet(
            &target,
            &[Event::Move(point(), false)],
            Delivery::Foreground,
            &context,
            || Ok(()),
        )
        .unwrap();
    assert!(context.maybe_executed());
    assert!(ledger.is_empty());
    cancel.cancel();
    let live = Live::matching(&expected);
    let events = [button()];
    let error = prepare_then_dispatch(
        || Ok(expected.clone()),
        Dispatch {
            ledger: &ledger,
            target: &target,
            events: &events,
            delivery: Delivery::Foreground,
            context: &context,
            pointer: true,
        },
        |prepared| bounded_admit(|| context.check(), prepared, &live),
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: true
        }
    );
    assert_eq!(error.error_code(), gcu_protocol::ErrorCode::StoppedByUser);
    assert!(error.maybe_executed());
    assert!(
        ledger.is_empty(),
        "the refused button must not stay recorded"
    );
}

#[test]
fn refusal_while_a_button_is_held_keeps_that_button_for_release() {
    let target = target();
    let expected = identity(&target);
    let cancel = Cancel::default();
    let context = ActionContext::new(&cancel);
    let ledger = InputLedger::default();
    ledger
        .dispatch_packet(&target, &[button()], Delivery::Foreground, &context, || {
            Ok(())
        })
        .unwrap();
    assert!(!ledger.is_empty());
    cancel.cancel();
    let live = Live::matching(&expected);
    let events = [Event::Move(point(), true)];
    let error = prepare_then_dispatch(
        || Ok(expected.clone()),
        Dispatch {
            ledger: &ledger,
            target: &target,
            events: &events,
            delivery: Delivery::Foreground,
            context: &context,
            pointer: true,
        },
        |prepared| bounded_admit(|| context.check(), prepared, &live),
        |_| panic!("insert after a refused admission"),
    )
    .unwrap_err();
    assert!(error.maybe_executed());
    assert_eq!(error.error_code(), gcu_protocol::ErrorCode::StoppedByUser);
    let mut released = Vec::new();
    ledger.release(|_, event, _| {
        released.push(event);
        Ok(())
    });
    assert_eq!(
        released,
        vec![Event::Button(
            Button::Left,
            false,
            WindowPoint { x: 1.0, y: 1.0 },
            false
        )]
    );
    assert!(ledger.is_empty());
}

#[test]
fn partial_insertion_keeps_the_down_and_reports_driver_error() {
    let target = target();
    let context = ActionContext::new(&Cancel::default());
    let ledger = InputLedger::default();
    let events = [button(), Event::Button(Button::Left, false, point(), false)];
    let error = ledger
        .dispatch_packet(&target, &events, Delivery::Foreground, &context, || {
            Err(DriverError::Native {
                message: "SendInput inserted an incomplete packet".into(),
                maybe_executed: true,
            })
        })
        .unwrap_err();
    assert!(error.maybe_executed());
    assert_eq!(error.error_code(), gcu_protocol::ErrorCode::DriverError);
    let mut released = Vec::new();
    ledger.release(|_, event, _| {
        released.push(event);
        Ok(())
    });
    assert_eq!(
        released,
        vec![Event::Button(
            Button::Left,
            false,
            WindowPoint { x: 1.0, y: 1.0 },
            false
        )]
    );
}

#[test]
fn window_moved_resized_minimized_or_restamped_after_validation_refuses_before_insert() {
    let cases = [
        (Flip::Moved, DriverError::InvalidRequest),
        (Flip::Resized, DriverError::InvalidRequest),
        (Flip::Minimized, DriverError::WindowMinimized),
        (Flip::Stamp, DriverError::WindowGone),
    ];
    for pointer in [false, true] {
        for (flip, error) in &cases {
            let ran = run(pointer, *flip);
            assert_eq!(ran.inserts, 0, "{pointer:?} {flip:?}");
            assert_clean(&ran);
            assert_eq!(ran.error, Err(error.clone()), "{pointer:?} {flip:?}");
        }
    }
}

#[test]
fn unchanged_frame_and_stamp_still_insert() {
    for pointer in [false, true] {
        let ran = run(pointer, Flip::None);
        assert!(
            ran.error.is_ok(),
            "the rectangle recorded at validate is the value the gate re-reads: {pointer}"
        );
        assert_eq!(ran.inserts, 1, "{pointer}");
        assert_clean(&ran);
    }
}

fn targeted(events: &[Event], alive: &AtomicBool, posts: &AtomicUsize) -> Result<()> {
    let target = target();
    let ledger = InputLedger::default();
    let context = ActionContext::new(&Cancel::default());
    dispatch_targeted(
        TargetedSend {
            ledger: &ledger,
            target: &target,
            events,
            delivery: Delivery::TargetedEvent,
            context: &context,
        },
        || Ok(()),
        || {
            if alive.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(DriverError::WindowGone)
            }
        },
        |_| {
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
}

#[test]
fn targeted_post_refuses_when_the_window_is_replaced_after_caller_validation() {
    let alive = AtomicBool::new(true);
    let posts = AtomicUsize::new(0);
    let caller = if alive.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err(DriverError::WindowGone)
    };
    caller.expect("caller validation still sees the window");
    alive.store(false, Ordering::SeqCst);
    let events = [
        Event::Key(0x41, true, false, false),
        Event::Unicode(0x4e2d, true),
    ];
    let error = targeted(&events, &alive, &posts).expect_err("replaced window");
    assert_eq!(error, DriverError::WindowGone);
    assert_eq!(posts.load(Ordering::SeqCst), 0, "PostMessage must not run");
}

#[test]
fn targeted_post_sends_when_the_window_still_matches() {
    let alive = AtomicBool::new(true);
    let posts = AtomicUsize::new(0);
    let events = [Event::Key(0x41, true, false, false)];
    targeted(&events, &alive, &posts).expect("matching window posts");
    assert_eq!(posts.load(Ordering::SeqCst), 1);
}

#[test]
fn targeted_post_stops_when_the_window_disappears_before_the_next_event() {
    let target = target();
    let ledger = InputLedger::default();
    let context = ActionContext::new(&Cancel::default());
    let posts = AtomicUsize::new(0);
    let step = AtomicUsize::new(0);
    let events = [
        Event::Key(0x41, true, false, false),
        Event::Key(0x41, false, false, false),
    ];
    let error = dispatch_targeted(
        TargetedSend {
            ledger: &ledger,
            target: &target,
            events: &events,
            delivery: Delivery::TargetedEvent,
            context: &context,
        },
        || Ok(()),
        || {
            if step.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(DriverError::WindowGone)
            }
        },
        |_| {
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .expect_err("second event");
    assert_eq!(error, DriverError::WindowGone);
    assert_eq!(posts.load(Ordering::SeqCst), 1);
}
