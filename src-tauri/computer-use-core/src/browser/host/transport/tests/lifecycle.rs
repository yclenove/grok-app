//! Original receiver and receipt transitions; no manual removal of occupancy.
use super::*;
use crate::browser::extension_completion::{CompletionPhase, CompletionRetirement};

fn cancel(host: &ExistingTabHost, mode: &str) {
    match mode {
        "pause" => host.quiesce_existing_operations("run-101"),
        "stop" => {
            host.cancel_run("run-101").unwrap();
        }
        "revoke" => host.revoke_pairing(),
        "deadline" => {
            host.inner.lock().extension_requests[0].deadline = Instant::now();
        }
        "navigation" => {
            host.note_user_navigation("101", "https://fixture.invalid/new")
                .unwrap();
        }
        "return" => {
            host.return_borrowed("run-101", "101").unwrap();
        }
        _ => unreachable!(),
    }
}

#[test]
fn claimed_observation_keeps_occupancy_after_cancel_until_physical_settle() {
    for mode in [
        "pause",
        "stop",
        "revoke",
        "deadline",
        "navigation",
        "return",
    ] {
        let (host, session, origin) = fixture();
        grant(&host, &session, &origin, "101", 1);
        let (request, receive) = host
            .enqueue_extension_observe("owner", "run-101", "101", false)
            .unwrap();
        assert_eq!(
            host.poll_extension_request(&origin, &session.session_key, &connection(&session))
                .unwrap(),
            Some(request.clone())
        );
        host.claim_extension_observation(&origin, &session.session_key, &request.completion)
            .unwrap();
        cancel(&host, mode);
        assert!(
            !host.retained_existing_operations_idle("run-101"),
            "retained {mode}"
        );
        assert!(
            host.retained_existing_operations_idle("foreign-run"),
            "foreign {mode}"
        );
        assert!(!host.existing_operations_idle("run-101"), "{mode}");
        assert!(
            receive
                .recv_timeout(Duration::from_millis(100))
                .unwrap()
                .is_err(),
            "{mode}"
        );
        let status = host
            .extension_observation_status(&origin, &request.completion)
            .unwrap();
        assert_eq!(status.phase, CompletionPhase::Claimed, "{mode}");
        assert!(status.cancel_requested, "{mode}");
        assert!(
            host.enqueue_extension_observe("owner", "run-101", "101", false)
                .is_err(),
            "{mode}"
        );
        assert!(
            host.complete_extension_request(&origin, &session.session_key, result(request.clone()))
                .is_err(),
            "{mode}"
        );
        assert!(host
            .settle_extension_observation("https://fixture.invalid", &request.completion)
            .is_err());
        assert!(!host.existing_operations_idle("run-101"));
        // The original synthetic reader has now returned; no Bearer is needed.
        host.settle_extension_observation(&origin, &request.completion)
            .unwrap();
        assert!(
            host.retained_existing_operations_idle("run-101"),
            "settled {mode}"
        );
        assert!(host.existing_operations_idle("run-101"), "{mode}");
        assert_eq!(
            host.extension_observation_retirement(&origin, &request.completion)
                .unwrap(),
            CompletionRetirement::Settled
        );
        host.settle_extension_observation(&origin, &request.completion)
            .unwrap();
        assert!(host
            .claim_extension_observation(&origin, &session.session_key, &request.completion)
            .is_err());
        assert!(host
            .complete_extension_request(&origin, &session.session_key, result(request))
            .is_err());
        assert!(host.inner.lock().tabs["101"].current_observation.is_none());
    }
}

#[test]
fn cancellation_before_claim_retires_not_started_work_and_rejects_late_claim() {
    for delivered in [false, true] {
        for mode in ["pause", "stop", "revoke", "deadline"] {
            let (host, session, origin) = fixture();
            grant(&host, &session, &origin, "101", 1);
            let (request, receive) = host
                .enqueue_extension_observe("owner", "run-101", "101", false)
                .unwrap();
            if delivered {
                host.poll_extension_request(&origin, &session.session_key, &connection(&session))
                    .unwrap();
            }
            cancel(&host, mode);
            assert!(
                host.existing_operations_idle("run-101"),
                "{delivered}/{mode}"
            );
            assert!(receive
                .recv_timeout(Duration::from_millis(100))
                .unwrap()
                .is_err());
            assert!(host
                .claim_extension_observation(&origin, &session.session_key, &request.completion)
                .is_err());
            assert!(host
                .complete_extension_request(&origin, &session.session_key, result(request))
                .is_err());
        }
    }
}

