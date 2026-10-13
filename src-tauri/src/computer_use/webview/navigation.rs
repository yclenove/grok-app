//! Document-scoped App WebView navigation and successor publication.

use super::super::adapter::{
    NativeScopeIdentity, NavigationDisposition, NavigationKind, NavigationResult,
};
use super::super::BrokerError;
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(15);
const HISTORY_DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

struct WebViewNavigationTransition {
    bound: Arc<Mutex<BTreeMap<String, Arc<BoundTab>>>>,
    previous: Arc<BoundTab>,
    successor: Option<Arc<BoundTab>>,
    result: NavigationResult,
}

impl Drop for WebViewNavigationTransition {
    fn drop(&mut self) {
        if let Some(successor) = self.successor.take() {
            successor.binding_cancellation.cancel();
        }
    }
}

impl StagedNavigation for WebViewNavigationTransition {
    fn result(&self) -> &NavigationResult {
        &self.result
    }

    fn commit(
        mut self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        let successor = self
            .successor
            .as_ref()
            .cloned()
            .ok_or(BrokerError::DeadTarget)?;
        let previous_id = self.previous.target_id();
        let successor_id = successor.target_id();
        let mut current = self.bound.lock();
        if !current
            .get(&previous_id)
            .is_some_and(|bound| Arc::ptr_eq(bound, &self.previous))
            || !successor.is_current()
            || current.values().any(|bound| {
                !Arc::ptr_eq(bound, &self.previous)
                    && bound.native_key() == successor.native_key()
                    && bound.is_current()
            })
        {
            return Err(BrokerError::DeadTarget);
        }

        let previous = self.previous.clone();
        let published = successor.clone();
        let mut native_commit = || {
            commit()?;
            let removed = current
                .remove(&previous_id)
                .ok_or(BrokerError::DeadTarget)?;
            if !Arc::ptr_eq(&removed, &previous) {
                current.insert(previous_id.clone(), removed);
                return Err(BrokerError::DeadTarget);
            }
            previous.binding_cancellation.cancel();
            current.insert(successor_id.clone(), published.clone());
            Ok(())
        };
        successor
            .view
            .with_document_authority(successor.generation, &mut native_commit)?;
        self.successor.take();
        Ok(())
    }
}

impl WebViewAdapter {
    pub(super) fn stage_navigation_owned(
        &self,
        request: &NavigationRequest,
    ) -> Result<Box<dyn StagedNavigation>, NavigationError> {
        request
            .cancellation
            .check()
            .map_err(NavigationError::not_dispatched)?;
        let previous = self
            .bound_target(&request.target_id)
            .map_err(NavigationError::not_dispatched)?;
        if previous.run_id != request.run_id {
            return Err(NavigationError::not_dispatched(
                "WebView run identity changed",
            ));
        }
        if !self.scripts.native_is_idle(&previous.native_key()) {
            return Err(NavigationError::not_dispatched(
                "WebView native execution is still pending",
            ));
        }

        let navigated = match previous.view.navigate_for_run(request, previous.generation) {
            Ok(navigated) => navigated,
            Err(error) => {
                if error.disposition == NavigationDisposition::DispatchedUnknown {
                    previous.binding_cancellation.cancel();
                }
                return Err(error);
            }
        };
        previous.binding_cancellation.cancel();
        if navigated.generation <= previous.generation
            || previous.view.navigation_generation() != navigated.generation
            || previous.view.label() != previous.label
            || previous.view.tab_id() != previous.native_tab_id
        {
            return Err(NavigationError::dispatched_unknown(
                "WebView successor document changed before staging",
            ));
        }

        let successor = Arc::new(BoundTab {
            view: previous.view.clone(),
            label: previous.label.clone(),
            native_tab_id: previous.native_tab_id.clone(),
            tab_id: format!("{}@{}", previous.native_tab_id, uuid::Uuid::new_v4()),
            generation: navigated.generation,
            session: previous.session.clone(),
            run_id: previous.run_id.clone(),
            observation: Mutex::new(dom::ObservationState::default()),
            execution_epoch: AtomicU64::new(0),
            binding_cancellation: ActionCancellation::default(),
        });
        if successor.target_id().len() > 256 {
            return Err(NavigationError::dispatched_unknown(
                "WebView successor identity is too long",
            ));
        }
        let title = if navigated.title.trim().is_empty() {
            navigated.url.clone()
        } else {
            navigated.title
        };
        let target = target_info(
            &successor,
            title.chars().take(selection::MAX_METADATA_CHARS).collect(),
        );
        let result = NavigationResult {
            identity: successor.scope_identity(),
            target,
            kind: request.kind,
            url: navigated.url,
        };
        Ok(Box::new(WebViewNavigationTransition {
            bound: self.bound.clone(),
            previous,
            successor: Some(successor),
            result,
        }))
    }
}

