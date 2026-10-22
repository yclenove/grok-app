//! Foreground send order. Blocking validation finishes before the ledger and the gate.
//!
//! `bounded_admit` is the only check that runs inside the gate, immediately before insertion.
//! It is cancellation, deadline, foreground, and scalar window identity: no enumeration, no COM,
//! and no log write. Keyboard packets skip the gate and use the same check beside `SendInput`.
use super::{api::Event, hook_probe, ledger::InputLedger};
use crate::uia::patterns::ActionContext;
use gcu_core::driver::{Delivery, DriverError, Result};

/// Identity captured by the blocking validation. The gate re-reads the same fields and compares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpectedIdentity {
    pub root: u64,
    pub hwnd: u64,
    pub class: String,
    pub pid: u32,
    pub tid: u32,
}

/// Scalar Win32 reads. Implementations must not enumerate windows, start COM, or log.
pub(crate) trait ScalarAdmission {
    fn foreground(&self) -> u64;
    fn is_window(&self, hwnd: u64) -> bool;
    fn root(&self, hwnd: u64) -> u64;
    /// `(thread id, process id)` from `GetWindowThreadProcessId`.
    fn thread_process(&self, hwnd: u64) -> (u32, u32);
    fn class_matches(&self, hwnd: u64, class: &str) -> bool;
}

/// Ledger and gate parameters for one already-built packet.
pub(crate) struct Dispatch<'a> {
    pub ledger: &'a InputLedger,
    pub target: &'a super::api::Target,
    pub events: &'a [Event],
    pub delivery: Delivery,
    pub context: &'a ActionContext,
    pub pointer: bool,
}

pub(crate) fn packet_is_pointer(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::Move(..) | Event::Button(..) | Event::Wheel(..)
        )
    })
}

/// Run `prepare` to completion, then take the ledger, then admit, then insert.
///
/// `prepare` is where enumeration, security checks, and COM belong. After it returns, a test
/// barrier may flip cancellation, the deadline, or the foreground window. `admit` observes
/// that flip. `insert` does not run when `admit` fails.
pub(crate) fn prepare_then_dispatch<T>(
    prepare: impl FnOnce() -> Result<T>,
    dispatch: Dispatch<'_>,
    admit: impl FnOnce(&T) -> Result<()>,
    insert: impl FnOnce(&T) -> Result<()>,
) -> Result<()> {
    let prepared = prepare()?;
    after_prepare();
    dispatch.ledger.dispatch_packet(
        dispatch.target,
        dispatch.events,
        dispatch.delivery,
        dispatch.context,
        || {
            if dispatch.pointer {
                match hook_probe::with_admitted_gate(
                    hook_probe::INJECTION_GATE_WAIT,
                    || admit(&prepared),
                    || insert(&prepared),
                ) {
                    Err(hook_probe::GateBusy) => Err(DriverError::Native {
                        message: hook_probe::gate_refusal_message(),
                        maybe_executed: false,
                    }),
                    Ok(result) => result,
                }
            } else {
                admit(&prepared)?;
                insert(&prepared)
            }
        },
    )
}

/// Last admission check. Nothing here may block on enumeration, COM, or a log sink.
pub(crate) fn bounded_admit(
    check: impl FnOnce() -> Result<()>,
    expected: &ExpectedIdentity,
    live: &impl ScalarAdmission,
) -> Result<()> {
    if let Err(error) = check() {
        return Err(not_executed(error));
    }
    if live.foreground() != expected.root {
        return Err(DriverError::CapabilityUnavailable);
    }
    if !live.is_window(expected.hwnd) || live.root(expected.hwnd) != expected.root {
        return Err(DriverError::WindowGone);
    }
    let (tid, pid) = live.thread_process(expected.hwnd);
    if tid == 0 || tid != expected.tid || pid != expected.pid {
        return Err(DriverError::WindowGone);
    }
    if !live.class_matches(expected.hwnd, &expected.class) {
        return Err(DriverError::StaleElement);
    }
    Ok(())
}

/// This packet was not inserted. Drop the uncertainty bit `begin` may have raised for it.
pub(crate) fn not_executed(error: DriverError) -> DriverError {
    match error {
        DriverError::Timeout { .. } => DriverError::Timeout {
            maybe_executed: false,
        },
        DriverError::Cancelled { .. } => DriverError::Cancelled {
            maybe_executed: false,
        },
        DriverError::Crashed { .. } => DriverError::Crashed {
            maybe_executed: false,
        },
        DriverError::Native { message, .. } => DriverError::Native {
            message,
            maybe_executed: false,
        },
        DriverError::NotSettable { .. } => DriverError::NotSettable {
            maybe_executed: false,
        },
        other => other,
    }
}

fn after_prepare() {
    #[cfg(test)]
    {
        AFTER_PREPARE.with(|slot| {
            if let Some(hook) = slot.borrow_mut().take() {
                hook();
            }
        });
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_PREPARE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_after_prepare(hook: impl FnOnce() + 'static) {
    AFTER_PREPARE.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::api::Target;
    use gcu_core::{
        driver::{Button, Cancel, DriverError},
        model::WindowPoint,
    };
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
        fn class_matches(&self, _hwnd: u64, class: &str) -> bool {
            self.class_ok.load(Ordering::SeqCst) && class == "Edit"
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
        ExpectedIdentity {
            root: target.window.hwnd,
            hwnd: target.hwnd,
            class: target.class.clone(),
            pid: target.window.pid,
            tid: 42,
        }
    }
    fn button() -> Event {
        Event::Button(Button::Left, true, WindowPoint { x: 1.0, y: 1.0 }, false)
    }
    enum Flip {
        None,
        Cancel,
        Deadline,
        Foreground,
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
        let heavy = Arc::new(Heavy::new());
        let live = Arc::new(Live {
            foreground: AtomicU64::new(expected.root),
            alive: AtomicBool::new(true),
            root: expected.root,
            pid: expected.pid,
            tid: expected.tid,
            class_ok: AtomicBool::new(true),
        });
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
        let live = Live {
            foreground: AtomicU64::new(expected.root),
            alive: AtomicBool::new(true),
            root: expected.root,
            pid: expected.pid,
            tid: expected.tid,
            class_ok: AtomicBool::new(true),
        };
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
        let live = Live {
            foreground: AtomicU64::new(expected.root),
            alive: AtomicBool::new(true),
            root: expected.root,
            pid: expected.pid,
            tid: expected.tid,
            class_ok: AtomicBool::new(true),
        };
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
}
