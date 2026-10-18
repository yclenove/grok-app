//! QuickJS runtime. User source is a string value, never pasted into host source.

use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc, Condvar, Mutex,
    },
    task::Waker,
    time::Instant,
};

use rquickjs::{context::intrinsic, prelude::Func, Context, Runtime};

use crate::{
    bindings,
    limits::{
        lock, CallGate, CODE_MAX_BYTES, DRIVER_CALL_MAX, EMIT_MAX, HOST_BUFFER_BYTES,
        JS_HEAP_BYTES, JS_STACK_BYTES, OUTPUT_MAX_BYTES,
    },
    report::{self, cap_output, classify_engine, classify_fail},
    route::state_json,
    rpc::{self, Frame, Op, Reply},
    ScriptReport, ScriptStatus,
};

/// Wakes the async service loop. The watchdog calls this without entering QuickJS.
#[derive(Default)]
pub(crate) struct Wake {
    slot: Mutex<Option<Waker>>,
}

impl Wake {
    pub(crate) fn wake(&self) {
        if let Some(waker) = lock(&self.slot).take() {
            waker.wake();
        }
    }

    pub(crate) fn set(&self, waker: Waker) {
        *lock(&self.slot) = Some(waker);
    }
}

pub(crate) struct Slot {
    value: Mutex<Option<Reply>>,
    cv: Condvar,
}

impl Slot {
    pub(crate) fn deliver(&self, reply: Reply) {
        *lock(&self.value) = Some(reply);
        self.cv.notify_all();
    }

    pub(crate) fn poke(&self) {
        self.cv.notify_all();
    }

    fn wait(&self, cancel: &AtomicBool, deadline: Instant) -> Option<Reply> {
        let mut guard = lock(&self.value);
        loop {
            if let Some(reply) = guard.take() {
                return Some(reply);
            }
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return None;
            }
            let (next, _) = self
                .cv
                .wait_timeout(guard, std::time::Duration::from_millis(10))
                .unwrap_or_else(|err| err.into_inner());
            guard = next;
        }
    }
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
            cv: Condvar::new(),
        }
    }
}

struct Meter {
    calls: u32,
    emits: u32,
    output_bytes: usize,
    emits_text: Vec<String>,
    last_text: String,
    latched: bool,
    latch_code: String,
    latch_message: String,
    limit: bool,
    fail: Option<String>,
    finished: Option<String>,
}

impl Meter {
    fn new() -> Self {
        Self {
            calls: 0,
            emits: 0,
            output_bytes: 0,
            emits_text: Vec::new(),
            last_text: String::new(),
            latched: false,
            latch_code: String::new(),
            latch_message: String::new(),
            limit: false,
            fail: None,
            finished: None,
        }
    }
}

struct IoPair {
    input: Box<dyn Read + Send>,
    output: Box<dyn Write + Send>,
}

enum Backend {
    Channel {
        tx: SyncSender<Op>,
        slot: Arc<Slot>,
        wake: Arc<Wake>,
    },
    Stdio(Mutex<IoPair>),
}

pub(crate) struct Shared {
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    meter: Mutex<Meter>,
    gate: Mutex<CallGate>,
    backend: Backend,
}

