//! Synthetic native receipts exercise the original Broker transaction, not UI consent.
use super::*;
use crate::adapter::{
    AdapterActResult, Capabilities, DispatchRequest, NativeScopeIdentity, ScopeCandidate,
    ScopeCandidatePage, TargetInfo, WebViewScopeReceipt, WebViewSelectedIdentity,
};
use crate::fake::FakeAdapter;
use crate::protocol::Observation;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::Weak;

struct Receipt {
    owner: Weak<Adapter>,
    page: usize,
    generation: u64,
}
impl WebViewScopeReceipt for Receipt {
    fn require_current(&self) -> Result<(), String> {
        let owner = self.owner.upgrade().ok_or("producer retired")?;
        if owner.documents.lock()[self.page] != self.generation {
            return Err("selected document retired".into());
        }
        Ok(())
    }
    fn bind(
        &self,
        executor: &dyn ComputerUseAdapter,
        session: &str,
        run: &str,
    ) -> Result<ScopeCandidate, String> {
        self.require_current()?;
        let owner = self.owner.upgrade().ok_or("producer retired")?;
        if !std::ptr::eq(
            executor as *const dyn ComputerUseAdapter as *const (),
            Arc::as_ptr(&owner).cast(),
        ) {
            return Err("foreign producer".into());
        }
        owner.binds.fetch_add(1, Ordering::SeqCst);
        if owner.fail_page.load(Ordering::SeqCst) == self.page {
            return Err("synthetic bind failure".into());
        }
        let target = format!(
            "wv|resource-browser-{}|native-{}@{}|{}|{session}|{run}",
            self.page,
            self.page,
            Uuid::new_v4(),
            self.generation
        );
        let identity = NativeScopeIdentity::WebViewPage {
            target_id: target.clone(),
            native_tab_id: format!("native-{}", self.page),
            document_generation: self.generation,
            session_id: session.into(),
            run_id: run.into(),
        };
        owner.fake.pin_synthetic_member(&target);
        owner
            .bindings
            .lock()
            .insert(target.clone(), identity.clone());
        let hook = owner.after_bind.lock().take();
        if let Some(hook) = hook {
            hook(run.into());
        }
        Ok(ScopeCandidate {
            info: owner.info(&target),
            identity,
        })
    }
}