pub(super) fn navigate_live(
    view: &SideBrowserLiveView,
    request: &NavigationRequest,
    expected_generation: u64,
) -> Result<LiveNavigationDocument, NavigationError> {
    let expected = view
        .document_identity(expected_generation)
        .map_err(NavigationError::not_dispatched)?;
    request
        .cancellation
        .check()
        .map_err(NavigationError::not_dispatched)?;

    let next_document = match request.kind {
        NavigationKind::Url => {
            let url = request
                .url
                .as_deref()
                .ok_or_else(|| NavigationError::not_dispatched("url navigation requires url"))?;
            super::super::browser::is_allowed_navigate_url(url)
                .map_err(|error| NavigationError::not_dispatched(error.to_string()))?;
            let parsed = tauri::Url::parse(url)
                .map_err(|_| NavigationError::not_dispatched("invalid WebView URL"))?;
            begin_and_dispatch(view, expected, request, move |webview| {
                webview.navigate(parsed).map_err(|error| error.to_string())
            })?
        }
        NavigationKind::Reload => {
            reject_unexpected_url(request)?;
            begin_and_dispatch(view, expected, request, |webview| {
                webview.reload().map_err(|error| error.to_string())
            })?
        }
        NavigationKind::Back | NavigationKind::Forward => {
            reject_unexpected_url(request)?;
            dispatch_history(view, expected, request)?
        }
    };

    let keep_waiting = || request.cancellation.check().is_ok();
    match view
        .native
        .wait_for_navigation_while(next_document, NAVIGATION_TIMEOUT, &keep_waiting)
    {
        Ok(lifecycle::NavigationCompletion::Finished) => {}
        Ok(lifecycle::NavigationCompletion::Cancelled) => {
            return Err(NavigationError::dispatched_unknown(
                "WebView navigation was cancelled after dispatch",
            ));
        }
        Err(error) => return Err(NavigationError::dispatched_unknown(error)),
    }
    request
        .cancellation
        .check()
        .map_err(NavigationError::dispatched_unknown)?;
    view.document_identity(next_document.generation)
        .map_err(NavigationError::dispatched_unknown)?;
    let url = view.url().map_err(NavigationError::dispatched_unknown)?;
    let title = view.title().unwrap_or_else(|_| url.clone());
    view.document_identity(next_document.generation)
        .map_err(NavigationError::dispatched_unknown)?;
    Ok(LiveNavigationDocument {
        generation: next_document.generation,
        url,
        title,
    })
}

fn reject_unexpected_url(request: &NavigationRequest) -> Result<(), NavigationError> {
    if request.url.is_some() {
        Err(NavigationError::not_dispatched(
            "url is only valid for kind=url",
        ))
    } else {
        Ok(())
    }
}

fn begin_and_dispatch(
    view: &SideBrowserLiveView,
    expected: lifecycle::DocumentIdentity,
    request: &NavigationRequest,
    dispatch: impl FnOnce(&tauri::Webview) -> Result<(), String>,
) -> Result<lifecycle::DocumentIdentity, NavigationError> {
    if !view.native.matches(expected) {
        return Err(NavigationError::not_dispatched(
            "WebView document changed before navigation",
        ));
    }
    request
        .cancellation
        .check()
        .map_err(NavigationError::not_dispatched)?;
    let started = view
        .native
        .begin_navigation()
        .map_err(NavigationError::not_dispatched)?;
    if !started.newly_started {
        return Err(NavigationError::not_dispatched(
            "WebView navigation is already pending",
        ));
    }
    if let Err(error) = dispatch(&view.webview) {
        view.native.navigation_cancelled();
        return Err(NavigationError::dispatched_unknown(format!(
            "WebView navigation dispatch failed: {error}"
        )));
    }
    Ok(started.document)
}

fn dispatch_history(
    view: &SideBrowserLiveView,
    expected: lifecycle::DocumentIdentity,
    request: &NavigationRequest,
) -> Result<lifecycle::DocumentIdentity, NavigationError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = Arc::new(AtomicBool::new(true));
    let callback_gate = gate.clone();
    let native = view.native.clone();
    let cancellation = request.cancellation.clone();
    let kind = request.kind;
    let scheduled = view.webview.with_webview(move |platform| {
        if callback_gate
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let _ = tx.send(platform_history_dispatch(
            platform,
            &native,
            expected,
            &cancellation,
            kind,
        ));
    });
    if scheduled.is_err() {
        gate.store(false, Ordering::SeqCst);
        return Err(NavigationError::not_dispatched(
            "WebView history dispatcher unavailable",
        ));
    }
    match rx.recv_timeout(HISTORY_DISPATCH_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            let callback_not_started = gate.swap(false, Ordering::SeqCst);
            if callback_not_started {
                Err(NavigationError::not_dispatched(
                    "WebView history dispatch timed out before execution",
                ))
            } else {
                Err(NavigationError::dispatched_unknown(
                    "WebView history dispatch outcome unknown",
                ))
            }
        }
    }
}

