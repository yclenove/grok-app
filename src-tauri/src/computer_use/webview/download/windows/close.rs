//! Fence admission, cancel on the original STA, and retain callbacks until drained.

use super::*;

fn fence(instance: u64) {
    if let Some(record) = INSTALLS.lock().get(&instance) {
        *record.state.lock() = InstallState::Closed;
        record.changed.notify_all();
    }
    mark_instance_cancelled(instance);
}

pub(crate) fn retire_instance(webview: &tauri::Webview, instance: u64) {
    fence(instance);
    let _ = webview.with_webview(move |_| cancel_instance_on_native_thread(instance));
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn retire_instance_now(instance: u64) {
    fence(instance);
    cancel_instance_on_native_thread(instance);
}

pub(super) fn physical_pending(instance: u64) -> bool {
    EXPECTATIONS.lock().values().any(|expectation| {
        if expectation.instance != instance {
            return false;
        }
        let state = expectation.state.lock();
        !physically_finished(&state)
    })
}

fn physically_finished(state: &ExpectationState) -> bool {
    state.native_exited || (state.wry_finished && state.terminal.is_some())
}

/// Run off the UI thread. A timeout leaves the native owner/callbacks intact.
pub(crate) fn wait_retired(instance: u64) -> Result<(), String> {
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    loop {
        let pending = EXPECTATIONS
            .lock()
            .values()
            .filter(|expectation| expectation.instance == instance)
            .cloned()
            .collect::<Vec<_>>();
        let mut drained = true;
        for expectation in pending {
            let mut state = expectation.state.lock();
            if physically_finished(&state) {
                continue;
            }
            drained = false;
            let now = Instant::now();
            if now >= deadline {
                return Err(
                    "WebView download cancellation is still pending; native view retained".into(),
                );
            }
            expectation
                .changed
                .wait_for(&mut state, (deadline - now).min(Duration::from_millis(50)));
        }
        if drained {
            return Ok(());
        }
    }
}

/// Post to the original UI thread before its close request. No COM crosses threads.
pub(crate) fn uninstall_instance(webview: &tauri::Webview, instance: u64) -> Result<(), String> {
    webview
        .with_webview(move |_| uninstall_instance_now(instance))
        .map_err(|_| "WebView download observer cleanup unavailable".into())
}

pub(crate) fn uninstall_instance_now(instance: u64) {
    if physical_pending(instance) {
        return;
    }
    exit::detach(instance);
    clear_native_operations(instance);
    INSTALLS.lock().remove(&instance);
}

pub(super) fn clear_native_operations(instance: u64) {
    let retired = NATIVE_OPERATIONS.with(|operations| {
        let mut operations = operations.borrow_mut();
        let ids = operations
            .iter()
            .filter(|(_, native)| native.instance == instance)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| operations.remove(&id))
            .collect::<Vec<_>>()
    });
    // Releasing COM can reenter callbacks; keep it outside the RefCell borrow.
    drop(retired);
}

#[cfg(feature = "computer-use-probe")]
pub(crate) fn probe_resources_retired(instance: u64) -> bool {
    !INSTALLS.lock().contains_key(&instance)
        && !exit::observer_present(instance)
        && NATIVE_OPERATIONS.with(|operations| {
            !operations
                .borrow()
                .values()
                .any(|native| native.instance == instance)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_fences_late_admission_without_retiring_another_instance() {
        let old = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 500_000;
        let new = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 500_000;
        super::super::tests::install_record(old);
        super::super::tests::install_record(new);
        fence(old);
        let tracker = ScriptTracker::default();
        let operation = tracker
            .begin(
                "closed-old",
                "old",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        let root = std::env::temp_dir().join(format!("cu-close-admission-{old}-{new}"));
        assert!(register(
            old,
            "https://fixture.invalid/old",
            root.join("old.bin"),
            operation
        )
        .is_err());
        assert!(tracker.is_idle("closed-old"));
        let operation = tracker
            .begin("live-new", "new", &Default::default(), &Default::default())
            .unwrap();
        let expectation = register(
            new,
            "https://fixture.invalid/new",
            root.join("new.bin"),
            operation,
        )
        .unwrap();
        assert!(!tracker.is_idle("live-new"));
        retire_before_dispatch(&expectation);
        INSTALLS.lock().remove(&old);
        INSTALLS.lock().remove(&new);
    }

    #[test]
    fn close_requires_both_terminal_callbacks_and_keeps_caller_ownership() {
        let (expectation, tracker, instance, url, path) = super::super::tests::expectation();
        fence(instance);
        complete(&expectation, NativeTerminal::Interrupted);
        assert!(physical_pending(instance));
        assert!(!tracker.is_idle("download-test-run"));
        assert!(finish_wry(instance, &url, Some(&path), false));
        assert!(!physical_pending(instance));
        assert!(wait_retired(instance).is_ok());
        assert!(!tracker.is_idle("download-test-run"));
        assert!(finish_caller(&expectation, NativeTerminal::Interrupted, true).is_err());
        assert!(tracker.is_idle("download-test-run"));
        INSTALLS.lock().remove(&instance);
    }

    #[test]
    fn late_install_reply_cannot_reopen_a_closed_instance() {
        let record = InstallRecord::pending();
        *record.state.lock() = InstallState::Closed;
        record.finish(true);
        assert!(matches!(*record.state.lock(), InstallState::Closed));
    }
}