type BindHook = Box<dyn FnOnce(String) + Send>;
struct Adapter {
    original: Weak<Adapter>,
    fake: FakeAdapter,
    documents: Mutex<[u64; 2]>,
    bindings: Mutex<HashMap<String, NativeScopeIdentity>>,
    binds: AtomicUsize,
    fail_page: AtomicUsize,
    corrupt_proof: AtomicBool,
    after_bind: Mutex<Option<BindHook>>,
    upload_field: AtomicBool,
    uploads: Mutex<Vec<crate::adapter::UploadRequest>>,
    after_upload: Mutex<Option<BindHook>>,
}
impl Adapter {
    fn new() -> Arc<Self> {
        Arc::new_cyclic(|original| Self {
            original: original.clone(),
            fake: FakeAdapter::new(),
            documents: Mutex::new([1, 1]),
            bindings: Mutex::new(HashMap::new()),
            binds: AtomicUsize::new(0),
            fail_page: AtomicUsize::new(usize::MAX),
            corrupt_proof: AtomicBool::new(false),
            after_bind: Mutex::new(None),
            upload_field: AtomicBool::new(false),
            uploads: Mutex::new(Vec::new()),
            after_upload: Mutex::new(None),
        })
    }
    fn info(&self, target: &str) -> TargetInfo {
        let mut info = self.fake.list_targets().unwrap().remove(0);
        info.target_id = target.into();
        info.title = "synthetic retained page".into();
        info.kind = "webview".into();
        info.backend = self.backend_id().into();
        info
    }
    fn current(&self, identity: &NativeScopeIdentity) -> bool {
        match identity {
            NativeScopeIdentity::WebViewPage {
                native_tab_id,
                document_generation,
                ..
            } => {
                let page = native_tab_id
                    .strip_prefix("native-")
                    .and_then(|s| s.parse::<usize>().ok());
                page.is_some_and(|page| {
                    self.documents.lock().get(page) == Some(document_generation)
                })
            }
            _ => false,
        }
    }
}
impl ComputerUseAdapter for Adapter {
    fn backend_id(&self) -> &'static str {
        "synthetic-webview-task"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::for_surface(SurfaceKind::WebView, self.backend_id())
    }
    fn scope_candidates_for_task(
        &self,
        limit: usize,
        current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ScopeCandidatePage, String> {
        let mut candidates = Vec::new();
        for page in 0..2.min(limit) {
            if !current() {
                return Err("retired picker".into());
            }
            let generation = self.documents.lock()[page];
            let target = format!("wv:selection:{}", Uuid::new_v4());
            candidates.push(ScopeCandidate {
                info: self.info(&target),
                identity: NativeScopeIdentity::WebViewSelected {
                    identity: WebViewSelectedIdentity {
                        target_id: target,
                        native_tab_id: format!("native-{page}"),
                        document_generation: generation,
                        receipt: Arc::new(Receipt {
                            owner: self.original.clone(),
                            page,
                            generation,
                        }),
                    },
                },
            });
        }
        Ok(ScopeCandidatePage {
            candidates,
            truncated: limit < 2,
        })
    }
    fn bind_scope_candidate(
        &self,
        identity: &NativeScopeIdentity,
        session: &str,
        run: &str,
    ) -> Result<ScopeCandidate, String> {
        let NativeScopeIdentity::WebViewSelected { identity } = identity else {
            return Err("not selected".into());
        };
        let mut bound = identity.receipt.bind(self, session, run)?;
        if self.corrupt_proof.load(Ordering::SeqCst) {
            let NativeScopeIdentity::WebViewPage { run_id, .. } = &mut bound.identity else {
                unreachable!()
            };
            *run_id = "foreign-run".into();
        }
        Ok(bound)
    }
    fn scope_identity(&self, target: &str) -> Result<NativeScopeIdentity, String> {
        let identity = self
            .bindings
            .lock()
            .get(target)
            .cloned()
            .ok_or("not executable")?;
        if !self.current(&identity) {
            return Err("document retired".into());
        }
        Ok(identity)
    }
    fn scope_target_info(&self, _run: &str, target: &str) -> Result<TargetInfo, String> {
        self.scope_identity(target)?;
        Ok(self.info(target))
    }
    fn with_scope_authority(
        &self,
        identities: &[NativeScopeIdentity],
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        let bound = self.bindings.lock();
        for identity in identities {
            if !self.current(identity)
                || identity
                    .target_id()
                    .and_then(|target| bound.get(&target).cloned())
                    .as_ref()
                    != Some(identity)
            {
                return Err(BrokerError::DeadTarget);
            }
        }
        commit()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        Ok(self
            .bindings
            .lock()
            .keys()
            .map(|target| self.info(target))
            .collect())
    }
    fn target_alive(&self, target: &str) -> bool {
        self.scope_identity(target).is_ok()
    }
    fn claim_target_for_run(&self, run: &str, target: &str) -> Result<(), String> {
        match self.scope_identity(target)? {
            NativeScopeIdentity::WebViewPage { run_id, .. } if run_id == run => Ok(()),
            _ => Err("foreign run claim".into()),
        }
    }
    fn release_target_for_run(&self, run: &str, target: &str) {
        let mut bindings = self.bindings.lock();
        if matches!(bindings.get(target), Some(NativeScopeIdentity::WebViewPage { run_id, .. }) if run_id == run)
        {
            bindings.remove(target);
        }
    }
    fn observe(&self, target: &str) -> Result<Observation, String> {
        self.scope_identity(target)?;
        let mut observation = self.fake.observe(target)?;
        if self.upload_field.load(Ordering::SeqCst) {
            observation.nodes[0].actions = vec!["upload".into(), "wait".into()];
        }
        Ok(observation)
    }
    fn stage_upload(
        &self,
        request: &crate::adapter::UploadRequest,
    ) -> Result<Box<dyn crate::adapter::StagedUpload>, crate::adapter::UploadError> {
        let identity = self
            .scope_identity(&request.target_id)
            .map_err(crate::adapter::UploadError::NotDispatched)?;
        self.uploads.lock().push(request.clone());
        if let Some(hook) = self.after_upload.lock().take() {
            hook(request.run_id.clone());
        }
        Ok(Box::new(upload::Staged {
            owner: self.original.clone(),
            identity,
            result: AdapterActResult {
                applied: true,
                verifiable: true,
                postcondition_ok: true,
                outcome: None,
                detail: "synthetic field assigned".into(),
            },
        }))
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.claim_target_for_run(&request.run_id, &request.target_id)?;
        self.fake.act(request)
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        self.fake.abort(run, generation)
    }
    fn is_idle(&self, run: &str) -> bool {
        self.fake.is_idle(run)
    }
    fn start_periodic_preview(&self, _target: &str) {}
    fn stop_periodic_preview(&self) {}
    fn periodic_preview_active(&self) -> bool {
        false
    }
}

