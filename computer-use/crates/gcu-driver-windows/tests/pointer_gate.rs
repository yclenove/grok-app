//! Named-mutex exclusion for the production pointer gate.
//!
//! This file is a normal dependency of `gcu-driver-windows`, not a `#[path]` copy of the
//! `cfg(test)` gate. On Windows it calls `CreateMutexW`. It does not call `SendInput` and does
//! not need a desktop. Other targets compile no cases: a passing run there is not coverage of
//! the kernel mutex.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{sync::Mutex, thread, time::Duration};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn gate_off_is_unset() {
    assert!(
        std::env::var_os("GCU_POINTER_GATE_OFF").is_none(),
        "GCU_POINTER_GATE_OFF would skip the named mutex"
    );
}

#[test]
fn host_process_reports_its_own_pid_for_the_named_gate() {
    let _hold = serial();
    gate_off_is_unset();
    assert_eq!(
        gcu_driver_windows::input::pointer_gate_owner_pid_for_test().unwrap(),
        std::process::id()
    );
}

#[test]
fn named_mutex_two_threads_exclude_each_other_without_a_desktop() {
    let _hold = serial();
    gate_off_is_unset();
    let held = gcu_driver_windows::input::pointer_gate_enter_for_test(Duration::ZERO)
        .expect("this process acquires its own pointer gate");
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