impl Shared {
    pub(crate) fn channel(
        tx: SyncSender<Op>,
        slot: Arc<Slot>,
        wake: Arc<Wake>,
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            cancel,
            deadline,
            meter: Mutex::new(Meter::new()),
            gate: Mutex::new(CallGate::new()),
            backend: Backend::Channel { tx, slot, wake },
        }
    }

    pub(crate) fn stdio(
        input: impl Read + Send + 'static,
        output: impl Write + Send + 'static,
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            cancel,
            deadline,
            meter: Mutex::new(Meter::new()),
            gate: Mutex::new(CallGate::new()),
            backend: Backend::Stdio(Mutex::new(IoPair {
                input: Box::new(input),
                output: Box::new(output),
            })),
        }
    }

    pub(crate) fn write_finish(&self, report: &ScriptReport) -> bool {
        let Backend::Stdio(io) = &self.backend else {
            return true;
        };
        let mut io = lock(io);
        rpc::write_frame(
            &mut io.output,
            &Frame::Finish {
                status: report::status_byte(report.status),
                output: report.output.clone(),
                driver_calls: report.driver_calls,
                error_code: report.error_code.clone().unwrap_or_default(),
                error_message: report.error_message.clone().unwrap_or_default(),
            },
        )
        .is_ok()
    }

    fn exchange(&self, op: Op) -> Reply {
        match &self.backend {
            Backend::Channel { tx, slot, wake } => {
                if tx.send(op).is_err() {
                    return Reply::stopped();
                }
                wake.wake();
                slot.wait(&self.cancel, self.deadline)
                    .unwrap_or_else(Reply::stopped)
            }
            Backend::Stdio(io) => {
                let mut io = lock(io);
                if rpc::write_frame(&mut io.output, &Frame::Request(op)).is_err() {
                    return Reply::stopped();
                }
                match rpc::read_frame(&mut io.input) {
                    Ok(Frame::Reply(reply)) => reply,
                    _ => Reply::stopped(),
                }
            }
        }
    }

    fn precheck(&self) -> Option<String> {
        if Instant::now() >= self.deadline {
            return Some("ERR:timeout:timeout".to_owned());
        }
        if self.cancel.load(Ordering::Acquire) {
            return Some("ERR:stopped_by_user:stopped".to_owned());
        }
        let mut meter = lock(&self.meter);
        if meter.latched {
            return Some(format!("ERR:{}:{}", meter.latch_code, meter.latch_message));
        }
        if meter.calls >= DRIVER_CALL_MAX {
            meter.limit = true;
            return Some("ERR:capability_unavailable:driver call limit".to_owned());
        }
        meter.calls += 1;
        drop(meter);
        if lock(&self.gate).try_admit().is_err() {
            let mut meter = lock(&self.meter);
            meter.calls = meter.calls.saturating_sub(1);
            meter.limit = true;
            return Some("ERR:capability_unavailable:queue full".to_owned());
        }
        None
    }

    fn release_gate(&self) {
        lock(&self.gate).release();
    }

    fn call(&self, op: Op) -> String {
        if let Some(error) = self.precheck() {
            return error;
        }
        let reply = self.exchange(op);
        self.release_gate();
        self.post(reply)
    }

    fn post(&self, reply: Reply) -> String {
        if reply
            .state
            .as_ref()
            .is_some_and(|state| state.text.len() > HOST_BUFFER_BYTES)
        {
            lock(&self.meter).limit = true;
            return "ERR:capability_unavailable:host buffer".to_owned();
        }
        let mut meter = lock(&self.meter);
        if reply.hard_stop || reply.unknown {
            meter.latched = true;
            meter.latch_code = if reply.code.is_empty() {
                "driver_error".to_owned()
            } else {
                reply.code.clone()
            };
            meter.latch_message = reply.message.clone();
        }
        if let Some(state) = &reply.state {
            meter.last_text = state.text.clone();
        }
        if reply.code.is_empty() {
            match &reply.state {
                Some(state) => state_json(state),
                None => "{}".to_owned(),
            }
        } else {
            format!("ERR:{}:{}", reply.code, reply.message)
        }
    }

    fn emit(&self, text: String) -> String {
        let mut meter = lock(&self.meter);
        if meter.emits >= EMIT_MAX {
            meter.limit = true;
            return "ERR:capability_unavailable:emit limit".to_owned();
        }
        if meter.output_bytes.saturating_add(text.len()) > OUTPUT_MAX_BYTES {
            meter.limit = true;
            return "ERR:capability_unavailable:output limit".to_owned();
        }
        meter.emits += 1;
        meter.output_bytes += text.len();
        meter.emits_text.push(text);
        "ok".to_owned()
    }

    fn finish(&self, text: String) -> String {
        let mut meter = lock(&self.meter);
        if meter.output_bytes.saturating_add(text.len()) > OUTPUT_MAX_BYTES {
            meter.limit = true;
            meter.fail = Some("capability_unavailable:output limit".to_owned());
            return "ok".to_owned();
        }
        meter.finished = Some(text);
        "ok".to_owned()
    }

    fn fail(&self, message: String) {
        lock(&self.meter).fail = Some(message);
    }

    fn report(&self, engine_error: Option<String>) -> ScriptReport {
        let meter = lock(&self.meter);
        let output = if !meter.emits_text.is_empty() {
            meter.emits_text.join("\n")
        } else if let Some(finished) = &meter.finished {
            if finished.is_empty() {
                meter.last_text.clone()
            } else {
                finished.clone()
            }
        } else {
            meter.last_text.clone()
        };
        let output = cap_output(&output);
        let timed_out = Instant::now() >= self.deadline;
        let cancelled = self.cancel.load(Ordering::Acquire);
        let (status, code, message) = if timed_out {
            (
                ScriptStatus::Timeout,
                "timeout".to_owned(),
                "timeout".to_owned(),
            )
        } else if cancelled && meter.finished.is_none() {
            (
                ScriptStatus::Stopped,
                "stopped_by_user".to_owned(),
                "stopped".to_owned(),
            )
        } else if meter.latched {
            (
                ScriptStatus::Stopped,
                meter.latch_code.clone(),
                meter.latch_message.clone(),
            )
        } else if meter.limit && meter.finished.is_none() {
            (
                ScriptStatus::Limit,
                "capability_unavailable".to_owned(),
                meter.fail.clone().unwrap_or_else(|| "limit".to_owned()),
            )
        } else if let Some(fail) = &meter.fail {
            classify_fail(fail)
        } else if let Some(error) = engine_error {
            classify_engine(&error)
        } else {
            (ScriptStatus::Done, String::new(), String::new())
        };
        ScriptReport {
            status,
            output,
            driver_calls: meter.calls,
            error_code: if code.is_empty() { None } else { Some(code) },
            error_message: if message.is_empty() {
                None
            } else {
                Some(message)
            },
        }
    }
}