struct Fixture {
    broker: Arc<ComputerUseBroker>,
    adapter: Arc<Adapter>,
    token: String,
    key: crate::task_consent::HostRequestKey,
}
impl Fixture {
    fn new(wrapped: bool) -> Self {
        let adapter = Adapter::new();
        let broker = Arc::new(ComputerUseBroker::request_endpoint(BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-webview-selected-{}.lease", Uuid::new_v4())),
            ..Default::default()
        }));
        let executor: Arc<dyn ComputerUseAdapter> = if wrapped {
            Arc::new(crate::owned::HostOwnedAdapter::new(adapter.clone()))
        } else {
            adapter.clone()
        };
        broker
            .register_surface_adapter(SurfaceKind::WebView, executor)
            .unwrap();
        let connection = broker.reserve_transport_connection("webview-app").unwrap();
        let token = connection.token().to_string();
        broker
            .bind_transport_agent(&token, "synthetic-agent")
            .unwrap();
        broker.submit_access(&token, json!({"action":"request","purpose":"use selected pages","surfaces":["app-webview"],"scopeIntent":"two original pages","capabilities":["observe","input"]})).unwrap();
        let key = broker.host_task_view(&token).unwrap().request.unwrap().key;
        Self {
            broker,
            adapter,
            token,
            key,
        }
    }
    fn picker(&self) -> HostScopePicker {
        self.broker
            .begin_task_scope_picker(&self.token, &self.key, SurfaceKind::WebView)
            .unwrap()
            .discover()
            .unwrap()
    }
    fn ids(picker: &HostScopePicker) -> Vec<String> {
        picker
            .views()
            .unwrap()
            .into_iter()
            .map(|view| view.candidate_id)
            .collect()
    }
    fn approval(&self, picker: &HostScopePicker, ids: &[String]) -> AccessApproval {
        let AccessApprovalReady::Prepare(approval) = self
            .broker
            .reserve_selected_access_approval(&self.token, picker.select(ids).unwrap())
            .unwrap()
        else {
            panic!("original prepare")
        };
        approval
    }
    fn drain(&self) {
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
    fn no_grant(&self) {
        assert_eq!(self.broker.executable_grant_count("webview-app"), 0);
        assert_eq!(self.broker.continuation_count("webview-app"), 0);
        assert_eq!(self.adapter.fake.observe_calls(), 0);
        assert!(self.adapter.fake.executions().is_empty());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.broker.fence_task_consent("webview-app");
        self.drain();
    }
}

mod upload;

#[test]
fn picker_and_reservation_never_bind_or_admit_selected_metadata() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let ids = Fixture::ids(&picker);
    let selection = picker.select(&ids).unwrap();
    for member in selection.members() {
        assert!(!f.adapter.target_alive(&member.target_id));
        assert!(f.adapter.observe(&member.target_id).is_err());
    }
    let approval = f.approval(&picker, &ids);
    assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 0);
    assert_eq!(f.broker.test_run_count(), 0);
    assert!(f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .is_err());
    f.no_grant();
    drop(approval);
}

