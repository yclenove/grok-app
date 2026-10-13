//! Original App main-thread discovery, metadata only, no global candidate cache.
use super::*;
use grok_computer_use_core::adapter::{
    NativeScopeIdentity, ScopeCandidate, ScopeCandidatePage, WebViewScopeReceipt,
    WebViewSelectedIdentity,
};

struct Receipt {
    adapter: Weak<WebViewAdapter>,
    selection: WebViewSelection,
}
impl WebViewScopeReceipt for Receipt {
    fn require_current(&self) -> Result<(), String> {
        self.selection.title().map(|_| ())
    }
    fn bind(
        &self,
        executor: &dyn ComputerUseAdapter,
        session: &str,
        run: &str,
    ) -> Result<ScopeCandidate, String> {
        let adapter = self
            .adapter
            .upgrade()
            .ok_or("WebView original producer retired")?;
        if !std::ptr::eq(
            executor as *const dyn ComputerUseAdapter as *const (),
            Arc::as_ptr(&adapter).cast(),
        ) {
            return Err("WebView original producer changed".into());
        }
        let info = adapter.bind_selected(&self.selection, session, run)?;
        let identity = match adapter.scope_identity(&info.target_id) {
            Ok(identity) => identity,
            Err(error) => {
                adapter.release_target_for_run(run, &info.target_id);
                return Err(error);
            }
        };
        Ok(ScopeCandidate { info, identity })
    }
}

impl WebViewAdapter {
    #[cfg(any(test, feature = "computer-use-probe"))]
    pub(crate) fn install_scope_fixture(self: &Arc<Self>, pages: Vec<Arc<dyn LiveWebView>>) {
        *self.scope_fixture.lock() = Some((Arc::downgrade(self), pages));
    }

    /// Installation retains the App only. It does not discover a page, prompt,
    /// create a binding, copy authentication or start a native worker.
    pub(crate) fn attach_scope_host(self: &Arc<Self>, app: tauri::AppHandle) {
        let _ = self.scope_host.set((app, Arc::downgrade(self)));
    }

    pub(super) fn selected_candidate(
        self: &Arc<Self>,
        selection: WebViewSelection,
    ) -> Result<ScopeCandidate, String> {
        let title = selection.title()?.to_string();
        let native_tab_id = selection.native_tab_id().to_string();
        let document_generation = selection.document_generation();
        let target_id = format!("wv:selection:{}", uuid::Uuid::new_v4().simple());
        let info = TargetInfo {
            target_id: target_id.clone(),
            title,
            app_name: "side-browser".into(),
            kind: "webview".into(),
            pid: None,
            backend: "webview".into(),
            execution_mode: "host-selection".into(),
            replay_policy: "never".into(),
            lifecycle_stamp: document_generation,
            display_id: selection.label().into(),
            coordinate_space: "image-pixels".into(),
            scope_label: "pending-selection".into(),
        };
        let identity = NativeScopeIdentity::WebViewSelected {
            identity: WebViewSelectedIdentity {
                target_id,
                native_tab_id,
                document_generation,
                receipt: Arc::new(Receipt {
                    adapter: Arc::downgrade(self),
                    selection,
                }),
            },
        };
        Ok(ScopeCandidate { info, identity })
    }

    pub(super) fn discover_scope_candidates(
        &self,
        limit: usize,
        is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ScopeCandidatePage, String> {
        if limit == 0
            || limit > grok_computer_use_core::broker::MAX_SCOPE_CANDIDATES
            || !is_current()
        {
            return Err("WebView picker retired or invalid".into());
        }
        #[cfg(any(test, feature = "computer-use-probe"))]
        if let Some((adapter, pages)) = self.scope_fixture.lock().clone() {
            let original = adapter
                .upgrade()
                .ok_or("WebView original producer retired")?;
            let truncated = pages.len() > limit;
            let mut candidates = Vec::with_capacity(pages.len().min(limit));
            for page in pages.into_iter().take(limit) {
                if !is_current() {
                    return Err("WebView picker retired".into());
                }
                candidates.push(original.selected_candidate(WebViewSelection::capture(page)?)?);
            }
            if !is_current() {
                return Err("WebView picker retired".into());
            }
            return Ok(ScopeCandidatePage {
                candidates,
                truncated,
            });
        }
        let (app, adapter) = self
            .scope_host
            .get()
            .ok_or("WebView original App producer unavailable")?;
        let app = app.clone();
        let original = adapter
            .upgrade()
            .ok_or("WebView original producer retired")?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handle = app.clone();
        app.run_on_main_thread(move || {
            use tauri::Manager;
            let result = (|| {
                if !is_current() {
                    return Err("WebView picker retired".into());
                }
                let mut labels: Vec<_> = handle
                    .webviews()
                    .into_keys()
                    .filter(|label| crate::side_browser_host::is_app_owned_side_browser(label))
                    .collect();
                labels.sort();
                let truncated = labels.len() > limit;
                let mut candidates = Vec::with_capacity(labels.len().min(limit));
                for label in labels.into_iter().take(limit) {
                    if !is_current() {
                        return Err("WebView picker retired".into());
                    }
                    let selection = Self::select_side_browser(handle.clone(), &label)?;
                    candidates.push(original.selected_candidate(selection)?);
                }
                if !is_current() {
                    return Err("WebView picker retired".into());
                }
                Ok(ScopeCandidatePage {
                    candidates,
                    truncated,
                })
            })();
            let _ = tx.send(result);
        })
        .map_err(|_| "WebView main-thread producer unavailable")?;
        // Never return a timeout while the original queued/native callback is
        // still alive. Its retained fence suppresses late metadata publication.
        rx.recv()
            .map_err(|_| "WebView original discovery callback retired")?
    }
}
