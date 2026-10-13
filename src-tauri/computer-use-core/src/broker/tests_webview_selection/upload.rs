//! Synthetic choice and field assignment test the real Broker, not native/UI consent.
use super::*;
use crate::adapter::{HostSelectedUploadFile, StagedUpload};

mod transport;

pub(super) struct Staged {
    pub(super) owner: Weak<Adapter>,
    pub(super) identity: NativeScopeIdentity,
    pub(super) result: AdapterActResult,
}
impl StagedUpload for Staged {
    fn result(&self) -> &AdapterActResult {
        &self.result
    }
    fn commit(
        self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), BrokerError>,
    ) -> Result<(), BrokerError> {
        self.owner
            .upgrade()
            .ok_or(BrokerError::DeadTarget)?
            .with_scope_authority(std::slice::from_ref(&self.identity), commit)
    }
}

struct UploadFixture {
    f: Fixture,
    permit: crate::task_consent::ExecutionPermit,
    intent: WebViewUploadIntent,
    address: HostExecutionAddress,
}
impl UploadFixture {
    fn new(wrapped: bool) -> Self {
        let f = Fixture::new(wrapped);
        f.adapter.upload_field.store(true, Ordering::SeqCst);
        let picker = f.picker();
        let mut approval = f.approval(&picker, &Fixture::ids(&picker)[..1]);
        approval.publish_feature().unwrap();
        approval.prepare_execution().unwrap();
        approval.prepare_publication().unwrap();
        let committed = approval.commit_for_transport(&f.token).unwrap();
        let permit = f
            .broker
            .begin_model_call(&f.token, "computer_upload")
            .unwrap();
        let observation = f
            .broker
            .observe_with_screenshot(&committed.run_id, false)
            .unwrap();
        let intent = WebViewUploadIntent {
            run_id: committed.run_id,
            target_id: observation.target_id,
            target_generation: observation.target_generation,
            action_id: "one-file-choice".into(),
            snapshot_id: observation.snapshot_id,
            geometry_revision: observation.geometry_revision,
            element_ref: observation.nodes[0].node_ref.clone(),
        };
        let view = f.broker.host_task_view(&f.token).unwrap();
        let execution = &view.executions[0];
        let address = HostExecutionAddress {
            key: execution.key.clone(),
            run_id: execution.run_id.clone(),
            generation: execution.generation.clone(),
        };
        Self {
            f,
            permit,
            intent,
            address,
        }
    }
    fn request(&self) -> WebViewUploadCall {
        match self
            .f
            .broker
            .request_webview_upload(self.intent.clone(), &self.permit)
            .unwrap()
        {
            WebViewUploadReady::Pending(call) => call,
            _ => panic!("original pending choice"),
        }
    }
    fn select(&self, call: &WebViewUploadCall) -> HostUploadSelection {
        self.f
            .broker
            .begin_host_upload_selection(&self.f.token, &self.address, call.request_id())
            .unwrap()
    }
    fn file() -> HostSelectedUploadFile {
        HostSelectedUploadFile::from_host_selection(
            "中文.txt".into(),
            "text/plain".into(),
            "不可变内容".as_bytes().to_vec(),
        )
        .unwrap()
    }
}

