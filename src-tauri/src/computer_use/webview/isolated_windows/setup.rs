//! Join the native create reply and unique-context event in either order.
//! The setup-only timer promptly rejects cancelled undispatched work, but never
//! settles a running JS evaluation or releases its physical execution ticket.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uuid::Uuid;
use webview2_com::DevToolsProtocolEventReceivedEventHandler;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{context_identity, wide, Job, Subscription, MAX_REPLY};

const SETUP_BUDGET_MS: u32 = 5_000;

pub(super) struct Watch {
    _subscription: Subscription,
    identity: IdentityJoin,
}

#[derive(Default)]
struct IdentityJoin {
    created: Option<i64>,
    observed: Option<(i64, String)>,
    resolved: bool,
}

#[derive(Debug, PartialEq)]
enum Progress {
    Waiting,
    Ready(String),
    Rejected,
}

impl IdentityJoin {
    fn created(&mut self, id: i64) -> Progress {
        if self.resolved {
            return Progress::Waiting;
        }
        if id <= 0 || self.created.is_some_and(|old| old != id) {
            self.resolved = true;
            return Progress::Rejected;
        }
        self.created = Some(id);
        self.join()
    }

    fn observed(&mut self, context: (i64, String)) -> Progress {
        if self.resolved {
            return Progress::Waiting;
        }
        if context.0 <= 0
            || context.1.is_empty()
            || self.observed.as_ref().is_some_and(|old| old != &context)
        {
            self.resolved = true;
            return Progress::Rejected;
        }
        self.observed = Some(context);
        self.join()
    }

    fn join(&mut self) -> Progress {
        let (Some(created), Some((observed, unique))) = (self.created, &self.observed) else {
            return Progress::Waiting;
        };
        self.resolved = true;
        if created != *observed {
            Progress::Rejected
        } else {
            Progress::Ready(unique.clone())
        }
    }
}

pub(super) fn begin(job: &Rc<Job>) {
    let Some(timer) = Timer::start(job) else {
        return job.fail("context setup deadline unavailable");
    };
    *job.setup_deadline.borrow_mut() = Some(timer);
    // Cover the entire no-script setup chain, including a missing frame-tree
    // reply. A timer expiry marks the Job terminal before releasing occupancy.
    job.call("Page.getFrameTree", json!({}), |job, tree| {
        #[cfg(feature = "computer-use-probe")]
        if super::probe_fault::suppress(&job, super::probe_fault::Phase::FrameReply) {
            return;
        }
        let Some(frame) = tree.pointer("/frameTree/frame/id").and_then(Value::as_str) else {
            return job.fail("main frame unavailable");
        };
        prepare(&job, frame.to_string());
    });
}

