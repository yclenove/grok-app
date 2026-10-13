//! Scoped debug witnesses and delayed delivery of an original Wry completion.
//! No synthetic download, terminal, timeout override or production registry.

use super::*;
use std::sync::Weak;

static WATCHERS: LazyLock<Mutex<HashMap<u64, Weak<Probe>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Counts {
    pub(crate) dispatch_acknowledged: u64,
    pub(crate) native_started: u64,
    pub(crate) wry_requested: u64,
    pub(crate) native_terminal: u64,
    pub(crate) wry_finished: u64,
    pub(crate) wry_processed: u64,
    pub(crate) browser_exited: u64,
}

pub(super) enum Event {
    DispatchAcknowledged,
    NativeStarted,
    WryRequested,
    NativeTerminal,
    WryFinished,
    WryProcessed,
    BrowserExited,
}

struct HeldFinish {
    expectation: Arc<Expectation>,
    path: Option<PathBuf>,
    success: bool,
}

pub(crate) struct Probe {
    instance: u64,
    started: Instant,
    stages: Mutex<std::collections::BTreeMap<&'static str, u128>>,
    destination: Mutex<Option<DestinationReceipt>>,
    cancel_expired_elapsed: Mutex<Option<Duration>>,
    cancel_expired: Condvar,
    counts: Mutex<Counts>,
    hold: AtomicBool,
    held: Mutex<Option<HeldFinish>>,
}

impl Probe {
    pub(crate) fn watch(instance: u64, hold: bool) -> Result<Arc<Self>, String> {
        let mut watchers = WATCHERS.lock();
        if watchers.get(&instance).and_then(Weak::upgrade).is_some() {
            return Err("native download witness already owned".into());
        }
        let probe = Arc::new(Self {
            instance,
            started: Instant::now(),
            stages: Mutex::new(Default::default()),
            destination: Mutex::new(None),
            cancel_expired_elapsed: Mutex::new(None),
            cancel_expired: Condvar::new(),
            counts: Mutex::new(Counts::default()),
            hold: AtomicBool::new(hold),
            held: Mutex::new(None),
        });
        watchers.insert(instance, Arc::downgrade(&probe));
        Ok(probe)
    }

    pub(crate) fn counts(&self) -> Counts {
        self.counts.lock().clone()
    }

    pub(crate) fn stages(&self) -> std::collections::BTreeMap<&'static str, u128> {
        self.stages.lock().clone()
    }

    pub(crate) fn destination(&self) -> Option<DestinationReceipt> {
        self.destination.lock().clone()
    }

    pub(crate) fn wait_cancel_expiry(&self) -> Result<Duration, String> {
        // The native grace is a cutoff, not a channel-reply SLA. Witness that
        // authentic cutoff first, then use the existing caller join deadline.
        let deadline =
            Instant::now() + crate::computer_use::webview::side_script::SCRIPT_REPLY_TIMEOUT;
        let mut elapsed = self.cancel_expired_elapsed.lock();
        while elapsed.is_none() {
            let now = Instant::now();
            if now >= deadline {
                return Err("original native cancellation grace did not expire".into());
            }
            self.cancel_expired.wait_for(&mut elapsed, deadline - now);
        }
        Ok(elapsed.expect("observed original cancellation expiry"))
    }

    /// Invoke on the original WebView UI thread. Deliver the captured real
    /// callback once; never manufacture a Finished or StateChanged event.
    pub(crate) fn release_on_native_thread(&self) -> Result<(), String> {
        self.hold.store(false, Ordering::SeqCst);
        let held = self.held.lock().take();
        if let Some(held) = held {
            if !finish_wry_expectation(&held.expectation, held.path.as_deref(), held.success) {
                return Err("original held Wry completion was rejected".into());
            }
        }
        Ok(())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        if self.held.lock().is_some() {
            eprintln!("native download witness dropped with an undelivered Wry callback");
        }
        let mut watchers = WATCHERS.lock();
        if watchers
            .get(&self.instance)
            .is_some_and(|watcher| std::ptr::eq(watcher.as_ptr(), self))
        {
            watchers.remove(&self.instance);
        }
    }
}

