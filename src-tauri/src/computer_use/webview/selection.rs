//! Host-retained page selection precedes execution binding.
//! Metadata cannot authorize a run, and a label cannot reacquire a changed page.

use super::*;

pub(super) const MAX_METADATA_CHARS: usize = 512;
const MAX_BINDINGS: usize = 128;

/// Neither serializable nor constructible from renderer/model JSON. Retains the
/// actual native object and the document selected by the original Host caller.
pub struct WebViewSelection {
    view: Arc<dyn LiveWebView>,
    label: String,
    native_tab_id: String,
    generation: u64,
    title: String,
}

impl WebViewSelection {
    pub fn capture(view: Arc<dyn LiveWebView>) -> Result<Self, String> {
        if !view.is_alive() {
            return Err("bind requires a live App-owned webview".into());
        }
        let label = view.label();
        let native_tab_id = view.tab_id();
        let generation = view.navigation_generation();
        if label.is_empty()
            || label.contains('|')
            || label.len() > 96
            || native_tab_id.is_empty()
            || native_tab_id.contains('|')
            || native_tab_id.len() > 128
            || generation == 0
        {
            return Err("bind requires a valid native WebView identity".into());
        }
        let url = view.url()?;
        if url.trim().is_empty() {
            return Err("live webview has no url".into());
        }
        let title = view.title().unwrap_or_default();
        let title = if title.trim().is_empty() { url } else { title };
        let selection = Self {
            view,
            label,
            native_tab_id,
            generation,
            title: title.chars().take(MAX_METADATA_CHARS).collect(),
        };
        selection.require_current()?;
        Ok(selection)
    }

    /// A metadata read still checks the original retained page. Never look up a
    /// replacement by label or infer identity from a matching title/URL.
    pub fn title(&self) -> Result<&str, String> {
        self.require_current()?;
        Ok(&self.title)
    }
    pub(super) fn native_tab_id(&self) -> &str {
        &self.native_tab_id
    }
    pub(super) fn document_generation(&self) -> u64 {
        self.generation
    }
    pub(super) fn label(&self) -> &str {
        &self.label
    }

    fn require_current(&self) -> Result<(), String> {
        if !self.view.is_alive()
            || self.view.label() != self.label
            || self.view.tab_id() != self.native_tab_id
            || self.view.navigation_generation() != self.generation
        {
            return Err("WebView selected document changed or retired".into());
        }
        Ok(())
    }
}

impl WebViewAdapter {
    /// The actual AppHandle and retained native view establish App ownership.
    /// This does not bind a run, read DOM/AX, execute a script or copy login data.
    pub fn select_side_browser(
        app: tauri::AppHandle,
        label: &str,
    ) -> Result<WebViewSelection, String> {
        if !crate::side_browser_host::is_app_owned_side_browser(label) {
            return Err("side browser label must be App-owned".into());
        }
        let view = Arc::new(SideBrowserLiveView::open(app, label.to_string())?);
        WebViewSelection::capture(view)
    }

    /// Only the Host can supply this receipt. Binding creates a fresh execution
    /// ID; selection metadata never becomes an execution grant.
    pub fn bind_selected(
        &self,
        selection: &WebViewSelection,
        session: &str,
        run_id: &str,
    ) -> Result<TargetInfo, String> {
        let session = session.trim();
        let run_id = run_id.trim();
        if session.is_empty() || run_id.is_empty() || session.contains('|') || run_id.contains('|')
        {
            return Err("bind requires session and run".into());
        }
        selection.require_current()?;
        let bound = Arc::new(BoundTab {
            view: selection.view.clone(),
            label: selection.label.clone(),
            native_tab_id: selection.native_tab_id.clone(),
            tab_id: format!("{}@{}", selection.native_tab_id, uuid::Uuid::new_v4()),
            generation: selection.generation,
            session: session.into(),
            run_id: run_id.into(),
            observation: Mutex::new(dom::ObservationState::default()),
            execution_epoch: AtomicU64::new(0),
            binding_cancellation: ActionCancellation::default(),
        });
        if bound.target_id().len() > 256 {
            return Err("WebView document changed during bind".into());
        }
        let target = target_info(&bound, selection.title()?.into());
        let mut current = self.bound.lock();
        selection.require_current()?;
        // An explicit selection cannot steal an active page from another task.
        // Navigation retires the original document; script occupancy remains
        // in the shared tracker until the original native operation finishes.
        if current.values().any(|old| {
            old.native_key() == bound.native_key()
                && old.is_current()
                && (old.session != session || old.run_id != run_id)
        }) {
            return Err("WebView page is already bound to another task".into());
        }
        current.retain(|_, old| {
            let keep = old.is_current() && old.native_key() != bound.native_key();
            if !keep {
                old.binding_cancellation.cancel();
            }
            keep
        });
        if current.len() >= MAX_BINDINGS {
            return Err("WebView binding capacity exhausted".into());
        }
        current.insert(target.target_id.clone(), bound);
        Ok(target)
    }
}

#[cfg(test)]
mod tests;