#[test]
fn first_observation_requires_one_claim_and_physical_settle_before_publication() {
    let (host, session, origin) = fixture();
    grant(&host, &session, &origin, "101", 1);
    assert!(host.inner.lock().tabs["101"].current_observation.is_none());
    let (request, receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    assert!(host
        .claim_extension_observation(&origin, &session.session_key, &request.completion)
        .is_err());
    host.poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap();
    assert!(host
        .complete_extension_request(&origin, &session.session_key, result(request.clone()))
        .is_err());
    host.claim_extension_observation(&origin, &session.session_key, &request.completion)
        .unwrap();
    assert!(host
        .claim_extension_observation(&origin, &session.session_key, &request.completion)
        .is_err());
    assert!(host
        .complete_extension_request(&origin, &session.session_key, result(request.clone()))
        .is_err());
    assert!(receive.try_recv().is_err());
    assert!(!host.existing_operations_idle("run-101"));
    assert!(
        host.existing_completion_status(&origin, &request.completion.proof)
            .is_err(),
        "observation proof must not authenticate as action cleanup"
    );
    assert!(
        host.claim_existing_completion(&origin, &session.session_key, &request.completion.proof)
            .is_err(),
        "observation proof must not grant input"
    );
    let mut changed = request.completion.clone();
    changed.proof.binding.snapshot_id = uuid::Uuid::new_v4().to_string();
    assert!(host
        .settle_extension_observation(&origin, &changed)
        .is_err());
    host.settle_extension_observation(&origin, &request.completion)
        .unwrap();
    host.complete_extension_request(&origin, &session.session_key, result(request))
        .unwrap();
    assert_eq!(receive.recv().unwrap().unwrap().text, "visible fixture");
    assert!(host.existing_operations_idle("run-101"));
    assert!(host.inner.lock().tabs["101"].current_observation.is_some());
    let snapshot = host.inner.lock().tabs["101"]
        .current_observation
        .as_ref()
        .unwrap()
        .snapshot_id
        .clone();
    let action = host
        .offer_existing_completion(
            "owner",
            "run-101",
            "101",
            &snapshot,
            crate::execution::ActionCancellation::default(),
        )
        .unwrap();
    let wrong_purpose = crate::browser::extension_observation::ObservationProof {
        purpose: crate::browser::extension_observation::ObservationPurpose::Observation,
        proof: action.clone(),
    };
    assert!(host
        .extension_observation_status(&origin, &wrong_purpose)
        .is_err());
    assert!(host
        .settle_extension_observation(&origin, &wrong_purpose)
        .is_err());
    host.settle_existing_completion(&origin, &action).unwrap();
}

#[test]
fn concurrent_claims_can_start_only_one_original_reader() {
    let (host, session, origin) = fixture();
    let host = std::sync::Arc::new(host);
    grant(&host, &session, &origin, "101", 1);
    let (request, receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    host.poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let host = host.clone();
            let origin = origin.clone();
            let token = session.session_key.clone();
            let proof = request.completion.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                host.claim_extension_observation(&origin, &token, &proof)
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>(),
        1
    );
    host.settle_extension_observation(&origin, &request.completion)
        .unwrap();
    host.complete_extension_request(&origin, &session.session_key, result(request))
        .unwrap();
    assert!(receive.recv().unwrap().is_ok());
}

#[test]
fn unshare_and_replacement_cannot_borrow_over_an_unfinished_reader() {
    let (host, session, origin) = fixture();
    grant(&host, &session, &origin, "101", 1);
    let (request, receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    host.poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap();
    host.claim_extension_observation(&origin, &session.session_key, &request.completion)
        .unwrap();
    host.unoffer_connected_tab(
        &origin,
        &session.session_key,
        &connection(&session),
        2,
        "101",
        7,
    )
    .unwrap();
    assert!(receive
        .recv_timeout(Duration::from_millis(100))
        .unwrap()
        .is_err());
    assert!(!host.existing_operations_idle("run-101"));
    host.offer_connected_tab_document(
        &connection(&session),
        3,
        SharedTabOffer {
            pairing_token: &session.session_key,
            origin: &origin,
            extension_id: Some(EXTENSION_ID),
            tab_id: "101",
            title: "replacement",
            url: "https://fixture.invalid/",
            browser_id: "chromium",
            profile_id: "fixture",
            document_generation: 8,
            connection_generation: session.generation,
            focused: true,
        },
        Some("fedcba9876543210fedcba9876543210"),
    )
    .unwrap();
    let selector = host.list_shared_candidates()[0].0.clone();
    assert!(host
        .grant_picker_tab("owner", "new-run", &selector)
        .is_err());
    host.settle_extension_observation(&origin, &request.completion)
        .unwrap();
    assert!(host.existing_operations_idle("run-101"));
    assert!(host.grant_picker_tab("owner", "new-run", &selector).is_ok());
    assert!(host
        .complete_extension_request(&origin, &session.session_key, result(request))
        .is_err());
}

#[test]
fn missing_share_document_does_not_borrow_identity_from_an_old_model_snapshot() {
    let (host, session, origin) = fixture();
    grant(&host, &session, &origin, "101", 1);
    host.inner.lock().shared.get_mut("101").unwrap().document_id = None;
    assert!(host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .is_err());
    assert!(host.inner.lock().extension_requests.is_empty());
    assert!(host.inner.lock().observation_completions.idle("run-101"));
}

#[test]
fn cancellation_during_host_image_validation_waits_for_original_validator() {
    for valid in [true, false] {
        let (host, session, origin) = fixture();
        let host = std::sync::Arc::new(host);
        grant(&host, &session, &origin, "101", 1);
        let (request, receive) = host
            .enqueue_extension_observe("owner", "run-101", "101", true)
            .unwrap();
        host.poll_extension_request(&origin, &session.session_key, &connection(&session))
            .unwrap();
        settle(&host, &session, &origin, &request);
        let mut reply = result(request);
        let ExtensionOutcome::Observation { observation } = &mut reply.outcome else {
            unreachable!()
        };
        observation.screenshot = Some(crate::browser::extension_image::fixture_image());
        let (entered, entry) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let decoding = host.clone();
        let worker = std::thread::spawn(move || {
            decoding.complete_extension_request_validated(
                &origin,
                &session.session_key,
                reply,
                |observation, snapshot, screenshot| {
                    let decoded = observation.validate(snapshot, screenshot);
                    entered.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(2)).unwrap();
                    decoded && valid
                },
            )
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        host.quiesce_existing_operations("run-101");
        assert!(receive
            .recv_timeout(Duration::from_millis(100))
            .unwrap()
            .is_err());
        assert!(!host.existing_operations_idle("run-101"));
        assert!(host.inner.lock().extension_requests[0].processing);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(host.existing_operations_idle("run-101"));
        assert!(host.inner.lock().tabs["101"].current_observation.is_none());
    }
}

#[test]
fn browser_cleanup_requires_original_browser_request_and_document() {
    let (host, session, origin) = fixture();
    let first = uuid::Uuid::new_v4().to_string();
    let second = uuid::Uuid::new_v4().to_string();
    host.browser_restart(
        &origin,
        &session.session_key,
        &connection(&session),
        &first,
        None,
        &[],
    )
    .unwrap();
    grant(&host, &session, &origin, "101", 1);
    let (request, receive) = host
        .enqueue_extension_observe("owner", "run-101", "101", false)
        .unwrap();
    host.poll_extension_request(&origin, &session.session_key, &connection(&session))
        .unwrap();
    host.claim_extension_observation(&origin, &session.session_key, &request.completion)
        .unwrap();
    host.quiesce_existing_operations("run-101");
    assert!(receive
        .recv_timeout(Duration::from_millis(100))
        .unwrap()
        .is_err());
    let mut cleanup = crate::browser::extension_completion::BrowserExitCleanup {
        browser_id: first.clone(),
        request_id: request.request_id.clone(),
        document_id: "wrong".into(),
        phase: "prepared".into(),
    };
    assert!(host
        .browser_restart(
            &origin,
            &session.session_key,
            &connection(&session),
            &second,
            Some(&first),
            &[cleanup.clone()]
        )
        .unwrap()
        .is_empty());
    assert!(!host.existing_operations_idle("run-101"));
    cleanup.document_id = request.document_id;
    assert_eq!(
        host.browser_restart(
            &origin,
            &session.session_key,
            &connection(&session),
            &second,
            Some(&first),
            &[cleanup]
        )
        .unwrap()
        .len(),
        1
    );
    assert!(host.existing_operations_idle("run-101"));
}
