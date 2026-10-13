//! Retained App-owned native view and document operations.

use super::*;

pub(super) struct SideBrowserLiveView {
    pub(super) app: tauri::AppHandle,
    pub(super) webview: tauri::Webview,
    pub(super) label: String,
    pub(super) tab_id: String,
    pub(super) native: lifecycle::NativeView,
    pub(super) instance: u64,
}

impl SideBrowserLiveView {
    pub(super) fn open(app: tauri::AppHandle, label: String) -> Result<Self, String> {
        use tauri::Manager;
        let native = lifecycle::current(&label).ok_or("WebView native identity unavailable")?;
        let document = native.document().ok_or("WebView native identity retired")?;
        let webview = app
            .get_webview(&label)
            .ok_or("WebView native instance unavailable")?;
        let url = webview
            .url()
            .map_err(|_| "WebView URL unavailable")?
            .to_string();
        if url.trim().is_empty() {
            return Err("selected tab has no url".into());
        }
        if !native.matches(document) {
            return Err("WebView document changed during bind".into());
        }
        Ok(Self {
            app,
            webview,
            label,
            tab_id: format!("native-{}", document.instance),
            native,
            instance: document.instance,
        })
    }

    pub(super) fn document_identity(
        &self,
        generation: u64,
    ) -> Result<lifecycle::DocumentIdentity, String> {
        let expected = lifecycle::DocumentIdentity {
            instance: self.instance,
            generation,
        };
        if self.native.matches(expected) && crate::side_browser_host::exists(&self.app, &self.label)
        {
            Ok(expected)
        } else {
            Err("WebView native document retired".into())
        }
    }
}

impl LiveWebView for SideBrowserLiveView {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        self.tab_id.clone()
    }
    fn url(&self) -> Result<String, String> {
        let generation = self.navigation_generation();
        self.document_identity(generation)?;
        let url = self
            .webview
            .url()
            .map_err(|_| "WebView URL unavailable")?
            .to_string();
        self.document_identity(generation)?;
        Ok(url)
    }
    fn title(&self) -> Result<String, String> {
        self.url()
    }
    fn is_alive(&self) -> bool {
        self.native
            .document()
            .is_some_and(|document| document.instance == self.instance)
            && crate::side_browser_host::exists(&self.app, &self.label)
    }
    fn navigation_generation(&self) -> u64 {
        self.native
            .document()
            .filter(|document| {
                document.instance == self.instance
                    && crate::side_browser_host::exists(&self.app, &self.label)
            })
            .map_or(0, |document| document.generation)
    }
    fn with_document_authority(
        &self,
        generation: u64,
        commit: &mut dyn FnMut() -> Result<(), crate::computer_use::BrokerError>,
    ) -> Result<(), crate::computer_use::BrokerError> {
        let expected = self
            .document_identity(generation)
            .map_err(|_| crate::computer_use::BrokerError::DeadTarget)?;
        self.native.with_document_authority(expected, commit)
    }
    #[cfg(any(test, feature = "computer-use-probe"))]
    fn navigate(&self, url: &str) -> Result<u64, String> {
        if !self.is_alive() {
            return Err("WebView native document retired".into());
        }
        let url = tauri::Url::parse(url).map_err(|_| "invalid WebView URL")?;
        if !matches!(url.scheme(), "http" | "https" | "file") {
            return Err("typed navigate scheme not allowed".into());
        }
        let started = self.native.begin_navigation()?;
        if let Err(error) = self.webview.navigate(url) {
            self.native.navigation_cancelled();
            return Err(format!("WebView navigation failed: {error}"));
        }
        Ok(started.document.generation)
    }
    fn host_script_owned(
        &self,
        js: &str,
        generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        if self.document_identity(generation).is_err() {
            operation.finished();
            return Err("WebView native document retired".into());
        }
        side_script::run(self, js, generation, operation)
    }
    fn navigate_for_run(
        &self,
        request: &NavigationRequest,
        expected_generation: u64,
    ) -> Result<LiveNavigationDocument, NavigationError> {
        navigation::navigate_live(self, request, expected_generation)
    }
    #[cfg(windows)]
    fn download_for_run(
        &self,
        request: &DownloadRequest,
        expected_generation: u64,
        prepared_url: &str,
        dispatch_script: &str,
        operation: ScriptOperation,
    ) -> Result<DownloadResult, DownloadError> {
        download::download_live(
            self,
            request,
            expected_generation,
            prepared_url,
            dispatch_script,
            operation,
        )
    }
}
