//! Named-mutex exclusion for the production pointer gate.
//!
//! This file is a normal dependency of `gcu-driver-windows`, not a `#[path]` copy of the
//! `cfg(test)` gate. On Windows it calls `CreateMutexW` on `Local\gcu-pointer-gate-v1`.
//! It does not call `SendInput` and does not need a desktop. Other targets compile no cases:
//! a passing run there is not coverage of the kernel mutex.
//!
//! The child-process cases re-exec this test binary. They must not take `SERIAL`: the parent
//! holds it, and the conflict case must run before this process's mutex `OnceLock` is filled.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, Once,
    },
    thread,
    time::{Duration, Instant},
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    assert!(
        std::env::var_os("GCU_POINTER_GATE_CHILD").is_none(),
        "a re-exec child must not take the parent serial lock"
    );
    let guard = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    // First holder only, and before this process calls `CreateMutexW`.
    ensure_create_failure_is_diagnosable();
    guard
}

fn gate_off_is_unset() {
    assert!(
        std::env::var_os("GCU_POINTER_GATE_OFF").is_none(),
        "GCU_POINTER_GATE_OFF would skip the named mutex"
    );
}

fn status_path(mode: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "gcu-pointer-gate-{mode}-{}-{n}.txt",
        std::process::id()
    ))
}

fn spawn_child(mode: &str, status: &Path, piped_stdin: bool) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "pointer_gate_child_process"])
        .env("GCU_POINTER_GATE_CHILD", mode)
        .env("GCU_POINTER_GATE_STATUS", status)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if piped_stdin {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    command.spawn().expect("spawn pointer-gate child")
}

fn wait_status(path: &Path, child: &mut Child) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.is_empty() {
                return text;
            }
        }
        if let Some(code) = child.try_wait().expect("poll child") {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            panic!("pointer-gate child exited {code} before a status write; file={text:?}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("pointer-gate child did not write {path:?} within 10s");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn ensure_create_failure_is_diagnosable() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        gate_off_is_unset();
        let status = status_path("conflict");
        let _ = std::fs::remove_file(&status);
        let mut child = spawn_child("conflict", &status, false);
        let text = wait_status(&status, &mut child);
        let code = child.wait().expect("wait conflict child");
        assert!(code.success(), "conflict child failed: {code}; body={text}");
        assert!(
            text.contains("Win32"),
            "CreateMutexW failure must keep its Win32 code, got {text}"
        );
        let _ = std::fs::remove_file(&status);
    });
}

fn child_hold(status: &str) {
    gate_off_is_unset();
    let guard = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("child acquires the named mutex");
    std::fs::write(status, "held").expect("status");
    let mut byte = [0u8; 1];
    let _ = std::io::stdin().read(&mut byte);
    drop(guard);
}

fn child_conflict(status: &str) {
    use windows::{
        core::PCWSTR,
        Win32::{Foundation::CloseHandle, System::Threading::CreateEventW},
    };
    gate_off_is_unset();
    // Same session name as the gate. An event already owns it, so CreateMutexW must fail.
    // `HANDLE` is `Copy`; dropping it does not close the object.
    let name: Vec<u16> = "Local\\gcu-pointer-gate-v1"
        .encode_utf16()
        .chain([0])
        .collect();
    let event = unsafe { CreateEventW(None, true, false, PCWSTR(name.as_ptr())) }
        .expect("reserve the pointer-gate name with an event");
    let text = match gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO) {
        Ok(guard) => {
            drop(guard);
            "entered".to_string()
        }
        Err(message) => message,
    };
    std::fs::write(status, &text).expect("status");
    // SAFETY: this child created `event` and has not closed it. Closing frees the name
    // before the process exits, so the parent can create the mutex afterwards.
    unsafe { CloseHandle(event) }.expect("close the name reservation");
}

/// Re-exec entry. A parent run without the env var is a no-op and does not touch the mutex.
#[test]
fn pointer_gate_child_process() {
    let Ok(mode) = std::env::var("GCU_POINTER_GATE_CHILD") else {
        return;
    };
    let status = std::env::var("GCU_POINTER_GATE_STATUS").expect("status path");
    match mode.as_str() {
        "hold" => child_hold(&status),
        "conflict" => child_conflict(&status),
        other => panic!("unknown pointer-gate child mode {other}"),
    }
}