/// Run one fresh context. Blocking: this thread until the script ends, the
/// deadline passes, or `cancel` is set. Timeout: `shared.deadline`. Cancellation:
/// `shared.cancel` aborts JS via the interrupt handler and unblocks a host wait
/// within 10ms. Does not replay a call after a hard stop.
pub(crate) fn execute(shared: Arc<Shared>, code: &str) -> ScriptReport {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute_inner(shared.clone(), code)
    }));
    match caught {
        Ok(report) => report,
        Err(_) => shared.report(Some("panic".to_owned())),
    }
}

fn execute_inner(shared: Arc<Shared>, code: &str) -> ScriptReport {
    if code.len() > CODE_MAX_BYTES {
        return ScriptReport {
            status: ScriptStatus::Limit,
            output: String::new(),
            driver_calls: 0,
            error_code: Some("capability_unavailable".to_owned()),
            error_message: Some("code exceeds 20000 UTF-8 bytes".to_owned()),
        };
    }
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(_) => return shared.report(Some("runtime".to_owned())),
    };
    runtime.set_memory_limit(JS_HEAP_BYTES);
    runtime.set_max_stack_size(JS_STACK_BYTES);
    let cancel = shared.cancel.clone();
    let deadline = shared.deadline;
    runtime.set_interrupt_handler(Some(Box::new(move || {
        cancel.load(Ordering::Acquire) || Instant::now() >= deadline
    })));
    // The async-function constructor is the host's compiler. It is enabled only
    // so user source can be compiled as data, then LOCKDOWN removes `eval` and
    // the constructor before that function runs.
    let context = match Context::builder()
        .with::<intrinsic::Json>()
        .with::<intrinsic::Promise>()
        .with::<intrinsic::Eval>()
        .build(&runtime)
    {
        Ok(context) => context,
        Err(_) => return shared.report(Some("context".to_owned())),
    };
    let engine_error = context.with(|ctx| -> Result<(), String> {
        install(&ctx, &shared)?;
        ctx.globals()
            .set("__src", code)
            .map_err(|_| "install source".to_owned())?;
        ctx.eval::<(), _>(bindings::COMPILE)
            .map_err(|err| js_err(&ctx, "compile", err))?;
        ctx.globals()
            .remove("__src")
            .map_err(|_| "clear source".to_owned())?;
        ctx.eval::<(), _>(bindings::LOCKDOWN)
            .map_err(|err| js_err(&ctx, "lockdown", err))?;
        let promise = ctx
            .eval_promise(bindings::RUNNER)
            .map_err(|err| js_err(&ctx, "runner", err))?;
        promise
            .finish::<()>()
            .map_err(|err| js_err(&ctx, "finish", err))?;
        Ok(())
    });
    shared.report(engine_error.err())
}