fn prepare(job: &Rc<Job>, frame: String) {
    let name = format!("grok-app-computer-use-{}", Uuid::new_v4());
    let event = wide("Runtime.executionContextCreated");
    let receiver = match unsafe {
        job.webview
            .GetDevToolsProtocolEventReceiver(PCWSTR(event.as_ptr()))
    } {
        Ok(receiver) => receiver,
        Err(_) => return job.fail("context observer unavailable"),
    };
    // The bounded native-thread deadline registry owns the pending Job. The
    // COM handler is weak: even a failed unsubscribe cannot form a WebView/Job
    // reference cycle and retain a finished native execution indefinitely.
    let observed_job = Rc::downgrade(job);
    let expected_name = name.clone();
    let expected_frame = frame.clone();
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        let Some(observed_job) = observed_job.upgrade() else {
            return Ok(());
        };
        let Some(args) = args else { return Ok(()) };
        let mut raw = PWSTR::null();
        unsafe { args.ParameterObjectAsJson(&mut raw)? };
        let text = unsafe { raw.to_string() };
        unsafe { CoTaskMemFree(Some(raw.0.cast())) };
        if let Ok(text) = text {
            if text.len() <= MAX_REPLY {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    if let Some(context) = context_identity(&value, &expected_name, &expected_frame)
                    {
                        #[cfg(feature = "computer-use-probe")]
                        if super::probe_fault::suppress(
                            &observed_job,
                            super::probe_fault::Phase::ContextEvent,
                        ) {
                            return Ok(());
                        }
                        let progress = observed_job
                            .setup
                            .borrow_mut()
                            .as_mut()
                            .map(|watch| watch.identity.observed(context));
                        if let Some(progress) = progress {
                            advance(&observed_job, progress);
                        }
                    }
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    if unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }.is_err() {
        return job.fail("context observer registration failed");
    }
    let subscription = Subscription { receiver, token };
    *job.setup.borrow_mut() = Some(Watch {
        _subscription: subscription,
        identity: IdentityJoin::default(),
    });
    job.call("Runtime.enable", json!({}), move |job, _| {
        job.call(
            "Page.createIsolatedWorld",
            json!({"frameId": frame, "worldName": name}),
            |job, created| {
                let Some(id) = created.get("executionContextId").and_then(Value::as_i64) else {
                    return job.fail("context create identity unavailable");
                };
                #[cfg(feature = "computer-use-probe")]
                if super::probe_fault::suppress(&job, super::probe_fault::Phase::CreateReply) {
                    return;
                }
                let progress = job
                    .setup
                    .borrow_mut()
                    .as_mut()
                    .map(|watch| watch.identity.created(id));
                if let Some(progress) = progress {
                    advance(&job, progress);
                }
            },
        );
    });
}

fn advance(job: &Rc<Job>, progress: Progress) {
    match progress {
        Progress::Waiting => {}
        Progress::Rejected => job.fail("context identity changed before execution"),
        Progress::Ready(unique) => {
            // Disarm before Runtime.evaluate. No delayed setup callback/timer
            // may release the ticket of an evaluation that is already running.
            let watch = job.setup.borrow_mut().take();
            drop(watch);
            let deadline = job.setup_deadline.borrow_mut().take();
            drop(deadline);
            if !job.current() || !job.world.publish(job.generation, unique.clone()) {
                return job.fail("context identity changed before execution");
            }
            job.evaluate(unique);
        }
    }
}

struct Deadline<T> {
    nonce: Uuid,
    expires: Instant,
    job: T,
}

thread_local! {
    static DEADLINES: RefCell<HashMap<usize, Deadline<Rc<Job>>>> = RefCell::new(HashMap::new());
}

pub(super) struct Timer {
    id: usize,
    nonce: Uuid,
}

impl Timer {
    pub(super) fn start(job: &Rc<Job>) -> Option<Self> {
        let expires = Instant::now() + Duration::from_millis(u64::from(SETUP_BUDGET_MS));
        // A monotonic deadline, not timer tick count, determines expiry. A coarse
        // or early Windows tick must not defer cleanup for a second full budget.
        let id = unsafe { SetTimer(None, 0, 100, Some(expired)) };
        if id == 0 {
            return None;
        }
        let nonce = Uuid::new_v4();
        DEADLINES.with(|deadlines| {
            deadlines.borrow_mut().insert(
                id,
                Deadline {
                    nonce,
                    expires,
                    job: job.clone(),
                },
            )
        });
        Some(Self { id, nonce })
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let removed = DEADLINES
            .try_with(|deadlines| remove_owned(&mut deadlines.borrow_mut(), self.id, self.nonce))
            .unwrap_or(false);
        if removed {
            unsafe {
                let _ = KillTimer(None, self.id);
            }
        }
    }
}

unsafe extern "system" fn expired(_: HWND, _: u32, id: usize, _: u32) {
    let deadline = DEADLINES.with(|deadlines| {
        take_retired(&mut deadlines.borrow_mut(), id, Instant::now(), |job| {
            !job.current()
        })
    });
    if let Some(deadline) = deadline {
        let _ = unsafe { KillTimer(None, id) };
        let job = deadline.job;
        let still_setup = job.setup_deadline.borrow().is_some();
        if still_setup {
            job.fail(if job.current() {
                "context setup timed out before execution"
            } else {
                "context setup cancelled before execution"
            });
        }
    }
}

fn remove_owned<T>(deadlines: &mut HashMap<usize, Deadline<T>>, id: usize, nonce: Uuid) -> bool {
    if deadlines
        .get(&id)
        .is_some_and(|deadline| deadline.nonce == nonce)
    {
        deadlines.remove(&id);
        true
    } else {
        false
    }
}

fn take_retired<T>(
    deadlines: &mut HashMap<usize, Deadline<T>>,
    id: usize,
    now: Instant,
    cancelled: impl FnOnce(&T) -> bool,
) -> Option<Deadline<T>> {
    // KillTimer does not purge already queued WM_TIMER messages, and Windows
    // may reuse IDs. A stale tick cannot expire a replacement before its time.
    if deadlines
        .get(&id)
        .is_some_and(|deadline| now >= deadline.expires || cancelled(&deadline.job))
    {
        deadlines.remove(&id)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_context_and_create_reply_join_in_either_order_once() {
        for event_first in [true, false] {
            let mut join = IdentityJoin::default();
            let context = (7, "unique-native-document".into());
            let (first, second) = if event_first {
                (join.observed(context.clone()), join.created(7))
            } else {
                (join.created(7), join.observed(context.clone()))
            };
            assert_eq!(first, Progress::Waiting);
            assert_eq!(second, Progress::Ready("unique-native-document".into()));
            assert_eq!(join.created(7), Progress::Waiting);
            assert_eq!(join.observed(context), Progress::Waiting);
        }
    }

    #[test]
    fn mismatch_fails_closed_and_cannot_be_corrected_into_execution() {
        let mut join = IdentityJoin::default();
        assert_eq!(join.created(7), Progress::Waiting);
        assert_eq!(join.observed((8, "wrong".into())), Progress::Rejected);
        assert_eq!(join.observed((7, "corrected".into())), Progress::Waiting);
        assert_eq!(join.created(7), Progress::Waiting);
    }

    #[test]
    fn conflicting_halves_and_missing_unique_identity_never_dispatch() {
        let mut join = IdentityJoin::default();
        assert_eq!(join.created(7), Progress::Waiting);
        assert_eq!(join.created(7), Progress::Waiting);
        assert_eq!(join.created(8), Progress::Rejected);
        let mut join = IdentityJoin::default();
        assert_eq!(join.observed((7, "one".into())), Progress::Waiting);
        assert_eq!(join.observed((7, "two".into())), Progress::Rejected);
        let mut join = IdentityJoin::default();
        assert_eq!(join.observed((7, String::new())), Progress::Rejected);
    }

    #[test]
    fn queued_old_ticks_and_old_drops_do_not_retire_reused_native_timer_ids() {
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let now = Instant::now();
        let expires = now + Duration::from_secs(5);
        let mut deadlines = HashMap::from([(
            7,
            Deadline {
                nonce: new,
                expires,
                job: (),
            },
        )]);
        assert!(!remove_owned(&mut deadlines, 7, old));
        assert!(take_retired(&mut deadlines, 7, now, |_| false).is_none());
        assert_eq!(deadlines.get(&7).unwrap().nonce, new);
        assert_eq!(
            take_retired(&mut deadlines, 7, expires, |_| false)
                .unwrap()
                .nonce,
            new
        );
        assert!(take_retired(&mut deadlines, 7, expires, |_| false).is_none());
    }

    #[test]
    fn completed_setup_disarms_its_deadline_before_dispatch() {
        let nonce = Uuid::new_v4();
        let now = Instant::now();
        let mut deadlines = HashMap::from([(
            7,
            Deadline {
                nonce,
                expires: now,
                job: (),
            },
        )]);
        assert!(remove_owned(&mut deadlines, 7, nonce));
        assert!(take_retired(&mut deadlines, 7, now + Duration::from_secs(60), |_| true).is_none());
        assert!(!remove_owned(&mut deadlines, 7, nonce));
    }

    #[test]
    fn cancellation_retires_only_its_pending_setup_before_the_deadline() {
        let tracker = super::super::super::execution::ScriptTracker::default();
        let now = Instant::now();
        let mut deadlines = HashMap::new();
        for (id, run, view) in [(1, "old", "view-a"), (2, "other", "view-b")] {
            deadlines.insert(
                id,
                Deadline {
                    nonce: Uuid::new_v4(),
                    expires: now + Duration::from_secs(5),
                    job: tracker
                        .begin(run, view, &Default::default(), &Default::default())
                        .unwrap(),
                },
            );
        }
        tracker.cancel("old");
        let retired = take_retired(&mut deadlines, 1, now, |operation| {
            operation.check().is_err()
        })
        .unwrap();
        assert!(
            !tracker.is_idle("old"),
            "selection alone does not settle native work"
        );
        retired.job.finished(); // Corresponds to Job::fail after proving still_setup.
        assert!(tracker.is_idle("old"));
        assert!(take_retired(&mut deadlines, 2, now, |operation| operation
            .check()
            .is_err())
        .is_none());
        assert!(!tracker.is_idle("other"));
        deadlines.remove(&2).unwrap().job.finished();
    }
}