#[test]
fn original_file_choice_assigns_once_and_replay_never_opens_a_second_choice() {
    for wrapped in [false, true] {
        let u = UploadFixture::new(wrapped);
        let call = u.request();
        let view = u.f.broker.host_task_view(&u.f.token).unwrap();
        assert_eq!(
            view.executions[0].upload.as_ref().unwrap().phase,
            HostUploadPhase::AwaitingFile
        );
        assert!(u.f.adapter.uploads.lock().is_empty());
        let selected = u.select(&call);
        assert_eq!(
            u.f.broker.host_task_view(&u.f.token).unwrap().executions[0]
                .upload
                .as_ref()
                .unwrap()
                .phase,
            HostUploadPhase::Selecting
        );
        assert!(u
            .f
            .broker
            .begin_host_upload_selection(&u.f.token, &u.address, call.request_id())
            .is_err());
        let assigned = selected.assign(UploadFixture::file()).unwrap();
        assert_eq!(assigned.outcome.kind, OutcomeKind::Verified);
        assert_eq!(assigned.bytes, Some("不可变内容".len() as u64));
        assert_eq!(call.wait().unwrap().outcome.kind, OutcomeKind::Verified);
        assert!(u.f.broker.model_snapshot_id(&u.intent.run_id).is_none());
        let WebViewUploadReady::Replay(replay) =
            u.f.broker
                .request_webview_upload(u.intent.clone(), &u.permit)
                .unwrap()
        else {
            panic!("replay original result")
        };
        assert_eq!(replay.outcome.kind, OutcomeKind::Verified);
        assert_eq!(u.f.adapter.uploads.lock().len(), 1);
        let mut conflict = u.intent.clone();
        conflict.element_ref.push('x');
        assert!(u
            .f
            .broker
            .request_webview_upload(conflict, &u.permit)
            .is_err());
    }
}

#[test]
fn host_cancel_requires_exact_request_and_keeps_original_picker_occupancy() {
    let u = UploadFixture::new(false);
    let call = u.request();
    let selected = u.select(&call);
    assert!(u
        .f
        .broker
        .cancel_host_upload(&u.f.token, &u.address, "wrong")
        .is_err());
    let mut other = u.address.clone();
    other.generation = "0".into();
    assert!(u
        .f
        .broker
        .cancel_host_upload(&u.f.token, &other, call.request_id())
        .is_err());
    let completion =
        u.f.broker
            .cancel_host_upload(&u.f.token, &u.address, call.request_id())
            .unwrap();
    assert_eq!(completion.outcome.kind, OutcomeKind::Rejected);
    assert!(u.f.broker.host_task_view(&u.f.token).unwrap().executions[0]
        .last_failure
        .is_none());
    assert_eq!(call.wait().unwrap().outcome.kind, OutcomeKind::Rejected);
    assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
        .in_flight
        .load(Ordering::SeqCst));
    assert!(selected.require_current().is_err());
    assert_eq!(
        u.f.broker.request_stop(&u.intent.run_id).unwrap(),
        StopState::StopRequested
    );
    drop(selected);
    assert_eq!(
        u.f.broker.stop_state(&u.intent.run_id).unwrap(),
        StopState::Stopped
    );
    assert!(u.f.adapter.uploads.lock().is_empty());
}

#[test]
fn cancel_and_abandoned_waiter_consume_the_original_action_without_assignment() {
    for abandoned in [false, true] {
        let u = UploadFixture::new(false);
        let call = u.request();
        let selected = u.select(&call);
        if abandoned {
            drop(call);
            assert!(selected.require_current().is_err());
            assert!(u.f.broker.inner.lock().runs[&u.intent.run_id]
                .in_flight
                .load(Ordering::SeqCst));
            assert!(selected.assign(UploadFixture::file()).is_err());
        } else {
            assert_eq!(selected.cancel().outcome.kind, OutcomeKind::Rejected);
            assert_eq!(call.wait().unwrap().outcome.kind, OutcomeKind::Rejected);
        }
        assert!(u.f.adapter.uploads.lock().is_empty());
        assert!(!u.f.broker.inner.lock().runs[&u.intent.run_id]
            .in_flight
            .load(Ordering::SeqCst));
        assert!(matches!(
            u.f.broker
                .request_webview_upload(u.intent.clone(), &u.permit)
                .unwrap(),
            WebViewUploadReady::Replay(_)
        ));
    }
}

