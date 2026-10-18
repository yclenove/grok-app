//! Sandbox limits, escapes, watchdog, cancellation, and batch-equivalent outcomes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_script::{
    Admit, CallGate, ElementView, Op, Reply, ScriptReport, ScriptStatus, StateView,
    HOST_BUFFER_BYTES, OUTPUT_MAX_BYTES, WATCHDOG_KILL_MS,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

fn drive<F, Fut>(code: &str, timeout: Duration, cancel: Arc<AtomicBool>, host: F) -> ScriptReport
where
    F: FnMut(Op) -> Fut,
    Fut: Future<Output = Reply>,
{
    gcu_core::testing::block_on(gcu_script::run(code, timeout, cancel, host)).unwrap()
}

fn answer(code: &str, timeout: Duration, host: impl FnMut(Op) -> Reply) -> ScriptReport {
    let host = {
        let host = Mutex::new(host);
        move |op| {
            let reply = host.lock().unwrap_or_else(|err| err.into_inner())(op);
            std::future::ready(reply)
        }
    };
    drive(code, timeout, Arc::new(AtomicBool::new(false)), host)
}

fn field_state() -> StateView {
    StateView {
        text: "1 text area".into(),
        focused: Some(1),
        elements: vec![ElementView {
            index: 1,
            role: "text area".into(),
            name: String::new(),
        }],
    }
}

fn hard(code: &str) -> Reply {
    Reply::fail(code, code, true, false)
}

#[test]
fn manifest_disables_forbidden_rquickjs_features() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text.find("rquickjs").unwrap();
    let window = &text[start..];
    let end = window.find("[dev-dependencies]").unwrap();
    let dep = &window[..end];
    assert!(dep.contains("default-features = false"), "{dep}");
    assert!(dep.contains("features = [\"std\"]"), "{dep}");
    for banned in [
        "rust-alloc",
        "allocator",
        "loader",
        "dyn-load",
        "full",
        "parallel",
    ] {
        assert!(!dep.contains(banned), "{banned} in {dep}");
    }
}

#[test]
fn queue_over_32_is_rejected_without_blocking() {
    let mut gate = CallGate::new();
    assert_eq!(gate.try_admit().unwrap(), Admit::Run);
    for _ in 0..32 {
        assert_eq!(gate.try_admit().unwrap(), Admit::Queued);
    }
    let started = Instant::now();
    assert!(gate.try_admit().is_err());
    assert!(started.elapsed() < Duration::from_millis(50));
    assert_eq!(gate.queued(), 32);
}

#[test]
fn native_concurrency_stays_at_one() {
    let mut gate = CallGate::new();
    assert_eq!(gate.try_admit().unwrap(), Admit::Run);
    assert_eq!(gate.try_admit().unwrap(), Admit::Queued);
    assert_eq!(gate.inflight(), 1);
    gate.release();
    assert_eq!(gate.inflight(), 1);
    assert_eq!(gate.queued(), 0);
}

#[test]
fn watchdog_kill_runs_within_500ms_of_deadline() {
    let hit = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&hit);
    let start = Instant::now();
    let deadline = start + Duration::from_millis(50);
    thread::spawn(move || {
        gcu_script::run_watchdog_until(deadline, move || flag.store(true, Ordering::Release));
    })
    .join()
    .unwrap();
    let elapsed = start.elapsed();
    assert!(hit.load(Ordering::Acquire));
    assert!(elapsed >= Duration::from_millis(50), "{elapsed:?}");
    assert!(
        elapsed < Duration::from_millis(50 + WATCHDOG_KILL_MS),
        "{elapsed:?}"
    );
}

