//! Synthetic metadata adapter using the production native identity guards.
use super::*;
use std::sync::atomic::AtomicUsize;

struct View {
    native: lifecycle::NativeView,
    document: lifecycle::DocumentIdentity,
    label: String,
    metadata: AtomicUsize,
    scripts: AtomicUsize,
}

impl View {
    fn new(registry: &lifecycle::Registry, label: &str) -> Arc<Self> {
        let native = registry.reserve().unwrap();
        registry.publish(label, &native).unwrap();
        Arc::new(Self {
            document: native.document().unwrap(),
            native,
            label: label.into(),
            metadata: AtomicUsize::new(0),
            scripts: AtomicUsize::new(0),
        })
    }
}

impl LiveWebView for View {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tab_id(&self) -> String {
        format!("native-{}", self.document.instance)
    }
    fn url(&self) -> Result<String, String> {
        self.metadata.fetch_add(1, Ordering::SeqCst);
        Ok("https://fixture.invalid/page".into())
    }
    fn title(&self) -> Result<String, String> {
        self.metadata.fetch_add(1, Ordering::SeqCst);
        Ok(self.label.clone())
    }
    fn is_alive(&self) -> bool {
        self.native.document().is_some()
    }
    fn navigation_generation(&self) -> u64 {
        self.native
            .document()
            .map_or(0, |document| document.generation)
    }
    fn navigate(&self, _url: &str) -> Result<u64, String> {
        self.native.navigation_started();
        Ok(self.navigation_generation())
    }
    fn with_document_authority(
        &self,
        generation: u64,
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        self.native.with_document_authority(
            lifecycle::DocumentIdentity {
                instance: self.document.instance,
                generation,
            },
            commit,
        )
    }
    fn host_script_owned(
        &self,
        _script: &str,
        _generation: u64,
        operation: ScriptOperation,
    ) -> Result<String, String> {
        self.scripts.fetch_add(1, Ordering::SeqCst);
        operation.finished();
        Err("metadata test must not execute scripts".into())
    }
}

fn bind(adapter: &WebViewAdapter, view: &Arc<View>, session: &str, run: &str) -> TargetInfo {
    let selection = WebViewSelection::capture(view.clone()).unwrap();
    adapter.bind_selected(&selection, session, run).unwrap()
}

#[test]
fn independent_tasks_keep_their_original_pages_and_only_read_their_own_metadata() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let a = View::new(&registry, "resource-browser-a");
    let b = View::new(&registry, "resource-browser-b");
    let ta = bind(&adapter, &a, "session-a", "run-a");
    let tb = bind(&adapter, &b, "session-b", "run-b");
    a.metadata.store(0, Ordering::SeqCst);
    b.metadata.store(0, Ordering::SeqCst);
    assert_eq!(
        adapter.list_targets_for_run("run-a").unwrap()[0].target_id,
        ta.target_id
    );
    assert_eq!(b.metadata.load(Ordering::SeqCst), 0);
    assert!(adapter.scope_target_info("run-a", &tb.target_id).is_err());
    assert_eq!(b.metadata.load(Ordering::SeqCst), 0);
    assert_eq!(
        adapter.list_targets_for_run("run-b").unwrap()[0].target_id,
        tb.target_id
    );
    assert_eq!(
        a.scripts.load(Ordering::SeqCst) + b.scripts.load(Ordering::SeqCst),
        0
    );
    adapter.release_target_for_run("run-a", &ta.target_id);
    adapter.abort("run-a", 2).unwrap();
    assert!(adapter.target_alive(&tb.target_id));
}

#[test]
fn competing_task_cannot_replace_the_live_page_binding() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let view = View::new(&registry, "resource-browser-a");
    let original = bind(&adapter, &view, "session-a", "run-a");
    let next = WebViewSelection::capture(view.clone()).unwrap();
    for (session, run) in [
        ("session-a", "run-b"),
        ("session-b", "run-a"),
        ("session-b", "run-b"),
    ] {
        assert!(adapter
            .bind_selected(&next, session, run)
            .unwrap_err()
            .contains("another task"));
        assert!(adapter.target_alive(&original.target_id));
    }
    assert!(!adapter.unbind_for_run("session-b", "run-a"));
    assert!(adapter.unbind_for_run("session-a", "run-a"));
    let replacement = adapter.bind_selected(&next, "session-b", "run-b").unwrap();
    adapter.release_target(&original.target_id);
    adapter.abort("run-a", 2).unwrap();
    assert!(adapter.target_alive(&replacement.target_id));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
}

