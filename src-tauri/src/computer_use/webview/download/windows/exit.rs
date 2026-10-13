//! Only an exact BrowserProcessExited witness can replace lost download callbacks.

use super::*;
use webview2_com::BrowserProcessExitedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Environment5, ICoreWebView2_2};

thread_local! {
    static OBSERVERS: RefCell<HashMap<u64, Observer>> = RefCell::new(HashMap::new());
}

struct Observer {
    environment: ICoreWebView2Environment5,
    token: i64,
}

impl Drop for Observer {
    fn drop(&mut self) {
        let _ = unsafe { self.environment.remove_BrowserProcessExited(self.token) };
    }
}

pub(super) fn detach(instance: u64) {
    // Drop COM outside the registry borrow: callbacks can reenter this STA.
    let observer = OBSERVERS.with(|observers| observers.borrow_mut().remove(&instance));
    drop(observer);
}

#[cfg(feature = "computer-use-probe")]
pub(super) fn observer_present(instance: u64) -> bool {
    OBSERVERS.with(|observers| observers.borrow().contains_key(&instance))
}

pub(super) fn install(core: &ICoreWebView2, instance: u64) -> Result<journal::Owners, String> {
    let core2 = core
        .cast::<ICoreWebView2_2>()
        .map_err(|_| "WebView download environment unavailable")?;
    let environment = unsafe { core2.Environment() }
        .and_then(|environment| environment.cast::<ICoreWebView2Environment5>())
        .map_err(|_| "WebView download exit observer unavailable")?;
    let mut expected = 0;
    unsafe { core.BrowserProcessId(&mut expected) }
        .map_err(|_| "WebView download browser identity unavailable")?;
    if expected == 0 {
        return Err("WebView download browser identity unavailable".into());
    }
    let owners = journal::Owners::capture(expected, crate::paths::app_data_root())?;
    let captured = owners.clone();
    // Environment owns the observer. The closure contains no COM reference,
    // reusable label or URL; another browser process cannot retire this instance.
    let handler = BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut exited = 0;
            unsafe { args.BrowserProcessId(&mut exited)? };
            if exited == expected && captured.browser.terminated() {
                #[cfg(feature = "computer-use-probe")]
                probe::record(instance, probe::Event::BrowserExited);
                retire_instance(instance);
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { environment.add_BrowserProcessExited(&handler, &mut token) }
        .map_err(|_| "WebView download exit observer registration failed".to_string())?;
    let mut current = 0;
    if unsafe { core.BrowserProcessId(&mut current) }.is_err()
        || current != expected
        || !owners.live()
    {
        let _ = unsafe { environment.remove_BrowserProcessExited(token) };
        return Err("WebView download original browser changed during installation".into());
    }
    let previous = OBSERVERS.with(|observers| {
        observers
            .borrow_mut()
            .insert(instance, Observer { environment, token })
    });
    drop(previous);
    Ok(owners)
}

pub(super) fn retire_instance(instance: u64) {
    let expectations = EXPECTATIONS
        .lock()
        .values()
        .filter(|expectation| expectation.instance == instance)
        .cloned()
        .collect::<Vec<_>>();
    for expectation in expectations {
        {
            let mut state = expectation.state.lock();
            state.native_exited = true;
            // A lost native/Wry completion remains unknown, never success. A
            // previously joined real completion keeps its original terminal.
            if !state.wry_finished || state.terminal.is_none() {
                state.terminal = Some(NativeTerminal::OwnerExited);
            }
        }
        // Exact exit retires lost native callbacks, not the original caller.
        // Joint retirement owns deletion; do not fake Wry completion or clean
        // files while the response/copy caller still belongs to this download.
        expectation.changed.notify_all();
        remove_if_finished(&expectation);
    }
    INSTALLS.lock().remove(&instance);
    close::clear_native_operations(instance);
    detach(instance);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_exit_retires_lost_callbacks_with_unknown_outcome_and_no_replay() {
        let tracker = ScriptTracker::default();
        let instance = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 100_000;
        super::super::tests::install_record(instance);
        let operation = tracker
            .begin(
                "exit-old",
                "exit-native",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        let expectation = register(
            instance,
            "https://fixture.invalid/exit.bin",
            std::env::temp_dir()
                .join(format!("cu-dl-exit-{instance}"))
                .join("exit.bin"),
            operation,
        )
        .unwrap();
        expectation.state.lock().native_started = true;
        retire_instance(u64::MAX);
        assert!(!tracker.is_idle("exit-old"));
        assert!(expectation.state.lock().terminal.is_none());
        retire_instance(instance);
        assert!(!tracker.is_idle("exit-old"));
        assert!(!expectation.state.lock().wry_finished);
        let error = finish_caller(&expectation, NativeTerminal::OwnerExited, true).unwrap_err();
        assert!(error.detail.contains("outcome unknown"));
        assert!(tracker.is_idle("exit-old"));
        assert!(!EXPECTATIONS.lock().contains_key(&expectation.id));
        let next = tracker
            .begin(
                "exit-new",
                "exit-native",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        retire_instance(instance);
        assert!(next.check().is_ok());
        assert!(!tracker.is_idle("exit-new"));
        next.finished();
    }

    #[test]
    fn physical_exit_settles_an_already_abandoned_caller_without_fake_wry_completion() {
        let tracker = ScriptTracker::default();
        let instance = NEXT_EXPECTATION.fetch_add(1, Ordering::SeqCst) + 100_000;
        super::super::tests::install_record(instance);
        let operation = tracker
            .begin(
                "exit-abandoned",
                "exit-native",
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        let expectation = register(
            instance,
            "https://fixture.invalid/abandoned.bin",
            std::env::temp_dir()
                .join(format!("cu-dl-exit-{instance}"))
                .join("exit.bin"),
            operation,
        )
        .unwrap();
        abandon(&expectation);
        assert!(!tracker.is_idle("exit-abandoned"));
        retire_instance(instance);
        assert!(!expectation.state.lock().wry_finished);
        assert!(tracker.is_idle("exit-abandoned"));
        assert!(!EXPECTATIONS.lock().contains_key(&expectation.id));
    }
}