pub(super) fn record(instance: u64, event: Event) {
    let probe = WATCHERS.lock().get(&instance).and_then(Weak::upgrade);
    let Some(probe) = probe else { return };
    let mut counts = probe.counts.lock();
    match event {
        Event::DispatchAcknowledged => counts.dispatch_acknowledged += 1,
        Event::NativeStarted => counts.native_started += 1,
        Event::WryRequested => counts.wry_requested += 1,
        Event::NativeTerminal => counts.native_terminal += 1,
        Event::WryFinished => counts.wry_finished += 1,
        Event::WryProcessed => counts.wry_processed += 1,
        Event::BrowserExited => counts.browser_exited += 1,
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DestinationReceipt {
    broker_utf16_length: usize,
    native_utf16_length: usize,
    buffered: bool,
}

pub(super) fn record_destination(
    instance: u64,
    broker: &Path,
    native: &destination::NativeDestination,
) {
    if let Some(probe) = WATCHERS.lock().get(&instance).and_then(Weak::upgrade) {
        *probe.destination.lock() = Some(DestinationReceipt {
            broker_utf16_length: broker.to_string_lossy().encode_utf16().count(),
            native_utf16_length: native.path().to_string_lossy().encode_utf16().count(),
            buffered: native.buffered(),
        });
    }
}

pub(crate) fn native_staging(instance: u64) -> Option<(PathBuf, bool)> {
    EXPECTATIONS
        .lock()
        .values()
        .find(|expectation| expectation.instance == instance)
        .map(|expectation| {
            (
                expectation.native_destination.path().to_path_buf(),
                expectation.native_destination.buffered(),
            )
        })
}

pub(super) fn native_state(operation: &ICoreWebView2DownloadOperation) {
    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
    let mut reason = Default::default();
    let mut received = 0;
    let mut total = 0;
    unsafe {
        let state_known = operation.State(&mut state).is_ok();
        let reason_known = operation.InterruptReason(&mut reason).is_ok();
        let received_known = operation.BytesReceived(&mut received).is_ok();
        let total_known = operation.TotalBytesToReceive(&mut total).is_ok();
        eprintln!(
            "gate: webview_download_native_state state_known={state_known} state={} reason_known={reason_known} reason={} received_known={received_known} received={received} total_known={total_known} total={total}",
            state.0, reason.0,
        );
    }
    if let Ok(path) = read_result_path(operation) {
        eprintln!(
            "gate: webview_download_result_path utf16_length={} extended={} parent_exists={}",
            path.as_os_str().to_string_lossy().encode_utf16().count(),
            path.to_string_lossy().starts_with(r"\\?\"),
            path.parent().is_some_and(Path::is_dir)
        );
    }
}

pub(crate) fn record_stage(instance: u64, stage: &'static str) {
    if let Some(probe) = WATCHERS.lock().get(&instance).and_then(Weak::upgrade) {
        probe
            .stages
            .lock()
            .insert(stage, probe.started.elapsed().as_millis());
    }
}

pub(crate) fn record_cancel_expiry(instance: u64, elapsed: Duration) {
    if let Some(probe) = WATCHERS.lock().get(&instance).and_then(Weak::upgrade) {
        *probe.cancel_expired_elapsed.lock() = Some(elapsed);
        probe.cancel_expired.notify_all();
    }
}

pub(super) fn hold_finish(
    expectation: &Arc<Expectation>,
    path: Option<&Path>,
    success: bool,
) -> bool {
    let probe = WATCHERS
        .lock()
        .get(&expectation.instance)
        .and_then(Weak::upgrade);
    let Some(probe) = probe else { return false };
    let mut held = probe.held.lock();
    if !probe.hold.load(Ordering::SeqCst) {
        return false;
    }
    if held.is_some() {
        eprintln!("duplicate original Wry completion while held");
        return true;
    }
    *held = Some(HeldFinish {
        expectation: expectation.clone(),
        path: path.map(Path::to_path_buf),
        success,
    });
    true
}

/// Witness the real cancellation fence before WebView2 creates its COM download.
pub(crate) fn cancelled_before_start(instance: u64) -> bool {
    let expectations = EXPECTATIONS
        .lock()
        .values()
        .filter(|expectation| expectation.instance == instance)
        .cloned()
        .collect::<Vec<_>>();
    expectations.len() == 1
        && expectations.iter().all(|expectation| {
            let state = expectation.state.lock();
            state.cancel_requested
                && !state.native_started
                && state.terminal.is_none()
                && !state.wry_requested
                && !state.wry_finished
                && !state.caller_done
        })
}

pub(crate) fn cancellation_grace() -> Duration {
    CANCEL_GRACE
}

/// Read COM only on the original native thread, without holding its registry
/// borrow across COM calls or releases (which can synchronously reenter).
pub(crate) fn received_bytes_on_native_thread(instance: u64) -> Result<u64, String> {
    let operations = NATIVE_OPERATIONS.with(|operations| {
        operations
            .borrow()
            .values()
            .filter(|operation| operation.instance == instance)
            .map(|operation| operation.operation.clone())
            .collect::<Vec<_>>()
    });
    let mut total = 0;
    for operation in operations {
        let mut bytes = 0;
        unsafe { operation.BytesReceived(&mut bytes) }
            .map_err(|_| "native download byte witness unavailable")?;
        total += u64::try_from(bytes).map_err(|_| "negative native download byte witness")?;
    }
    Ok(total)
}