fn begin_history(
    native: &lifecycle::NativeView,
    expected: lifecycle::DocumentIdentity,
    cancellation: &ActionCancellation,
) -> Result<lifecycle::NavigationStart, NavigationError> {
    cancellation
        .check()
        .map_err(NavigationError::not_dispatched)?;
    if !native.matches(expected) {
        return Err(NavigationError::not_dispatched(
            "WebView document changed before history navigation",
        ));
    }
    let started = native
        .begin_navigation()
        .map_err(NavigationError::not_dispatched)?;
    if !started.newly_started {
        return Err(NavigationError::not_dispatched(
            "WebView navigation is already pending",
        ));
    }
    Ok(started)
}

#[cfg(windows)]
fn platform_history_dispatch(
    platform: tauri::webview::PlatformWebview,
    native: &lifecycle::NativeView,
    expected: lifecycle::DocumentIdentity,
    cancellation: &ActionCancellation,
    kind: NavigationKind,
) -> Result<lifecycle::DocumentIdentity, NavigationError> {
    use windows::core::BOOL;

    let core = unsafe { platform.controller().CoreWebView2() }
        .map_err(|_| NavigationError::not_dispatched("WebView native controller unavailable"))?;
    let mut available = BOOL(0);
    let checked = unsafe {
        match kind {
            NavigationKind::Back => core.CanGoBack(&mut available),
            NavigationKind::Forward => core.CanGoForward(&mut available),
            _ => unreachable!("history dispatcher only accepts back/forward"),
        }
    };
    checked.map_err(|_| NavigationError::not_dispatched("WebView history unavailable"))?;
    if !available.as_bool() {
        return Err(NavigationError::not_dispatched(
            "WebView has no history entry",
        ));
    }
    let started = begin_history(native, expected, cancellation)?;
    let dispatched = unsafe {
        match kind {
            NavigationKind::Back => core.GoBack(),
            NavigationKind::Forward => core.GoForward(),
            _ => unreachable!("history dispatcher only accepts back/forward"),
        }
    };
    if dispatched.is_err() {
        native.navigation_cancelled();
        return Err(NavigationError::dispatched_unknown(
            "WebView history navigation dispatch failed",
        ));
    }
    Ok(started.document)
}

#[cfg(target_os = "linux")]
fn platform_history_dispatch(
    platform: tauri::webview::PlatformWebview,
    native: &lifecycle::NativeView,
    expected: lifecycle::DocumentIdentity,
    cancellation: &ActionCancellation,
    kind: NavigationKind,
) -> Result<lifecycle::DocumentIdentity, NavigationError> {
    use webkit2gtk::WebViewExt;

    let webview = platform.inner();
    let available = match kind {
        NavigationKind::Back => webview.can_go_back(),
        NavigationKind::Forward => webview.can_go_forward(),
        _ => unreachable!("history dispatcher only accepts back/forward"),
    };
    if !available {
        return Err(NavigationError::not_dispatched(
            "WebView has no history entry",
        ));
    }
    let started = begin_history(native, expected, cancellation)?;
    match kind {
        NavigationKind::Back => webview.go_back(),
        NavigationKind::Forward => webview.go_forward(),
        _ => unreachable!("history dispatcher only accepts back/forward"),
    }
    Ok(started.document)
}

#[cfg(target_os = "macos")]
fn platform_history_dispatch(
    platform: tauri::webview::PlatformWebview,
    native: &lifecycle::NativeView,
    expected: lifecycle::DocumentIdentity,
    cancellation: &ActionCancellation,
    kind: NavigationKind,
) -> Result<lifecycle::DocumentIdentity, NavigationError> {
    use objc2::{msg_send, runtime::AnyObject};

    let webview = platform.inner().cast::<AnyObject>();
    if webview.is_null() {
        return Err(NavigationError::not_dispatched(
            "WebView native controller unavailable",
        ));
    }
    let available: bool = unsafe {
        match kind {
            NavigationKind::Back => msg_send![webview, canGoBack],
            NavigationKind::Forward => msg_send![webview, canGoForward],
            _ => unreachable!("history dispatcher only accepts back/forward"),
        }
    };
    if !available {
        return Err(NavigationError::not_dispatched(
            "WebView has no history entry",
        ));
    }
    let started = begin_history(native, expected, cancellation)?;
    unsafe {
        match kind {
            NavigationKind::Back => {
                let _: () = msg_send![webview, goBack];
            }
            NavigationKind::Forward => {
                let _: () = msg_send![webview, goForward];
            }
            _ => unreachable!("history dispatcher only accepts back/forward"),
        }
    }
    Ok(started.document)
}