#[test]
fn named_mutex_two_threads_exclude_each_other_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let held = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("this process acquires the session pointer gate");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let waiter = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        // The guard is !Send. Drop it here and return only the status.
        // Bounded so a missed release fails the test instead of hanging it.
        match gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::from_secs(2)) {
            Ok(guard) => {
                drop(guard);
                Ok(())
            }
            Err(error) => Err(error),
        }
    });
    ready_rx.recv().unwrap();
    // Joining this attempt before `held` is dropped is the exclusion proof: zero wait
    // returns only after WaitForSingleObject, and this thread already owns the mutex.
    let denied = thread::spawn(|| {
        gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO).is_err()
    });
    assert!(
        denied.join().expect("contender thread"),
        "a second thread must not enter the named mutex while it is held"
    );
    drop(held);
    waiter
        .join()
        .expect("waiter thread")
        .expect("the waiter enters after the owner releases");
}

#[test]
fn named_mutex_abandoned_owner_is_acquired_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let owner = thread::spawn(move || {
        let guard = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
            .expect("owner acquires");
        // Leave the mutex owned when this thread ends. Windows reports WAIT_ABANDONED,
        // which the gate treats as acquisition.
        std::mem::forget(guard);
        done_tx.send(()).unwrap();
    });
    done_rx.recv().unwrap();
    owner.join().unwrap();
    let abandoned = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::from_secs(2))
        .expect("WAIT_ABANDONED grants ownership");
    drop(abandoned);
    let next = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("the abandoned acquisition was released");
    drop(next);
}

#[test]
fn named_mutex_other_process_blocks_until_it_releases_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let status = status_path("hold");
    let _ = std::fs::remove_file(&status);
    let mut child = spawn_child("hold", &status, true);
    assert_eq!(wait_status(&status, &mut child), "held");
    assert!(
        gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO).is_err(),
        "zero wait must fail while the child holds the named mutex"
    );
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(&[1])
        .expect("release child");
    let code = child.wait().expect("wait hold child");
    assert!(code.success(), "hold child failed: {code}");
    let guard = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("the parent enters after the child releases");
    drop(guard);
    let _ = std::fs::remove_file(&status);
}

#[test]
fn named_mutex_killed_child_grants_abandoned_ownership_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let status = status_path("abandon");
    let _ = std::fs::remove_file(&status);
    let mut child = spawn_child("hold", &status, true);
    assert_eq!(wait_status(&status, &mut child), "held");
    // Opening the mutex now caches the handle. A later wait can observe WAIT_ABANDONED.
    // If the child died first, the kernel object would vanish with its last handle.
    assert!(
        gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO).is_err(),
        "the child still holds the mutex"
    );
    child.kill().expect("kill child");
    let _ = child.wait();
    let abandoned = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::from_secs(2))
        .expect("WAIT_ABANDONED grants ownership after the child is killed");
    drop(abandoned);
    let next = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("the abandoned acquisition was released");
    drop(next);
    let _ = std::fs::remove_file(&status);
}

#[test]
fn named_mutex_same_thread_reacquires_and_inner_drop_keeps_the_outer_hold() {
    let _hold = serial();
    gate_off_is_unset();
    let outer = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("outer acquire");
    let inner = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("same thread reacquires a Win32 mutex");
    let denied = thread::spawn(|| {
        gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO).is_err()
    });
    assert!(
        denied.join().expect("contender"),
        "another thread must not enter while the inner guard is held"
    );
    drop(inner);
    let still_held = thread::spawn(|| {
        gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO).is_err()
    });
    assert!(
        still_held.join().expect("contender"),
        "dropping the inner guard must not release the outer hold"
    );
    drop(outer);
    let next = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("the outer drop releases the mutex");
    drop(next);
}

#[test]
fn named_mutex_unwind_releases_the_hold_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
            .expect("acquire before panic");
        panic!("drop the pointer gate on unwind");
    }));
    assert!(panicked.is_err(), "the panic must still unwind");
    let guard = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("unwind dropped the guard and released the mutex");
    drop(guard);
}
