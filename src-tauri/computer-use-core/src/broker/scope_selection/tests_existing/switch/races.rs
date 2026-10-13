use super::*;

#[test]
fn existing_task_group_acquired_switch_rolls_back_after_feature_agent_or_transport_changes() {
    for mode in ["feature", "agent", "transport"] {
        let (f, gate) = gate::fixture();
        let outcome = approve_group(&f);
        let broker = f.broker.clone();
        gate.after_borrow("102", move || match mode {
            "feature" => broker.set_feature_enabled(false),
            "agent" => broker
                .bind_agent_session("existing-app", "synthetic-successor")
                .unwrap(),
            "transport" => broker.rotate_connection("existing-app").unwrap(),
            _ => unreachable!(),
        });
        assert!(open(&f, &outcome.run_id, "102").is_err(), "{mode}");
        assert!(
            f.broker.tabs().existing_target_info("102").is_none(),
            "{mode}"
        );
        assert!(f.broker.lease.holder().is_none());
        assert!(!f.broker.inner.lock().runs[&outcome.run_id].switch_in_flight);
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0, "{mode}");
        assert!(f.broker.continuation_count("existing-app") <= 1, "{mode}");
        assert!(
            f.broker.claim_continuation("existing-app").is_none(),
            "{mode}"
        );
        recovery_fences::verify_shares_and_no_io(&f, mode);
    }
}

#[test]
fn existing_task_group_public_executor_registration_cannot_replace_original_switch_owner() {
    let (f, gate) = gate::fixture();
    let outcome = approve_group(&f);
    let broker = f.broker.clone();
    gate.after_borrow("102", move || {
        assert!(broker.register_existing_browser_adapter().is_err());
    });
    open(&f, &outcome.run_id, "102").unwrap();
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert_eq!(
        f.broker.tabs().existing_target_info("102").unwrap().run_id,
        outcome.run_id
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    f.no_io();
}

#[test]
fn existing_task_group_pause_during_owned_switch_cannot_resume_until_original_owner_exits() {
    let (f, gate) = gate::fixture();
    let outcome = approve_group(&f);
    let (entered, ready) = mpsc::channel();
    let (release, waiting) = mpsc::channel();
    gate.after_borrow("102", move || {
        entered.send(()).unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let broker = f.broker.clone();
    let token = f.token.clone();
    let run = outcome.run_id.clone();
    let worker = std::thread::spawn(move || {
        broker.open_approved_task_target(&token, &json!({"runId":run,"targetId":"tab:102"}))
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    f.broker
        .pause_host_execution(&f.token, &io::address(&f, &outcome.run_id))
        .unwrap();
    let reserved = f
        .broker
        .reserve_host_task_resume(&f.token, &io::address(&f, &outcome.run_id));
    let available = f.broker.host_task_view(&f.token).unwrap().executions[0].resume_available;
    release.send(()).unwrap();
    let result = worker.join().unwrap();
    assert!(matches!(reserved, Err(BrokerError::LeaseHeld { .. })));
    assert!(!available);
    assert!(result.is_err());
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(
        f.broker.tabs().existing_target_info("101").unwrap().run_id,
        outcome.run_id
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    let mut resume = recovery::reserve(&f, &outcome.run_id);
    let worker = std::thread::spawn(move || {
        resume.prepare_native(Duration::from_secs(1))?;
        resume.commit()
    });
    io::complete(&f, io::poll(&f));
    worker.join().unwrap().unwrap();
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    f.no_io();
}

#[test]
fn existing_task_group_stop_during_acquired_borrow_waits_for_original_switch_owner() {
    let (f, gate) = gate::fixture();
    let outcome = approve_group(&f);
    let (entered, ready) = mpsc::channel();
    let (release, waiting) = mpsc::channel();
    let host = f.broker.tabs.clone();
    let run = outcome.run_id.clone();
    gate.after_borrow("102", move || {
        assert_eq!(host.existing_target_info("101").unwrap().run_id, run);
        assert_eq!(host.existing_target_info("102").unwrap().run_id, run);
        entered.send(()).unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let broker = f.broker.clone();
    let token = f.token.clone();
    let run = outcome.run_id.clone();
    let worker = std::thread::spawn(move || {
        broker.open_approved_task_target(&token, &json!({"runId":run,"targetId":"tab:102"}))
    });
    let started = ready.recv_timeout(Duration::from_secs(2));
    let before = Instant::now();
    let tickets = f.broker.consent_stop("existing-app");
    let elapsed = before.elapsed();
    let states = tickets
        .iter()
        .map(|ticket| f.broker.finish_stop_cleanup(ticket).unwrap())
        .collect::<Vec<_>>();
    let _ = release.send(());
    let result = worker.join().unwrap();
    assert!(started.is_ok());
    assert!(elapsed < Duration::from_millis(100));
    assert_eq!(states, [StopState::StopRequested]);
    assert!(result.is_err());
    assert_eq!(
        f.broker.stop_state(&outcome.run_id).unwrap(),
        StopState::Stopped
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 3);
    f.no_io();
}

#[test]
fn existing_task_group_reshare_after_acquisition_rolls_back_without_returning_old_active() {
    let (f, gate) = gate::fixture();
    let outcome = approve_group(&f);
    let before = f.broker.authorized_target(&outcome.run_id).unwrap();
    let host = f.broker.tabs.clone();
    let origin = f.origin.clone();
    let pairing = f.pairing.clone();
    gate.after_borrow("102", move || {
        host.offer_connected_tab_document(
            &connection(&pairing),
            4,
            SharedTabOffer {
                pairing_token: &pairing.session_key,
                origin: &origin,
                extension_id: Some(EXTENSION_ID),
                tab_id: "102",
                title: "Second",
                url: "https://fixture.invalid/",
                browser_id: "chromium",
                profile_id: "fixture",
                document_generation: 7,
                connection_generation: pairing.generation,
                focused: true,
            },
            Some(DOCUMENT),
        )
        .unwrap();
    });
    assert!(open(&f, &outcome.run_id, "102").is_err());
    assert_eq!(f.broker.authorized_target(&outcome.run_id).unwrap(), before);
    assert_eq!(
        f.broker.tabs().existing_target_info("101").unwrap().run_id,
        outcome.run_id
    );
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    f.no_io();
}

#[test]
fn existing_task_group_foreign_successor_of_acquired_borrow_is_never_released() {
    let (f, gate) = gate::fixture();
    let outcome = approve_group(&f);
    let host = f.broker.tabs.clone();
    gate.after_borrow("102", move || {
        let original = host.existing_target_info("102").unwrap();
        assert!(host.release_existing_grant(&original));
        let selector = host
            .list_shared_candidates()
            .into_iter()
            .find(|(id, _, _)| id.starts_with("102@"))
            .unwrap()
            .0;
        host.grant_picker_tab("foreign", "foreign-run", &selector)
            .unwrap();
    });
    assert!(open(&f, &outcome.run_id, "102").is_err());
    let host = f.broker.tabs();
    assert_eq!(
        host.existing_target_info("102").unwrap().run_id,
        "foreign-run"
    );
    assert!(!host.is_closed("102"));
    f.broker.fence_task_consent("existing-app");
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert_eq!(
        host.existing_target_info("102").unwrap().run_id,
        "foreign-run"
    );
    host.return_borrowed("foreign-run", "102").unwrap();
    f.no_io();
}
