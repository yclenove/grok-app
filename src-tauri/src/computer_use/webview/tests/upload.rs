//! File assignment must retain the original reference and physical operation.

use super::*;
use grok_computer_use_core::adapter::HostSelectedUploadFile;

fn upload_request(adapter: &WebViewAdapter, target: &TargetInfo) -> UploadRequest {
    let bound = adapter.bound_target(&target.target_id).unwrap();
    bound.observation.lock().snapshot = Some(dom::Published {
        id: "mock-upload".into(),
        nodes: [("mock-upload:1".into(), vec!["upload".into(), "wait".into()])]
            .into_iter()
            .collect(),
    });
    UploadRequest {
        run_id: "run".into(),
        target_id: target.target_id.clone(),
        target_generation: target.lifecycle_stamp,
        action_id: "host-file-once".into(),
        snapshot_id: "mock-upload".into(),
        element_ref: "mock-upload:1".into(),
        file: HostSelectedUploadFile::from_host_selection(
            "私有文件.txt".into(),
            "text/plain".into(),
            b"private file content".to_vec(),
        )
        .unwrap(),
        cancellation: Default::default(),
    }
}

#[test]
fn upload_assigns_once_and_publishes_only_under_retained_authority() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let request = upload_request(&adapter, &target);
    let staged = adapter.stage_upload(&request).unwrap();
    assert!(staged.result().applied && staged.result().postcondition_ok);
    assert!(!staged.result().detail.contains("private file content"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    let mut commits = 0;
    staged
        .commit(&mut || {
            commits += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(commits, 1);
    assert!(matches!(
        adapter.stage_upload(&request),
        Err(UploadError::NotDispatched(_))
    ));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
}

#[test]
fn wrong_run_ref_snapshot_generation_or_precancel_never_queue_file_content() {
    for field in [
        "run",
        "ref",
        "snapshot",
        "generation",
        "zero-generation",
        "action",
        "cancel",
    ] {
        let adapter = WebViewAdapter::new();
        let view = TestView::new(0);
        let target = bind(&adapter, view.clone());
        let mut request = upload_request(&adapter, &target);
        match field {
            "run" => request.run_id = "another-run".into(),
            "ref" => request.element_ref = "#arbitrary-selector".into(),
            "snapshot" => request.snapshot_id = "another-snapshot".into(),
            "generation" => request.target_generation += 1,
            "zero-generation" => request.target_generation = 0,
            "action" => request.action_id.clear(),
            _ => request.cancellation.cancel(),
        }
        assert!(
            matches!(
                adapter.stage_upload(&request),
                Err(UploadError::NotDispatched(_))
            ),
            "{field}"
        );
        assert_eq!(view.scripts.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn pause_during_upload_preflight_never_queues_the_assignment() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(1);
    let target = bind(&adapter, view.clone());
    let request = upload_request(&adapter, &target);
    let worker = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.stage_upload(&request))
    };
    view.entered.wait();
    adapter.abort("run", 2).unwrap();
    view.release.wait();
    assert!(matches!(
        worker.join().unwrap(),
        Err(UploadError::DispatchedUnknown(_))
    ));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 1);
    assert!(adapter.is_idle("run"));
}

#[test]
fn timed_out_upload_keeps_occupancy_and_cannot_replay_after_unbind() {
    let adapter = WebViewAdapter::new();
    let view = TestView::new(0);
    let target = bind(&adapter, view.clone());
    let request = upload_request(&adapter, &target);
    view.timeout_at.store(2, Ordering::SeqCst);
    assert!(matches!(
        adapter.stage_upload(&request),
        Err(UploadError::DispatchedUnknown(_))
    ));
    assert!(!adapter.is_idle("run"));
    adapter.release_target(&target.target_id);
    let replacement = bind(&adapter, view.clone());
    let next = upload_request(&adapter, &replacement);
    assert!(adapter.stage_upload(&next).is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    let callback = view.pending.lock().take().unwrap();
    assert!(callback.check().is_err());
    callback.finished();
    assert!(adapter.is_idle("run"));
    assert!(adapter.stage_upload(&request).is_err());
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    let staged = adapter.stage_upload(&next).unwrap();
    staged.commit(&mut || Ok(())).unwrap();
    assert_eq!(view.scripts.load(Ordering::SeqCst), 4);
}

#[test]
fn retirement_after_assignment_never_publishes_or_replays_old_upload() {
    for control in ["pause", "navigate", "unbind", "rebind", "cancel"] {
        let adapter = WebViewAdapter::new();
        let view = TestView::new(0);
        let target = bind(&adapter, view.clone());
        let request = upload_request(&adapter, &target);
        let staged = adapter.stage_upload(&request).unwrap();
        match control {
            "pause" => adapter.abort("run", 2).unwrap(),
            "navigate" => {
                view.navigate("https://fixture.invalid/replaced").unwrap();
            }
            "unbind" => adapter.release_target(&target.target_id),
            "rebind" => {
                bind(&adapter, view.clone());
            }
            _ => request.cancellation.cancel(),
        }
        let mut published = 0;
        assert!(
            staged
                .commit(&mut || {
                    published += 1;
                    Ok(())
                })
                .is_err(),
            "{control}"
        );
        assert_eq!(published, 0);
        assert!(adapter.stage_upload(&request).is_err());
        assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn cancellation_during_native_assignment_does_not_publish_a_late_success() {
    let adapter = Arc::new(WebViewAdapter::new());
    let view = TestView::new(2);
    let target = bind(&adapter, view.clone());
    let request = upload_request(&adapter, &target);
    let worker = {
        let adapter = adapter.clone();
        std::thread::spawn(move || adapter.stage_upload(&request))
    };
    view.entered.wait();
    adapter.abort("run", 2).unwrap();
    assert!(!adapter.is_idle("run"));
    view.release.wait();
    assert!(matches!(
        worker.join().unwrap(),
        Err(UploadError::DispatchedUnknown(_))
    ));
    assert!(adapter.is_idle("run"));
    assert_eq!(view.scripts.load(Ordering::SeqCst), 2);
}
