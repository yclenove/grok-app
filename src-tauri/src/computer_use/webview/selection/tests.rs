use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::Barrier;

struct View {
    live: AtomicBool,
    generation: AtomicU64,
    label: Mutex<String>,
    native: Mutex<String>,
    title: String,
    title_gate: Option<(Arc<Barrier>, Arc<Barrier>)>,
    scripts: AtomicU64,
}

impl View {
    fn new() -> Arc<Self> {
        Self::with_title("Selected page".into(), None)
    }

    fn with_title(title: String, title_gate: Option<(Arc<Barrier>, Arc<Barrier>)>) -> Arc<Self> {
        Arc::new(Self {
            live: AtomicBool::new(true),
            generation: AtomicU64::new(1),
            label: Mutex::new("resource-browser-selected".into()),
            native: Mutex::new("native-selected".into()),
            title,
            title_gate,
            scripts: AtomicU64::new(0),
        })
    }
}

impl LiveWebView for View {
    fn label(&self) -> String {
        self.label.lock().clone()
    }
    fn tab_id(&self) -> String {
        self.native.lock().clone()
    }
    fn url(&self) -> Result<String, String> {
        Ok("https://fixture.invalid/selected".into())
    }
    fn title(&self) -> Result<String, String> {
        if let Some((entered, release)) = &self.title_gate {
            entered.wait();
            release.wait();
        }
        Ok(self.title.clone())
    }
    fn is_alive(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    fn navigation_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    fn navigate(&self, _url: &str) -> Result<u64, String> {
        Ok(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn host_script_owned(
        &self,
        _js: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        self.scripts.fetch_add(1, Ordering::SeqCst);
        operation.finished();
        Err("selection must not execute scripts".into())
    }
}

#[test]
fn selection_is_bounded_metadata_without_an_executable_target() {
    let adapter = WebViewAdapter::new();
    let view = View::new();
    let selection = WebViewSelection::capture(view.clone()).unwrap();
    assert_eq!(selection.title().unwrap(), "Selected page");
    assert!(adapter.list_targets().unwrap().is_empty());
    assert!(!adapter.target_alive(&selection.label));
    assert!(!adapter.capabilities().observe_ax);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    let target = adapter.bind_selected(&selection, "session", "run").unwrap();
    assert_ne!(target.target_id, selection.label);
    assert!(adapter.target_alive(&target.target_id));
    assert!(adapter.capabilities().observe_ax);
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn navigation_during_the_user_selection_wait_cannot_authorize_the_new_document() {
    let adapter = WebViewAdapter::new();
    let view = View::new();
    let selection = WebViewSelection::capture(view.clone()).unwrap();
    view.navigate("https://fixture.invalid/selected").unwrap(); // Same URL is a new document.
    assert!(selection.title().is_err());
    assert!(adapter.bind_selected(&selection, "session", "run").is_err());
    assert!(adapter.list_targets().unwrap().is_empty());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    let fresh = WebViewSelection::capture(view.clone()).unwrap();
    let target = adapter.bind_selected(&fresh, "session", "run").unwrap();
    assert_eq!(target.lifecycle_stamp, 2);
    assert!(adapter.target_alive(&target.target_id));
}

#[test]
fn failed_stale_selection_does_not_revoke_an_existing_binding() {
    let adapter = WebViewAdapter::new();
    let current_view = View::new();
    let current = adapter
        .bind(
            current_view.clone(),
            "current",
            "current-run",
            &serde_json::json!({}),
        )
        .unwrap();
    let selected_view = View::new();
    *selected_view.native.lock() = "native-other".into();
    let selection = WebViewSelection::capture(selected_view.clone()).unwrap();
    selected_view.live.store(false, Ordering::SeqCst);
    assert!(adapter
        .bind_selected(&selection, "next", "next-run")
        .is_err());
    assert!(adapter.target_alive(&current.target_id));
    assert_eq!(
        adapter.list_targets_for_run("current-run").unwrap().len(),
        1
    );
    assert!(adapter.list_targets_for_run("next-run").unwrap().is_empty());
    assert_eq!(current_view.scripts.load(Ordering::SeqCst), 0);
    assert_eq!(selected_view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn same_label_url_and_title_cannot_replace_the_retained_native_object() {
    let adapter = WebViewAdapter::new();
    let old_view = View::new();
    let old = WebViewSelection::capture(old_view.clone()).unwrap();
    let new_view = View::new();
    *new_view.native.lock() = "native-replacement".into();
    let new = WebViewSelection::capture(new_view.clone()).unwrap();
    old_view.live.store(false, Ordering::SeqCst);
    assert!(adapter.bind_selected(&old, "old", "old-run").is_err());
    let target = adapter.bind_selected(&new, "new", "new-run").unwrap();
    assert!(target.target_id.contains("native-replacement@"));
    assert_eq!(old_view.scripts.load(Ordering::SeqCst), 0);
    assert_eq!(new_view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn navigation_while_metadata_is_being_read_rejects_the_entire_receipt() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let view = View::with_title(
        "Selected page".into(),
        Some((entered.clone(), release.clone())),
    );
    let worker = {
        let view = view.clone();
        std::thread::spawn(move || WebViewSelection::capture(view))
    };
    entered.wait();
    view.navigate("https://fixture.invalid/new").unwrap();
    release.wait();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn retained_identity_fields_and_session_address_are_all_checked_before_bind() {
    let adapter = WebViewAdapter::new();
    for change in ["label", "native", "generation", "close"] {
        let view = View::new();
        let selection = WebViewSelection::capture(view.clone()).unwrap();
        match change {
            "label" => *view.label.lock() = "resource-browser-other".into(),
            "native" => *view.native.lock() = "native-other".into(),
            "generation" => {
                view.generation.store(2, Ordering::SeqCst);
            }
            "close" => {
                view.live.store(false, Ordering::SeqCst);
            }
            _ => unreachable!(),
        }
        assert!(adapter.bind_selected(&selection, "session", "run").is_err());
        assert!(adapter.list_targets().unwrap().is_empty());
    }
    let selection = WebViewSelection::capture(View::new()).unwrap();
    for (session, run) in [
        ("", "run"),
        ("session", ""),
        ("foreign|session", "run"),
        ("session", "foreign|run"),
    ] {
        assert!(adapter.bind_selected(&selection, session, run).is_err());
    }
    assert!(adapter.list_targets().unwrap().is_empty());
}

#[test]
fn unicode_picker_metadata_is_bounded_and_empty_title_uses_the_selected_url() {
    let long = WebViewSelection::capture(View::with_title("中文🌏".repeat(300), None)).unwrap();
    assert_eq!(long.title().unwrap().chars().count(), MAX_METADATA_CHARS);
    let empty = WebViewSelection::capture(View::with_title(String::new(), None)).unwrap();
    assert_eq!(empty.title().unwrap(), "https://fixture.invalid/selected");
}

#[test]
fn task_candidate_is_metadata_only_and_receipt_binds_the_original_producer() {
    use grok_computer_use_core::adapter::NativeScopeIdentity;
    let adapter = Arc::new(WebViewAdapter::new());
    let view = View::new();
    let candidate = adapter
        .selected_candidate(WebViewSelection::capture(view.clone()).unwrap())
        .unwrap();
    assert!(candidate.info.target_id.starts_with("wv:selection:"));
    assert!(adapter.scope_identity(&candidate.info.target_id).is_err());
    assert!(adapter.observe(&candidate.info.target_id).is_err());
    assert!(adapter.list_targets().unwrap().is_empty());
    let foreign = WebViewAdapter::new();
    assert!(foreign
        .bind_scope_candidate(&candidate.identity, "session", "new-run")
        .is_err());
    assert!(adapter.list_targets().unwrap().is_empty());
    let bound = adapter
        .bind_scope_candidate(&candidate.identity, "session", "new-run")
        .unwrap();
    assert_ne!(bound.info.target_id, candidate.info.target_id);
    assert!(
        matches!(&bound.identity, NativeScopeIdentity::WebViewPage { native_tab_id, session_id, run_id, document_generation, .. }
        if native_tab_id == "native-selected" && session_id == "session" && run_id == "new-run" && *document_generation == 1)
    );
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    adapter.release_target_for_run("new-run", &bound.info.target_id);
    assert!(adapter.list_targets().unwrap().is_empty());
}

#[test]
fn retained_task_receipt_rejects_navigation_close_and_producer_retirement() {
    use grok_computer_use_core::adapter::NativeScopeIdentity;
    for retired in ["navigation", "close", "producer"] {
        let adapter = Arc::new(WebViewAdapter::new());
        let view = View::new();
        let candidate = adapter
            .selected_candidate(WebViewSelection::capture(view.clone()).unwrap())
            .unwrap();
        match retired {
            "navigation" => {
                view.generation.fetch_add(1, Ordering::SeqCst);
            }
            "close" => {
                view.live.store(false, Ordering::SeqCst);
            }
            "producer" => {
                drop(adapter);
            }
            _ => unreachable!(),
        }
        let NativeScopeIdentity::WebViewSelected { identity } = candidate.identity else {
            unreachable!()
        };
        let executor = WebViewAdapter::new();
        assert!(identity
            .receipt
            .bind(&executor, "session", "new-run")
            .is_err());
        if retired != "producer" {
            assert!(identity.receipt.require_current().is_err());
        }
        assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    }
}