#[test]
fn one_task_can_pin_a_group_and_unbind_only_its_own_members() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let a = View::new(&registry, "resource-browser-a");
    let b = View::new(&registry, "resource-browser-b");
    let c = View::new(&registry, "resource-browser-c");
    let ta = bind(&adapter, &a, "session-a", "run-a");
    let tb = bind(&adapter, &b, "session-a", "run-a");
    let tc = bind(&adapter, &c, "session-c", "run-c");
    let proofs = [
        adapter.scope_identity(&tb.target_id).unwrap(),
        adapter.scope_identity(&ta.target_id).unwrap(),
    ];
    let mut commits = 0;
    adapter
        .with_scope_authority(&proofs, &mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(commits, 1);
    assert_eq!(adapter.list_targets_for_run("run-a").unwrap().len(), 2);
    assert!(adapter.unbind_for_run("session-a", "run-a"));
    assert!(!adapter.target_alive(&ta.target_id));
    assert!(!adapter.target_alive(&tb.target_id));
    assert!(adapter.target_alive(&tc.target_id));
    assert!(adapter
        .with_scope_authority(&proofs, &mut || {
            commits += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(commits, 1);
}

#[test]
fn stale_or_forged_proof_never_reaches_publication_and_other_pages_stay_live() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let a = View::new(&registry, "resource-browser-a");
    let b = View::new(&registry, "resource-browser-b");
    let ta = bind(&adapter, &a, "session-a", "run-a");
    let tb = bind(&adapter, &b, "session-b", "run-b");
    let proof = adapter.scope_identity(&ta.target_id).unwrap();
    let mut forged = proof.clone();
    let NativeScopeIdentity::WebViewPage { run_id, .. } = &mut forged else {
        unreachable!()
    };
    *run_id = "run-b".into();
    let mut commits = 0;
    let mut commit = || {
        commits += 1;
        Ok(())
    };
    assert!(adapter
        .with_scope_authority(&[forged], &mut commit)
        .is_err());
    assert!(adapter
        .with_scope_authority(&[proof.clone(), proof.clone()], &mut commit)
        .is_err());
    a.navigate("https://fixture.invalid/page").unwrap();
    assert!(adapter.with_scope_authority(&[proof], &mut commit).is_err());
    assert_eq!(commits, 0);
    let other = adapter.scope_identity(&tb.target_id).unwrap();
    adapter
        .with_scope_authority(&[other], &mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(commits, 1);
    assert_eq!(
        a.scripts.load(Ordering::SeqCst) + b.scripts.load(Ordering::SeqCst),
        0
    );
}

#[test]
fn rebinding_and_same_label_replacement_do_not_revive_an_old_proof() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let a = View::new(&registry, "resource-browser-a");
    let first = bind(&adapter, &a, "session-a", "run-a");
    let old = adapter.scope_identity(&first.target_id).unwrap();
    let second = bind(&adapter, &a, "session-a", "run-a");
    assert_ne!(first.target_id, second.target_id);
    let mut commits = 0;
    assert!(adapter
        .with_scope_authority(&[old], &mut || {
            commits += 1;
            Ok(())
        })
        .is_err());
    let second_proof = adapter.scope_identity(&second.target_id).unwrap();
    registry.closed(&a.label, &a.native);
    let replacement = View::new(&registry, "resource-browser-a");
    let fresh = bind(&adapter, &replacement, "session-b", "run-b");
    assert!(adapter
        .with_scope_authority(&[second_proof], &mut || {
            commits += 1;
            Ok(())
        })
        .is_err());
    adapter.release_target(&second.target_id);
    assert!(adapter.target_alive(&fresh.target_id));
    assert_eq!(commits, 0);
}

#[test]
fn pending_native_work_survives_transfer_and_blocks_new_publication_until_completion() {
    let registry = lifecycle::Registry::default();
    let adapter = WebViewAdapter::new();
    let view = View::new(&registry, "resource-browser-pending");
    let old = bind(&adapter, &view, "session-a", "run-a");
    let original = adapter.bound_target(&old.target_id).unwrap();
    let operation = adapter
        .scripts
        .begin(
            "run-a",
            &original.native_key(),
            &Default::default(),
            &original.binding_cancellation,
        )
        .unwrap();
    assert!(adapter.unbind_for_run("session-a", "run-a"));
    let new = bind(&adapter, &view, "session-b", "run-b");
    let proof = adapter.scope_identity(&new.target_id).unwrap();
    let mut commits = 0;
    assert!(adapter
        .with_scope_authority(std::slice::from_ref(&proof), &mut || {
            commits += 1;
            Ok(())
        })
        .is_err());
    assert!(!adapter.is_idle("run-a"));
    assert!(operation.check().is_err());
    assert_eq!(commits, 0);
    operation.finished();
    adapter
        .with_scope_authority(&[proof], &mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    adapter.release_target(&old.target_id);
    assert!(adapter.target_alive(&new.target_id));
    assert_eq!(commits, 1);
}