#[test]
fn retired_picker_keeps_original_occupancy_until_owner_returns() {
    for cause in ["pause", "stop", "agent", "transport", "feature", "document"] {
        let u = UploadFixture::new(false);
        let call = u.request();
        let selected = u.select(&call);
        match cause {
            "pause" => u.f.broker.pause(&u.intent.run_id).unwrap(),
            "stop" => {
                assert_eq!(
                    u.f.broker.request_stop(&u.intent.run_id).unwrap(),
                    StopState::StopRequested
                );
            }
            "agent" => {
                // Agent replacement first fences its original App owner.
                u.f.broker.fence_task_consent("webview-app")
            }
            "transport" => u.f.broker.retire_transport_connection(&u.f.token),
            "feature" => u.f.broker.set_feature_enabled(false),
            "document" => u.f.adapter.documents.lock()[0] += 1,
            _ => unreachable!(),
        }
        assert!(selected.require_current().is_err(), "{cause}");
        assert!(
            u.f.broker.inner.lock().runs[&u.intent.run_id]
                .in_flight
                .load(Ordering::SeqCst),
            "{cause}"
        );
        assert!(selected.assign(UploadFixture::file()).is_err(), "{cause}");
        assert!(call.wait().is_err(), "{cause}");
        assert!(u.f.adapter.uploads.lock().is_empty());
        u.f.drain();
    }
}

#[test]
fn retirement_after_actual_assignment_never_publishes_success_or_replays() {
    for cause in ["pause", "stop", "document"] {
        let u = UploadFixture::new(false);
        let call = u.request();
        let selected = u.select(&call);
        let broker = u.f.broker.clone();
        let adapter = u.f.adapter.clone();
        *u.f.adapter.after_upload.lock() = Some(Box::new(move |run| match cause {
            "pause" => broker.pause(&run).unwrap(),
            "stop" => {
                broker.request_stop(&run).unwrap();
            }
            _ => adapter.documents.lock()[0] += 1,
        }));
        assert_eq!(
            selected.assign(UploadFixture::file()).unwrap().outcome.kind,
            OutcomeKind::Unknown
        );
        assert!(call.wait().is_err());
        assert_eq!(u.f.adapter.uploads.lock().len(), 1);
        assert!(u
            .f
            .broker
            .request_webview_upload(u.intent.clone(), &u.permit)
            .is_err());
        u.f.drain();
    }
}

#[test]
fn stale_non_file_cross_task_and_model_paths_cannot_request_selection() {
    let u = UploadFixture::new(false);
    for field in ["path", "url", "selector", "code", "bytes", "file"] {
        let mut json = serde_json::to_value(&u.intent).unwrap();
        json[field] = json!("forbidden");
        assert!(serde_json::from_value::<WebViewUploadIntent>(json).is_err());
    }
    let mut stale = u.intent.clone();
    stale.snapshot_id.push('x');
    assert!(u.f.broker.request_webview_upload(stale, &u.permit).is_err());
    let mut stale = u.intent.clone();
    stale.target_generation += 1;
    assert!(u.f.broker.request_webview_upload(stale, &u.permit).is_err());
    let mut stale = u.intent.clone();
    stale.geometry_revision += 1;
    assert!(u.f.broker.request_webview_upload(stale, &u.permit).is_err());
    u.f.broker
        .inner
        .lock()
        .runs
        .get_mut(&u.intent.run_id)
        .unwrap()
        .upload_refs
        .clear();
    assert!(u
        .f
        .broker
        .request_webview_upload(u.intent.clone(), &u.permit)
        .is_err());
    assert!(u.f.adapter.uploads.lock().is_empty());
}

#[test]
fn wrong_host_request_and_duplicate_action_never_open_a_picker() {
    let u = UploadFixture::new(false);
    let call = u.request();
    assert!(u
        .f
        .broker
        .request_webview_upload(u.intent.clone(), &u.permit)
        .is_err());
    let mut other = u.intent.clone();
    other.action_id.push('2');
    assert!(u.f.broker.request_webview_upload(other, &u.permit).is_err());
    let mut address = u.address.clone();
    address.key.task_id.push('x');
    assert!(u
        .f
        .broker
        .begin_host_upload_selection(&u.f.token, &address, call.request_id())
        .is_err());
    let mut address = u.address.clone();
    address.generation = "01".into();
    assert!(u
        .f
        .broker
        .begin_host_upload_selection(&u.f.token, &address, call.request_id())
        .is_err());
    assert!(u
        .f
        .broker
        .begin_host_upload_selection(&u.f.token, &u.address, "forged")
        .is_err());
    assert!(u.f.adapter.uploads.lock().is_empty());
}