#[test]
fn original_group_converts_to_one_new_run_and_retry_keeps_original_choice() {
    for wrapped in [false, true] {
        let f = Fixture::new(wrapped);
        let picker = f.picker();
        let ids = Fixture::ids(&picker);
        let mut approval = f.approval(&picker, &ids);
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        f.no_grant();
        approval.prepare_publication().unwrap();
        let outcome = approval.commit_for_transport(&f.token).unwrap();
        assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 2);
        assert_eq!(f.broker.test_run_count(), 1);
        for identity in f.adapter.bindings.lock().values() {
            assert!(
                matches!(identity, NativeScopeIdentity::WebViewPage { session_id, run_id, .. } if session_id == "webview-app" && run_id == &outcome.run_id)
            );
        }
        assert_eq!(
            picker
                .committed_outcome(&f.token, &ids)
                .unwrap()
                .unwrap()
                .run_id,
            outcome.run_id
        );
        assert!(picker.committed_outcome(&f.token, &ids[..1]).is_err());
        assert_eq!(f.broker.executable_grant_count("webview-app"), 1);
        assert_eq!(f.broker.continuation_count("webview-app"), 1);
        assert_eq!(
            f.broker.request_stop(&outcome.run_id).unwrap(),
            StopState::Stopped
        );
        assert!(f.adapter.bindings.lock().is_empty());
    }
}

#[test]
fn stopped_or_navigated_selection_cannot_start_binding() {
    for stop in [false, true] {
        let f = Fixture::new(false);
        let picker = f.picker();
        let ids = Fixture::ids(&picker);
        if stop {
            picker.cancel_handle().fence();
        } else {
            f.adapter.documents.lock()[1] += 1;
        }
        if let Ok(selection) = picker.select(&ids) {
            assert!(f
                .broker
                .reserve_selected_access_approval(&f.token, selection)
                .is_err());
        }
        assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 0);
        f.no_grant();
    }
}

#[test]
fn partial_binding_failure_releases_all_original_new_ids() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    f.adapter.fail_page.store(1, Ordering::SeqCst);
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    drop(approval);
    f.drain();
    assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 2);
    assert!(f.adapter.bindings.lock().is_empty());
    assert!(!f.broker.feature_enabled());
    f.no_grant();
}

#[test]
fn stop_during_original_bind_retains_pending_work_then_discards_late_result() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    let broker = f.broker.clone();
    *f.adapter.after_bind.lock() = Some(Box::new(move |run| {
        assert_eq!(broker.request_stop(&run).unwrap(), StopState::StopRequested);
        assert!(broker.inner.lock().runs[&run]
            .in_flight
            .load(Ordering::SeqCst));
    }));
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    drop(approval);
    f.drain();
    assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 1);
    assert!(f.adapter.bindings.lock().is_empty());
    f.no_grant();
}

#[test]
fn foreign_run_returned_by_producer_never_becomes_execution_authority() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    f.adapter.corrupt_proof.store(true, Ordering::SeqCst);
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    drop(approval);
    f.drain();
    assert!(f.adapter.bindings.lock().is_empty());
    f.no_grant();
}

#[test]
fn native_retirement_after_preflight_prevents_final_publication_and_cleans_group() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    approval.prepare_publication().unwrap();
    f.adapter.documents.lock()[1] += 1;
    assert!(approval.commit_for_transport(&f.token).is_err());
    drop(approval);
    f.drain();
    assert!(f.adapter.bindings.lock().is_empty());
    f.no_grant();
}