#[test]
fn unexposed_globals_are_absent_in_user_script() {
    let report = answer(
        r#"
        var names = ["eval","Function","process","require","os","std","fs","Deno","fetch","setTimeout","setInterval"];
        var found = [];
        for (var i = 0; i < names.length; i++) {
          if (typeof globalThis[names[i]] !== "undefined") found.push(names[i]);
        }
        return found.join(",");
        "#,
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.output, "\"\"");
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn prototype_constructor_escape_is_rejected() {
    let report = answer(
        r#"
        var hits = [];
        function attempt(label, fn) {
          try { fn(); hits.push(label); } catch (e) { hits.push("no"); }
        }
        attempt("ctor", function () { (function(){}).constructor("return this")(); });
        attempt("fn", function () { Function("return 1")(); });
        attempt("obj", function () { ({}).constructor.constructor("return this")(); });
        attempt("gen", function () { Object.getPrototypeOf(function* () {}).constructor("yield 1")(); });
        attempt("agen", function () { Object.getPrototypeOf(async function* () {}).constructor("yield 1")(); });
        attempt("async", function () { Object.getPrototypeOf(async function () {}).constructor("return 1")(); });
        return hits.join(",");
        "#,
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.output, "\"no,no,no,no,no,no\"");
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn wrapper_breakout_does_not_run_as_host_code() {
    let calls = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&calls);
    let report = answer(
        "}); await cu.app(\"escaped\");",
        Duration::from_secs(2),
        move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            Reply::clear(None)
        },
    );
    assert_ne!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn fresh_runtime_drops_previous_globals() {
    let first = answer(
        "globalThis.marker = 7; return globalThis.marker;",
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    let second = answer(
        "return globalThis.marker || 0;",
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    assert_eq!(first.status, ScriptStatus::Done, "{first:?}");
    assert_eq!(first.output, "7");
    assert_eq!(second.output, "0", "{second:?}");
}

#[test]
fn infinite_loop_hits_wall_clock_before_cap() {
    let start = Instant::now();
    let report = answer("while (true) {}", Duration::from_millis(200), |_| {
        Reply::clear(None)
    });
    let elapsed = start.elapsed();
    assert_eq!(report.status, ScriptStatus::Timeout, "{report:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn memory_bomb_stays_inside_process_and_rejects() {
    let report = answer(
        r#"
        var bag = [];
        try {
          for (var i = 0; i < 40; i++) bag.push("x".repeat(1024 * 1024));
          return "allocated";
        } catch (e) {
          return "caught";
        }
        "#,
        Duration::from_secs(5),
        |_| Reply::clear(None),
    );
    assert_ne!(report.output, "\"allocated\"", "{report:?}");
    assert!(
        report.status == ScriptStatus::Limit || report.output.contains("caught"),
        "{report:?}"
    );
    assert!(report.output.len() < 64, "{report:?}");
}

#[test]
fn host_buffer_over_32mb_is_not_retained() {
    let bomb = "z".repeat(HOST_BUFFER_BYTES + 1);
    let report = answer(
        "await cu.app(\"bomb\"); return \"seen\";",
        Duration::from_secs(5),
        {
            let bomb = bomb.clone();
            move |_| {
                Reply::clear(Some(StateView {
                    text: bomb.clone(),
                    focused: None,
                    elements: Vec::new(),
                }))
            }
        },
    );
    assert_eq!(report.status, ScriptStatus::Limit, "{report:?}");
    assert!(report.output.len() < 4_096, "{}", report.output.len());
    assert!(!report.output.contains('z'));
    drop(bomb);
}

#[test]
fn oversized_emit_is_rejected_at_2mb() {
    let report = answer(
        &format!(
            "try {{ emit(\"x\".repeat({})); return \"sent\"; }} catch (e) {{ return \"rejected\"; }}",
            OUTPUT_MAX_BYTES + 8
        ),
        Duration::from_secs(5),
        |_| Reply::clear(None),
    );
    assert!(report.output.contains("rejected"), "{report:?}");
    assert!(report.output.len() < 64, "{report:?}");
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn emit_over_five_is_rejected() {
    let report = answer(
        r#"
        for (var i = 0; i < 8; i++) {
          try { emit(i); } catch (e) { return "stopped"; }
        }
        return "all";
        "#,
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.output.lines().count(), 5, "{report:?}");
    assert!(!report.output.contains("stopped"));
}

#[test]
fn deep_recursion_is_rejected_by_stack_limit() {
    let report = answer(
        "function dive(n) { return dive(n + 1); } dive(0);",
        Duration::from_secs(2),
        |_| Reply::clear(None),
    );
    assert_eq!(report.status, ScriptStatus::Limit, "{report:?}");
    assert!(report.output.len() < 256, "{report:?}");
}

#[test]
fn driver_call_over_200_does_not_reach_host() {
    let calls = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&calls);
    let report = answer(
        r#"
        var n = 0;
        try {
          for (var i = 0; i < 250; i++) { await cu.app("a"); n++; }
        } catch (e) {}
        return String(n);
        "#,
        Duration::from_secs(5),
        move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            Reply::clear(None)
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 200, "{report:?}");
    assert_eq!(report.driver_calls, 200, "{report:?}");
    assert!(report.output.contains("200"), "{report:?}");
}

#[test]
fn local_find_does_not_call_host() {
    let calls = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&calls);
    let report = answer(
        r#"
        const app = await cu.app("TextEdit");
        const s = await app.state({ screenshot: false });
        const field = s.find({ role: "text_area" });
        return field ? field.index : -1;
        "#,
        Duration::from_secs(2),
        move |op| {
            seen.fetch_add(1, Ordering::Relaxed);
            match op {
                Op::State { screenshot, .. } => {
                    assert!(!screenshot);
                    Reply::clear(Some(field_state()))
                }
                _ => Reply::clear(None),
            }
        },
    );
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.output, "1", "{report:?}");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn success_path_returns_script_value() {
    let ops = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&ops);
    let report = answer(
        r#"
        const app = await cu.app("TextEdit");
        let s = await app.state({ screenshot: false });
        const field = s.find({ role: "text_area" });
        if (!field) throw new Error("missing");
        await app.setValue(field, "季度总结");
        await app.press("cmd+s");
        await app.waitFor({ element: { role: "sheet" } });
        emit(await app.state());
        return { ok: true };
        "#,
        Duration::from_secs(2),
        move |op| {
            seen.lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(op.clone());
            match op {
                Op::State { .. } | Op::App { .. } | Op::Press { .. } | Op::WaitElement { .. } => {
                    Reply::clear(Some(field_state()))
                }
                Op::SetValue { index, value, .. } => {
                    assert_eq!(index, 1);
                    assert_eq!(value, "季度总结");
                    Reply::clear(Some(field_state()))
                }
                other => panic!("unexpected {other:?}"),
            }
        },
    );
    assert_eq!(report.status, ScriptStatus::Done, "{report:?}");
    assert_eq!(report.driver_calls, 6, "{report:?}");
    let ops = ops.lock().unwrap_or_else(|err| err.into_inner());
    assert!(matches!(ops.first(), Some(Op::App { name }) if name == "TextEdit"));
    assert!(matches!(
        ops.get(1),
        Some(Op::State {
            screenshot: false,
            ..
        })
    ));
    assert!(report.output.contains("text area"), "{report:?}");
}

fn stopped_retry(code: &str, reply: Reply) {
    let calls = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&calls);
    let report = answer(
        r#"
        try { await cu.app("a"); } catch (e) {}
        try { await cu.app("b"); } catch (e) {}
        return "after";
        "#,
        Duration::from_secs(2),
        move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            reply.clone()
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "{report:?}");
    assert_eq!(report.status, ScriptStatus::Stopped, "{report:?}");
    assert_eq!(report.error_code.as_deref(), Some(code), "{report:?}");
}

#[test]
fn stale_error_is_not_replayed() {
    stopped_retry("stale_element", hard("stale_element"));
}

#[test]
fn approval_denied_is_not_replayed() {
    stopped_retry("approval_denied", hard("approval_denied"));
}

#[test]
fn timeout_error_is_not_replayed() {
    stopped_retry("timeout", hard("timeout"));
}

#[test]
fn unknown_outcome_is_not_replayed() {
    stopped_retry(
        "driver_error",
        Reply::fail("driver_error", "unknown", true, true),
    );
}

#[test]
fn cancel_flag_stops_infinite_loop() {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        flag.store(true, Ordering::Release);
    });
    let start = Instant::now();
    let report = drive("while (true) {}", Duration::from_secs(5), cancel, |_| {
        std::future::ready(Reply::clear(None))
    });
    assert_eq!(report.status, ScriptStatus::Stopped, "{report:?}");
    assert!(start.elapsed() < Duration::from_secs(2), "{report:?}");
    assert_eq!(report.driver_calls, 0);
}

#[test]
fn cancel_during_host_call_unblocks_without_replay() {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let started = Arc::new(AtomicBool::new(false));
    let started2 = Arc::clone(&started);
    thread::spawn(move || {
        while !started2.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(20));
        flag.store(true, Ordering::Release);
    });
    let calls = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&calls);
    let gate = Arc::clone(&cancel);
    let report = drive(
        r#"
        try { await cu.app("a"); } catch (e) {}
        try { await cu.app("b"); } catch (e) {}
        return "after";
        "#,
        Duration::from_secs(5),
        cancel,
        move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            started.store(true, Ordering::Release);
            let gate = Arc::clone(&gate);
            async move {
                let until = Instant::now() + Duration::from_millis(250);
                while Instant::now() < until && !gate.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
                thread::sleep(Duration::from_millis(30));
                Reply::clear(None)
            }
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "{report:?}");
    assert_eq!(report.status, ScriptStatus::Stopped, "{report:?}");
}