fn js_err(ctx: &rquickjs::Ctx<'_>, step: &str, err: rquickjs::Error) -> String {
    let caught = ctx.catch();
    let detail = if let Some(exc) = caught.as_exception() {
        exc.message()
            .or_else(|| exc.stack())
            .unwrap_or_else(|| err.to_string())
    } else {
        err.to_string()
    };
    format!("{step}: {detail}")
}

fn install(ctx: &rquickjs::Ctx<'_>, shared: &Arc<Shared>) -> Result<(), String> {
    let app = shared.clone();
    ctx.globals()
        .set(
            "__cu_app",
            Func::from(move |name: String| app.call(Op::App { name })),
        )
        .map_err(|_| "bind".to_owned())?;
    let state = shared.clone();
    ctx.globals()
        .set(
            "__cu_state",
            Func::from(move |app: String, shot: bool| {
                state.call(Op::State {
                    app,
                    screenshot: shot,
                })
            }),
        )
        .map_err(|_| "bind".to_owned())?;
    let set = shared.clone();
    ctx.globals()
        .set(
            "__cu_set",
            Func::from(move |app: String, index: f64, value: String| {
                set.call(Op::SetValue {
                    app,
                    index: as_u32(index),
                    value,
                })
            }),
        )
        .map_err(|_| "bind".to_owned())?;
    let press = shared.clone();
    ctx.globals()
        .set(
            "__cu_press",
            Func::from(move |app: String, key: String| press.call(Op::Press { app, key })),
        )
        .map_err(|_| "bind".to_owned())?;
    let click = shared.clone();
    ctx.globals()
        .set(
            "__cu_click",
            Func::from(move |app: String, index: f64| {
                click.call(Op::Click {
                    app,
                    index: as_u32(index),
                })
            }),
        )
        .map_err(|_| "bind".to_owned())?;
    let typed = shared.clone();
    ctx.globals()
        .set(
            "__cu_type",
            Func::from(move |app: String, text: String, index: f64| {
                typed.call(Op::TypeText {
                    app,
                    text,
                    index: if index < 0.0 {
                        None
                    } else {
                        Some(as_u32(index))
                    },
                })
            }),
        )
        .map_err(|_| "bind".to_owned())?;
    let wait_el = shared.clone();
    ctx.globals()
        .set(
            "__cu_wait_el",
            Func::from(
                move |app: String, role: String, name: String, timeout: f64| {
                    wait_el.call(Op::WaitElement {
                        app,
                        role,
                        name: if name.is_empty() { None } else { Some(name) },
                        timeout_ms: as_u32(timeout),
                    })
                },
            ),
        )
        .map_err(|_| "bind".to_owned())?;
    let wait_text = shared.clone();
    ctx.globals()
        .set(
            "__cu_wait_text",
            Func::from(
                move |app: String, text: String, present: bool, timeout: f64| {
                    wait_text.call(Op::WaitText {
                        app,
                        text,
                        present,
                        timeout_ms: as_u32(timeout),
                    })
                },
            ),
        )
        .map_err(|_| "bind".to_owned())?;
    let emit = shared.clone();
    ctx.globals()
        .set("__cu_emit", Func::from(move |text: String| emit.emit(text)))
        .map_err(|_| "bind".to_owned())?;
    let finish = shared.clone();
    ctx.globals()
        .set(
            "__cu_finish",
            Func::from(move |text: String| finish.finish(text)),
        )
        .map_err(|_| "bind".to_owned())?;
    let fail = shared.clone();
    ctx.globals()
        .set("__cu_fail", Func::from(move |text: String| fail.fail(text)))
        .map_err(|_| "bind".to_owned())?;
    Ok(())
}

fn as_u32(value: f64) -> u32 {
    if value.is_finite() && value >= 0.0 && value <= f64::from(u32::MAX) {
        value as u32
    } else {
        0
    }
}