#[test]
fn switching_back_keeps_group_bindings_and_stop_preserves_another_owner() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let mut targets: Vec<_> = f.adapter.bindings.lock().keys().cloned().collect();
    targets.sort();
    let other = "wv|resource-browser-other|native-other@binding|1|other-app|other-run";
    f.adapter.bindings.lock().insert(
        other.into(),
        NativeScopeIdentity::WebViewPage {
            target_id: other.into(),
            native_tab_id: "native-0".into(),
            document_generation: 1,
            session_id: "other-app".into(),
            run_id: "other-run".into(),
        },
    );
    for target in [&targets[1], &targets[0], &targets[1], &targets[0]] {
        f.broker
            .open_approved_task_target(&f.token, &json!({"runId":outcome.run_id,"targetId":target}))
            .unwrap();
        assert!(f.adapter.target_alive(&targets[0]));
        assert!(f.adapter.target_alive(&targets[1]));
    }
    f.broker.request_stop(&outcome.run_id).unwrap();
    assert_eq!(f.adapter.bindings.lock().len(), 1);
    assert!(f.adapter.bindings.lock().contains_key(other));
}

#[test]
fn pause_resume_keeps_exact_group_and_retires_old_observation() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    let old = f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .unwrap();
    f.broker.observe_for_permit(&old, false).unwrap();
    f.broker.pause(&outcome.run_id).unwrap();
    assert!(f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .is_err());
    let execution = f
        .broker
        .host_task_view(&f.token)
        .unwrap()
        .executions
        .remove(0);
    assert!(execution.resume_available);
    let address = HostExecutionAddress {
        key: f.key.clone(),
        run_id: outcome.run_id.clone(),
        generation: execution.generation,
    };
    let HostTaskResumeReady::Prepare(resume) = f
        .broker
        .reserve_host_task_resume(&f.token, &address)
        .unwrap()
    else {
        panic!("one recovery")
    };
    let receipt = resume.complete(Duration::from_secs(1)).unwrap();
    assert_ne!(receipt.address.generation, address.generation);
    assert_eq!(f.adapter.bindings.lock().len(), 2);
    assert_eq!(f.adapter.binds.load(Ordering::SeqCst), 2);
    assert!(f.broker.observe_for_permit(&old, false).is_err());
    let fresh = f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .unwrap();
    f.broker.observe_for_permit(&fresh, false).unwrap();
}

#[test]
fn navigation_after_recovery_capture_cannot_publish_old_native_authority() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    f.broker.pause(&outcome.run_id).unwrap();
    let execution = f
        .broker
        .host_task_view(&f.token)
        .unwrap()
        .executions
        .remove(0);
    let address = HostExecutionAddress {
        key: f.key.clone(),
        run_id: outcome.run_id.clone(),
        generation: execution.generation,
    };
    let HostTaskResumeReady::Prepare(mut resume) = f
        .broker
        .reserve_host_task_resume(&f.token, &address)
        .unwrap()
    else {
        panic!("recovery")
    };
    resume.prepare_native(Duration::from_secs(1)).unwrap();
    f.adapter.documents.lock()[1] += 1;
    assert!(resume.commit().is_err());
    drop(resume);
    assert!(f.broker.is_paused(&outcome.run_id).unwrap());
    assert!(f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .is_err());
    assert_eq!(f.adapter.fake.executions().len(), 0);
}

#[test]
fn webview_switch_and_stop_preserve_an_independent_desktop_input_lease() {
    let f = Fixture::new(false);
    let picker = f.picker();
    let mut approval = f.approval(&picker, &Fixture::ids(&picker));
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    assert!(f.broker.lease.holder().is_none());
    f.broker
        .lease
        .try_acquire("independent-desktop-run", &f.broker.instance_id)
        .unwrap();
    let targets: Vec<_> = f.adapter.bindings.lock().keys().cloned().collect();
    for target in targets.iter().cycle().take(4) {
        f.broker
            .open_approved_task_target(&f.token, &json!({"runId":outcome.run_id,"targetId":target}))
            .unwrap();
        assert_eq!(
            f.broker.lease.holder().unwrap().run_id,
            "independent-desktop-run"
        );
    }
    f.broker.request_stop(&outcome.run_id).unwrap();
    assert_eq!(
        f.broker.lease.holder().unwrap().run_id,
        "independent-desktop-run"
    );
    f.broker.lease.release("independent-desktop-run").unwrap();
}
